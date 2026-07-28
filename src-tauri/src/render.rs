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
//! Three properties are worth keeping in mind when changing anything here.
//!
//! - **Growth is lazy.** A document opens with one worker and gains another only
//!   when a request arrives while the first is busy. A reader turning one page
//!   at a time never pays for a second parse of the document, which is what
//!   makes the pool affordable at 7.8--48.2 MB per worker.
//! - **Dequeue order is still FIFO**, because there is still one channel. Only
//!   *execution* overlaps. That is what lets [`Workers::close`] stay correct by
//!   draining rather than by taking a lock over the whole service.
//! - **No lock is held across a render.** Every critical section here is pool
//!   bookkeeping; the render happens in another process entirely.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

use pdfium_render::prelude::*;

use crate::outline::{self, Outline};
use crate::progressive::{self, Bindings, CancelToken, Outcome, RawDocument, TileSpec};
use crate::queue::{Claim, SharedQueue};
use crate::search::{self, PageMatches};
use crate::startup::{mark, since_process_start_ms};
use crate::text::{self, PageText};
use crate::worker::{Request, Response, Shm, Worker, WorkerSender};

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

type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

/// Looks up an open document, distinguishing an id that never existed from one
/// that has been closed.
///
/// Both backends keep their documents in a `Vec` whose index *is* the id, and
/// a closed document leaves a hole rather than being removed. Removing it would
/// shift every id after it, so a request already in flight would silently be
/// answered from the wrong document --- which is a far worse failure than an
/// error, because it looks like a rendering bug.
fn open_slot<T>(slots: &[Option<T>], doc: u32) -> Result<&T, String> {
    match slots.get(doc as usize) {
        Some(Some(value)) => Ok(value),
        Some(None) => Err(not_open(doc, true)),
        None => Err(not_open(doc, false)),
    }
}

