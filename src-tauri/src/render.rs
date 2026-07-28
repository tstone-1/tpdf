//! The render service: the single owner of Pdfium and of every open document.
//!
//! Everything Pdfium touches happens on one dedicated thread. This is not a
//! stylistic choice, but the reason is not the one this comment used to give.
//! `pdfium-render`'s `thread_safe` feature does **not** serialize Pdfium calls
//! --- there is no mutex anywhere in the crate's native path, and concurrent
//! renders from two threads segfault on a complex page while merely appearing
//! to work on a simple one. The single thread is what keeps that undefined
//! behaviour off the table. Measured by `bin/thread_probe.rs`; see AGENTS.md.
//!
//! ## Abandoning work
//!
//! One FIFO thread means a queue, and a queue means tiles rendering for a
//! viewport that has moved on. Spike 0.8 measured what that costs: sustained
//! 60 fps over a screen that was 0--4% sharp, because every second of renderer
//! time went to tiles nobody could see any more. So a request carries a `rid`
//! and can be withdrawn --- [`RenderService::cancel`]. A withdrawn request that
//! has not started is dropped without rendering; one already in flight is
//! abandoned through PDFium's progressive API (`progressive.rs`), which returns
//! in 0.25--24 ms against a render that would otherwise have run 6.3 s.
//!
//! Only the client can know a tile is no longer wanted --- the window is its
//! state, not ours --- so cancellation is explicit rather than inferred from an
//! epoch. An epoch would have to either cancel still-wanted long renders on
//! every window change, which never finishes anything on a hard page, or leave
//! stale ones running, which is the behaviour being fixed.
//!
//! ## Where the document is actually parsed
//!
//! Two backends sit behind this one interface, chosen by [`Backend`]. The
//! default on macOS is [`Backend::Worker`]: every document is parsed in a
//! sandboxed child process, because `docs/THREAT-MODEL.md` requires it and
//! `AGENTS.md` records why it cannot be a later hardening pass. The in-process
//! path is kept, and not out of sentiment --- it is the control the worker is
//! compared against, and `bin/backend_probe.rs` is that comparison.
//!
//! Almost nothing above this module changes with the switch. What does:
//!
//! - **The app process never binds PDFium** in worker mode, so it never maps
//!   the parsing code, let alone runs it on a document.
//! - **A reply can fail because the worker died**, which an in-process render
//!   could only do by taking the app with it. A death is not usually visible to
//!   the caller: the request is retried against a replacement holding the same
//!   bytes --- see [`Workers::with_worker`].
//! - **A withdrawal now has two halves.** The queue here decides what the caller
//!   sees; a `Withdraw` on the wire decides whether the worker keeps burning CPU
//!   on a tile nobody wants. Neither is redundant --- see [`RenderService::cancel`].
//!
//! ## The pool
//!
//! The worker backend is served by several threads sharing one job queue, and
//! each document has a pool of processes they draw from --- so several tiles of
//! the same page render at once. The in-process backend is **not**: concurrent
//! Pdfium in one process is undefined behaviour whatever the handles are, which
//! is why security and performance wanted the same architecture.
//!
//! All of that lives in [`crate::workers`]. What stays here is the service, the
//! [`Engine`] trait both backends implement, the dispatch written once against
//! it, and the in-process control the pool is measured against --- so this file
//! is about *what a render request is*, and that one is about where it runs.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

use crate::outline::{self, Outline};
use crate::progressive::{self, Bindings, CancelToken, Outcome, RawDocument, TileSpec};
use crate::queue::{Claim, SharedQueue};
use crate::search::{self, PageMatches};
use crate::startup::{mark, since_process_start_ms};
use crate::text::{self, PageText};
use crate::workers::{reap_idle, serve_pooled, service_threads, Workers};

/// The pool's knobs, re-exported on the path they have always had.
///
/// `bin/pool_bench.rs` and `bin/backend_probe.rs` both call
/// `tpdf_lib::render::pool_size()`, and moving the pool into its own module is a
/// tidying of *this* file rather than a change to what a benchmark is allowed to
/// ask for. `AGENTS.md` records that a spike entry point still reachable from a
/// shell command in `BUILD.md` is not dead code; the same applies to the paths
/// those entry points import through.
pub use crate::workers::{idle_timeout, pool_size, DEFAULT_IDLE, DEFAULT_POOL};

