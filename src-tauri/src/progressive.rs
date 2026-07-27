//! Cancellable tile rendering, driven through PDFium's progressive API.
//!
//! `FPDF_RenderPageBitmap()` cannot be interrupted once entered, so a one-second
//! A0 render occupies the renderer long after the viewport has left the tile
//! that asked for it. `FPDF_RenderPageBitmap_Start()` takes an `IFSDK_PAUSE`
//! whose `NeedToPauseNow()` PDFium polls while it works; returning non-zero
//! suspends the render, and `FPDF_RenderPage_Continue()` resumes it. That is the
//! only cancellation mechanism PDFium offers.
//!
//! ## Why this is raw
//!
//! `PdfDocument::handle`, `PdfPage::page_handle` and `PdfBitmap::handle` are all
//! `pub(crate)` in `pdfium-render`, while the progressive functions are public on
//! `PdfiumLibraryBindings` and take raw handles. So the progressive API **cannot
//! be called on anything the safe API produced** --- the safe wrapper is
//! all-or-nothing, and cancellable rendering means owning `FPDF_DOCUMENT`,
//! `FPDF_PAGE` and `FPDF_BITMAP` ourselves. The RAII types below are that
//! ownership, and nothing more.
//!
//! This points the same way as the worker design, which wants raw handles anyway:
//! a worker renders into a shared mapping, and [`RawBitmap::borrowed`] exists so
//! PDFium can write straight into one rather than into a buffer that then has to
//! be copied (spike 0.5).
//!
//! ## Threading
//!
//! Nothing here makes PDFium thread-safe. Concurrent PDFium calls are undefined
//! behaviour and crash (see `bin/thread_probe.rs`); these handles must be used
//! from one thread at a time. The only thing that legitimately crosses a thread
//! boundary is the [`CancelToken`], which is an `AtomicBool` and touches no
//! PDFium state.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::os::raw::{c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

/// `pdfium-render`'s `bindgen` module is private; only the handle *types* are
/// `pub use`d out of it, so these values are restated rather than imported.
///
/// They are not asserted anywhere directly, because there is nothing to assert
/// them against --- but they are not taken on trust either. A wrong `FPDF_ANNOT`,
/// `FPDF_REVERSE_BYTE_ORDER` or `FPDFBitmap_BGRA` changes the pixels, and the
/// probe's `identity` mode compares this path byte-for-byte against the safe one;
/// a wrong `FPDF_RENDER_DONE` or `FPDF_RENDER_TOBECONTINUED` either hangs the
/// resume loop or reports a completed render as failed.
mod raw {
    use std::os::raw::c_int;

    /// Render annotations that need no user interaction. On by default in
    /// `PdfRenderConfig`, so it is on here.
    pub const FPDF_ANNOT: c_int = 1;
    /// Ask PDFium to emit RGBA rather than BGRA. Also a `PdfRenderConfig`
    /// default, and the reason the safe path's `as_rgba_bytes()` is a no-op
    /// rather than a swizzle.
    pub const FPDF_REVERSE_BYTE_ORDER: c_int = 16;

    /// 8-bit BGRA, the only four-channel format with an alpha byte.
    pub const FPDF_BITMAP_BGRA: c_int = 4;

    /// Opaque white, as `PdfColor::WHITE.as_pdfium_color()` encodes it (ABGR).
    /// Widened to `FPDF_DWORD` at the call site, which is `c_ulong` --- 64 bits
    /// on macOS and 32 on Windows, so it cannot be typed here.
    pub const CLEAR_WHITE: u32 = 0xFFFF_FFFF;

    /// Progressive render status, from `fpdf_progressive.h`. The other two ---
    /// `FPDF_RENDER_READY` (0) and `FPDF_RENDER_FAILED` (3) --- are reported by
    /// value rather than by name, so an unexpected status stays legible.
    pub const FPDF_RENDER_TOBECONTINUED: c_int = 1;
    /// See [`FPDF_RENDER_TOBECONTINUED`].
    pub const FPDF_RENDER_DONE: c_int = 2;
}

