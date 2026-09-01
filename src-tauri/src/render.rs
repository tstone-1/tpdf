//! The render service: the owner of every open document and of the queue they
//! are served from.
//!
//! **Dequeue order is FIFO; execution overlaps.** There is one channel, so a job
//! comes off it after everything queued before it --- but on the shipping
//! backend that channel is served by `pool + 2` threads, each driving a
//! different worker *process*, so several tiles of the same page render at once.
//! The two halves of that sentence are load-bearing separately: the first is
//! what lets [`Workers::close`] stay correct by draining rather than by locking
//! the whole service, and the second is why replies arrive in **completion**
//! order and why anything reading them positionally is wrong.
//!
//! **The in-process backend is the one-thread special case**, and not by
//! preference. `pdfium-render`'s `thread_safe` feature does **not** serialize
//! Pdfium calls --- there is no mutex anywhere in the crate's native path, and
//! concurrent renders from two threads segfault on a complex page while merely
//! appearing to work on a simple one. So that backend is one thread whatever
//! pool size it is handed, and the only route to parallel rendering is the
//! process boundary. Measured by `examples/thread_probe.rs`; see AGENTS.md.
//!
//! ## Abandoning work
//!
//! A queue means tiles rendering for a viewport that has moved on, whatever is
//! serving it. Spike 0.8 measured what that costs: sustained
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
//! default on both platforms that have a boundary --- macOS and Windows --- is
//! [`Backend::Worker`]: every document is parsed in a contained child process,
//! because `docs/THREAT-MODEL.md` requires it and `AGENTS.md` records why it
//! cannot be a later hardening pass. Anywhere else the default is the
//! in-process path *with a warning it records*, for the reason
//! [`Backend::default_here`] gives at length. That path is kept, and not out of
//! sentiment --- it is the control the worker is compared against, and
//! `examples/backend_probe.rs` is that comparison.
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
//! Each document has a pool of processes the service threads draw from, grown
//! under contention and given back when the scrolling stops --- which is where
//! the overlapping execution at the top of this note actually comes from, and
//! why security and performance wanted the same architecture.
//!
//! All of that lives in [`crate::workers`]. What stays here is the service, the
//! [`Engine`] trait both backends implement, the dispatch written once against
//! it, and the in-process control the pool is measured against --- so this file
//! is about *what a render request is*, and that one is about where it runs.

use crate::document::OpenDocument;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

use crate::annots::Comments;
use crate::docinfo::Properties;
use crate::encoding::PageMapping;
use crate::links::Links;
use crate::outline::{self, Outline};
use crate::progressive::{self, Bindings, CancelToken, Outcome, TileSpec};
use crate::queue::{Claim, SharedQueue};
use crate::redact;
use crate::search::{self, PageMatches};
use crate::startup::{mark, since_process_start_ms};
use crate::text::{self, PageText};
use crate::workers::{
    call_deadline, reap_idle, serve_pooled, service_threads, watch_calls, Workers,
};

/// The pool's knobs, re-exported on the path they have always had.
///
/// `examples/pool_bench.rs` and `examples/backend_probe.rs` both call
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
    /// The page's crop box as the reader's edits have it, or `None` for the
    /// file's own.
    ///
    /// `[llx, lly, urx, ury]` in the page's own space, y upwards. Carried on
    /// every request rather than set once on the document, because a worker's
    /// pages are cached and shared: a crop is a property of *this* request, and
    /// a page holding the last one is the state `RawDocument::page` exists to
    /// make unreachable.
    pub crop: Option<[f32; 4]>,
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

/// Startup mark recorded when this process parses documents uncontained.
///
/// Named as a constant so a check can look for the same string the code writes.
/// `AGENTS.md` records two copies of a distinction drifting until a mutation of
/// one survived; a mark asserted by its spelling in two places is that shape.
///
/// **Recorded for either route to the uncontained backend**, and it did not
/// used to be: the announcement lived inside [`Backend::default_here`], so
/// `TPDF_BACKEND=in-process` on a platform that *has* a boundary switched it off
/// leaving no mark and no log line. That is the one route where the outcome ---
/// hostile input parsed in the app process --- is invisible from inside, which
/// is exactly where a record is worth having. The wording no longer says "no
/// worker on this platform", because that is now true of only one of the two
/// ways to get here.
pub const UNSANDBOXED_MARK: &str = "unsandboxed: documents parsed in this process";

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
        let chosen = Self::parse(std::env::var("TPDF_BACKEND").ok().as_deref())?;
        if chosen == Self::InProcess {
            announce_uncontained();
        }
        Ok(chosen)
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
    /// **Worker on both platforms that have a boundary**, which since 2026-07-29
    /// is macOS and Windows: `sandbox_init` SBPL there, a low-integrity token
    /// inside a job object here. Anywhere else `Worker::spawn` refuses, and
    /// defaulting to something that cannot start is worse than defaulting to the
    /// control.
    ///
    /// Windows was the fail-open path until this line changed, and the change is
    /// worth stating precisely because it is one word. The refusal in
    /// `Worker::spawn` was asserted by tests the whole time, but only a caller
    /// asking for `TPDF_BACKEND=worker` ever reached it --- the default never
    /// asked, so every real launch parsed hostile input in the app process with
    /// none of the containment `docs/THREAT-MODEL.md` requires.
    ///
    /// **The evidence is external, and the mark below is not it.** A milestone we
    /// record says what our code believes it did; `scripts/win_modules.py` reads
    /// the app process's module list from *outside* it while a document is open
    /// and asserts `pdfium.dll` is absent, with the module count beside it so a
    /// failed enumeration cannot read as containment. That check was run against
    /// this function *before* the flip and reported the parser mapped --- a
    /// control, and the reason the pass afterwards means anything.
    ///
    /// The remaining branch keeps the mark rather than becoming a refusal, for
    /// the reason it always had: refusing would make a platform useless rather
    /// than uncontained, and that is a product decision rather than a defect to
    /// fix in passing.
    fn default_here() -> Self {
        if cfg!(any(target_os = "macos", windows)) {
            Self::Worker
        } else {
            Self::InProcess
        }
    }
}

/// Records that this process parses documents uncontained.
///
/// Called from [`Backend::from_env`] rather than from
/// [`Backend::default_here`], so it covers **both** ways of arriving at the
/// uncontained backend --- the platform having no boundary, and an operator
/// asking for it by name. It used to sit in the second of those, which left the
/// asked-for route silent.
///
/// Deliberately not called from `parse`, which is the unit the tests drive:
/// marks and the log sink are process-global, so announcing there would make
/// `each_backend_can_be_asked_for_by_name` --- which asks for `in-process` ---
/// leave a mark that the test asserting the default's mark then reads, and the
/// pair would pass or fail on the order the harness happened to run them in.
/// The seam is the same one `diag::note_to` exists for, and the same reason.
///
/// Once per process: `from_env` is called per service, and a benchmark holding
/// several would otherwise print the same warning per construction and bury it
/// in its own repetition.
fn announce_uncontained() {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        mark(UNSANDBOXED_MARK);
        crate::diag::note(
            "[WARN] documents are parsed in the app process, uncontained --- either \
             this platform has no sandbox or TPDF_BACKEND asked for it. See BUILD.md.",
        );
    });
}