/// Pixel format of a returned tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileFormat {
    /// Uncompressed RGBA8. Larger on the wire, no decode cost.
    Raw,
    /// PNG. Smaller on the wire, pays encode and decode.
    Png,
}

/// A request for one tile of one page at one zoom level.
#[derive(Clone, Debug)]
pub struct TileRequest {
    /// Caller-assigned identity, unique for the life of the process, by which
    /// the request can later be withdrawn. Zero means "not withdrawable".
    pub rid: u64,
    pub doc: u32,
    pub page: u32,
    /// Device pixels per PDF point.
    pub scale: f32,
    /// Quarter-turns clockwise the view is rotated by, 0 to 3.
    ///
    /// A property of how the reader is looking at the document, not of the
    /// document --- rotating the view never touches the file. It composes on top
    /// of the page's own `/Rotate`.
    pub turns: u8,
    /// Whether to invert the page's lightness, for reading in the dark.
    ///
    /// Like `turns`, a property of the view rather than the document. It is part
    /// of the request rather than something the frontend does to the pixels
    /// because a CSS filter is applied by the compositor and cannot be read
    /// back: a check could then only assert that the style was set, which is the
    /// style agreeing with itself and no evidence about what is on screen.
    pub invert: bool,
    /// Tile origin in device pixels, relative to the scaled page's top-left.
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
    pub format: TileFormat,
}

/// A rendered tile plus the timings that make spike 0.1 answerable.
#[derive(Clone, Debug)]
pub struct Tile {
    pub bytes: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub format: TileFormat,
    /// Time inside Pdfium.
    pub render_us: u64,
    /// Time spent encoding, zero for `Raw`.
    pub encode_us: u64,
}

/// What became of a tile request.
#[derive(Clone, Debug)]
pub enum TileOutcome {
    /// The tile was rendered.
    Rendered(Tile),
    /// The caller withdrew the request. Not an error, and deliberately not an
    /// empty tile either: there is nothing to draw, and a caller that treated
    /// this as a blank tile would paint over content it already had.
    Abandoned,
}

/// Page geometry in PDF points, sent to the frontend up front so the virtual
/// scroller can size the document without rendering anything.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct PageSize {
    pub width_pt: f32,
    pub height_pt: f32,
}

/// Result of opening a document.
#[derive(Clone, Debug, serde::Serialize)]
pub struct DocumentInfo {
    pub id: u32,
    /// Geometry of every page, or only of page 1 when the open was lazy.
    pub pages: Vec<PageSize>,
    /// Pages in the document, which is known even when their sizes are not.
    pub page_count: usize,
    /// Whether `pages` is the whole table or only its first entry.
    pub lazy_geometry: bool,
    /// Time spent opening, i.e. parse and cross-reference repair.
    pub open_ms: f64,
    /// Milliseconds since process start when the open completed.
    pub at_ms: f64,
}

/// Where documents are parsed and rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// In this process, on the render thread. The control, and the only thing
    /// available where no sandbox is implemented.
    InProcess,
    /// In a sandboxed child process, one per document.
    Worker,
}

impl Backend {
    /// The backend for this platform, unless `TPDF_BACKEND` overrides it.
    ///
    /// # Errors
    ///
    /// A value that is neither `worker` nor `in-process`. Refused rather than
    /// defaulted: the whole point of the variable is to pin down which of two
    /// implementations ran, and a typo that silently selects the *other* one
    /// makes a comparison between them report whatever it likes.
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("TPDF_BACKEND").ok().as_deref())
    }

    /// Reads a backend name, or the platform default when there is none.
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Ok(Self::default_here()),
            Some("worker") => Ok(Self::Worker),
            Some("in-process") => Ok(Self::InProcess),
            Some(other) => Err(format!(
                "TPDF_BACKEND={other:?} is not a backend --- expected \"worker\" or \"in-process\""
            )),
        }
    }

    /// The default where no one has asked for anything.
    ///
    /// Worker on macOS, because that is where a sandbox exists. Everywhere else
    /// `Worker::spawn` refuses, and defaulting to something that cannot start is
    /// worse than defaulting to the control.
    fn default_here() -> Self {
        if cfg!(target_os = "macos") {
            Self::Worker
        } else {
            Self::InProcess
        }
    }
}

pub(crate) type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

