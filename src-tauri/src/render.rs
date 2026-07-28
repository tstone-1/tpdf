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
//! This is still one FIFO thread. A worker *pool* is what buys parallelism, and
//! it is not this change: spike 0.5 measured 3.9x on four workers across
//! documents but only 3.2x on six tiles of one A0 page, which is the workload a
//! viewport actually asks for.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
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

/// Every live worker's write half, for broadcasting a withdrawal.
type Senders = Arc<Mutex<Vec<WorkerSender>>>;

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
}

/// Handle to the render thread. Cheap to clone.
#[derive(Clone)]
pub struct RenderService {
    tx: Sender<Job>,
    /// Which requests are outstanding and which have been withdrawn. See
    /// `queue.rs`, which is where that state machine lives and is tested.
    queue: SharedQueue,
    /// Empty in-process. One entry per open document in worker mode, appended
    /// by the render thread and read by whichever thread withdraws.
    senders: Senders,
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
        let (tx, rx) = channel::<Job>();
        let queue = SharedQueue::default();
        let thread_queue = queue.clone();
        let senders: Senders = Senders::default();
        let thread_senders = senders.clone();

        std::thread::Builder::new()
            .name("tpdf-render".into())
            .spawn(move || match backend {
                Backend::InProcess => match InProcess::start(&library_dir, thread_queue) {
                    Ok(mut engine) => serve(rx, &mut engine),
                    // Drain the queue, failing every job with the bind error, so
                    // callers get a diagnosable message instead of a hang.
                    Err(e) => drain(rx, &e),
                },
                Backend::Worker => {
                    let mut engine = Workers::new(library_dir, thread_queue, thread_senders);
                    serve(rx, &mut engine);
                }
            })
            .expect("failed to spawn render thread");

        Self {
            tx,
            queue,
            senders,
            backend,
        }
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

        let senders = self.senders.lock().unwrap_or_else(|e| e.into_inner());
        for sender in senders.iter() {
            // A dead worker is not this call's problem: the render thread's own
            // reply will report it, with an epitaph this path cannot produce.
            let _ = sender.withdraw(rid);
        }
    }
}

/// What a backend has to be able to do.
///
/// One method per job, so the dispatch loop below is written once and neither
/// backend can quietly grow a job the other does not serve. Every method takes
/// `&mut self` and runs on the render thread: an in-process `RawDocument` is not
/// `Send`, and a worker's stdout has exactly one reader.
trait Engine {
    fn open(&mut self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String>;
    fn tile(&mut self, request: &TileRequest) -> Result<TileOutcome, String>;
    fn text(&mut self, doc: u32, page: u32) -> Result<PageText, String>;
    fn search(&mut self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String>;
    fn outline(&mut self, doc: u32) -> Result<Outline, String>;
}

/// Serves jobs until every handle to the service is dropped.
fn serve(rx: Receiver<Job>, engine: &mut dyn Engine) {
    for job in rx {
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
        }
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
        }
    }
}

// ---------------------------------------------------------------- in-process

/// Documents parsed in this process, on the render thread.
struct InProcess {
    bindings: Bindings,
    docs: Vec<RawDocument>,
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
            docs: Vec::new(),
            queue,
        })
    }
}