/// What a job is answered through, for any failure type.
///
/// Two failure types exist and there is one alias for both, rather than two
/// spellings of the same `Box<dyn FnOnce(..)>` that have to be kept in step.
pub(crate) type ReplyTo<T, E> = Box<dyn FnOnce(Result<T, E>) + Send>;

pub(crate) type Reply<T> = ReplyTo<T, String>;

/// A reply whose failure is more than prose.
///
/// Only [`Job::Open`] uses it, and only because one of its refusals is a
/// question rather than a verdict: a locked document is not a broken one, and
/// the difference has to survive the trip back to the reader. Every other job
/// fails one way and keeps [`Reply`].
pub(crate) type ReplyRefusal<T> = ReplyTo<T, progressive::Refusal>;

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
        /// The reader's password, when a previous attempt came back locked.
        ///
        /// Owned rather than borrowed because a job outlives its caller's frame,
        /// and dropped with the job: nothing here retains it, and the only place
        /// it is held for a document's lifetime is the pool, which needs it to
        /// build that document's later workers.
        password: Option<String>,
        reply: ReplyRefusal<DocumentInfo>,
    },
    Tile {
        request: TileRequest,
        reply: Reply<TileOutcome>,
    },
    Text {
        doc: u32,
        page: u32,
        crop: Option<[f32; 4]>,
        reply: Reply<PageText>,
    },
    Search {
        doc: u32,
        page: u32,
        query: String,
        options: search::Options,
        /// The previous page's tail, when the walk had one --- see
        /// `search::Carry`.
        carry: Option<search::Carry>,
        reply: Reply<PageMatches>,
    },
    Content {
        doc: u32,
        page: u32,
        reply: Reply<Option<[f64; 4]>>,
    },
    Geometry {
        doc: u32,
        page: u32,
        crop: Option<[f32; 4]>,
        reply: Reply<CropGeometry>,
    },
    CropBox {
        doc: u32,
        page: u32,
        rect: [f32; 4],
        reply: Reply<[f32; 4]>,
    },
    RedactPlans {
        doc: u32,
        page: u32,
        regions: Vec<[f32; 4]>,
        reply: Reply<Vec<redact::RegionPlan>>,
    },
    Outline {
        doc: u32,
        reply: Reply<Outline>,
    },
    Links {
        doc: u32,
        reply: Reply<Links>,
    },
    Comments {
        doc: u32,
        reply: Reply<Comments>,
    },
    Mapping {
        doc: u32,
        reply: Reply<Vec<PageMapping>>,
    },
    Properties {
        doc: u32,
        reply: Reply<Properties>,
    },
    Append {
        doc: u32,
        plan: crate::edits::Plan,
        reply: Reply<crate::save::Update>,
    },
    /// What password this document was opened with, for a caller that has to
    /// parse its bytes itself. See [`RenderService::password`].
    Password {
        doc: u32,
        reply: Reply<Option<String>>,
    },
    Close {
        doc: u32,
        reply: Reply<()>,
    },
    /// Close every open document, and say how many there were.
    ///
    /// **For a webview that has just started**, which by definition holds no
    /// document id. See [`RenderService::release_all`].
    ReleaseAll {
        reply: Reply<usize>,
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
                // The deadline is read here rather than taken as a parameter,
                // which is the opposite of the choice made for the pool size and
                // the idle timeout, and for the reason those two give: they are
                // parameters because a harness compares them against each other
                // in one process. Nothing compares two deadlines --- it is a
                // bound on pathological input, not a variant --- so `TPDF_CALL_MS`
                // is enough. It becomes a parameter the day something needs to
                // hold two services at different deadlines at once.
                let deadline = call_deadline();
                let engine = Arc::new(Workers::new(
                    library_dir,
                    queue.clone(),
                    pool,
                    idle_after,
                    deadline,
                ));
                // Immediately, and this is the point of the whole mechanism: the
                // link, the sandbox and the font walk happen while Tauri and
                // WebKit are still coming up -- ~250 ms of which none is ours --
                // rather than while a reader waits for a first page.
                engine.prewarm();
                reap_idle(&engine, idle_after);
                watch_calls(&engine, deadline);
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
    /// Exposed for `examples/backend_probe.rs`, and it earns its place twice. A spare
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

    /// Opens a document, invoking `reply` on a service thread when done.
    ///
    /// `lazy_geometry` skips collecting every page's size, which spike 0.2
    /// measured at 86 ms on a 775-page document --- the largest avoidable item
    /// in the startup budget. See PLAN §4.
    /// `password` is the reader's, when a previous attempt came back locked. It
    /// travels no further than the worker's stdin --- see `Request::Unlock`.
    pub fn open(
        &self,
        path: PathBuf,
        lazy_geometry: bool,
        password: Option<String>,
        reply: ReplyRefusal<DocumentInfo>,
    ) {
        if self
            .tx
            .send(Job::Open {
                path,
                lazy_geometry,
                password,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Requests a tile, invoking `reply` on a service thread when done.
    ///
    /// The reply runs on the thread that served the job deliberately: the
    /// protocol responder can be satisfied there directly, so no thread is
    /// spawned per request.
    ///
    /// **The reply is answered even when there is nobody to serve it.** Every
    /// other job here can be dropped on a failed send, because its reply closure
    /// owns a channel and dropping it wakes the caller's `recv` with an error.
    /// A tile's does not: it owns a `UriSchemeResponder`, and dropping one
    /// leaves the webview's `fetch` pending for as long as the page lives ---
    /// the frontend's own in-flight set never clears the entry, so that tile is
    /// never re-requested either. A shutdown would show as a viewport that
    /// stopped filling in rather than as an error.
    pub fn tile(&self, request: TileRequest, reply: Reply<TileOutcome>) {
        let rid = request.rid;
        self.queue.with(|queue| queue.enqueue(rid));

        if let Err(std::sync::mpsc::SendError(job)) = self.tx.send(Job::Tile { request, reply }) {
            // Forget the request rather than leaving it outstanding forever,
            // since nothing will ever dequeue it.
            self.queue.with(|queue| queue.forget(rid));
            if let Job::Tile { reply, .. } = job {
                reply(Err("the render service has stopped".to_string()));
            }
        }
    }

    /// Extracts one page's characters, invoking `reply` on a service thread.
    ///
    /// This shares the job queue with tiles rather than getting one of its own.
    /// In worker mode that costs a text request only the wait for a *free
    /// thread*, since one of `pool + 2` is usually idle and the extraction then
    /// runs in a worker beside whatever tiles are rendering. In-process it is
    /// the older, harder cost: one thread, so the request queues behind
    /// whatever tile is running --- up to a second on the A0 sheet --- and a
    /// second thread there would need a second `FPDF_DOCUMENT`, which is
    /// undefined behaviour (see AGENTS.md).
    pub fn text(&self, doc: u32, page: u32, crop: Option<[f32; 4]>, reply: Reply<PageText>) {
        if self
            .tx
            .send(Job::Text {
                doc,
                page,
                crop,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Searches one page, invoking `reply` on a service thread.
    ///
    /// One page per job, rather than one job for the document, and that is the
    /// whole design: a job that scanned 775 pages would hold a service thread
    /// for a second and a half --- the only one, in-process --- and it would
    /// hold the *worker* it was running in for as long either way, which is one
    /// of the six a document may have. At page granularity a search interleaves
    /// with rendering, and the caller stops asking to cancel it --- there is
    /// nothing to withdraw.
    pub fn search(
        &self,
        doc: u32,
        page: u32,
        query: String,
        options: search::Options,
        carry: Option<search::Carry>,
        reply: Reply<PageMatches>,
    ) {
        if self
            .tx
            .send(Job::Search {
                doc,
                page,
                query,
                options,
                carry,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Measures a page's content box, invoking `reply` on a service thread.
    ///
    /// One job per page, and it costs a render --- the reader asked for this
    /// page, so measuring the document would be measuring 774 pages nobody is
    /// looking at.
    pub fn content(&self, doc: u32, page: u32, reply: Reply<Option<[f64; 4]>>) {
        if self.tx.send(Job::Content { doc, page, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reports a page's displayed size under a crop, on a service thread.
    pub fn geometry(
        &self,
        doc: u32,
        page: u32,
        crop: Option<[f32; 4]>,
        reply: Reply<CropGeometry>,
    ) {
        if self
            .tx
            .send(Job::Geometry {
                doc,
                page,
                crop,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Maps a rectangle a reader dragged into a crop box, on a service thread.
    ///
    /// The inverse of [`Service::geometry`] and asked for once per gesture, not
    /// once per frame: the preview a reader watches is drawn in the space they
    /// are dragging in, and only the committed rectangle needs the page's turn.
    pub fn crop_box(&self, doc: u32, page: u32, rect: [f32; 4], reply: Reply<[f32; 4]>) {
        if self
            .tx
            .send(Job::CropBox {
                doc,
                page,
                rect,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Asks what removing each of one page's regions would take, on a service thread.
    ///
    /// Asked when a reader opens the redactions panel and after an edit that
    /// changes it, never on the path a page is drawn on: it costs a page load
    /// and an object walk, and the pool that answers it is the pool drawing
    /// tiles.
    pub fn redaction_plans(
        &self,
        doc: u32,
        page: u32,
        regions: Vec<[f32; 4]>,
        reply: Reply<Vec<redact::RegionPlan>>,
    ) {
        if self
            .tx
            .send(Job::RedactPlans {
                doc,
                page,
                regions,
                reply,
            })
            .is_err()
        {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reads a document's outline, invoking `reply` on a service thread.
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

    /// Reads every comment in a document, invoking `reply` on a service thread.
    ///
    /// One job for the whole document rather than one per page, which is the
    /// same choice `outline` makes and for a stronger reason: the answer comes
    /// from a single `lopdf` parse, so asking per page would repeat that parse
    /// once per page to produce a slice of the same list. See `crate::annots`.
    pub fn comments(&self, doc: u32, reply: Reply<Comments>) {
        if self.tx.send(Job::Comments { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reads every link in a document, invoking `reply` on a service thread.
    ///
    /// One job for the whole document, for the same reason `comments` is: both
    /// come out of one `lopdf` parse, so a per-page request would repeat that
    /// parse once per page to return a slice of the same list.
    pub fn links(&self, doc: u32, reply: Reply<Links>) {
        if self.tx.send(Job::Links { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reads what a document says about itself, invoking `reply` on a service
    /// thread.
    ///
    /// One job for the whole document, like `comments` and `links`, and out of
    /// the same single `lopdf` parse. See `crate::docinfo`.
    pub fn properties(&self, doc: u32, reply: Reply<Properties>) {
        if self.tx.send(Job::Properties { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Reports, per page, whether the text means anything, on a service thread.
    ///
    /// Asked for lazily --- see [`crate::encoding`] and `RawDocument::mapping`.
    /// The reply is always exactly one entry per page, and a page nobody could
    /// judge comes back `truncated` rather than clean.
    pub fn mapping(&self, doc: u32, reply: Reply<Vec<PageMapping>>) {
        if self.tx.send(Job::Mapping { doc, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// Builds the update section for a save that only adds marks, on a service
    /// thread.
    ///
    /// **The only request whose answer becomes a file**, and the reason it is a
    /// request at all is that building it is a parse of attacker-controlled
    /// bytes: doing it here puts it in the worker that already holds this
    /// document, under the same sandbox, deadline and restart as every render.
    /// What comes back is bytes and two numbers; every decision about the file
    /// on disk stays with the caller. See `save::append_update` and
    /// `docs/THREAT-MODEL.md` residual risk 18.
    ///
    /// Asked for **before** the document is closed, which is not a preference:
    /// the worker builds this from the document it has mapped, so there is no
    /// document to build it from afterwards. `save_document` in `lib.rs` already
    /// had that order, for the unrelated reason that a rename over a mapped file
    /// leaves the mapping serving the old inode.
    pub fn append(&self, doc: u32, plan: crate::edits::Plan, reply: Reply<crate::save::Update>) {
        if self.tx.send(Job::Append { doc, plan, reply }).is_err() {
            // Render thread is gone; nothing left to reply with.
        }
    }

    /// What password this document was opened with, if it needed one.
    ///
    /// **Asked for, rather than read out of the pool, because the pool is not
    /// the only place it lives.** `Workers` holds one per open document so it
    /// can unlock the workers it grows later, and reading it from there would be
    /// a synchronous accessor like [`RenderService::pool_size`] --- and would
    /// answer `None` for the in-process backend, where the document and its
    /// password sit on the render thread. A job goes to whichever engine is
    /// running and gets the same answer from both.
    ///
    /// The one caller is a save. An append to an encrypted document re-reads the
    /// written file to check the cross-reference chained correctly, and `lopdf`
    /// parses **no objects at all** without the key --- so that check would see
    /// zero pages against the two it expects and roll a correct save back. The
    /// value goes no further than that read: `docs/THREAT-MODEL.md` §T6.9.
    pub fn password(&self, doc: u32, reply: Reply<Option<String>>) {
        if self.tx.send(Job::Password { doc, reply }).is_err() {
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

    /// Closes every open document, on a service thread.
    ///
    /// **For a webview that has just started.** Nothing else may call it: a
    /// running page holds document ids, and this invalidates all of them.
    ///
    /// The reply is a count rather than `()` because a caller cannot see the
    /// table --- without it, *nothing was open* and *everything was released*
    /// arrive as the same answer, and one of those is the interesting one.
    pub fn release_all(&self, reply: Reply<usize>) {
        if self.tx.send(Job::ReleaseAll { reply }).is_err() {
            // Render thread is gone; every document went with it.
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
    /// requests still outstanding. [`Workers::close`] owns that guarantee and
    /// states the argument for it --- FIFO dequeue puts the close after
    /// everything already queued, and a drain covers the requests that are
    /// still *running* in workers about to be killed. Restating it here would
    /// be a second copy to drift, and the half that is easy to lose is the
    /// drain. Anything arriving after the close is answered "has been closed"
    /// rather than from another document, because the id leaves a hole rather
    /// than being removed.
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
    fn open(
        &self,
        path: &Path,
        lazy_geometry: bool,
        password: Option<&str>,
    ) -> Result<DocumentInfo, progressive::Refusal>;
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String>;
    fn text(&self, doc: u32, page: u32, crop: Option<[f32; 4]>) -> Result<PageText, String>;
    fn search(
        &self,
        doc: u32,
        page: u32,
        query: &str,
        options: search::Options,
        carry: Option<&search::Carry>,
    ) -> Result<PageMatches, String>;
    fn content(&self, doc: u32, page: u32) -> Result<Option<[f64; 4]>, String>;
    fn geometry(&self, doc: u32, page: u32, crop: Option<[f32; 4]>)
        -> Result<CropGeometry, String>;
    fn crop_box(&self, doc: u32, page: u32, rect: [f32; 4]) -> Result<[f32; 4], String>;
    fn redaction_plans(
        &self,
        doc: u32,
        page: u32,
        regions: &[[f32; 4]],
    ) -> Result<Vec<redact::RegionPlan>, String>;
    fn outline(&self, doc: u32) -> Result<Outline, String>;
    fn comments(&self, doc: u32) -> Result<Comments, String>;

    fn links(&self, doc: u32) -> Result<Links, String>;
    fn mapping(&self, doc: u32) -> Result<Vec<PageMapping>, String>;
    fn properties(&self, doc: u32) -> Result<Properties, String>;
    fn append(&self, doc: u32, plan: &crate::edits::Plan) -> Result<crate::save::Update, String>;
    fn password(&self, doc: u32) -> Result<Option<String>, String>;
    fn close(&self, doc: u32) -> Result<(), String>;

    /// Closes every open document, returning how many were closed.
    ///
    /// Separate from calling [`Engine::close`] in a loop by the caller, because
    /// the caller cannot see which slots are filled --- ids leave holes rather
    /// than being removed, so "every id below the highest" is neither the set of
    /// open documents nor knowable from outside.
    fn release_all(&self) -> Result<usize, String>;
}

/// Serves one job and answers it.
pub(crate) fn dispatch(job: Job, engine: &dyn Engine) {
    match job {
        Job::Open {
            path,
            lazy_geometry,
            password,
            reply,
        } => reply(engine.open(&path, lazy_geometry, password.as_deref())),
        Job::Tile { request, reply } => reply(engine.tile(&request)),
        Job::Text {
            doc,
            page,
            crop,
            reply,
        } => reply(engine.text(doc, page, crop)),
        Job::Search {
            doc,
            page,
            query,
            options,
            carry,
            reply,
        } => reply(engine.search(doc, page, &query, options, carry.as_ref())),
        Job::Content { doc, page, reply } => reply(engine.content(doc, page)),
        Job::Geometry {
            doc,
            page,
            crop,
            reply,
        } => reply(engine.geometry(doc, page, crop)),
        Job::CropBox {
            doc,
            page,
            rect,
            reply,
        } => reply(engine.crop_box(doc, page, rect)),
        Job::RedactPlans {
            doc,
            page,
            regions,
            reply,
        } => reply(engine.redaction_plans(doc, page, &regions)),
        Job::Outline { doc, reply } => reply(engine.outline(doc)),
        Job::Comments { doc, reply } => reply(engine.comments(doc)),
        Job::Properties { doc, reply } => reply(engine.properties(doc)),
        Job::Links { doc, reply } => reply(engine.links(doc)),
        Job::Mapping { doc, reply } => reply(engine.mapping(doc)),
        Job::Append { doc, plan, reply } => reply(engine.append(doc, &plan)),
        Job::Password { doc, reply } => reply(engine.password(doc)),
        Job::Close { doc, reply } => reply(engine.close(doc)),
        Job::ReleaseAll { reply } => reply(engine.release_all()),
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
            // Not locked: a backend that never started has not looked at the
            // document, so it cannot be saying anything about its encryption.
            Job::Open { reply, .. } => reply(Err(progressive::Refusal {
                reason: error.to_string(),
                locked: false,
            })),
            Job::Tile { reply, .. } => reply(Err(error.to_string())),
            Job::Text { reply, .. } => reply(Err(error.to_string())),
            Job::Search { reply, .. } => reply(Err(error.to_string())),
            Job::Content { reply, .. } => reply(Err(error.to_string())),
            Job::Geometry { reply, .. } => reply(Err(error.to_string())),
            Job::CropBox { reply, .. } => reply(Err(error.to_string())),
            Job::RedactPlans { reply, .. } => reply(Err(error.to_string())),
            Job::Outline { reply, .. } => reply(Err(error.to_string())),
            Job::Comments { reply, .. } => reply(Err(error.to_string())),
            Job::Links { reply, .. } => reply(Err(error.to_string())),
            Job::Mapping { reply, .. } => reply(Err(error.to_string())),
            Job::Properties { reply, .. } => reply(Err(error.to_string())),
            Job::Append { reply, .. } => reply(Err(error.to_string())),
            Job::Password { reply, .. } => reply(Err(error.to_string())),
            Job::Close { reply, .. } => reply(Err(error.to_string())),
            Job::ReleaseAll { reply } => reply(Err(error.to_string())),
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
    docs: std::cell::RefCell<Vec<Option<OpenDocument>>>,
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
    fn open(
        &self,
        path: &Path,
        lazy_geometry: bool,
        password: Option<&str>,
    ) -> Result<DocumentInfo, progressive::Refusal> {
        let t0 = Instant::now();
        let doc = OpenDocument::open(self.bindings, path, password)?;
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

    fn text(&self, doc: u32, page: u32, crop: Option<[f32; 4]>) -> Result<PageText, String> {
        run_text(open_slot(&self.docs.borrow(), doc)?, page, crop)
    }

    fn search(
        &self,
        doc: u32,
        page: u32,
        query: &str,
        options: search::Options,
        carry: Option<&search::Carry>,
    ) -> Result<PageMatches, String> {
        run_search(
            open_slot(&self.docs.borrow(), doc)?,
            page,
            query,
            options,
            carry,
        )
    }

    fn mapping(&self, doc: u32) -> Result<Vec<PageMapping>, String> {
        Ok(run_mapping(open_slot(&self.docs.borrow(), doc)?))
    }

    fn content(&self, doc: u32, page: u32) -> Result<Option<[f64; 4]>, String> {
        run_content(
            self.bindings,
            open_slot(&self.docs.borrow(), doc)?,
            page,
            &CancelToken::default(),
        )
    }

    fn geometry(
        &self,
        doc: u32,
        page: u32,
        crop: Option<[f32; 4]>,
    ) -> Result<CropGeometry, String> {
        geometry_of(open_slot(&self.docs.borrow(), doc)?, page, crop)
    }

    fn crop_box(&self, doc: u32, page: u32, rect: [f32; 4]) -> Result<[f32; 4], String> {
        crop_box_of(open_slot(&self.docs.borrow(), doc)?, page, rect)
    }

    fn redaction_plans(
        &self,
        doc: u32,
        page: u32,
        regions: &[[f32; 4]],
    ) -> Result<Vec<redact::RegionPlan>, String> {
        redaction_plans_of(open_slot(&self.docs.borrow(), doc)?, page, regions)
    }

    fn outline(&self, doc: u32) -> Result<Outline, String> {
        Ok(run_outline(open_slot(&self.docs.borrow(), doc)?))
    }

    fn comments(&self, doc: u32) -> Result<Comments, String> {
        run_comments(open_slot(&self.docs.borrow(), doc)?)
    }

    fn links(&self, doc: u32) -> Result<Links, String> {
        run_links(open_slot(&self.docs.borrow(), doc)?)
    }

    fn properties(&self, doc: u32) -> Result<Properties, String> {
        run_properties(open_slot(&self.docs.borrow(), doc)?)
    }

    fn append(&self, doc: u32, plan: &crate::edits::Plan) -> Result<crate::save::Update, String> {
        run_append(open_slot(&self.docs.borrow(), doc)?, plan)
    }

    fn password(&self, doc: u32) -> Result<Option<String>, String> {
        Ok(open_slot(&self.docs.borrow(), doc)?
            .graph()
            .password()
            .map(str::to_string))
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

    fn release_all(&self) -> Result<usize, String> {
        let mut docs = self.docs.borrow_mut();
        // Counted before they go, because `Option::take` on an empty slot is
        // indistinguishable from one on a full slot afterwards --- and the count
        // is the whole reply: a caller cannot see this table, so a bare `Ok(())`
        // would leave "nothing was open" and "everything was released" identical.
        let held = docs.iter().filter(|slot| slot.is_some()).count();
        // Cleared rather than emptied. A `Vec::clear` renumbers, and an id that
        // arrives late from a webview that has not stopped talking would then
        // name whatever opened next --- which is the reason a close leaves a hole
        // rather than removing the entry.
        for slot in docs.iter_mut() {
            *slot = None;
        }
        Ok(held)
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
    doc: &OpenDocument,
    req: &TileRequest,
    cancel: &CancelToken,
) -> Result<TileOutcome, String> {
    // Cached, because Pdfium re-parses a page on every `FPDF_LoadPage` --- 44 ms
    // on the A0 sheet, which loading per tile request would charge a six-tile
    // screenful nearly four times over.
    let page = doc.page_cropped(req.page, req.crop)?;

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

/// A crop box's size and its place inside the page the file describes.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CropGeometry {
    /// The cropped page's displayed width in points.
    pub width_pt: f32,
    /// The cropped page's displayed height in points.
    pub height_pt: f32,
    /// The crop's left edge in the file's display space, points from its corner.
    pub left: f32,
    /// The crop's top edge in the file's display space, points from its corner.
    pub top: f32,
}

/// Measures one page's content box on the render thread.
pub(crate) fn run_content(
    bindings: Bindings,
    document: &OpenDocument,
    page: u32,
    cancel: &CancelToken,
) -> Result<Option<[f64; 4]>, String> {
    crate::content::content_box(bindings, &document.page(page)?, cancel)
}

/// Where a crop box lands inside the page the file describes, and how big it is.
///
/// **The frontend keeps every rectangle in the file's display space and this is
/// what lets it.** A comment, a link and one of the reader's own marks all
/// arrive measured from the *file's* displayed corner; a crop moves that corner,
/// and rather than restating all three in a space that changes whenever the
/// reader crops, they stay put and are drawn at `rect - (left, top)`. One number
/// pair, applied at the point of drawing, against three vocabularies that would
/// otherwise each need a crop-aware variant.
///
/// It is also why a mark is *sent* in the file's space: a mark made while
/// cropped and saved after the crop changed would otherwise be written where the
/// crop was rather than where the words are.
pub fn geometry_of(
    document: &OpenDocument,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<CropGeometry, String> {
    // The file's own box first, and its size: everything below is expressed
    // against it, so it has to be read from a page nothing has cropped.
    let (file_box, file_width, file_height, turns) = {
        let page = document.page(page)?;
        (
            page.crop_pt(),
            page.width_pt(),
            page.height_pt(),
            page.quarter_turns(),
        )
    };
    let Some(want) = crop else {
        return Ok(CropGeometry {
            width_pt: file_width,
            height_pt: file_height,
            left: 0.0,
            top: 0.0,
        });
    };

    let placed = place_crop(turns, file_width, file_height, file_box, want);
    // The cropped page's *own* report of its size rather than the rectangle's
    // width and height, so that a disagreement between the two is something a
    // check can see instead of something this function defines away.
    let cropped = document.page_cropped(page, crop)?;
    Ok(CropGeometry {
        width_pt: cropped.width_pt(),
        height_pt: cropped.height_pt(),
        left: placed[0],
        top: placed[1],
    })
}

/// Where a crop box sits in the file's display space: `[left, top, right, bottom]`.
///
/// Split out of [`geometry_of`], which reads only the corner, so that
/// [`crop_from_display`] has something to be the inverse *of*. The two carry
/// separate rotation arms --- [`crate::text::to_device`] and
/// [`crate::text::from_device`] --- so a round trip through both is a real
/// comparison rather than a tautology, which is what
/// `a_dragged_rectangle_maps_back_to_the_crop_that_produced_it` asserts.
///
/// `file_box` is the page's own `/CropBox` and is subtracted first: a crop box
/// is in absolute page coordinates, and the display space starts at the file
/// box's corner rather than at the origin.
fn place_crop(
    turns: u8,
    file_width: f32,
    file_height: f32,
    file_box: [f32; 4],
    want: [f32; 4],
) -> [f32; 4] {
    // Through the one implementation that knows the turn. A second rotation
    // table here is the trap `docs/TRAPS.md` records as two tables disagreeing
    // at every turn but zero.
    crate::text::to_device(
        turns,
        file_width,
        file_height,
        [
            f64::from(want[0] - file_box[0]),
            f64::from(want[1] - file_box[1]),
            f64::from(want[2] - file_box[0]),
            f64::from(want[3] - file_box[1]),
        ],
    )
}

/// The crop box that would place a rectangle a reader dragged where they dragged it.
///
/// **The inverse of [`place_crop`], and it exists because a crop is the one
/// thing a reader draws that is not stored in the space they drew it in.** A
/// mark's `/QuadPoints` and a crop box are both in the page's own unrotated
/// space, so both need this turn undone --- the difference is that a mark goes
/// through the model as a *display* rectangle and is turned by `save.rs` at the
/// moment it is written, whereas a crop box **is** what the model holds. So the
/// turn has to be undone before the edit is made rather than when it is saved,
/// and that is why this is asked for rather than computed in `crop.ts`: the
/// frontend is never told a page's `/Rotate`.
///
/// `rect` is `[left, top, right, bottom]` in the **file's** display space, y
/// downwards from the file's displayed corner --- which is the space every
/// rectangle in the frontend is in, and what `Viewer.fileRectOn` produces.
/// Returns `[left, bottom, right, top]` with y upwards, in the page's absolute
/// coordinates, which is the space `page_content_box` answers in and `page_crop`
/// accepts.
///
/// Clamped to the file's own box, which the drag's own clamp cannot do: the
/// reader's rectangle is clamped to the *cropped* page they can see, and a crop
/// already in force means that page is inside the file's. Composing two crops is
/// legal and this keeps the result a subset rather than trusting the arithmetic
/// to have stayed inside.
pub fn crop_from_display(
    turns: u8,
    file_width: f32,
    file_height: f32,
    file_box: [f32; 4],
    rect: [f32; 4],
) -> [f32; 4] {
    let back = crate::text::from_device(turns, file_width, file_height, rect);
    let want = [
        back[0] as f32 + file_box[0],
        back[1] as f32 + file_box[1],
        back[2] as f32 + file_box[0],
        back[3] as f32 + file_box[1],
    ];
    [
        want[0].max(file_box[0]).min(file_box[2]),
        want[1].max(file_box[1]).min(file_box[3]),
        want[2].max(file_box[0]).min(file_box[2]),
        want[3].max(file_box[1]).min(file_box[3]),
    ]
}

/// Reads the page a drag landed on, and maps the drag into a crop box.
///
/// The document half of [`crop_from_display`]: the turn and the file's own box
/// are the page's, and everything else is pure.
pub fn crop_box_of(document: &OpenDocument, page: u32, rect: [f32; 4]) -> Result<[f32; 4], String> {
    let page = document.page(page)?;
    Ok(crop_from_display(
        page.quarter_turns(),
        page.width_pt(),
        page.height_pt(),
        page.crop_pt(),
        rect,
    ))
}

/// What removing each of a page's marked regions would take, and what it would miss.
///
/// **The one thing a reader reviewing a redaction cannot work out for
/// themselves.** The frontend knows which words a region *covers* --- it holds
/// the character boxes --- and it cannot know which text-showing operations
/// those characters belong to, because that is a fact about the content stream.
/// Route B removes a whole operation when any of its glyphs is inside, so the
/// two answers differ by exactly the collateral, which is what `docs/PLAN.md`
/// §6 step 2 calls an over-selection.
///
/// One call per page rather than per region, for the reason the comments panel's
/// covered words are fetched a page at a time: the page load and the object walk
/// are the cost and they are per page, while [`redact::covered`] is a pure
/// comparison per region.
///
/// `regions` are in the **file's display space**, `[left, top, right, bottom]`
/// with y downwards --- the space `Viewer.fileRectOn` produces and the model
/// holds a pending redaction in. They are brought into the page's own space by
/// [`crop_from_display`], which is the same conversion a dragged crop goes
/// through and is shared rather than written twice: two rotation tables
/// disagreeing at every turn but zero is a trap this repository has paid for.
pub fn redaction_plans_of(
    document: &OpenDocument,
    page: u32,
    regions: &[[f32; 4]],
) -> Result<Vec<redact::RegionPlan>, String> {
    let page = document.page(page)?;
    let objects = crate::objects::read(&page)?;
    let turns = page.quarter_turns();
    let (width, height) = (page.width_pt(), page.height_pt());
    let file_box = page.crop_pt();
    Ok(regions
        .iter()
        .map(|region| {
            let want = crop_from_display(turns, width, height, file_box, *region);
            let plan = redact::covered(&objects.all, &objects.forms, want);
            redact::RegionPlan {
                text_objects: objects.text.len(),
                images: plan.images.clone(),
                image_objects: objects
                    .all
                    .iter()
                    .filter(|object| object.kind == "image")
                    .count(),
                form_shows: plan.form_shows.clone(),
                // Every form on the page, whether or not this region touches it.
                // See the field: a plan merges a page's regions, and a count
                // present only when some region covered that form would be
                // missing exactly when another region needed it.
                form_text_objects: objects
                    .forms
                    .iter()
                    .map(|form| (form.at, form.text.len()))
                    .collect(),
                // Joined by a space, because a row shows one line and the
                // operations either side of a break are two operations. What is
                // deliberately not done here is trimming: an operation that
                // draws only spaces is still an operation the removal deletes,
                // and a caller flattening this for a row is what decides how it
                // reads.
                taking: plan
                    .shows
                    .iter()
                    .filter_map(|ordinal| objects.text.get(*ordinal))
                    .map(String::as_str)
                    // Then the text inside forms, which is drawn on the same
                    // page and is as much *what this removal takes* as the
                    // page's own -- so it reaches every reader that asks what
                    // went: the review panel, the byte scan's needles, the
                    // outline and form carriers, and the OCR gate's survivors.
                    .chain(plan.form_shows.iter().filter_map(|(at, ordinal)| {
                        objects
                            .forms
                            .iter()
                            .find(|form| form.at == *at)
                            .and_then(|form| form.text.get(*ordinal))
                            .map(|text| text.draws.as_str())
                    }))
                    .collect::<Vec<_>>()
                    .join(" "),
                unhandled: plan.unhandled,
                shows: plan.shows,
                area: want,
            }
        })
        .collect())
}

/// Extracts one page's characters on the render thread.
///
/// `crop` for the reason [`TileRequest::crop`] gives, and it is not optional in
/// the sense of "nice to have": character boxes are measured from the displayed
/// page's corner, so text extracted without the reader's crop lands every caret
/// and every highlight out by the crop's offset.
pub(crate) fn run_text(
    document: &OpenDocument,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<PageText, String> {
    text::extract(&document.page_cropped(page, crop)?)
}

/// Searches one page on the render thread.
///
/// Extraction happens here and the characters are dropped again: only the
/// matches cross to the frontend. A 775-page document's text is tens of
/// megabytes as JSON and the frontend needs it only for the pages it draws, so
/// shipping it in order to search it would be the expensive half of a cheap
/// operation.
pub(crate) fn run_search(
    document: &OpenDocument,
    page: u32,
    query: &str,
    options: search::Options,
    carry: Option<&search::Carry>,
) -> Result<PageMatches, String> {
    // `None`, and that is a statement rather than a shortcut: a crop moves
    // character *boxes* and leaves character *indices* alone, and a match is a
    // range of indices. Extracting under the reader's crop would cost the crop
    // set-and-restore on every page of a document-wide search to produce
    // identical answers.
    Ok(search::search_page(
        &run_text(document, page, None)?,
        page,
        query,
        options,
        carry,
    ))
}

/// Walks a document's outline on the render thread.
pub(crate) fn run_outline(document: &OpenDocument) -> Outline {
    outline::read(document)
}

/// Reads a document's comments on the render thread.
///
/// Cached inside [`crate::docgraph::DocumentGraph`], so the `lopdf` parse this costs happens at
/// most once per open document however often the panel is opened --- the same
/// arrangement `run_mapping` has, and `annots::scan` is what it wraps.
pub(crate) fn run_comments(document: &OpenDocument) -> Result<Comments, String> {
    document.graph().comments(document.page_count() as usize)
}

/// Builds a save's update section on the render thread.
///
/// The counterpart of `run_comments` and its siblings, and the one that produces
/// bytes destined for a file rather than a fact about the document --- see
/// `worker_proto::Request::Append` for why that belongs here anyway.
pub(crate) fn run_append(
    document: &OpenDocument,
    plan: &crate::edits::Plan,
) -> Result<crate::save::Update, String> {
    document.graph().append(plan)
}

/// Rewrites the mapped document under a plan, on the render thread.
///
/// The whole-document counterpart of [`run_append`]. What comes back is the
/// serialised file, which the caller writes down the worker's output channel
/// rather than into a reply --- see `worker_proto::Request::Rewrite`.
pub(crate) fn run_rewrite(
    document: &OpenDocument,
    plan: &crate::edits::Plan,
    job: crate::save::Job,
) -> Result<Vec<u8>, crate::save::Refusal> {
    document.graph().rewrite(plan, job)
}

/// Merges the mapped document with the handed-over files on the render thread.
///
/// [`run_rewrite`]'s widest counterpart --- see
/// `crate::worker_proto::Request::Merge`.
pub(crate) fn run_merge(
    document: &OpenDocument,
    plan: &crate::edits::Plan,
    inputs: crate::save::Inputs<'_>,
) -> Result<(Vec<u8>, u32), crate::save::Refusal> {
    document.graph().merge(plan, inputs)
}

/// Builds a print job for a page range on the render thread.
///
/// [`run_rewrite`]'s counterpart for the route that carries no plan --- see
/// `crate::worker_proto::Request::PrintRange`.
pub(crate) fn run_print_range(
    document: &OpenDocument,
    job: &crate::print::Job,
) -> Result<Vec<u8>, String> {
    document.graph().print_range(job)
}

/// Re-reads the mapped document with `lopdf` and counts its pages.
///
/// The counterpart of [`run_append`] on the verification side --- see
/// `worker_proto::Request::Reread` for why the check belongs here and why it is
/// `lopdf` rather than the PDFium already holding this file open.
///
/// Deliberately **not** cached, where the read-only facts above are: the whole
/// question is what is in the file *now*, and a worker answering it is spawned
/// for that one answer and dropped.
pub(crate) fn run_reread(document: &OpenDocument) -> Result<usize, String> {
    document.graph().reread_pages()
}

/// Reads a document's links on the render thread.
///
/// Cached inside [`crate::docgraph::DocumentGraph`] like the comments are, so the `lopdf` parse
/// happens once per open document however often it is asked for --- which
/// matters more here, because a re-open after a rotation would otherwise repeat
/// it on a document that has not changed.
pub(crate) fn run_links(document: &OpenDocument) -> Result<Links, String> {
    document.graph().links(document.page_count() as usize)
}

/// Reads the document's font dictionaries on the render thread.
///
/// Cached inside [`crate::docgraph::DocumentGraph`], so the `lopdf` parse this costs happens at most
/// once per open document however often a reader searches.
pub(crate) fn run_mapping(document: &OpenDocument) -> Vec<PageMapping> {
    document
        .graph()
        .mapping(document.page_count() as usize)
        .to_vec()
}

/// Reads what a document says about itself, on the render thread.
///
/// Cached inside [`crate::docgraph::DocumentGraph`] like the comments and the links are, so a
/// reader who opens the properties dialog twice pays for the `lopdf` parse once.
pub(crate) fn run_properties(document: &OpenDocument) -> Result<Properties, String> {
    document.graph().properties(document.page_count())
}

#[cfg(test)]
mod tests {
    use super::{crop_from_display, place_crop, Backend};

    /// The file's own box, deliberately not at the origin.
    ///
    /// A `/CropBox` starting at `(0, 0)` makes the subtraction in
    /// [`place_crop`] a no-op, so every test using one would pass with that
    /// term deleted --- which is the "property that holds by construction"
    /// trap. A4 offset by 12 by 20 is the smallest thing that is not a no-op.
    const FILE_BOX: [f32; 4] = [12.0, 20.0, 607.0, 862.0];
    const FILE_W: f32 = 595.0;
    const FILE_H: f32 = 842.0;

    /// A drag maps back to the crop box that would place it there, at every turn.
    ///
    /// **The two directions carry separate rotation tables**, so this is a real
    /// comparison and not a tautology: [`place_crop`] goes through
    /// `text::to_device` and [`crop_from_display`] through `text::from_device`,
    /// and `docs/TRAPS.md` records two such tables disagreeing at every turn but
    /// zero. All four turns, because a wrong arm for one of them is exactly the
    /// defect that shipped in the outline resolver and in the print rotation.
    #[test]
    fn a_dragged_rectangle_maps_back_to_the_crop_that_produced_it() {
        let want = [80.0_f32, 140.0, 400.0, 700.0];
        for turns in 0..4_u8 {
            let (w, h) = if turns % 2 == 0 {
                (FILE_W, FILE_H)
            } else {
                (FILE_H, FILE_W)
            };
            let placed = place_crop(turns, w, h, FILE_BOX, want);
            let back = crop_from_display(turns, w, h, FILE_BOX, placed);
            for (index, (got, expected)) in back.iter().zip(want.iter()).enumerate() {
                assert!(
                    (got - expected).abs() < 0.01,
                    "turn {turns} corner {index}: {got} != {expected}",
                );
            }
        }
    }

    /// A drag off the page edge crops to the edge rather than past it.
    ///
    /// The drag's own clamp bounds it to the **cropped** page the reader can
    /// see, which on an already-cropped page is a strictly smaller rectangle
    /// than the file's box --- so this bound is the only one that can say the
    /// result is a subset of the paper. Asserted on both axes and in both
    /// directions, because a clamp written with one `max` and one `min` passes
    /// a test that only pushes one way.
    #[test]
    fn a_crop_dragged_off_the_paper_stops_at_the_paper() {
        let past = crop_from_display(
            0,
            FILE_W,
            FILE_H,
            FILE_BOX,
            [-500.0, -500.0, 4000.0, 4000.0],
        );
        assert_eq!(past, FILE_BOX);
    }

    /// The file box's corner is carried, so a crop is in absolute coordinates.
    ///
    /// Dragging the whole visible page must name the file's own box back, and on
    /// a box that does not start at the origin that is only true if the offset
    /// survives the trip.
    ///
    /// **This is what the round trip above structurally cannot see.** A round
    /// trip is a composition, so it is blind to any error the two directions
    /// make *symmetrically* --- measured, not assumed: deleting the file-box
    /// term from both `place_crop` and [`crop_from_display`] leaves it green and
    /// reddens only this. A one-sided deletion reddens both, which is the case
    /// that misled the first draft of this comment.
    #[test]
    fn a_crop_is_measured_from_the_page_and_not_from_the_origin() {
        let whole = crop_from_display(0, FILE_W, FILE_H, FILE_BOX, [0.0, 0.0, FILE_W, FILE_H]);
        assert_eq!(whole, FILE_BOX);
    }

    /// The platform list, spelled out a second time on purpose.
    ///
    /// Sharing a predicate with `default_here` would make this a check deriving
    /// its expectation from the thing it tests --- it would agree with whatever
    /// the code said. The duplication is the assertion. It is also what caught
    /// the Windows flip: both tests here went red on that one-word change,
    /// because they still said macOS was the only platform with a boundary.
    #[test]
    fn an_unset_backend_is_the_platform_default() {
        let expected = if cfg!(target_os = "macos") || cfg!(windows) {
            Backend::Worker
        } else {
            Backend::InProcess
        };
        assert_eq!(Backend::parse(None), Ok(expected));
    }

    /// The uncontained default leaves a trace, and the contained one does not.
    ///
    /// Both halves are asserted on both platforms, from the same run, because
    /// either alone passes with the code wrong: a mark recorded unconditionally
    /// satisfies "it is marked", and one never recorded satisfies "it is not
    /// marked where a sandbox exists". Only the pair pins the *condition*.
    ///
    /// It reads the real timeline rather than a return value, which is the point
    /// --- the defect being fixed was that nothing observable said an uncontained
    /// parse had happened, so asserting anything other than the observable would
    /// re-create it.
    ///
    /// **Stated as an equivalence rather than per platform.** It used to name
    /// macOS, which made it a second place the platform list had to be kept
    /// current, and it duly went red on the Windows flip for a reason that had
    /// nothing to do with what it asserts. The invariant does not mention a
    /// platform at all: the mark is on the timeline exactly when the default is
    /// the uncontained one.
    ///
    /// **What it can catch now depends on the platform, and only one half is
    /// live here.** Measured rather than assumed, against the two mutations the
    /// note above names. A mark recorded *unconditionally* fails it, on any
    /// platform. A mark *never* recorded does not --- since both macOS and
    /// Windows now default to a worker, the branch that would record it is never
    /// executed, so its contents cannot be wrong in a way anything observes. The
    /// check has not weakened; its precondition has stopped occurring, which is a
    /// different thing and the reason it is written down rather than left to be
    /// rediscovered on the platform where the branch comes back.
    ///
    /// **The announcement moved out of `default_here` and into `from_env`**, so
    /// this drives `from_env` --- the entry point the application uses and the
    /// only one that announces. Reading the timeline after calling `parse`
    /// would now assert nothing at all, which is the quiet way this test could
    /// have stopped meaning anything.
    ///
    /// **The asked-for route is asserted here rather than in a test of its
    /// own**, and that is not tidiness. The mark is process-global and `cargo
    /// test` runs this binary's tests on several threads, so a second test that
    /// announced would race this one: whichever ran first would decide what the
    /// other read, and the pair would pass or fail on the harness's scheduling.
    /// One test, in a defined order --- observe the default, then announce ---
    /// is the only version of this that means the same thing every run.
    #[test]
    fn the_uncontained_backend_says_so_by_either_route_and_the_contained_one_does_not() {
        // No `TPDF_BACKEND` in the test environment, so this is the default
        // path. Asserted rather than assumed: a variable set by the harness
        // would make the comparison below one between two other things.
        assert!(
            std::env::var("TPDF_BACKEND").is_err(),
            "this test is about the default, so nothing may have asked"
        );
        let uncontained = |marks: &[(String, f64)]| {
            marks
                .iter()
                .filter(|(name, _)| name == super::UNSANDBOXED_MARK)
                .count()
        };

        assert_eq!(Backend::from_env(), Ok(Backend::default_here()));
        assert_eq!(
            uncontained(&crate::startup::timeline()) > 0,
            Backend::default_here() == Backend::InProcess,
            "the uncontained mark and the uncontained default must agree"
        );

        // The route that used to be silent: on a platform that *has* a
        // boundary, asking for the uncontained backend by name turns it off,
        // and nothing inside the process said so. Every claim of the form "the
        // app process never maps the parser" is checked from outside by
        // `scripts/win_modules.py`; this left the inside with no record at all.
        super::announce_uncontained();
        assert_eq!(
            uncontained(&crate::startup::timeline()),
            1,
            "asking for it by name records the mark --- once, however many \
             services ask, so a benchmark holding several does not bury the \
             warning in copies of itself"
        );

        // What this cannot reach is the wiring in `from_env`, which is one
        // line, because driving it needs `TPDF_BACKEND` set and the variable is
        // read once per process. `mutate_rust.py` carries the mutation that
        // deletes that line, and it is what covers the join.
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