/// Looks up an open document, distinguishing an id that never existed from one
/// that has been closed.
///
/// Both backends keep their documents in a `Vec` whose index *is* the id, and
/// a closed document leaves a hole rather than being removed. Removing it would
/// shift every id after it, so a request already in flight would silently be
/// answered from the wrong document --- which is a far worse failure than an
/// error, because it looks like a rendering bug.
pub(crate) fn open_slot<T>(slots: &[Option<T>], doc: u32) -> Result<&T, String> {
    match slots.get(doc as usize) {
        Some(Some(value)) => Ok(value),
        Some(None) => Err(not_open(doc, true)),
        None => Err(not_open(doc, false)),
    }
}

/// As [`open_slot`], for a caller that needs to mutate what it finds.
pub(crate) fn open_slot_mut<T>(slots: &mut [Option<T>], doc: u32) -> Result<&mut T, String> {
    match slots.get_mut(doc as usize) {
        Some(Some(value)) => Ok(value),
        Some(None) => Err(not_open(doc, true)),
        None => Err(not_open(doc, false)),
    }
}

/// Why an id does not name an open document.
///
/// Shared by the two lookups above rather than written out in each, and that is
/// not tidiness: a mutation of the message in one of them survived every check,
/// because the worker path goes through `_mut` and the in-process tile path does
/// not. Two copies of a distinction are two places for it to drift, and the
/// drift is invisible --- both still refuse.
///
/// The distinction is worth having when one turns up in a log: an id past the
/// end is a caller that invented one, a hole is a caller still using a document
/// it closed itself.
pub(crate) fn not_open(doc: u32, in_range: bool) -> String {
    if in_range {
        format!("document {doc} has been closed")
    } else {
        format!("no such document: {doc}")
    }
}

pub(crate) enum Job {
    Open {
        path: PathBuf,
        /// Collect only page 1's size instead of the whole table.
        lazy_geometry: bool,
        reply: Reply<DocumentInfo>,
    },
    Tile {
        request: TileRequest,
        reply: Reply<TileOutcome>,
    },
    Text {
        doc: u32,
        page: u32,
        reply: Reply<PageText>,
    },
    Search {
        doc: u32,
        page: u32,
        query: String,
        reply: Reply<PageMatches>,
    },
    Outline {
        doc: u32,
        reply: Reply<Outline>,
    },
    Close {
        doc: u32,
        reply: Reply<()>,
    },
}

/// Handle to the render thread. Cheap to clone.
#[derive(Clone)]
pub struct RenderService {
    tx: Sender<Job>,
    /// Which requests are outstanding and which have been withdrawn. See
    /// `queue.rs`, which is where that state machine lives and is tested.
    queue: SharedQueue,
    /// The worker pool, in worker mode, so a withdrawal can reach a render
    /// already inside Pdfium. `None` in-process, where there is nothing to
    /// withdraw *to* --- the token in `queue` is the whole mechanism there.
    workers: Option<Arc<Workers>>,
    backend: Backend,
}

impl RenderService {
    /// Starts the render thread on the backend this platform and environment
    /// select.
    ///
    /// # Panics
    ///
    /// An unreadable `TPDF_BACKEND`, which the app has already refused in `run()`
    /// before any window exists --- this is the backstop for a caller that has
    /// not. It is deliberately not where the message is *delivered*: a panic here
    /// happens inside the Tauri setup hook, which `App::run` invokes from AppKit's
    /// own frames, so it is non-unwinding and aborts through an unsymbolicated
    /// backtrace that races the watchdog's report about an occluded page.
    pub fn start(library_dir: PathBuf) -> Self {
        match Backend::from_env() {
            Ok(backend) => Self::start_with(library_dir, backend),
            Err(e) => panic!("{e}"),
        }
    }

    /// Starts the render thread on a named backend.
    ///
    /// Binding Pdfium (in-process) or spawning a worker both happen off this
    /// call, so a missing or mismatched library surfaces as a failed open rather
    /// than a panic during app setup.
    pub fn start_with(library_dir: PathBuf, backend: Backend) -> Self {
        Self::start_with_pool(library_dir, backend, pool_size())
    }