/// As [`open_slot`], for a caller that needs to mutate what it finds.
fn open_slot_mut<T>(slots: &mut [Option<T>], doc: u32) -> Result<&mut T, String> {
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
fn not_open(doc: u32, in_range: bool) -> String {
    if in_range {
        format!("document {doc} has been closed")
    } else {
        format!("no such document: {doc}")
    }
}

enum Job {
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
                let engine = Arc::new(Workers::new(library_dir, queue.clone(), pool));
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
        self.workers.as_ref().map_or(1, |w| w.capacity)
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
    /// this side.
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
trait Engine {
    fn open(&self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String>;
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String>;
    fn text(&self, doc: u32, page: u32) -> Result<PageText, String>;
    fn search(&self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String>;
    fn outline(&self, doc: u32) -> Result<Outline, String>;
    fn close(&self, doc: u32) -> Result<(), String>;
}

/// Serves one job and answers it.
fn dispatch(job: Job, engine: &dyn Engine) {
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

/// Serves jobs from `threads` threads sharing one receiver.
///
/// The receiver is behind a mutex and each thread holds it only across `recv`,
/// so a thread that has taken a job releases it before doing any work. What that
/// buys is the whole point of the pool: several tiles of the same document are
/// rendered at once, in different processes.
///
/// **Dequeue order is still FIFO** --- one channel, one queue --- and only
/// *execution* overlaps. That is what keeps `close` correct without a lock of its
/// own: a close is taken off the queue after everything queued before it, and
/// drains whatever is still running (see [`Workers::close`]).
/// Returns as soon as the threads are running --- it does not join them. They
/// are detached exactly as the single render thread always was: they end when
/// the last `RenderService` handle is dropped and the channel closes.
fn serve_pooled(rx: Receiver<Job>, engine: Arc<Workers>, threads: usize) {
    let rx = Arc::new(Mutex::new(rx));

    for index in 0..threads {
        let rx = rx.clone();
        let engine = engine.clone();
        std::thread::Builder::new()
            .name(format!("tpdf-render-{index}"))
            .spawn(move || loop {
                let job = {
                    let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                    guard.recv()
                };
                match job {
                    Ok(job) => dispatch(job, engine.as_ref()),
                    // Every sender is gone: the service was dropped.
                    Err(_) => break,
                }
            })
            .expect("failed to spawn a render thread");
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

// -------------------------------------------------------------------- workers

/// How many workers one document may have, unless `TPDF_POOL` says otherwise.
///
/// Six, because that is where the curve flattens on the workload a viewport
/// actually issues. Measured through this service by `bin/pool_bench.rs`, six
/// 1024-square tiles of the A0 sheet, interleaved across rounds (4P+6E machine):
///
/// | workers | 1 | 2 | 4 | 6 | 8 |
/// |---|---|---|---|---|---|
/// | screenful | 3457--3465 ms | 1800--1868 ms | 1263--1299 ms | 830--837 ms | 843--851 ms |
/// | speedup | 1.00x | 1.92--1.94x | 2.67--2.93x | 4.15--4.18x | 4.07--4.12x |
///
/// Past six there is nothing --- eight is *slower*, by less than the spread, so
/// read it as flat rather than as a cost. Note six is neither the core count
/// (10) nor the performance-core count (4): it is where this workload
/// saturates, and `AGENTS.md` records the earlier mistake of carrying a pool
/// size over from a different one.
///
/// Two runs are quoted as a range rather than one as a number, because the
/// four-worker figure moved 2.67--2.93x between them while six moved 0.03x. A
/// single run would have made that look like a measurement.
///
/// The cost of the number is not CPU: every worker holds its own parse of the
/// document, which `worker-probe` measured at 7.8--48.2 MB depending on the
/// corpus, so a fully grown pool on the A0 sheet is about 290 MB. What makes
/// that affordable is that growth is lazy --- a reader turning one page at a
/// time never has more than one worker. What makes it a real cost is that
/// nothing retires an idle one afterwards.
pub const DEFAULT_POOL: usize = 6;

/// How many threads serve the job queue, for a given per-document pool.
///
/// **More than the pool, deliberately, and it took a mutation to see why.** With
/// one thread per worker the two bounds in [`Workers::checkout`] --- the capacity
/// ceiling and the wait for a free worker --- are both unreachable: `idle` can
/// only be empty when every worker is checked out, which needs one thread each,
/// so a thread arriving to find none free cannot exist. A mutation removing the
/// ceiling entirely survived every check, because the thread count was silently
/// doing the ceiling's job.
///
/// The spare threads are not there to satisfy a test, though. They are what
/// stops one document starving another: with exactly `pool` threads, six tiles
/// of a slow document occupy every one of them, and a request for a *different*
/// document waits behind a render even though its own workers are idle.
fn service_threads(pool: usize) -> usize {
    pool + 2
}

/// How many workers a document may have.
///
/// # Panics
///
/// Never: an unreadable `TPDF_POOL` falls back to the default rather than
/// refusing, because unlike `TPDF_BACKEND` a wrong value here cannot make two
/// measurements silently incomparable --- the size is reported in every place
/// that reports a speedup.
#[must_use]
pub fn pool_size() -> usize {
    std::env::var("TPDF_POOL")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_POOL)
}

/// One open document: its bytes, and the pool of processes parsing them.
struct Held {
    /// The document mapping, owned here rather than by any one worker so that
    /// every worker in the pool --- and every replacement for a dead one --- is
    /// handed the same bytes. See [`Worker::spawn_shared`].
    doc: Arc<Shm>,
    /// Workers not currently serving a request.
    idle: Vec<Worker>,
    /// How many exist at all, idle or checked out. Not `idle.len()`: that would
    /// grow the pool again every time one was busy.
    spawned: usize,
    /// Every live worker's write half, by pid.
    ///
    /// Kept here rather than read off `idle`, because the worker that most needs
    /// a withdrawal is precisely the one that is **checked out** --- it is the
    /// one inside Pdfium. Removed on discard, since the entry holds a clone of
    /// the child's stdin and a stale one is a leaked descriptor.
    senders: Vec<(u32, WorkerSender)>,
}

/// Documents parsed in sandboxed child processes, several per document.
///
/// Shared across the service's threads, which is what makes the pool a pool:
/// each thread takes a job, checks a worker out, and renders in that process
/// while the others do the same. Everything here is short critical sections ---
/// no lock is ever held across a render.
struct Workers {
    library_dir: PathBuf,
    /// Indexed by document id, with a hole where one has been closed. See
    /// [`open_slot`].
    docs: Mutex<Vec<Option<Held>>>,
    /// Signalled when a worker returns to a pool, is discarded, or fails to
    /// spawn --- i.e. whenever waiting for one might have become worthwhile.
    returned: Condvar,
    /// The most workers any one document may have.
    capacity: usize,
    queue: SharedQueue,
}

impl Workers {
    fn new(library_dir: PathBuf, queue: SharedQueue, capacity: usize) -> Self {
        Self {
            library_dir,
            docs: Mutex::new(Vec::new()),
            returned: Condvar::new(),
            capacity,
            queue,
        }
    }

    /// The document table. Poisoning is recovered from rather than propagated:
    /// a panic in one job must not take every open document with it.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Option<Held>>> {
        self.docs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes a worker out of a document's pool, growing or waiting as needed.
    ///
    /// Growth is **lazy**, and that is the whole reason a pool is affordable: a
    /// reader turning one page at a time never has more than one worker, and so
    /// never pays for more than one parse of the document. A second appears only
    /// when a second request arrives while the first is still rendering --- which
    /// is exactly the case a pool is for.
    fn checkout(&self, doc: u32) -> Result<Worker, String> {
        let mut docs = self.lock();
        loop {
            let held = open_slot_mut(&mut docs, doc)?;
            if let Some(worker) = held.idle.pop() {
                return Ok(worker);
            }
            if held.spawned < self.capacity {
                // The reservation is taken *before* the lock is released, so two
                // threads arriving together cannot both decide there is room for
                // the last worker.
                held.spawned += 1;
                let bytes = held.doc.clone();
                drop(docs);
                return self.spawn_into(doc, bytes);
            }
            // At capacity and all of them busy. Waiting is right rather than
            // queueing another request: this thread has nothing else to do, and
            // the caller's tile cannot start until a process is free anyway.
            docs = self.returned.wait(docs).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Spawns a worker against a reservation already taken by [`checkout`].
    fn spawn_into(&self, doc: u32, bytes: Arc<Shm>) -> Result<Worker, String> {
        // Outside the lock: a spawn is ~12 ms, and holding the table for that
        // would stall every other document as well as this one's other threads.
        let worker = match Worker::spawn_shared(bytes, &self.library_dir) {
            Ok(worker) => worker,
            Err(e) => {
                // Give the reservation back, or the pool shrinks by one every
                // time a spawn fails and eventually deadlocks at zero.
                let mut docs = self.lock();
                if let Ok(held) = open_slot_mut(&mut docs, doc) {
                    held.spawned = held.spawned.saturating_sub(1);
                }
                drop(docs);
                self.returned.notify_all();
                return Err(e);
            }
        };

        let mut docs = self.lock();
        let Ok(held) = open_slot_mut(&mut docs, doc) else {
            // Closed while this was spawning. Dropping the worker kills it,
            // which is what the close would have done.
            return Err(not_open(doc, true));
        };
        held.senders.push((worker.pid(), worker.sender()));
        Ok(worker)
    }

    /// Returns a worker to its pool.
    fn checkin(&self, doc: u32, worker: Worker) {
        let mut docs = self.lock();
        match open_slot_mut(&mut docs, doc) {
            Ok(held) => held.idle.push(worker),
            // The document was closed while this worker was out. Dropping it
            // kills it --- and `close` is waiting for exactly this, so the
            // notify below is what lets it finish.
            Err(_) => drop(worker),
        }
        drop(docs);
        self.returned.notify_all();
    }

    /// Retires a worker rather than returning it, so a fresh one takes its slot.
    fn discard(&self, doc: u32, worker: Worker) {
        let pid = worker.pid();
        // Dropped first: `Worker`'s own `Drop` kills and reaps, and doing that
        // outside the lock keeps a dying child off the critical section.
        drop(worker);

        let mut docs = self.lock();
        if let Ok(held) = open_slot_mut(&mut docs, doc) {
            held.spawned = held.spawned.saturating_sub(1);
            // The sender holds a clone of the child's stdin, so leaving it here
            // would keep the pipe open for the life of the service --- one
            // descriptor per worker that ever died.
            held.senders.retain(|(other, _)| *other != pid);
        }
        drop(docs);
        self.returned.notify_all();
    }

    /// Sends a withdrawal to every worker of every open document.
    ///
    /// Broadcast rather than addressed, because a `rid` is unique for the life
    /// of the process and a worker that has never seen one ignores it. With a
    /// pool that is more useful than before rather than less: the parent does
    /// not know which of a document's workers took the request.
    fn broadcast_withdraw(&self, rid: u64) {
        let docs = self.lock();
        for held in docs.iter().flatten() {
            for (_, sender) in &held.senders {
                // A dead worker is not this call's problem: whichever thread is
                // holding it will report that, with an epitaph.
                let _ = sender.withdraw(rid);
            }
        }
    }

    /// Runs one exchange with one of a document's workers, replacing it if it
    /// has died.
    ///
    /// Retried exactly once, and the bound is the retry rather than a counter.
    /// A crash the document *causes* reproduces on the retry --- so the reader
    /// pays two crashes for that tile and gets an error, and the next request
    /// pays one more. That is bounded by the requests the reader makes, which is
    /// what makes a restart budget on top of it unreachable defence: there is no
    /// loop here for one to break.
    ///
    /// The trade it does make is that a death caused by the *previous* request,
    /// or by anything outside the document at all, is invisible to the caller.
    /// That is the point of restarting.
    fn with_worker<T>(
        &self,
        doc: u32,
        exchange: impl Fn(&mut Worker) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut worker = self.checkout(doc)?;

        let error = match exchange(&mut worker) {
            Ok(value) => {
                self.checkin(doc, worker);
                return Ok(value);
            }
            Err(e) => e,
        };

        // Only a *dead* worker is worth replacing. A live one that answered with
        // an error answered: restarting on that would hide a bug here behind a
        // process that gets the next question right, and would cost a document
        // reopen per malformed request.
        if worker.is_running() {
            self.checkin(doc, worker);
            return Err(error);
        }

        // Said out loud, once, because a successful retry makes the death
        // invisible to the caller and a worker that dies quietly is the hardest
        // thing in this design to diagnose.
        eprintln!(
            "[render] document {doc}: worker {}; starting a replacement",
            worker.epitaph()
        );
        self.discard(doc, worker);

        // The discard freed a slot, so this checkout spawns rather than waits.
        let mut replacement = self.checkout(doc).map_err(|e| format!("{error} --- {e}"))?;
        let second = exchange(&mut replacement);
        self.checkin(doc, replacement);
        second
    }

    /// Sends a request that answers with JSON, and reads the answer back.
    fn ask<T: serde::de::DeserializeOwned>(
        &self,
        doc: u32,
        request: &Request,
    ) -> Result<T, String> {
        self.with_worker(doc, |worker| {
            let response = worker.call(request)?;
            if !response.ok {
                return Err(response.error);
            }
            let json = response.json.ok_or("worker replied without a payload")?;
            serde_json::from_value(json).map_err(|e| format!("unreadable reply from a worker: {e}"))
        })
    }

    /// Renders through a worker, having already claimed the request.
    fn render(&self, request: &TileRequest, token: &CancelToken) -> Result<TileOutcome, String> {
        self.with_worker(request.doc, |worker| {
            let response = worker.call(&Request::Tile {
                rid: request.rid,
                page: request.page,
                scale: request.scale,
                turns: request.turns,
                invert: request.invert,
                x: request.x,
                y: request.y,
                width: request.width,
                height: request.height,
                png: request.format == TileFormat::Png,
            })?;

            if !response.ok {
                return Err(response.error);
            }
            if response.abandoned {
                return Ok(TileOutcome::Abandoned);
            }
            // The withdrawal that lost its race to the pipe. The worker rendered
            // the tile because the `Withdraw` arrived before the request it
            // names, and the caller stopped wanting it regardless --- so this
            // side's token, not the worker's answer, is what decides.
            if token.is_cancelled() {
                return Ok(TileOutcome::Abandoned);
            }

            let length = payload_length(&response, request, worker.tile.len())?;
            let bytes = worker.tile.as_slice()[..length].to_vec();
            mark("first tile rendered");

            Ok(TileOutcome::Rendered(Tile {
                bytes,
                width: request.width,
                height: request.height,
                format: request.format,
                render_us: response.render_us,
                encode_us: response.encode_us,
            }))
        })
    }
}

/// How many bytes of the shared mapping a reply is entitled to.
///
/// The worker is our code, and it is also the process holding the attacker's
/// document --- so its replies are the one thing crossing back out of the blast
/// radius, and a length it states is a claim rather than a fact. Reading past
/// the mapping on a claim of 4 GB would be the boundary handing over the
/// authority it exists to withhold.
///
/// For raw pixels the answer is arithmetic rather than a bound: a tile is
/// exactly `width x height x 4` bytes, so anything else is wrong even when it
/// fits. PNG has no such closed form and gets the mapping's size.
fn payload_length(
    response: &Response,
    request: &TileRequest,
    capacity: usize,
) -> Result<usize, String> {
    let stated = response.bytes;
    if stated > capacity {
        return Err(format!(
            "worker claims a {stated}-byte tile and the shared mapping holds {capacity}"
        ));
    }
    if request.format == TileFormat::Raw {
        let expected = request.width as usize * request.height as usize * 4;
        if stated != expected {
            return Err(format!(
                "worker returned {stated} bytes for a {}x{} raw tile, which is {expected}",
                request.width, request.height
            ));
        }
    }
    Ok(stated)
}

impl Engine for Workers {
    /// Spawns the document's first worker and asks it for the geometry.
    ///
    /// One, not `capacity`: the pool grows only under contention, so a document
    /// that is opened and read one page at a time costs exactly one process. The
    /// spawn is on the critical path to the first page and what it costs is
    /// measured rather than assumed --- see PLAN §9.
    fn open(&self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String> {
        let t0 = Instant::now();
        // Mapped here rather than inside the spawn, because this is the copy
        // every later worker for this document will be handed. The file is read
        // once, at open, and never again.
        let doc = Arc::new(Shm::map_file(path)?);
        let mut worker = Worker::spawn_shared(doc.clone(), &self.library_dir)?;
        mark("worker spawned");

        let response = worker.call(&Request::Open { lazy_geometry })?;
        if !response.ok {
            return Err(response.error);
        }
        let json = response.json.ok_or("worker opened without a payload")?;
        let opened: OpenReply = serde_json::from_value(json)
            .map_err(|e| format!("unreadable open reply from a worker: {e}"))?;
        let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
        mark("document parsed");

        let mut docs = self.lock();
        let id = docs.len() as u32;
        docs.push(Some(Held {
            doc,
            senders: vec![(worker.pid(), worker.sender())],
            spawned: 1,
            idle: vec![worker],
        }));
        drop(docs);
        mark("document open complete");

        Ok(DocumentInfo {
            id,
            pages: opened.pages,
            page_count: opened.page_count,
            lazy_geometry: opened.lazy_geometry,
            open_ms,
            at_ms: since_process_start_ms(),
        })
    }

    /// Claims the request here, then renders it there.
    ///
    /// Two queues, and each catches what the other cannot: this one drops a
    /// request that was withdrawn before it ever reached a worker, and the
    /// worker's own reaches a render already inside Pdfium. See
    /// [`RenderService::cancel`].
    ///
    /// The claim is taken before the checkout on purpose. A request withdrawn
    /// while it is waiting for a free worker should not then occupy one.
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String> {
        let token = match self.queue.with(|queue| queue.claim(request.rid)) {
            Claim::Start(token) => token,
            Claim::Withdrawn => return Ok(TileOutcome::Abandoned),
        };

        let result = self.render(request, &token);
        self.queue.with(|queue| queue.release(request.rid));
        result
    }

    fn text(&self, doc: u32, page: u32) -> Result<PageText, String> {
        self.ask(doc, &Request::Text { page })
    }

    fn search(&self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String> {
        self.ask(
            doc,
            &Request::Search {
                page,
                query: query.to_string(),
            },
        )
    }

    fn outline(&self, doc: u32) -> Result<Outline, String> {
        self.ask(doc, &Request::Outline)
    }

    /// Drops the document, which kills every process holding it.
    ///
    /// **It waits for the pool to come home first**, and that wait is what keeps
    /// the guarantee the single-threaded version got for free. Dequeue order is
    /// still FIFO, so a close is taken off the queue after every request made
    /// before it --- but with several threads those requests may still be
    /// *running*, in workers this is about to kill. Draining first means a
    /// request never loses its worker mid-render.
    ///
    /// No goodbye on the wire. `Worker`'s own `Drop` kills and reaps, and a
    /// request to exit cleanly would be a message the one process in this design
    /// that is *assumed hostile* gets to ignore --- the reader would then wait on
    /// a shutdown that never comes.
    fn close(&self, doc: u32) -> Result<(), String> {
        let mut docs = self.lock();
        loop {
            // Looked up every time round, and inside the loop, because a second
            // close of the same id must be an error rather than a wait that
            // never ends. Unlike a withdrawal, a caller here *does* know what it
            // has open.
            let held = open_slot_mut(&mut docs, doc)?;
            if held.idle.len() >= held.spawned {
                break;
            }
            docs = self.returned.wait(docs).unwrap_or_else(|e| e.into_inner());
        }
        docs[doc as usize] = None;
        Ok(())
    }
}

/// What a worker answers [`Request::Open`] with.
///
/// A struct rather than poking at the `serde_json::Value`, so a field the worker
/// stops sending is a deserialisation error here instead of a zero somewhere
/// downstream.
#[derive(serde::Deserialize)]
struct OpenReply {
    pages: Vec<PageSize>,
    page_count: usize,
    lazy_geometry: bool,
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
    use super::{payload_length, Backend, Response, TileFormat, TileRequest};

    /// A tile request of a given size and format, with nothing else meaningful.
    fn request(width: u16, height: u16, format: TileFormat) -> TileRequest {
        TileRequest {
            rid: 1,
            doc: 0,
            page: 0,
            scale: 1.0,
            turns: 0,
            invert: false,
            x: 0,
            y: 0,
            width,
            height,
            format,
        }
    }

    /// A successful reply claiming `bytes` of payload.
    fn reply(bytes: usize) -> Response {
        Response {
            ok: true,
            bytes,
            ..Default::default()
        }
    }

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

    #[test]
    fn a_raw_tile_of_exactly_the_right_size_is_accepted() {
        // The control: without it every check below passes on a function that
        // refuses everything.
        let req = request(64, 32, TileFormat::Raw);
        assert_eq!(
            payload_length(&reply(64 * 32 * 4), &req, crate::worker::TILE_CAPACITY),
            Ok(64 * 32 * 4)
        );
    }

    #[test]
    fn a_raw_tile_of_the_wrong_size_is_refused_even_though_it_fits() {
        // Well inside the mapping on purpose, so only the arithmetic can catch
        // it --- if this went over capacity too, deleting the arithmetic would
        // still leave the test green and the check unpinned.
        let req = request(64, 32, TileFormat::Raw);
        for stated in [0, 64 * 32 * 4 - 1, 64 * 32 * 4 + 1] {
            assert!(
                payload_length(&reply(stated), &req, crate::worker::TILE_CAPACITY).is_err(),
                "{stated} bytes was accepted for a 64x32 raw tile"
            );
        }
    }

    #[test]
    fn a_payload_larger_than_the_mapping_is_refused() {
        // PNG, which has no expected length, so the capacity bound is the only
        // thing standing between a worker's claim and a read past the mapping.
        let req = request(64, 32, TileFormat::Png);
        assert!(payload_length(&reply(4097), &req, 4096).is_err());
        // And the control, since a compressed tile is legitimately any size.
        assert_eq!(payload_length(&reply(4096), &req, 4096), Ok(4096));
    }
}