/// Flags matching `PdfRenderConfig::default()`, which is what the safe path in
/// `render.rs` uses. Any divergence here shows up as a pixel difference.
const RENDER_FLAGS: c_int = raw::FPDF_ANNOT | raw::FPDF_REVERSE_BYTE_ORDER;

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/// Bindings to the loaded PDFium library.
///
/// `Pdfium` promotes its bindings into a global `OnceCell` on construction, and
/// hands out `&'static` references to it, so a raw handle can be closed without
/// having to carry the `Pdfium` that produced it.
pub type Bindings = &'static dyn PdfiumLibraryBindings;

/// Returns the process-wide bindings, given the `Pdfium` that installed them.
pub fn bindings_of(pdfium: &'static Pdfium) -> Bindings {
    pdfium.bindings()
}

/// How many loaded pages a document keeps alive at once.
///
/// A loaded page holds its parsed content, so caching every page of a 775-page
/// document is not an option. The bound is deliberately small --- a viewport
/// spans one or two pages, plus prefetch --- and is **untuned**: it was chosen to
/// be obviously safe, not measured to be optimal.
const PAGE_CACHE: usize = 4;

/// An `FPDF_DOCUMENT`, closed on drop, with a small cache of loaded pages.
///
/// The cache is not a micro-optimisation. `FPDF_LoadPage` re-parses the page
/// every time --- PDFium does not cache it --- and that costs **44 ms per call on
/// the A0 sheet** against 0.2 ms on the text corpus (`progressive-probe --mode
/// pageload`). Loading per tile request therefore charges a screenful of tiles
/// several hundred milliseconds of pure re-parsing, on precisely the document
/// where there is none to spare.
///
/// It caches the *handles* rather than [`RawPage`] values, which keeps the whole
/// thing safe: a `RawPage` borrows the document, so storing one inside the
/// document would be self-referential. A handle is a plain pointer, copied out
/// under a short borrow, and the document closes every one it holds on drop.
pub struct RawDocument {
    bindings: Bindings,
    handle: FPDF_DOCUMENT,
    /// Loaded page handles, and the order they were loaded in for eviction.
    pages: RefCell<(HashMap<u32, FPDF_PAGE>, VecDeque<u32>)>,
}

impl RawDocument {
    /// Opens a document from a path.
    ///
    /// This is the probe's route in. The viewer's route is a mapped descriptor
    /// handed to a sandboxed worker, which has no path to open --- see the threat
    /// model. Both end at the same `FPDF_DOCUMENT`.
    pub fn open(bindings: Bindings, path: &Path) -> Result<Self, String> {
        let text = path
            .to_str()
            .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))?;

        // SAFETY: `text` outlives the call, and Pdfium copies what it needs.
        let handle = unsafe { bindings.FPDF_LoadDocument(text, None) };
        if handle.is_null() {
            return Err(format!("could not open {}", path.display()));
        }

        Ok(Self {
            bindings,
            handle,
            pages: RefCell::new((HashMap::new(), VecDeque::new())),
        })
    }

    /// How many pages the document has.
    ///
    /// Unlike collecting page *geometry*, this is cheap --- it reads the page
    /// tree's count rather than loading anything (spike 0.2 measured 0.6 ms to
    /// open a 775-page document against 86 ms to size its pages).
    pub fn page_count(&self) -> u32 {
        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        let count = unsafe { self.bindings.FPDF_GetPageCount(self.handle) };
        count.max(0) as u32
    }

    /// Returns one page by zero-based index, loading it if it is not cached.
    pub fn page(&self, index: u32) -> Result<RawPage<'_>, String> {
        if let Some(&handle) = self.pages.borrow().0.get(&index) {
            return Ok(self.borrow_page(handle));
        }

        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        let handle = unsafe { self.bindings.FPDF_LoadPage(self.handle, index as c_int) };
        if handle.is_null() {
            return Err(format!("no such page: {index}"));
        }

        let evicted = {
            let mut pages = self.pages.borrow_mut();
            let (map, order) = &mut *pages;
            map.insert(index, handle);
            order.push_back(index);
            if order.len() > PAGE_CACHE {
                order.pop_front().and_then(|old| map.remove(&old))
            } else {
                None
            }
        };
        // Closed outside the borrow, so a Pdfium call can never re-enter the
        // RefCell and panic.
        if let Some(old) = evicted {
            // SAFETY: loaded by this function, cached since, and now unreachable
            // -- every `RawPage` handed out borrows `self`, so none can be live
            // across a call that mutates the cache.
            unsafe { self.bindings.FPDF_ClosePage(old) };
        }

        Ok(self.borrow_page(handle))
    }

    /// Drops a cached page, closing it.
    ///
    /// Exists so the cache's value can be measured rather than assumed: the probe
    /// evicts between loads to time the uncached path. Also the honest response
    /// to memory pressure.
    pub fn evict_page(&self, index: u32) {
        let handle = {
            let mut pages = self.pages.borrow_mut();
            let (map, order) = &mut *pages;
            order.retain(|i| *i != index);
            map.remove(&index)
        };
        if let Some(handle) = handle {
            // SAFETY: as in `page`.
            unsafe { self.bindings.FPDF_ClosePage(handle) };
        }
    }

    fn borrow_page(&self, handle: FPDF_PAGE) -> RawPage<'_> {
        RawPage {
            bindings: self.bindings,
            handle,
            _document: std::marker::PhantomData,
        }
    }
}

