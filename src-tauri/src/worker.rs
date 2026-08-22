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
//! numbers are the reason the design looks the way it does. The one that decided
//! what this file is:
//!
//! - **The sandbox profile is the measured one**, not the one that looks right.
//!   Denying `file-read*` and allowing the font directories back still renders
//!   a different typeface, because font lookup needs *metadata* reads across the
//!   filesystem. See [`SANDBOX_PROFILE`].
//!
//! The other two numbers are recorded where they bear: what a round trip costs
//! in `worker_proto.rs`, and what a mapped descriptor buys in
//! `worker_handover.rs`.
//!
//! One worker serves exactly one document. That is a stronger isolation story
//! than multiplexing --- a document that kills its worker takes nothing else
//! with it --- and it is also what makes a worker restartable without a
//! reopening protocol.
//!
//! **Two platforms, and the boundary is built from a different end on each.**
//! macOS's is applied by the child to *itself*, after `exec` and after PDFium is
//! bound: `sandbox_init` with [`SANDBOX_PROFILE`], which is why `worker_child.rs`
//! says the ordering there is the whole of the security argument. Windows offers
//! nothing a process can apply to itself, so the *parent* builds it --- a
//! low-integrity token and a job object, fitted while the child is still
//! suspended, so it is in force from the child's first instruction. That is
//! `sandbox_win.rs`, and `examples/win_sandbox_probe.rs` is the measurement of which
//! rung of containment PDFium survives.
//!
//! The asymmetry decides the handover too, which is why the two are separate
//! mechanisms here rather than one with a parameter. A macOS parent **sends** the
//! document's descriptor over a socket ([`SOCK_FD`], `SCM_RIGHTS`); a Windows
//! parent **writes** a handle into the running child's table with
//! `DuplicateHandle` and then names the number it wrote (`Handover`) --- the
//! direction integrity levels permit, since medium may reach into low and never
//! the reverse.
//!
//! On a platform with neither, [`Worker::spawn`] refuses rather than running a
//! worker that would be uncontained while wearing the same name. See
//! `NO_WORKERS`, which exists so that the four refusals cannot drift apart.
//!
//! **Split along its four seams at 2,861 lines**, by the reasoning that took
//! `workers.rs` out of `render.rs`. What is left here is the process and its
//! lifetime --- [`Worker`], [`PreWorker`], [`WarmWorker`], the constants both
//! halves must agree on before `exec`, and how a child's death is put into words.
//! The rest moved out whole:
//!
//! - `worker_proto.rs` --- what the two processes say to each other.
//! - `worker_shm.rs` --- the mappings a document and a tile travel in.
//! - `worker_handover.rs` --- giving a document to a worker that already exists.
//! - `worker_argv.rs` --- the argv this side writes and the child reads back.
//!
//! Nothing changed in the move, and every path still resolves: this module
//! re-exports each moved item, so `crate::worker::Shm` and
//! `tpdf_lib::worker::Request` mean what they meant. The control for that claim
//! is the test suite --- the same check names, moved with the code they test ---
//! plus `worker-probe` and `backend-probe`, which exercise the boundary against
//! the running program.

use std::io::{BufReader, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
#[cfg(not(windows))]
use std::process::{Child, ChildStdin, ChildStdout};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;

#[cfg(windows)]
use crate::worker_argv::command_line;
#[cfg(target_os = "macos")]
use crate::worker_handover::{is_scratch, send_document, socket_pair};
use crate::worker_proto::{read_reply_line, ReplyError};
#[cfg(windows)]
use crate::worker_shm::duplicate_into;

// The four modules this was split into, re-exported so that every path a caller
// ever used still resolves --- `workers.rs`, `worker_child.rs`, the probes and
// every example name them through `worker::`, and a split that renamed a path
// would be a split that had to edit its consumers to prove it changed nothing.
#[cfg(windows)]
pub use crate::worker_argv::{doc_handle_arg, tile_handle_arg};
pub use crate::worker_argv::{doc_len_arg, library_dir_arg};
#[cfg(target_os = "macos")]
pub use crate::worker_handover::recv_document;
#[cfg(windows)]
pub use crate::worker_proto::Handover;
pub use crate::worker_proto::{Request, Response, MAX_REPLY_BYTES};
pub use crate::worker_shm::Shm;

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
/// data once there is something to open. `examples/fdpass_probe.rs` is the standing
/// proof that this crosses a sandboxed boundary --- with the control that the
/// child cannot read `/etc/hosts` at the time, since the transfer works equally
/// well on a process that never sandboxed itself.
pub const SOCK_FD: i32 = 5;

/// The flag the document section's handle arrives on, on Windows.
///
/// Windows has no counterpart to [`DOC_FD`]: handles are inherited by *value*,
/// not by number, so there is nothing for the two sides to agree on in advance
/// and the value has to be told to the child. argv is where every Win32 sandbox
/// does this, and `examples/win_sandbox_probe.rs` measured it working under the
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
/// This is the whole point of pre-spawning. `examples/prespawn_bench.rs` measures what
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

    /// The worker's peak commit charge in bytes, where the kernel bounds it.
    ///
    /// The Windows counterpart to [`Self::footprint`], and the two are split
    /// rather than merged into one "how much memory" accessor because they are
    /// different measurements answering different questions. A footprint is a
    /// poll, and it exists on macOS because there is no kernel bound to ask ---
    /// it can only catch a leak, never a burst. This is a high-water mark
    /// beside a limit the kernel enforces, so it says how close a worker came
    /// to being refused rather than how large it was at the moment of asking.
    ///
    /// Merging them would need one name to mean both, and `docs/TRAPS.md`
    /// already records what happens when one constant stands for two platform
    /// distinctions.
    ///
    /// `None` off Windows, exactly as [`Self::footprint`] is `None` off macOS.
    #[must_use]
    pub fn peak_commit(&self) -> Option<u64> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            self.child.peak_commit()
        }
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

