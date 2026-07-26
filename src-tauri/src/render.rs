//! The render service: the single owner of Pdfium and of every open document.
//!
//! Everything Pdfium touches happens on one dedicated thread. This is not a
//! stylistic choice --- `pdfium-render`'s `thread_safe` feature serializes every
//! Pdfium call behind one global mutex, so extra threads buy nothing but
//! contention, and `PdfDocument` is not `Send`. See AGENTS.md.
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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::Instant;

use pdfium_render::prelude::*;

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
    pub doc: u32,
    pub page: u16,
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
    /// Time spent in `load_pdf_from_file`, i.e. parse and cross-reference repair.
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
        reply: Reply<Tile>,
    },
}

/// Handle to the render thread. Cheap to clone.
#[derive(Clone)]
pub struct RenderService {
    tx: Sender<Job>,
}

impl RenderService {
    /// Starts the render thread and binds Pdfium.
    ///
    /// Binding happens on the render thread rather than here, so a missing or
    /// mismatched library surfaces as a failed open rather than a panic during
    /// app setup.
    pub fn start(library_dir: PathBuf) -> Self {
        let (tx, rx) = channel::<Job>();

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

                let mut docs: Vec<PdfDocument<'static>> = Vec::new();

                for job in rx {
                    match job {
                        Job::Open {
                            path,
                            lazy_geometry,
                            reply,
                        } => {
                            reply(open_document(pdfium, &path, lazy_geometry, &mut docs));
                        }
                        Job::Tile { request, reply } => {
                            reply(render_tile(&docs, &request));
                        }
                    }
                }
            })
            .expect("failed to spawn render thread");

        Self { tx }
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
    pub fn tile(&self, request: TileRequest, reply: Reply<Tile>) {
        if self.tx.send(Job::Tile { request, reply }).is_err() {
            // Render thread is gone.
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
    pdfium: &'static Pdfium,
    path: &Path,
    lazy_geometry: bool,
    docs: &mut Vec<PdfDocument<'static>>,
) -> Result<DocumentInfo, String> {
    let t0 = Instant::now();
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
    mark("document parsed");

    let size_of = |page: &PdfPage<'_>| PageSize {
        width_pt: page.width().value,
        height_pt: page.height().value,
    };

    let page_count = doc.pages().len() as usize;
    // Lazy geometry loads exactly one page, because the first page's size is
    // what the viewer needs to lay out its first frame. The scroller estimates
    // the rest from it and corrects as pages arrive (PLAN §4).
    let pages: Vec<PageSize> = if lazy_geometry {
        doc.pages()
            .get(0)
            .map(|page| vec![size_of(&page)])
            .unwrap_or_default()
    } else {
        doc.pages().iter().map(|page| size_of(&page)).collect()
    };

    let id = docs.len() as u32;
    docs.push(doc);
    // Distinct from `document parsed`: collecting page geometry walks every
    // page object, which on a long document is its own measurable cost.
    mark("document open complete");

    Ok(DocumentInfo {
        id,
        pages,
        page_count,
        lazy_geometry,
        open_ms,
        at_ms: since_process_start_ms(),
    })
}

fn render_tile(docs: &[PdfDocument<'static>], req: &TileRequest) -> Result<Tile, String> {
    let doc = docs
        .get(req.doc as usize)
        .ok_or_else(|| format!("no such document: {}", req.doc))?;

    let page = doc
        .pages()
        .get(req.page as PdfPageIndex)
        .map_err(|e| format!("no such page {}: {e}", req.page))?;

    // Full scaled page size in device pixels. The tile is a window onto this,
    // positioned by a negative origin -- Pdfium renders the whole page into a
    // tile-sized bitmap and clips, so no full-page bitmap is ever allocated.
    let full_width = (page.width().value * req.scale).round() as i32;
    let full_height = (page.height().value * req.scale).round() as i32;

    let mut bitmap = PdfBitmap::empty(
        req.width as Pixels,
        req.height as Pixels,
        PdfBitmapFormat::BGRA,
    )
    .map_err(|e| format!("could not allocate {}x{} tile: {e}", req.width, req.height))?;

    let config = PdfRenderConfig::new()
        .set_target_width(full_width)
        .set_target_height(full_height)
        .set_origin(-req.x, -req.y);

    let t0 = Instant::now();
    page.render_into_bitmap_with_config(&mut bitmap, &config)
        .map_err(|e| format!("render failed: {e}"))?;
    let render_us = t0.elapsed().as_micros() as u64;

    let rgba = bitmap.as_rgba_bytes();

    let t1 = Instant::now();
    let bytes = match req.format {
        TileFormat::Raw => rgba,
        TileFormat::Png => encode_png(&rgba, req.width as u32, req.height as u32)?,
    };
    let encode_us = t1.elapsed().as_micros() as u64;
    mark("first tile rendered");

    Ok(Tile {
        bytes,
        width: req.width,
        height: req.height,
        format: req.format,
        render_us,
        encode_us,
    })
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