impl Drop for RawDocument {
    fn drop(&mut self) {
        for handle in self.pages.borrow().0.values() {
            // SAFETY: each was loaded by `page` and closed exactly once here.
            unsafe { self.bindings.FPDF_ClosePage(*handle) };
        }
        // SAFETY: closed exactly once, after every page it owns, and every
        // `RawPage` borrows `self` so none can outlive this.
        unsafe { self.bindings.FPDF_CloseDocument(self.handle) };
    }
}

/// A borrowed view of a loaded `FPDF_PAGE`.
///
/// Deliberately **not** an owner: the handle belongs to the [`RawDocument`]'s
/// cache, which closes it on eviction or on drop. The lifetime is what makes
/// that sound --- a page cannot outlive its document, and PDFium does not
/// tolerate one that does.
pub struct RawPage<'doc> {
    bindings: Bindings,
    handle: FPDF_PAGE,
    _document: std::marker::PhantomData<&'doc RawDocument>,
}

impl RawPage<'_> {
    /// Page width in PDF points.
    pub fn width_pt(&self) -> f32 {
        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        unsafe { self.bindings.FPDF_GetPageWidthF(self.handle) }
    }

    /// Page height in PDF points.
    pub fn height_pt(&self) -> f32 {
        // SAFETY: as above.
        unsafe { self.bindings.FPDF_GetPageHeightF(self.handle) }
    }
}

/// An `FPDF_BITMAP` over a caller-owned buffer.
///
/// PDFium is pointed at the buffer rather than allocating its own, so the
/// rendered pixels are already where they need to be --- in a `Vec` the caller
/// can hand on, or in a shared mapping a worker can hand across a process
/// boundary without a copy. `FPDFBitmap_Destroy` explicitly does not free an
/// external buffer, so ownership stays on the Rust side throughout.
pub struct RawBitmap<'buf> {
    bindings: Bindings,
    handle: FPDF_BITMAP,
    width: u16,
    height: u16,
    buffer: &'buf mut [u8],
}

impl<'buf> RawBitmap<'buf> {
    /// Wraps an existing buffer, which must be exactly `width * height * 4` bytes.
    ///
    /// The buffer is not cleared here: [`render`] fills it, matching what the
    /// safe path does. A caller re-using a buffer therefore sees stale pixels
    /// only outside the filled page rect, which is where a freshly allocated
    /// buffer would show zeroes.
    pub fn borrowed(
        bindings: Bindings,
        buffer: &'buf mut [u8],
        width: u16,
        height: u16,
    ) -> Result<Self, String> {
        let stride = width as usize * 4;
        let needed = stride * height as usize;
        if buffer.len() != needed {
            return Err(format!(
                "buffer is {} bytes, need exactly {needed} for {width}x{height}",
                buffer.len()
            ));
        }

        // SAFETY: the buffer outlives the handle -- it is borrowed for `'buf`
        // and `FPDFBitmap_Destroy` in `Drop` does not free external buffers.
        let handle = unsafe {
            bindings.FPDFBitmap_CreateEx(
                width as c_int,
                height as c_int,
                raw::FPDF_BITMAP_BGRA,
                buffer.as_mut_ptr() as *mut c_void,
                stride as c_int,
            )
        };
        if handle.is_null() {
            return Err(format!("could not create a {width}x{height} bitmap"));
        }

        Ok(Self {
            bindings,
            handle,
            width,
            height,
            buffer,
        })
    }