#[cfg(test)]
mod tests {
    /// The lock both arms of [`QuietChildStderr`] take.
    ///
    /// Process-wide state needs process-wide exclusion, and libtest runs these
    /// in parallel threads: two overlapping guards would have the first restore
    /// what the second installed, leaving the harness with no stderr at all for
    /// the rest of the run.
    fn quiet_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // A poisoned lock means an earlier test panicked inside the window. Its
        // `Drop` ran on the way out, so the handle is already restored and the
        // state this guards is sound --- refusing here would turn one failure
        // into every later one.
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sends a test child's stderr to the null device for the length of a spawn.
    ///
    /// The two checks below spawn a worker whose child is the libtest harness,
    /// which has no `--render-worker` dispatch and says so. That refusal is
    /// their *control* --- it is what makes the child die at once --- but a worker
    /// inherits its parent's stderr by design, so it landed on the console as a
    /// bare `error: Unrecognized option: 'render-worker'` line above the `ok`
    /// that followed it. `AGENTS.md` records what that costs: a reader cannot
    /// tell a green run carrying an expected control from a run that failed and
    /// reported it badly, and the run that looks broken is the one that is fine.
    ///
    /// **It swaps the process's own stderr rather than adding a parameter to the
    /// spawn**, because that handle *is* the input the spawn path reads
    /// (`Stdio::with_inherited_stderr` on Windows, `Stdio::inherit()` on macOS).
    /// Production keeps one path with no `cfg(test)` branch in it, which is the
    /// whole reason these checks are worth running. The child copies the handle
    /// at `CreateProcess`/`exec`, so the window only has to cover the spawn ---
    /// a child quieted here stays quiet however long it lives afterwards.
    ///
    /// Two things it deliberately does not silence. Rust's own `eprintln!` and
    /// panic messages go through libtest's per-test capture, never through this
    /// handle, so a failure still says everything it said before. And nothing
    /// survives a leak: `Drop` restores the real handle even when the test
    /// panics inside the window.
    ///
    /// Only the two arms exist. A target that is neither fails to compile here,
    /// which is the right direction --- the alternative is a no-op arm that
    /// silently stops quieting on a platform nobody checked.
    ///
    /// **Mutate it with `--test-threads=1`, or the result is a lie.** The window
    /// is process-wide, so a guard held by *one* test quiets every child any
    /// other test spawns while it is open. Deleting the `install` below and
    /// running the module's nineteen checks in parallel printed nothing at all
    /// --- the other guard happened to cover this spawn --- and that reads exactly
    /// like a guard nothing depends on. Alone, the same deletion puts the line
    /// straight back. `AGENTS.md`: a control contaminated by the phase beside it.
    #[cfg(windows)]
    struct QuietChildStderr {
        /// The handle this process had on the way in, restored on the way out.
        previous: windows_sys::Win32::Foundation::HANDLE,
        /// Held open for the window, and closed *after* `Drop` restores.
        _null: std::fs::File,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    #[cfg(windows)]
    impl QuietChildStderr {
        fn install() -> Self {
            use std::os::windows::io::AsRawHandle;

            use windows_sys::Win32::System::Console::{
                GetStdHandle, SetStdHandle, STD_ERROR_HANDLE,
            };

            let lock = quiet_lock();
            let null = std::fs::OpenOptions::new()
                .write(true)
                .open("NUL")
                .expect("NUL is always openable on Windows");
            // SAFETY: a documented constant, and neither call takes a pointer.
            // `null` is moved into the guard below, so the handle installed here
            // stays live until `Drop` has put `previous` back.
            let previous = unsafe {
                let previous = GetStdHandle(STD_ERROR_HANDLE);
                SetStdHandle(STD_ERROR_HANDLE, null.as_raw_handle().cast());
                previous
            };
            Self {
                previous,
                _null: null,
                _lock: lock,
            }
        }
    }