    /// Starts the render thread with an explicit pool size.
    ///
    /// Separate from the environment variable so that a benchmark can hold
    /// several services at different sizes in one process --- reading `TPDF_POOL`
    /// per service would make the sizes depend on when each was constructed,
    /// which is exactly the kind of thing an interleaved A/B is supposed to rule
    /// out rather than introduce.
    ///
    /// Ignored in-process: concurrent Pdfium in one process is undefined
    /// behaviour, so that backend is one thread whatever is asked for.
    pub fn start_with_pool(library_dir: PathBuf, backend: Backend, pool: usize) -> Self {
        Self::start_tuned(library_dir, backend, pool, idle_timeout())
    }

    /// Starts the render thread with an explicit pool size and idle timeout.
    ///
    /// The timeout is a parameter for the same reason the pool size is: a harness
    /// needs to watch a retirement happen without waiting the default thirty
    /// seconds for it, and `TPDF_IDLE_MS` would set it for every service in the
    /// process --- including the ones a check is using as controls. `AGENTS.md`
    /// records a control in this repository contaminated by the phase that ran
    /// before it; a shared environment variable is the same hazard with a wider
    /// blast radius.
    ///
    /// Ignored in-process, where there are no worker processes to retire.
    pub fn start_tuned(
        library_dir: PathBuf,
        backend: Backend,
        pool: usize,
        idle_after: Duration,
    ) -> Self {
        let pool = pool.max(1);
        let (tx, rx) = channel::<Job>();
        let queue = SharedQueue::default();

        let workers = match backend {
            Backend::InProcess => {
                let thread_queue = queue.clone();
                // One thread, because concurrent Pdfium in this process is
                // undefined behaviour whatever the handles are (module note).
                std::thread::Builder::new()
                    .name("tpdf-render".into())
                    .spawn(move || match InProcess::start(&library_dir, thread_queue) {
                        Ok(engine) => serve(rx, &engine),
                        // Drain the queue, failing every job with the bind
                        // error, so callers get a diagnosable message instead
                        // of a hang.
                        Err(e) => drain(rx, &e),
                    })
                    .expect("failed to spawn render thread");
                None
            }
            Backend::Worker => {
                let engine = Arc::new(Workers::new(library_dir, queue.clone(), pool, idle_after));
                // Immediately, and this is the point of the whole mechanism: the
                // link, the sandbox and the font walk happen while Tauri and
                // WebKit are still coming up -- ~250 ms of which none is ours --
                // rather than while a reader waits for a first page.
                engine.prewarm();
                reap_idle(&engine, idle_after);
                serve_pooled(rx, engine.clone(), service_threads(pool));
                Some(engine)
            }
        };

        Self {
            tx,
            queue,
            workers,
            backend,
        }
    }

    /// How many workers one document may have on this service.
    #[must_use]
    pub fn pool_size(&self) -> usize {
        self.workers.as_ref().map_or(1, |w| w.capacity())
    }

    /// The process id of the warmed spare, if one is waiting.
    ///
    /// Exposed for `bin/backend_probe.rs`, and it earns its place twice. A spare
    /// is a child process like any other, so anything counting this process's
    /// children counts it too --- which is how the pool-capacity check first went
    /// red, at `7 workers, capacity 6`, correctly. Excluding it needs its
    /// identity, not a bigger allowance.
    ///
    /// It is also the only *observable* that a pre-spawn happened at all. The
    /// alternative is a startup mark of ours, and `AGENTS.md` records why that is
    /// weaker: a mark says what our code believes it did, where a pid can be
    /// looked up in the process table by something that did not write it.
    #[must_use]
    pub fn spare_pid(&self) -> Option<u32> {
        self.workers.as_ref().and_then(|w| w.spare_pid())
    }

    /// Every process the spare slot is responsible for, warm or still warming.
    ///
    /// Distinct from [`RenderService::spare_pid`], which answers "is one ready to
    /// use". This answers "which children are not pool workers" --- and during the
    /// window between `fork` and the readiness notice those are different sets.
    #[must_use]
    pub fn spare_pids(&self) -> Vec<u32> {
        self.workers
            .as_ref()
            .map_or_else(Vec::new, |w| w.spare_pids())
    }

    /// Whether every spare process can currently be named.
    ///
    /// For a caller counting this process's children: between `fork` and
    /// registration there is a child that [`RenderService::spare_pids`] does not
    /// list, so a count taken then attributes it to a document's pool.
    #[must_use]
    pub fn spares_settled(&self) -> bool {
        self.workers.as_ref().is_none_or(|w| w.spares_settled())
    }