    /// The rendered pixels, RGBA8, `width * height * 4` bytes.
    ///
    /// RGBA rather than BGRA because [`RENDER_FLAGS`] asks PDFium to reverse its
    /// byte order during rendering, which costs nothing --- the alternative is a
    /// 0.27 ms swizzle per 4 MB tile (spike 0.5).
    pub fn pixels(&self) -> &[u8] {
        self.buffer
    }

    /// Bitmap width in device pixels.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Bitmap height in device pixels.
    pub fn height(&self) -> u16 {
        self.height
    }
}

impl Drop for RawBitmap<'_> {
    fn drop(&mut self) {
        // SAFETY: destroyed exactly once; the external buffer is untouched.
        unsafe { self.bindings.FPDFBitmap_Destroy(self.handle) };
    }
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// A flag any thread may set to abandon a render in flight.
///
/// Cheap to clone; every clone refers to the same flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that has not been cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the render to stop at the next poll.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// State shared with the C pause callback.
///
/// Every field the callback touches is atomic, so the callback needs only a
/// shared reference. That is deliberate: taking `&mut` inside the callback would
/// alias whatever the Rust side holds across the FFI call.
struct PauseState {
    cancel: CancelToken,
    /// Nanoseconds after `origin` at which to pause regardless of cancellation,
    /// or [`NEVER`] for "run to completion". Re-armed by the caller between
    /// resumes.
    deadline_ns: AtomicU64,
    origin: Instant,
    polls: AtomicU64,
    /// Nanoseconds after `origin` of the previous poll, or [`NEVER`] if none yet.
    last_poll_ns: AtomicU64,
    /// Longest observed interval between two consecutive polls. This is the real
    /// bound on how late a cancellation can be noticed, and it is not the same
    /// number as the slice: the slice says when we *ask* to pause, this says how
    /// long PDFium can go without asking.
    max_gap_ns: AtomicU64,
}

/// "No deadline" / "no previous poll", as a nanosecond count that cannot occur.
///
/// It is `u64::MAX` and **not zero**, which was a real bug rather than a
/// stylistic preference. `Instant` on Apple Silicon ticks at the 24 MHz timebase,
/// so its resolution is about 41.67 ns and two reads that close together return
/// the same value. `arm()` runs a few nanoseconds after `origin` is taken, so
/// with a zero slice `origin.elapsed()` is genuinely 0 --- colliding with the
/// old sentinel and turning "pause at the first opportunity" into "run to
/// completion". It reproduced only sometimes, because whether the two reads land
/// in the same tick depends on what else the caller did in between.
const NEVER: u64 = u64::MAX;

impl PauseState {
    fn new(cancel: CancelToken) -> Self {
        Self {
            cancel,
            deadline_ns: AtomicU64::new(NEVER),
            origin: Instant::now(),
            polls: AtomicU64::new(0),
            last_poll_ns: AtomicU64::new(NEVER),
            max_gap_ns: AtomicU64::new(0),
        }
    }

    fn arm(&self, slice: Option<Duration>) {
        let deadline = match slice {
            Some(slice) => (self.origin.elapsed() + slice).as_nanos() as u64,
            None => NEVER,
        };
        self.deadline_ns.store(deadline, Ordering::Relaxed);
    }
}