    #[cfg(windows)]
    impl Drop for QuietChildStderr {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};

            // SAFETY: the handle this process was given on the way in. Fields
            // drop after this body, so `_null` is still open as it is replaced.
            unsafe { SetStdHandle(STD_ERROR_HANDLE, self.previous) };
        }
    }

    /// As the Windows arm above, over descriptor 2 instead of a std handle.
    ///
    /// Only `the_prespawn_constant_matches_what_prespawn_does` reaches this, and
    /// only where `prespawn` really starts a child --- which today is macOS,
    /// whose `Command` asks for `Stdio::inherit()` for the same reason Windows
    /// shares its handle: a sandbox denial or a fatal signal has to be visible.
    ///
    /// Written on Windows and run on macOS for the first time on 2026-07-31.
    /// It compiles and the arm is correct --- and the run settled the claim it was
    /// written on, which was wrong. **The noise it addresses does not occur on
    /// macOS**, so here this guard silences nothing.
    ///
    /// Measured rather than inferred, because "no output" is what a guard that
    /// works and a guard that is never needed both look like. Removing the
    /// `install()` call changed no output over 40 runs, while the same harness
    /// invoked directly (`--render-worker --prespawn --lib .`) prints
    /// `error: Unrecognized option: 'render-worker'` and exits 101 --- so the
    /// child is capable of the complaint and does not make it. Holding the
    /// `PreWorker` for 400 ms before dropping it makes the line appear exactly
    /// once, which names the mechanism: `prespawn` returns as soon as
    /// `fork`/`exec` is issued, the test drops the child immediately, and the
    /// kill lands while the child is still in dyld --- before libtest parses
    /// argv. Windows creates the process suspended and resumes it, and loses
    /// that race.
    ///
    /// **Kept rather than deleted, and the distinction matters.** A guard no
    /// mutation can break is usually one to remove, but the impossibility here
    /// is not local to this arm --- it lives in `PreWorker`'s drop timing, in
    /// another type. Deleting this would make a clean console depend silently on
    /// how fast a kill lands, with nothing to fail when that changes; a test that
    /// ever waits on a pre-spawned child would resurrect the noise. `AGENTS.md`
    /// records that shape: when the impossibility lives elsewhere, keep the
    /// guard and say where the guarantee actually comes from.
    #[cfg(unix)]
    struct QuietChildStderr {
        /// A duplicate of the real stderr, `dup2`'d back on the way out.
        previous: std::os::fd::OwnedFd,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    #[cfg(unix)]
    impl QuietChildStderr {
        fn install() -> Self {
            use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

            let lock = quiet_lock();
            // SAFETY: a duplicate of a descriptor this process owns. It is
            // handed to `OwnedFd` immediately, so nothing else can close it and
            // it is closed exactly once, after `Drop` has restored it.
            let previous = unsafe {
                let saved = libc::dup(libc::STDERR_FILENO);
                assert!(saved >= 0, "stderr could not be duplicated");
                OwnedFd::from_raw_fd(saved)
            };
            let null = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .expect("/dev/null is always openable");
            // SAFETY: both descriptors are live here; `dup2` closes descriptor 2,
            // whose only other reference is the duplicate saved above.
            unsafe { libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO) };
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for QuietChildStderr {
        fn drop(&mut self) {
            use std::os::fd::AsRawFd;

            // SAFETY: the saved duplicate is still owned by this guard and is
            // closed only after this body returns.
            unsafe { libc::dup2(self.previous.as_raw_fd(), libc::STDERR_FILENO) };
        }
    }

    /// A worker whose child dies is *reported* dead, not waited on forever.
    ///
    /// Under `cargo test` this is a real spawn with a fake worker: `current_exe`
    /// is the test harness, which has no `--render-worker` dispatch, so the child
    /// exits immediately. That is the point --- what is under test here is the
    /// plumbing, not the protocol, and a child that dies at once exercises it
    /// harder than one that answers. Its complaint about the flag is quieted by
    /// [`QuietChildStderr`], which is about the console and not about the child:
    /// the spawn, the death and the epitaph are all exactly what they were.
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
        let spawned = {
            let _quiet = QuietChildStderr::install();
            super::Worker::spawn(&path, std::path::Path::new("."))
        };
        let _ = std::fs::remove_file(&path);
        // `map_err` first: a live child is not something to format into a panic.
        let mut worker = match spawned {
            Ok(w) => w,
            Err(e) => panic!("Worker::spawn must start a contained child: {e}"),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let answer = worker.call(&super::Request::Open {
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
        // The guard covers the child's whole life, not only the spawn: `is_ok`
        // drops the `PreWorker` inside the block, so the kill happens here too.
        let started = {
            let _quiet = QuietChildStderr::install();
            super::Worker::prespawn(std::path::Path::new(".")).is_ok()
        };
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