impl Engine for InProcess {
    fn open(&mut self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String> {
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

        let id = self.docs.len() as u32;
        self.docs.push(doc);
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
    fn tile(&mut self, request: &TileRequest) -> Result<TileOutcome, String> {
        let token = match self.queue.with(|queue| queue.claim(request.rid)) {
            Claim::Start(token) => token,
            Claim::Withdrawn => return Ok(TileOutcome::Abandoned),
        };

        let result = render_tile(self.bindings, &self.docs, request, &token);
        self.queue.with(|queue| queue.release(request.rid));
        result
    }

    fn text(&mut self, doc: u32, page: u32) -> Result<PageText, String> {
        run_text(&self.docs, doc, page)
    }

    fn search(&mut self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String> {
        run_search(&self.docs, doc, page, query)
    }

    fn outline(&mut self, doc: u32) -> Result<Outline, String> {
        run_outline(&self.docs, doc)
    }
}

// -------------------------------------------------------------------- workers

/// One open document: the bytes, and whichever process is currently holding
/// them.
struct Held {
    /// The document mapping, owned here rather than by the worker so that a
    /// replacement is handed the same bytes. See [`Worker::spawn_shared`].
    doc: Arc<Shm>,
    /// The worker serving it, `None` between a death and its replacement.
    worker: Option<Worker>,
}

/// Documents parsed in sandboxed child processes, one per document.
struct Workers {
    library_dir: PathBuf,
    docs: Vec<Held>,
    senders: Senders,
    queue: SharedQueue,
}

impl Workers {
    fn new(library_dir: PathBuf, queue: SharedQueue, senders: Senders) -> Self {
        Self {
            library_dir,
            docs: Vec::new(),
            senders,
            queue,
        }
    }

    /// A document by the id [`Engine::open`] returned.
    fn held(&mut self, doc: u32) -> Result<&mut Held, String> {
        self.docs
            .get_mut(doc as usize)
            .ok_or_else(|| format!("no such document: {doc}"))
    }

    /// Replaces a document's worker with a fresh one holding the same bytes.
    ///
    /// There is no reopening protocol to run: the worker parses its document
    /// during its own startup, before it reads a single request, so a
    /// replacement is ready as soon as it is spawned. That is a property of "one
    /// worker serves one document" rather than a convenience --- a multiplexing
    /// worker would have a session to re-establish here.
    fn restart(&mut self, doc: u32) -> Result<(), String> {
        let index = doc as usize;
        let held = self.held(doc)?;
        // Said out loud, once, because a successful retry makes the death
        // invisible to the caller and a worker that dies quietly is the hardest
        // thing in this design to diagnose. The epitaph has to be read before
        // the child is dropped, since dropping it is what reaps it.
        let epitaph = held
            .worker
            .as_mut()
            .map_or_else(|| "already gone".to_string(), Worker::epitaph);
        // Dropped before the replacement is spawned, so no document ever holds
        // two 16 MB tile mappings at once. Not for the reaping, which happens
        // either way --- assigning over the old `Worker` would drop it too, just
        // later. Nothing pins this ordering, and it is a footprint choice rather
        // than a correctness one.
        held.worker = None;
        let bytes = held.doc.clone();
        eprintln!("[render] document {doc}: worker {epitaph}; starting a replacement");

        let worker = Worker::spawn_shared(bytes, &self.library_dir)?;
        // Overwritten in place: a withdrawal broadcast between the death and
        // here reaches nothing, which is harmless because the queue in the
        // parent has already recorded it and is what the caller sees.
        if let Some(slot) = self
            .senders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(index)
        {
            *slot = worker.sender();
        }
        self.held(doc)?.worker = Some(worker);
        Ok(())
    }

    /// A document's worker, starting a replacement if it is already known dead.
    fn live(&mut self, doc: u32) -> Result<&mut Worker, String> {
        if self.held(doc)?.worker.is_none() {
            self.restart(doc)?;
        }
        self.held(doc)?
            .worker
            .as_mut()
            .ok_or_else(|| format!("document {doc} has no worker"))
    }

    /// Whether a document's worker process is still there.
    fn running(&mut self, doc: u32) -> bool {
        self.docs
            .get_mut(doc as usize)
            .and_then(|held| held.worker.as_mut())
            .is_some_and(Worker::is_running)
    }

    /// Runs one exchange with a document's worker, replacing it if it has died.
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
        &mut self,
        doc: u32,
        mut exchange: impl FnMut(&mut Worker) -> Result<T, String>,
    ) -> Result<T, String> {
        // Scoped so the borrow of `self` ends before anything below touches it.
        let first = {
            let worker = self.live(doc)?;
            exchange(worker)
        };
        let error = match first {
            Ok(value) => return Ok(value),
            Err(e) => e,
        };

        if self.running(doc) {
            return Err(error);
        }
        self.restart(doc).map_err(|e| format!("{error} --- {e}"))?;

        let worker = self.live(doc)?;
        exchange(worker)
    }

    /// Sends a request that answers with JSON, and reads the answer back.
    fn ask<T: serde::de::DeserializeOwned>(
        &mut self,
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

    /// Renders through the worker, having already claimed the request.
    fn render(
        &mut self,
        request: &TileRequest,
        token: &CancelToken,
    ) -> Result<TileOutcome, String> {
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
    /// Spawns a worker holding this document and asks it for the geometry.
    ///
    /// The spawn is on this thread and therefore on the critical path to the
    /// first page. What it costs is measured rather than assumed --- see PLAN §9.
    fn open(&mut self, path: &Path, lazy_geometry: bool) -> Result<DocumentInfo, String> {
        let t0 = Instant::now();
        // Mapped here rather than inside the spawn, because this is the copy a
        // replacement worker will be handed if this one dies. The file is read
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

        let id = self.docs.len() as u32;
        self.senders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(worker.sender());
        self.docs.push(Held {
            doc,
            worker: Some(worker),
        });
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
    fn tile(&mut self, request: &TileRequest) -> Result<TileOutcome, String> {
        let token = match self.queue.with(|queue| queue.claim(request.rid)) {
            Claim::Start(token) => token,
            Claim::Withdrawn => return Ok(TileOutcome::Abandoned),
        };

        let result = self.render(request, &token);
        self.queue.with(|queue| queue.release(request.rid));
        result
    }

    fn text(&mut self, doc: u32, page: u32) -> Result<PageText, String> {
        self.ask(doc, &Request::Text { page })
    }

    fn search(&mut self, doc: u32, page: u32, query: &str) -> Result<PageMatches, String> {
        self.ask(
            doc,
            &Request::Search {
                page,
                query: query.to_string(),
            },
        )
    }

    fn outline(&mut self, doc: u32) -> Result<Outline, String> {
        self.ask(doc, &Request::Outline)
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

pub(crate) fn render_tile(
    bindings: Bindings,
    docs: &[RawDocument],
    req: &TileRequest,
    cancel: &CancelToken,
) -> Result<TileOutcome, String> {
    let doc = docs
        .get(req.doc as usize)
        .ok_or_else(|| format!("no such document: {}", req.doc))?;

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
pub(crate) fn run_text(docs: &[RawDocument], doc: u32, page: u32) -> Result<PageText, String> {
    let document = docs
        .get(doc as usize)
        .ok_or_else(|| format!("no such document: {doc}"))?;
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
    docs: &[RawDocument],
    doc: u32,
    page: u32,
    query: &str,
) -> Result<PageMatches, String> {
    Ok(search::search_page(
        &run_text(docs, doc, page)?,
        page,
        query,
    ))
}

/// Walks a document's outline on the render thread.
pub(crate) fn run_outline(docs: &[RawDocument], doc: u32) -> Result<Outline, String> {
    let document = docs
        .get(doc as usize)
        .ok_or_else(|| format!("no such document: {doc}"))?;
    Ok(outline::read(document))
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