/// `IFSDK_PAUSE::NeedToPauseNow`. Non-zero suspends the render.
///
/// # Safety
///
/// PDFium calls this with the pointer it was handed, whose `user` field this
/// module set to a live `PauseState` that outlives the render.
unsafe extern "C" fn need_to_pause_now(this: *mut IFSDK_PAUSE) -> FPDF_BOOL {
    // SAFETY: see the contract above.
    let state = unsafe { &*((*this).user as *const PauseState) };

    let now = state.origin.elapsed().as_nanos() as u64;
    state.polls.fetch_add(1, Ordering::Relaxed);

    let last = state.last_poll_ns.swap(now, Ordering::Relaxed);
    if last != NEVER {
        state
            .max_gap_ns
            .fetch_max(now.saturating_sub(last), Ordering::Relaxed);
    }

    let expired = match state.deadline_ns.load(Ordering::Relaxed) {
        NEVER => false,
        deadline => now >= deadline,
    };

    FPDF_BOOL::from(state.cancel.is_cancelled() || expired)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Where the page sits inside the bitmap, in device pixels.
///
/// A tile is a window onto the scaled page: `size` is the *whole* page at the
/// requested scale, and `start` is a negative offset that slides the wanted
/// region into the bitmap. PDFium clips, so no full-page bitmap is ever
/// allocated.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// Left edge of the scaled page in bitmap coordinates, usually negative.
    pub start_x: i32,
    /// Top edge of the scaled page in bitmap coordinates, usually negative.
    pub start_y: i32,
    /// Width of the whole scaled page in device pixels.
    pub size_x: i32,
    /// Height of the whole scaled page in device pixels.
    pub size_y: i32,
}

impl Placement {
    /// The placement for a tile at `(x, y)` of a page scaled by `scale`.
    pub fn tile(page: &RawPage<'_>, scale: f32, x: i32, y: i32) -> Self {
        Self {
            start_x: -x,
            start_y: -y,
            size_x: (page.width_pt() * scale).round() as i32,
            size_y: (page.height_pt() * scale).round() as i32,
        }
    }
}

/// How a render ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// PDFium reported the page fully rendered.
    Done,
    /// Abandoned because the token was cancelled. The bitmap holds however much
    /// had been drawn, which is a usable partial tile rather than garbage.
    Cancelled,
    /// PDFium reported a failure, or a status this code does not know. The raw
    /// status is carried so an unexpected one is diagnosable rather than
    /// flattened into "it didn't work".
    Failed(c_int),
}

impl Outcome {
    /// Whether the bitmap holds a complete render of the requested region.
    pub fn is_done(self) -> bool {
        self == Self::Done
    }
}

/// What a render cost, and what it did.
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    /// How the render ended.
    pub outcome: Outcome,
    /// How many times PDFium asked whether to pause.
    ///
    /// **Zero makes any cancellation claim vacuous**: a render that never
    /// yielded was never interruptible, and would have finished identically with
    /// no pause callback at all. Assert on this, not on the outcome.
    pub polls: u64,
    /// How many times the render was resumed after a pause.
    pub resumes: u64,
    /// Longest interval between consecutive polls --- the bound on cancellation
    /// latency, independent of how small a slice the caller asks for.
    pub max_poll_gap: Duration,
    /// Wall time from `Start` to `Close`.
    pub elapsed: Duration,
}

