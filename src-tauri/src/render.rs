//! The render service: the single owner of Pdfium and of every open document.
//!
//! Everything Pdfium touches happens on one dedicated thread. This is not a
//! stylistic choice --- `pdfium-render`'s `thread_safe` feature serializes every
//! Pdfium call behind one global mutex, so extra threads buy nothing but
//! contention, and `PdfDocument` is not `Send`. See AGENTS.md.
//!
//! Phase 0 note: this is an in-process service. Spike 0.5 replaces it with
//! supervised worker processes, which is what actually delivers parallelism and
//! the security boundary. The channel interface here is deliberately shaped so
//! that swap does not change callers.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::Instant;

use pdfium_render::prelude::*;

/// Wall-clock origin for the whole process, stamped as early as `main` can.
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Stamps the process start marker. Call first thing in `main`.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

/// Milliseconds since process start, for the startup timeline (spike 0.2).
pub fn since_process_start_ms() -> f64 {
    PROCESS_START
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN)
}

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
    pub pages: Vec<PageSize>,
    /// Time spent in `load_pdf_from_file`, i.e. parse and cross-reference repair.
    pub open_ms: f64,
    /// Milliseconds since process start when the open completed.
    pub at_ms: f64,
}

type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

enum Job {
    Open {
        path: PathBuf,
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
                    Ok(p) => p,
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
                        Job::Open { path, reply } => {
                            reply(open_document(pdfium, &path, &mut docs));
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
    pub fn open(&self, path: PathBuf, reply: Reply<DocumentInfo>) {
        if self.tx.send(Job::Open { path, reply }).is_err() {
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
    docs: &mut Vec<PdfDocument<'static>>,
) -> Result<DocumentInfo, String> {
    let t0 = Instant::now();
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let open_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let pages = doc
        .pages()
        .iter()
        .map(|page| PageSize {
            width_pt: page.width().value,
            height_pt: page.height().value,
        })
        .collect();

    let id = docs.len() as u32;
    docs.push(doc);

    Ok(DocumentInfo {
        id,
        pages,
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