    /// Which backend this service is running.
    #[must_use]
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Opens a document, invoking `reply` on the render thread when done.
    ///
    /// `lazy_geometry` skips collecting every page's size, which spike 0.2
    /// measured at 86 ms on a 775-page document --- the largest avoidable item
    /// in the startup budget. See PLAN §4.
    pub fn open(&self, path: PathBuf, lazy_geometry: bool, reply: Reply<DocumentInfo>) {
        if self
            .tx
            .send(Job::Open {
                path,
                lazy_geometry,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Requests a tile, invoking `reply` on the render thread when done.
    ///
    /// The reply runs on the render thread deliberately: the protocol responder
    /// can be satisfied there directly, so no thread is spawned per request.
    pub fn tile(&self, request: TileRequest, reply: Reply<TileOutcome>) {
        let rid = request.rid;
        self.queue.with(|queue| queue.enqueue(rid));

        if self.tx.send(Job::Tile { request, reply }).is_err() {
            // Render thread is gone. Forget the request rather than leaving it
            // outstanding forever, since nothing will ever dequeue it.
            self.queue.with(|queue| queue.forget(rid));
        }
    }

    /// Extracts one page's characters, invoking `reply` on the render thread.
    ///
    /// This shares the render thread with tiles rather than getting its own,
    /// which means a text request queues behind whatever tile is rendering ---
    /// up to a second on the A0 sheet. That is a known cost and not an oversight:
    /// a second thread would need a second `FPDF_DOCUMENT`, and concurrent
    /// PDFium is undefined behaviour (see AGENTS.md). Parallelism here arrives
    /// with the worker pool or not at all.
    pub fn text(&self, doc: u32, page: u32, reply: Reply<PageText>) {
        if self.tx.send(Job::Text { doc, page, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Searches one page, invoking `reply` on the render thread.
    ///
    /// One page per job, rather than one job for the document, and that is the
    /// whole design: the render thread is FIFO, so a job that scanned 775 pages
    /// would hold it for a second and a half and every tile behind it would
    /// wait. At page granularity a search interleaves with rendering, and the
    /// caller stops asking to cancel it --- there is nothing to withdraw.
    pub fn search(&self, doc: u32, page: u32, query: String, reply: Reply<PageMatches>) {
        if self
            .tx
            .send(Job::Search {
                doc,
                page,
                query,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reads a document's outline, invoking `reply` on the render thread.
    ///
    /// One job for the whole tree rather than one per level, which is the
    /// opposite of the choice `search` makes and for the opposite reason: the
    /// walk is bounded at 10,000 entries and touches no page content, so it
    /// finishes in single-digit milliseconds, and a per-level protocol would
    /// hand the caller a cycle to terminate rather than a tree.
    pub fn outline(&self, doc: u32, reply: Reply<Outline>) {
        if self.tx.send(Job::Outline { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Withdraws a tile request by its `rid`.
    ///
    /// Safe to call at any point in the request's life, including after it has
    /// finished --- an unknown `rid` is simply ignored, because the caller
    /// cannot know whether its reply is already on the way.
    ///
    /// In worker mode this has two halves and they do different jobs. The queue
    /// here is what the **caller** sees: a request withdrawn before it is
    /// claimed never reaches a worker at all, and one withdrawn afterwards comes
    /// back `Abandoned` whatever the worker did with it. The wire withdrawal is
    /// what stops the **worker** burning a second of CPU on a tile nobody wants.
    /// Neither substitutes for the other --- and it is the first that makes the
    /// second's race harmless, since a `Withdraw` that overtakes its own tile on
    /// the pipe is a no-op in the worker's queue but has already set the token
    /// this side. That last part is only true because `Queue` tracks **every**
    /// in-flight request rather than the most recent one; it did not, and the
    /// note in `queue.rs` records what that silently cost.
    ///
    /// Broadcast rather than addressed, because a `rid` is unique for the life
    /// of the process and a worker that has never seen one ignores it --- which
    /// is `Queue::withdraw`'s documented behaviour rather than an accident to
    /// lean on. The alternative is a rid-to-document table in the parent, which
    /// is a second thing to keep in step with the queue for no benefit at the
    /// one or two documents a reader has open.
    pub fn cancel(&self, rid: u64) {
        self.queue.with(|queue| queue.withdraw(rid));

        if let Some(workers) = &self.workers {
            workers.broadcast_withdraw(rid);
        }
    }

    /// Releases a document and everything holding it open.
    ///
    /// In worker mode that is a process, which is why this exists at all: before
    /// the boundary a leaked document was a heap allocation, and now it is a
    /// sandboxed child holding 7.8--48.2 MB (`worker-probe`, per corpus). A
    /// reader who opens a dozen files in a session should not end up with a
    /// dozen of them.
    ///
    /// **Safe to call the moment the caller stops wanting the document**, with
    /// requests still outstanding. The render thread is FIFO, so everything
    /// already queued for this document is served before the close is; anything
    /// that arrives afterwards is answered "has been closed" rather than from
    /// another document, because the id leaves a hole rather than being removed.
    pub fn close(&self, doc: u32, reply: Reply<()>) {
        if self.tx.send(Job::Close { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with, and every
            // document went with it.
        }
    }
}

/// What a backend has to be able to do.
///
/// One method per job, so the dispatch below is written once and neither backend
/// can quietly grow a job the other does not serve.
///
/// `&self` rather than `&mut self`, because the worker backend is served by
/// several threads at once and the in-process one cannot be. Each keeps whatever
/// interior mutability it needs: a `RefCell` where there is provably one thread,
/// a `Mutex` and a `Condvar` where there are several.
pub(crate) trait Engine {
    fn open(&self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String>;
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String>;
    fn text(&self, doc: u32, page: u32) -> Result<PageText, String>;
    fn search(&self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String>;
    fn outline(&self, doc: u32) -> Result<Outline, String>;
    fn close(&self, doc: u32) -> Result<(), String>;
}

/// Serves one job and answers it.
pub(crate) fn dispatch(job: Job, engine: &dyn Engine) {
    match job {
        Job::Open {
            path,
            lazy_geometry,
            reply,
        } => reply(engine.open(&path, lazy_geometry)),
        Job::Tile { request, reply } => reply(engine.tile(&request)),
        Job::Text { doc, page, reply } => reply(engine.text(doc, page)),
        Job::Search {
            doc,
            page,
            query,
            reply,
        } => reply(engine.search(doc, page, &query)),
        Job::Outline { doc, reply } => reply(engine.outline(doc)),
        Job::Close { doc, reply } => reply(engine.close(doc)),
    }
}

/// Serves jobs on this thread until every handle to the service is dropped.
fn serve(rx: Receiver<Job>, engine: &dyn Engine) {
    for job in rx {
        dispatch(job, engine);
    }
}

/// Fails every job with the same message, for a backend that never started.
fn drain(rx: Receiver<Job>, error: &str) {
    for job in rx {
        match job {
            Job::Open { reply, .. } => reply(Err(error.to_string())),
            Job::Tile { reply, .. } => reply(Err(error.to_string())),
            Job::Text { reply, .. } => reply(Err(error.to_string())),
            Job::Search { reply, .. } => reply(Err(error.to_string())),
            Job::Outline { reply, .. } => reply(Err(error.to_string())),
            Job::Close { reply, .. } => reply(Err(error.to_string())),
        }
    }
}

// ---------------------------------------------------------------- in-process

/// Documents parsed in this process, on the render thread.
///
/// A `RefCell` and not a `Mutex`, and the difference is the whole reason this
/// backend exists as a separate one: concurrent Pdfium is undefined behaviour
/// (see the module note), so this is served by exactly **one** thread and a lock
/// here would suggest otherwise while never being contended. The worker backend
/// is the one that gets a pool.
struct InProcess {
    bindings: Bindings,
    /// Indexed by document id, with a hole where one has been closed. See
    /// [`open_slot`].
    docs: std::cell::RefCell<Vec<Option<RawDocument>>>,
    queue: SharedQueue,
}

impl InProcess {
    /// Binds Pdfium, which is the only part of this that can fail up front.
    fn start(library_dir: &Path, queue: SharedQueue) -> Result<Self, String> {
        let pdfium = bind_pdfium(library_dir)?;
        // Loading and binding the Pdfium dylib is a fixed cost paid before any
        // document can be opened, so it needs its own line in the startup
        // budget. It has no counterpart in worker mode: the app process does not
        // bind Pdfium at all there, which is the point.
        mark("pdfium bound");
        Ok(Self {
            bindings: progressive::bindings_of(pdfium),
            docs: std::cell::RefCell::new(Vec::new()),
            queue,
        })
    }
}

impl Engine for InProcess {
    fn open(&self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String> {
        let t0 = Instant::now();
        let doc = RawDocument::open(self.bindings, path)?;
        let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
        mark("document parsed");

        let size_of = |index: u32| -> Result<PageSize, String> {
            let page = doc.page(index)?;
            Ok(PageSize {
                width_pt: page.width_pt(),
                height_pt: page.height_pt(),
            })
        };

        let page_count = doc.page_count();
        // Lazy geometry loads exactly one page, because the first page's size is
        // what the viewer needs to lay out its first frame. The scroller
        // estimates the rest from it and corrects as pages arrive (PLAN §4).
        let pages: Vec<PageSize> = if lazy_geometry {
            match page_count {
                0 => Vec::new(),
                _ => vec![size_of(0)?],
            }
        } else {
            (0..page_count).map(size_of).collect::<Result<_, _>>()?
        };

        // Borrowed only to append. Pdfium is never called under this borrow:
        // `AGENTS.md` records a re-entrant call panicking a `RefCell` here.
        let mut docs = self.docs.borrow_mut();
        let id = docs.len() as u32;
        docs.push(Some(doc));
        drop(docs);
        // Distinct from `document parsed`: collecting page geometry walks every
        // page object, which on a long document is its own measurable cost.
        mark("document open complete");

        Ok(DocumentInfo {
            id,
            pages,
            page_count: page_count as usize,
            lazy_geometry,
            open_ms,
            at_ms: since_process_start_ms(),
        })
    }

    /// Claims a request, renders it, and releases it --- or drops it if it was
    /// withdrawn while queued.
    ///
    /// The claim is what makes a withdrawal unambiguous: it moves the request
    /// from queued to in flight under one lock, so a withdrawal arriving at any
    /// instant either finds it queued and marks it, or finds it running and
    /// cancels it.
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String> {
        let token = match self.queue.with(|queue| queue.claim(request.rid)) {
            Claim::Start(token) => token,
            Claim::Withdrawn => return Ok(TileOutcome::Abandoned),
        };

        let docs = self.docs.borrow();
        let result = open_slot(&docs, request.doc)
            .and_then(|doc| render_tile(self.bindings, doc, request, &token));
        drop(docs);
        self.queue.with(|queue| queue.release(request.rid));
        result
    }

    fn text(&self, doc: u32, page: u32) -> Result<PageText, String> {
        run_text(open_slot(&self.docs.borrow(), doc)?, page)
    }

    fn search(&self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String> {
        run_search(open_slot(&self.docs.borrow(), doc)?, page, query)
    }

    fn outline(&self, doc: u32) -> Result<Outline, String> {
        Ok(run_outline(open_slot(&self.docs.borrow(), doc)?))
    }

    /// Drops the document, which is what closes the Pdfium handle.
    fn close(&self, doc: u32) -> Result<(), String> {
        let mut docs = self.docs.borrow_mut();
        // Looked up first, so closing an id twice is an error rather than a
        // silent success. Unlike a withdrawal, a caller here *does* know what it
        // has open --- a second close is a caller that has lost track, and that
        // is worth saying rather than absorbing.
        open_slot_mut(&mut docs, doc)?;
        docs[doc as usize] = None;
        Ok(())
    }
}

/// Binds to the Pdfium dynamic library.
///
/// The binary must match the API version `pdfium-render` was built against ---
/// `pdfium_latest` currently means chromium/7881. A newer Pdfium is not
/// automatically compatible. See AGENTS.md.
fn bind_pdfium(library_dir: &Path) -> Result<&'static Pdfium, String> {
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }

    let path = Pdfium::pdfium_platform_library_name_at_path(library_dir);
    let bindings = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;

    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

/// Renders one tile of one document.
///
/// Takes the document rather than the table it lives in, so that the two
/// backends can disagree about how documents are stored --- a worker holds
/// exactly one and the app process holds a `Vec` with holes in it --- without
/// this needing to know.
pub(crate) fn render_tile(
    bindings: Bindings,
    doc: &RawDocument,
    req: &TileRequest,
    cancel: &CancelToken,
) -> Result<TileOutcome, String> {
    // Cached, because Pdfium re-parses a page on every `FPDF_LoadPage` --- 44 ms
    // on the A0 sheet, which loading per tile request would charge a six-tile
    // screenful nearly four times over.
    let page = doc.page(req.page)?;

    let spec = TileSpec {
        scale: req.scale,
        turns: req.turns,
        x: req.x,
        y: req.y,
        width: req.width,
        height: req.height,
    };

    let t0 = Instant::now();
    // No slice: the pause callback returns "stop" the moment the token is set,
    // so cancellation costs one poll interval either way, and slicing measured a
    // 1--2% overhead for nothing this path needs.
    let (rgba, progress) = progressive::render_tile(bindings, &page, spec, None, cancel)?;
    let render_us = t0.elapsed().as_micros() as u64;

    match progress.outcome {
        Outcome::Done => {}
        // The bitmap holds a genuine partial composite, but whether a partial
        // tile is worth putting on screen is not measured (AGENTS.md), so it is
        // dropped rather than shipped on a guess.
        Outcome::Cancelled => return Ok(TileOutcome::Abandoned),
        Outcome::Failed(status) => {
            return Err(format!("render failed with Pdfium status {status}"))
        }
    }

    // Before the encode, so PNG and raw ship the same pixels, and after the
    // cancellation check, so a tile that is about to be dropped is not paid for.
    let mut rgba = rgba;
    if req.invert {
        crate::invert::invert_lightness(&mut rgba);
    }

    let t1 = Instant::now();
    let bytes = match req.format {
        // Already RGBA and already in a `Vec` --- Pdfium rendered straight into
        // it. The safe path's `as_rgba_bytes()` allocated and copied a second
        // one, which at 2048² is 16 MB per tile.
        TileFormat::Raw => rgba,
        TileFormat::Png => encode_png(&rgba, req.width as u32, req.height as u32)?,
    };
    let encode_us = t1.elapsed().as_micros() as u64;
    mark("first tile rendered");

    Ok(TileOutcome::Rendered(Tile {
        bytes,
        width: req.width,
        height: req.height,
        format: req.format,
        render_us,
        encode_us,
    }))
}

pub(crate) fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Fast compression: this is a latency path, not an archival one.
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header failed: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("png encode failed: {e}"))?;
    }
    Ok(out)
}

/// Extracts one page's characters on the render thread.
pub(crate) fn run_text(document: &RawDocument, page: u32) -> Result<PageText, String> {
    text::extract(&document.page(page)?)
}

/// Searches one page on the render thread.
///
/// Extraction happens here and the characters are dropped again: only the
/// matches cross to the frontend. A 775-page document's text is tens of
/// megabytes as JSON and the frontend needs it only for the pages it draws, so
/// shipping it in order to search it would be the expensive half of a cheap
/// operation.
pub(crate) fn run_search(
    document: &RawDocument,
    page: u32,
    query: &str,
) -> Result<PageMatches, String> {
    Ok(search::search_page(&run_text(document, page)?, page, query))
}

/// Walks a document's outline on the render thread.
pub(crate) fn run_outline(document: &RawDocument) -> Outline {
    outline::read(document)
}

#[cfg(test)]
mod tests {
    use super::Backend;

    #[test]
    fn an_unset_backend_is_the_platform_default() {
        let expected = if cfg!(target_os = "macos") {
            Backend::Worker
        } else {
            Backend::InProcess
        };
        assert_eq!(Backend::parse(None), Ok(expected));
    }

    #[test]
    fn each_backend_can_be_asked_for_by_name() {
        // Both directions, because a parser that answered `Worker` to everything
        // would satisfy the default test above on this machine.
        assert_eq!(Backend::parse(Some("worker")), Ok(Backend::Worker));
        assert_eq!(Backend::parse(Some("in-process")), Ok(Backend::InProcess));
    }

    #[test]
    fn a_backend_that_is_not_a_backend_is_refused_rather_than_defaulted() {
        // The failure this is here for is silent: `in_process` for `in-process`
        // would fall back to the default, which on macOS is the *other* one ---
        // so a comparison between the two would be a worker against a worker,
        // reporting a perfect match for the wrong reason.
        for name in ["in_process", "inprocess", "workers", ""] {
            assert!(
                Backend::parse(Some(name)).is_err(),
                "{name:?} was accepted as a backend"
            );
        }
    }
}
