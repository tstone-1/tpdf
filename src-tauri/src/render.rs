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
//! Phase 0 note: this is an in-process service. Supervised worker processes
//! replace it, which is what actually delivers parallelism and the security
//! boundary. The channel interface here is deliberately shaped so that swap does
//! not change callers.
//!
//! Spike 0.5 measured the replacement and it is cheap: 6 µs per control round
//! trip, 0.11 ms to move a 4 MB tile through shared memory, 3.9x throughput on
//! four workers. See `bin/worker_bench.rs` and PLAN §3. The two things that
//! change for callers are that the document must be handed over as mapped bytes
//! rather than a path, and that a reply can now fail because the worker died.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use pdfium_render::prelude::*;

use crate::progressive::{self, Bindings, CancelToken, Outcome, RawDocument, TileSpec};
use crate::startup::{mark, since_process_start_ms};

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
#[derive(Clone, Copy, Debug, serde::Serialize)]
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

type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

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
}

/// Which requests are outstanding, and which have been withdrawn.
///
/// Shared between whatever thread issues requests and the render thread. Both
/// halves of a withdrawal --- registering a render as in flight and cancelling
/// it --- happen under this one lock, so a request cannot slip between them and
/// start after it was cancelled.
#[derive(Default)]
struct Queue {
    /// Sent to the render thread but not yet started.
    queued: HashSet<u64>,
    /// Withdrawn while still queued. A subset of `queued` by construction, so
    /// it drains with it: a withdrawal naming a request that already finished
    /// is dropped rather than remembered, which is what keeps this bounded.
    cancelled: HashSet<u64>,
    /// The render currently running, and the token that stops it.
    inflight: Option<(u64, CancelToken)>,
}

/// Handle to the render thread. Cheap to clone.
#[derive(Clone)]
pub struct RenderService {
    tx: Sender<Job>,
    queue: Arc<Mutex<Queue>>,
}

/// Locks the queue, ignoring poisoning.
///
/// A panic on the render thread would poison this, and refusing every later
/// request because of it turns one failed tile into a dead viewer. The state
/// behind it is three sets of integers with no invariant a partial update can
/// break.
fn lock(queue: &Mutex<Queue>) -> std::sync::MutexGuard<'_, Queue> {
    queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RenderService {
    /// Starts the render thread and binds Pdfium.
    ///
    /// Binding happens on the render thread rather than here, so a missing or
    /// mismatched library surfaces as a failed open rather than a panic during
    /// app setup.
    pub fn start(library_dir: PathBuf) -> Self {
        let (tx, rx) = channel::<Job>();
        let queue = Arc::new(Mutex::new(Queue::default()));
        let thread_queue = Arc::clone(&queue);

        std::thread::Builder::new()
            .name("tpdf-render".into())
            .spawn(move || {
                let pdfium = match bind_pdfium(&library_dir) {
                    Ok(p) => {
                        // Loading and binding the Pdfium dylib is a fixed cost
                        // paid before any document can be opened, so it needs
                        // its own line in the startup budget.
                        mark("pdfium bound");
                        p
                    }
                    Err(e) => {
                        // Drain the queue, failing every job with the bind error,
                        // so callers get a diagnosable message instead of a hang.
                        for job in rx {
                            match job {
                                Job::Open { reply, .. } => reply(Err(e.clone())),
                                Job::Tile { reply, .. } => reply(Err(e.clone())),
                            }
                        }
                        return;
                    }
                };

                let bindings = progressive::bindings_of(pdfium);
                let mut docs: Vec<RawDocument> = Vec::new();

                for job in rx {
                    match job {
                        Job::Open {
                            path,
                            lazy_geometry,
                            reply,
                        } => {
                            reply(open_document(bindings, &path, lazy_geometry, &mut docs));
                        }
                        Job::Tile { request, reply } => {
                            reply(run_tile(bindings, &docs, &thread_queue, &request));
                        }
                    }
                }
            })
            .expect("failed to spawn render thread");

        Self { tx, queue }
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
        if rid != 0 {
            lock(&self.queue).queued.insert(rid);
        }

        if self.tx.send(Job::Tile { request, reply }).is_err() {
            // Render thread is gone. Forget the request rather than leaving it
            // outstanding forever, since nothing will ever dequeue it.
            if rid != 0 {
                let mut queue = lock(&self.queue);
                queue.queued.remove(&rid);
                queue.cancelled.remove(&rid);
            }
        }
    }

    /// Withdraws a tile request by its `rid`.
    ///
    /// Safe to call at any point in the request's life, including after it has
    /// finished --- an unknown `rid` is simply ignored, because the caller
    /// cannot know whether its reply is already on the way.
    pub fn cancel(&self, rid: u64) {
        if rid == 0 {
            return;
        }
        let mut queue = lock(&self.queue);
        match &queue.inflight {
            Some((inflight, token)) if *inflight == rid => token.cancel(),
            _ => {
                if queue.queued.contains(&rid) {
                    queue.cancelled.insert(rid);
                }
            }
        }
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

fn open_document(
    bindings: Bindings,
    path: &Path,
    lazy_geometry: bool,
    docs: &mut Vec<RawDocument>,
) -> Result<DocumentInfo, String> {
    let t0 = Instant::now();
    let doc = RawDocument::open(bindings, path)?;
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
    // what the viewer needs to lay out its first frame. The scroller estimates
    // the rest from it and corrects as pages arrive (PLAN §4).
    let pages: Vec<PageSize> = if lazy_geometry {
        match page_count {
            0 => Vec::new(),
            _ => vec![size_of(0)?],
        }
    } else {
        (0..page_count).map(size_of).collect::<Result<_, _>>()?
    };

    let id = docs.len() as u32;
    docs.push(doc);
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
/// The claim and the release are the only places `inflight` is written, and both
/// happen under the queue lock while nothing else holds it, so a withdrawal
/// arriving at any instant either finds the request queued (and marks it) or
/// finds it in flight (and cancels it).
fn run_tile(
    bindings: Bindings,
    docs: &[RawDocument],
    queue: &Mutex<Queue>,
    req: &TileRequest,
) -> Result<TileOutcome, String> {
    let token = {
        let mut queue = lock(queue);
        queue.queued.remove(&req.rid);
        if queue.cancelled.remove(&req.rid) {
            // Withdrawn before it ever started: the whole render is saved, not
            // merely interrupted. On a page costing a second a tile this is the
            // larger of the two savings.
            return Ok(TileOutcome::Abandoned);
        }
        let token = CancelToken::new();
        if req.rid != 0 {
            queue.inflight = Some((req.rid, token.clone()));
        }
        token
    };

    let result = render_tile(bindings, docs, req, &token);

    if req.rid != 0 {
        let mut queue = lock(queue);
        // Only clear our own claim: a reply is delivered before the next job is
        // dequeued, so this cannot be someone else's, but the check costs
        // nothing and stops a future change from silently clearing one.
        if matches!(&queue.inflight, Some((rid, _)) if *rid == req.rid) {
            queue.inflight = None;
        }
    }

    result
}

fn render_tile(
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

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
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