/// Renders a page into a bitmap, pausing so it can be abandoned.
///
/// `slice` bounds how long PDFium runs before handing control back; `None` runs
/// to completion, still polling so cancellation is possible but never pausing of
/// its own accord. Cancellation is checked at every pause, and the pause callback
/// itself returns "pause now" the moment the token is set, so the latency is one
/// poll interval rather than one slice.
///
/// The flags, clear colour and cleared rect match `PdfRenderConfig::default()`
/// exactly, so an uncancelled render here is byte-identical to the safe path ---
/// with one documented exception: the safe path also calls `FPDF_FFLDraw` to
/// overlay interactive form-field appearances, and this does not. Documents with
/// form widgets will differ until that pass exists.
pub fn render(
    bitmap: &mut RawBitmap<'_>,
    page: &RawPage<'_>,
    placement: Placement,
    slice: Option<Duration>,
    cancel: &CancelToken,
) -> Progress {
    let bindings = bitmap.bindings;
    let started = Instant::now();

    // Match `render_into_bitmap_with_settings`: fill the *page* rect, not the
    // bitmap. Where a tile overhangs the page the buffer keeps whatever it came
    // in with, which for a fresh allocation is transparent black.
    // SAFETY: handles are live; Pdfium clips the rect to the bitmap.
    unsafe {
        bindings.FPDFBitmap_FillRect(
            bitmap.handle,
            placement.start_x,
            placement.start_y,
            placement.size_x,
            placement.size_y,
            raw::CLEAR_WHITE as FPDF_DWORD,
        );
    }

    let state = PauseState::new(cancel.clone());
    state.arm(slice);

    let mut pause = IFSDK_PAUSE {
        version: 1,
        NeedToPauseNow: Some(need_to_pause_now),
        user: &state as *const PauseState as *mut c_void,
    };
    let pause_ptr: *mut IFSDK_PAUSE = &mut pause;

    // SAFETY: `state` and `pause` outlive every call below, and no Rust
    // reference into `state` is held across one -- the callback reaches it
    // through atomics only.
    let mut status = unsafe {
        bindings.FPDF_RenderPageBitmap_Start(
            bitmap.handle,
            page.handle,
            placement.start_x,
            placement.start_y,
            placement.size_x,
            placement.size_y,
            0, // no rotation; the caller bakes orientation into `placement`
            RENDER_FLAGS,
            pause_ptr,
        )
    };

    let mut resumes = 0u64;
    while status == raw::FPDF_RENDER_TOBECONTINUED && !cancel.is_cancelled() {
        state.arm(slice);
        // SAFETY: as above.
        status = unsafe { bindings.FPDF_RenderPage_Continue(page.handle, pause_ptr) };
        resumes += 1;
    }

    // Required after finishing *and* after cancelling; skipping it on the
    // cancelled path leaks the render's scratch state onto the page.
    // SAFETY: exactly one open render on this page, started above.
    unsafe { bindings.FPDF_RenderPage_Close(page.handle) };

    // The loop only exits on a status that is not TOBECONTINUED, or on
    // cancellation -- so a TOBECONTINUED here means we abandoned it. Anything
    // else is a failure, including a status this code does not know: PDFium also
    // defines FPDF_RENDER_READY, and silently treating an unexpected value as
    // success would hand back a bitmap nothing had drawn into.
    let outcome = match status {
        raw::FPDF_RENDER_DONE => Outcome::Done,
        raw::FPDF_RENDER_TOBECONTINUED => Outcome::Cancelled,
        other => Outcome::Failed(other),
    };

    Progress {
        outcome,
        polls: state.polls.load(Ordering::Relaxed),
        resumes,
        max_poll_gap: Duration::from_nanos(state.max_gap_ns.load(Ordering::Relaxed)),
        elapsed: started.elapsed(),
    }
}

/// Which region of a page to render, at what resolution.
#[derive(Clone, Copy, Debug)]
pub struct TileSpec {
    /// Device pixels per PDF point.
    pub scale: f32,
    /// Tile origin in device pixels, relative to the scaled page's top-left.
    pub x: i32,
    /// See [`TileSpec::x`].
    pub y: i32,
    /// Tile size in device pixels.
    pub width: u16,
    /// See [`TileSpec::width`].
    pub height: u16,
}

/// Convenience for the common case: allocate, render to completion, return pixels.
///
/// Takes a token so a caller can still abandon it; pass a fresh one to opt out.
pub fn render_tile(
    bindings: Bindings,
    page: &RawPage<'_>,
    spec: TileSpec,
    slice: Option<Duration>,
    cancel: &CancelToken,
) -> Result<(Vec<u8>, Progress), String> {
    let TileSpec {
        scale,
        x,
        y,
        width,
        height,
    } = spec;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let placement = Placement::tile(page, scale, x, y);

    let progress = {
        let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
        render(&mut bitmap, page, placement, slice, cancel)
    };

    Ok((buffer, progress))
}
