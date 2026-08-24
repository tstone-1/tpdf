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
//! behaviour and crash (see `examples/thread_probe.rs`); these handles must be used
//! from one thread at a time. The only thing that legitimately crosses a thread
//! boundary is the [`CancelToken`], which is an `AtomicBool` and touches no
//! PDFium state.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::os::raw::{c_int, c_void};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

use crate::annots::{self, Comments};
use crate::docinfo::{self, Properties};
use crate::encoding::{self, PageMapping};
use crate::links::{self, Links};
use crate::pagetree;

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

/// Loads PDFium from `library_dir`. In a worker, must run *before* the sandbox.
///
/// Public so a probe can exercise *this* binding rather than a copy of it: a
/// feasibility check that reimplements the thing it is checking measures the
/// reimplementation.
///
/// It lives here rather than in `worker_child`, where it was written, because
/// that module is `#[cfg(unix)]` --- so on Windows the one binding a probe was
/// meant to share was the one it could not reach, and the intent above quietly
/// inverted into an invitation to copy. Three probes had already copied it.
///
/// # Errors
///
/// The library being absent or unloadable at `library_dir`.
pub fn bind(library_dir: &Path) -> Result<&'static Pdfium, String> {
    let path = Pdfium::pdfium_platform_library_name_at_path(library_dir);
    let bindings = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(Box::leak(Box::new(Pdfium::new(bindings))))
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
/// Where a document's bytes came from, so its object graph can be read as well
/// as rendered.
///
/// PDFium answers questions about *drawing*; some questions are only answerable
/// from the file's own structure --- whether a font states what its glyphs mean
/// is the one that forced this (`crate::encoding`). PDFium exposes no API for it,
/// so the bytes have to be reachable a second time.
///
/// Two variants because the two backends genuinely differ: a worker is handed a
/// mapping and never learns a path, which is the property `docs/THREAT-MODEL.md`
/// §3 rests on, and the in-process backend has only a path.
enum Source {
    /// The mapping the worker was handed. Already in memory; no re-read.
    Bytes(&'static [u8]),
    /// A path the in-process backend opened. Read on demand, once.
    Path(PathBuf),
}

pub struct RawDocument {
    bindings: Bindings,
    handle: FPDF_DOCUMENT,
    form: Option<RawForm>,
    /// Loaded page handles, and the order they were loaded in for eviction.
    pages: RefCell<(HashMap<u32, FPDF_PAGE>, VecDeque<u32>)>,
    /// Each page's `/CropBox` as the file states it, read before any override.
    ///
    /// **PDFium has no "put it back": `FPDFPage_SetCropBox` overwrites, and
    /// `FPDFPage_GetCropBox` then answers with whatever was written.** So the
    /// only moment the file's own box can be read is before the first override,
    /// which is what [`RawDocument::page_cropped`] does on the load.
    ///
    /// It has to be remembered at all because **pages are cached** (see
    /// [`PAGE_CACHE`]): a page handed to a request that cropped it is the same
    /// handle the next request gets, so a crop set once is in force until
    /// something sets it back.
    original_crops: RefCell<HashMap<u32, [f32; 4]>>,
    /// Where to find the bytes again, for questions PDFium cannot answer.
    source: Source,
    /// The password this document was opened with, if it needed one.
    ///
    /// **Held because `lopdf` needs it too, and it is the same key to the same
    /// bytes.** Every question PDFium cannot answer --- comments, links,
    /// properties, the character mapping, and the update section a save appends
    /// --- is a second parse of `source`, and a parse without the password reads
    /// *no objects at all*: `lopdf` returns a `Document` that loads cleanly and
    /// reports zero pages. So a locked document would open, render and search
    /// while its comments, links and properties came back empty, and the save
    /// path would refuse it.
    ///
    /// What this costs is one more copy of the password in a process that
    /// already has it: `Workers::open` holds it for the document's lifetime so
    /// that a pool growing under contention can unlock its new workers, which
    /// `docs/THREAT-MODEL.md` §T6.9 states. This is the worker's own copy, in
    /// the process that is sandboxed.
    password: Option<String>,
    /// Per-page character-mapping verdicts, computed at most once.
    ///
    /// Lazy, and **not for the reason first written here**. The original comment
    /// said this costs a full `lopdf` parse and that on a 337 MB document that is
    /// the dominant cost of opening one --- a guess, and wrong. Measured in
    /// release: 0.1 ms small, 5.8 ms on the 775-page document, 11.9 ms on the
    /// 337 MB scan, because `lopdf` reads the xref and object headers rather than
    /// every stream and the cost tracks object count, not bytes.
    ///
    /// It is still lazy, because warm startup is ~276 ms against a 300 ms target
    /// (`docs/PLAN.md` §4) and 6--12 ms is a quarter of the whole margin. Off the
    /// critical path that is free; on it, it is expensive. So it is computed when
    /// first asked for --- after first paint, by a search that found nothing or by
    /// the accessibility layer --- and cached for the document's lifetime.
    mapping: OnceCell<Vec<PageMapping>>,
    /// Every comment in the document, read at most once.
    ///
    /// Lazy for the same reason [`RawDocument::mapping`] is, and cached for the
    /// same one: it costs a second `lopdf` parse of the whole file, nothing on
    /// the startup path asks for it, and a reader who opens the comments panel
    /// twice should pay for it once. The `Result` is cached too --- a document
    /// that could not be read does not become readable on the second attempt,
    /// and re-parsing to rediscover that is the same work for the same answer.
    comments: OnceCell<Result<Comments, String>>,
    /// Every link in the document, read at most once.
    ///
    /// Lazy and cached for the same reasons as [`RawDocument::comments`], with
    /// one difference in when it is asked for: the viewer wants links as soon as
    /// a page is on screen rather than when a panel opens, so this is warmed
    /// just after first paint instead of on demand.
    links: OnceCell<Result<Links, String>>,
    /// What the document says about itself, read at most once.
    ///
    /// Lazy and cached like [`RawDocument::comments`], and the laziest of the
    /// three: nothing asks for this until a reader opens the properties dialog,
    /// which most never will.
    properties: OnceCell<Result<Properties, String>>,
    /// The box each page is displayed from, per the page tree, read at most once.
    ///
    /// **Only consulted for a page PDFium has no `/MediaBox` for**, which is the
    /// tell that the page inherits one --- `FPDFPage_GetMediaBox` does not walk
    /// `/Parent`. So this is another whole-file `lopdf` parse, and on the
    /// overwhelming majority of documents it never happens at all: a page that
    /// states its own box never asks.
    ///
    /// Lazy and cached like [`RawDocument::comments`], and for one more reason
    /// than the others: it is read on the path that loads a page, which is a
    /// path a reader waits on. A document that needs it pays once.
    sheets: OnceCell<Result<Vec<[f32; 4]>, String>>,
}

/// Which box to lay a page out from, given both readings of it.
///
/// **PDFium wins wherever it answers.** It is the engine that renders, so a box
/// it already agrees with makes every downstream number consistent by
/// construction, and preferring a second opinion there could only introduce a
/// disagreement between the size a page reports and the pixels it produces.
///
/// `media` is PDFium's `/MediaBox` reading and is the *discriminator*, not the
/// answer: `FPDFPage_GetMediaBox` does not walk `/Parent`, so `None` means the
/// page inherits its box from an ancestor --- and that is exactly the page
/// `FPDF_GetPageWidthF` reports `width x width` for when it also carries a
/// quarter turn. The answer is `crop`, which is the crop box intersected with
/// the media box and the rectangle everything downstream is measured from.
///
/// A free function, and small enough that the reason is worth stating: as a
/// branch inside the method it would sit beside two FFI calls, where no test can
/// reach it --- which this project records as a guard written inline with an FFI
/// call being reachable by nothing.
pub(crate) fn box_to_use(
    media: Option<[f32; 4]>,
    crop: [f32; 4],
    tree: Option<[f32; 4]>,
) -> [f32; 4] {
    match (media, tree) {
        // PDFium has no sheet for this page and the page tree does. The only
        // case where anything changes.
        (None, Some(from_tree)) => from_tree,
        // Either PDFium answered, or nothing did. `crop` is PDFium's own
        // reading in both, which leaves a document whose bytes could not be
        // re-read exactly as it was rather than refused.
        _ => crop,
    }
}

/// `FPDF_ERR_*` codes from `fpdfview.h`.
mod err {
    use std::os::raw::c_ulong;

    pub const FILE: c_ulong = 2;
    pub const FORMAT: c_ulong = 3;
    pub const PASSWORD: c_ulong = 4;
    pub const SECURITY: c_ulong = 5;
}

/// Why a document would not open, in words a reader can act on.
///
/// **The message this replaces was a wrong diagnosis, not a vague one.** Both
/// open paths reported "could not open" or "could not parse N bytes as a PDF"
/// whatever had happened --- so a document that is entirely well formed and
/// merely locked was announced as corrupt. Measured on this machine: 3 of the 39
/// PDFs in a real Downloads folder carry `/Encrypt`, and a reader meeting one
/// was told their file was broken.
///
/// PDFium keeps the reason and it costs one call to ask. Note that it keeps
/// **one error per thread** and any later call overwrites it, so this is only
/// meaningful immediately after the failure it describes --- which is why the
/// code is read at the call site and passed in rather than fetched here.
///
/// The wording says what a reader can do next, because that is the only part of
/// an error message that changes anyone's afternoon. Nothing here comes from the
/// document: these are four sentences chosen in this file, so no error path can
/// become a route for a string the file wrote.
pub fn open_failure(code: std::os::raw::c_ulong) -> String {
    match code {
        err::PASSWORD => "This document is locked, and needs a password.".into(),
        err::SECURITY => "This document uses a security scheme tpdf cannot read.".into(),
        err::FORMAT => "This file is not a PDF, or it is damaged beyond reading.".into(),
        err::FILE => "This file could not be read from disk.".into(),
        // Including `FPDF_ERR_SUCCESS`, which is reachable: PDFium can return a
        // null handle with no error set, and reporting "no error" for a document
        // that did not open is worse than admitting the reason is unknown.
        _ => "This document could not be opened, and PDFium did not say why.".into(),
    }
}

/// Why a document would not open, in a form a caller can branch on.
///
/// A string was enough while every refusal was final. It stopped being enough
/// when one of them became a *question*: a locked document is not damaged, and a
/// caller that paints "this document is locked" the way it paints "this file is
/// not a PDF" tells the reader to go and find a better copy of a file that is
/// perfectly good. So the one distinction anything acts on is a field, and the
/// rest stays prose.
///
/// [`locked`](Self::locked) is deliberately *not* three-valued. PDFium answers
/// `FPDF_ERR_PASSWORD` for a document given no password and for one given the
/// wrong password alike --- measured, not assumed --- so the worker cannot tell
/// them apart and does not pretend to. Whether the reader has already tried is a
/// fact about the conversation, and it belongs to whoever is holding it.
///
/// `Serialize` because it is what a Tauri command answers with: the frontend
/// needs the distinction as much as the pool does, and a serialised `{reason,
/// locked}` is what lets it show a password prompt for one refusal and an error
/// for every other. Nothing deserialises it --- it travels one way.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Refusal {
    /// What to show a reader. Chosen in this file; never from the document.
    pub reason: String,
    /// The document is encrypted, and the password it was given --- which may
    /// have been none --- did not open it.
    pub locked: bool,
}

impl Refusal {
    /// Words an `FPDF_ERR_*` code and says whether it is answerable.
    ///
    /// Separate from [`last`](Self::last) so that there is something to test: the
    /// call that *reads* the code needs bindings and a failed load in front of
    /// it, and this needs neither. What could go wrong here is the flag being
    /// set for the wrong code --- prompting for a password on a file that is not
    /// a PDF, or reporting a locked document as damaged --- and that is a
    /// property of this line alone.
    #[must_use]
    pub fn of(code: std::os::raw::c_ulong) -> Self {
        Self {
            reason: open_failure(code),
            locked: code == err::PASSWORD,
        }
    }

    /// Reads PDFium's last error and words it.
    ///
    /// # Safety
    ///
    /// Meaningful only immediately after the failed load it describes: PDFium
    /// keeps one error per thread and the next call overwrites it.
    unsafe fn last(bindings: Bindings) -> Self {
        // SAFETY: no arguments, and the contract above is the caller's.
        Self::of(unsafe { bindings.FPDF_GetLastError() })
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl From<Refusal> for String {
    fn from(refusal: Refusal) -> Self {
        refusal.reason
    }
}

/// Widens a plain failure into a refusal, which is always *not* locked.
///
/// Safe in that direction and only in that one: a `String` carries no
/// locked-ness, so nothing is being inferred here --- a caller that had the
/// distinction would not have thrown it away first. It exists so that `?` works
/// on the ordinary failures inside an open, which are page geometry and are
/// about as far from encryption as this module gets.
impl From<String> for Refusal {
    fn from(reason: String) -> Self {
        Self {
            reason,
            locked: false,
        }
    }
}

impl From<&str> for Refusal {
    fn from(reason: &str) -> Self {
        Self::from(reason.to_string())
    }
}

/// PDFium's form-fill environment, retained for exactly the document lifetime.
///
/// Even a read-only viewer needs this: PDFium does not load or draw interactive
/// widget values until the environment exists. The callback table is pinned
/// because PDFium retains its address until `FPDFDOC_ExitFormFillEnvironment`;
/// moving it before then makes the final call dereference stale memory.
struct RawForm {
    bindings: Bindings,
    handle: FPDF_FORMHANDLE,
    _info: Pin<Box<FPDF_FORMFILLINFO>>,
}

impl RawForm {
    /// Creates the inert, no-JavaScript form environment used for rendering.
    fn open(bindings: Bindings, document: FPDF_DOCUMENT) -> Option<Self> {
        // SAFETY: every field in FPDF_FORMFILLINFO is an integer, raw pointer,
        // or Option<extern fn>; all-zero is therefore its documented inert
        // configuration. Version 2 matches pdfium-render and keeps XFA disabled
        // explicitly even though the pinned PDFium build contains no XFA.
        let mut info: Pin<Box<FPDF_FORMFILLINFO>> = Box::pin(unsafe { std::mem::zeroed() });
        info.as_mut().get_mut().version = 2;
        info.as_mut().get_mut().xfa_disabled = 1;

        // SAFETY: `info` is pinned and remains owned by the returned RawForm.
        let handle =
            unsafe { bindings.FPDFDOC_InitFormFillEnvironment(document, info.as_mut().get_mut()) };
        if handle.is_null() {
            return None;
        }

        let form = Self {
            bindings,
            handle,
            _info: info,
        };

        // Match pdfium-render: an empty environment is closed immediately and
        // not carried on every page. FPDF_GetFormType returns FORMTYPE_NONE (0)
        // when there is no AcroForm or XFA form to draw. Returning drops the
        // owner, so this path cannot leak the environment.
        let form_type = unsafe { bindings.FPDF_GetFormType(document) };
        if form_type == 0 {
            return None;
        }

        Some(form)
    }
}

impl Drop for RawForm {
    fn drop(&mut self) {
        // SAFETY: this owner was created by InitFormFillEnvironment; its pinned
        // callback table is still a live field and the document closes pages
        // before dropping the form.
        unsafe { self.bindings.FPDFDOC_ExitFormFillEnvironment(self.handle) };
    }
}

impl RawDocument {
    /// Opens a document from a path.
    ///
    /// This is the probe's route in. The viewer's route is a mapped descriptor
    /// handed to a sandboxed worker, which has no path to open --- see the threat
    /// model. Both end at the same `FPDF_DOCUMENT`.
    pub fn open(bindings: Bindings, path: &Path, password: Option<&str>) -> Result<Self, Refusal> {
        let text = path.to_str().ok_or_else(|| Refusal {
            reason: format!("path is not UTF-8: {}", path.display()),
            locked: false,
        })?;

        // SAFETY: `text` outlives the call, and Pdfium copies what it needs.
        let handle = unsafe { bindings.FPDF_LoadDocument(text, password) };
        if handle.is_null() {
            // SAFETY: read immediately after the failure it describes --- PDFium
            // keeps one error per thread and the next call overwrites it.
            return Err(unsafe { Refusal::last(bindings) });
        }

        let form = RawForm::open(bindings, handle);
        Ok(Self {
            bindings,
            handle,
            form,
            pages: RefCell::new((HashMap::new(), VecDeque::new())),
            original_crops: RefCell::new(HashMap::new()),
            source: Source::Path(path.to_path_buf()),
            password: password.map(str::to_string),
            mapping: OnceCell::new(),
            comments: OnceCell::new(),
            links: OnceCell::new(),
            properties: OnceCell::new(),
            sheets: OnceCell::new(),
        })
    }

    /// Opens a document from bytes that outlive it.
    ///
    /// The worker's route in, and the reason the sandbox can exist: a mapped
    /// descriptor has no path, so a policy that denies `file-read*` outright
    /// still leaves the document reachable. PDFium does **not** copy the buffer
    /// --- it reads from it for as long as the document is open, which is why
    /// this takes `&'static [u8]` rather than a borrow the caller could end.
    ///
    /// `password` is the reader's, when they have supplied one. It is a key to
    /// bytes this process already holds rather than a new authority: a worker
    /// that cannot read them is a worker that renders nothing, so nothing here
    /// widens what a compromised one could reach. It **is** stored on the
    /// document, and [`RawDocument::password`] says why --- `lopdf` needs the
    /// same key for every question PDFium cannot answer.
    ///
    /// **A failed load poisons nothing**, which is what makes retrying in place
    /// legal rather than merely plausible. Measured on
    /// `testdata/incr-encrypted-pw.pdf` (AES-256, user password `swordfish`):
    /// loading the same bytes with no password, then the right one, then a wrong
    /// one, then the right one again, in one process, opens on both correct
    /// attempts and refuses on both others. `docs/PLAN.md` §5 has the run.
    ///
    /// # Errors
    ///
    /// Bytes PDFium will not parse, or an encrypted document whose password this
    /// was not. The two are distinguished by [`Refusal::locked`], because only
    /// the second is worth asking a reader about.
    pub fn open_bytes(
        bindings: Bindings,
        bytes: &'static [u8],
        password: Option<&str>,
    ) -> Result<Self, Refusal> {
        // SAFETY: the buffer is `'static`, so it outlives the document, which is
        // exactly what this call requires and what the safe wrapper cannot
        // express.
        let handle = unsafe { bindings.FPDF_LoadMemDocument64(bytes, password) };
        if handle.is_null() {
            // SAFETY: as in `open` --- read immediately after the failure.
            return Err(unsafe { Refusal::last(bindings) });
        }

        let form = RawForm::open(bindings, handle);
        Ok(Self {
            bindings,
            handle,
            form,
            pages: RefCell::new((HashMap::new(), VecDeque::new())),
            original_crops: RefCell::new(HashMap::new()),
            source: Source::Bytes(bytes),
            password: password.map(str::to_string),
            mapping: OnceCell::new(),
            comments: OnceCell::new(),
            links: OnceCell::new(),
            properties: OnceCell::new(),
            sheets: OnceCell::new(),
        })
    }

    /// The bindings this document was opened through.
    pub fn bindings(&self) -> Bindings {
        self.bindings
    }

    /// The raw `FPDF_DOCUMENT`.
    ///
    /// Valid for the borrow, and only for the thread the document was opened
    /// on --- concurrent PDFium is undefined behaviour, see the module docs.
    pub fn handle(&self) -> FPDF_DOCUMENT {
        self.handle
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

    /// Per-page verdicts on whether the text means anything, computed once.
    ///
    /// Always exactly `page_count()` long, so index `n` is page `n`.
    ///
    /// **Every failure is "unknown", never "clean".** Bytes that cannot be read,
    /// a document `lopdf` refuses, a page it cannot account for --- all produce a
    /// `PageMapping` with `truncated` set and `certain()` false. That is the rule
    /// `docs/PLAN.md` §6 states for a redaction verification, and it applies here
    /// for the same reason: this exists so a reader is not told "no matches" on a
    /// page nobody could search, and a scan that failed silently reporting clean
    /// would reinstate exactly that.
    pub fn mapping(&self) -> &[PageMapping] {
        self.mapping.get_or_init(|| {
            let count = self.page_count() as usize;
            let unknown = || {
                vec![
                    PageMapping {
                        truncated: true,
                        ..PageMapping::default()
                    };
                    count
                ]
            };
            let Some(bytes) = self.source_bytes() else {
                return unknown();
            };
            encoding::scan(&bytes, count, self.password()).unwrap_or_else(|_| unknown())
        })
    }

    /// Every comment in the document, read at most once.
    ///
    /// A failure is kept as a failure rather than answered with an empty list:
    /// "this document has no comments" and "this document could not be read"
    /// are different things to tell a reader, and only one of them is
    /// reassuring. See `crate::annots`.
    pub fn comments(&self) -> Result<Comments, String> {
        self.comments
            .get_or_init(|| {
                let bytes = self
                    .source_bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                annots::scan(&bytes, self.page_count() as usize, self.password())
            })
            .clone()
    }

    /// The update section for a save that only adds marks.
    ///
    /// Deliberately **not** cached, where the comments, links, mapping and
    /// properties beside it are. Those four are read-only facts about the
    /// document: asked for repeatedly, identical every time. This is a function
    /// of the *plan*, which differs on every save, so a cache keyed on the
    /// document would answer a second save with the first save's bytes --- and
    /// silently, because those bytes are a perfectly valid update section for a
    /// document that no longer matches them.
    ///
    /// # Errors
    ///
    /// The document's bytes are unreadable, or [`crate::save::append_update`]
    /// refuses --- see there for the reasons, all of which are about the document
    /// or the plan rather than about this process.
    pub fn append(&self, plan: &crate::edits::Plan) -> Result<crate::save::Update, String> {
        let bytes = self
            .source_bytes()
            .ok_or_else(|| "the document's bytes could not be read".to_string())?;
        // `into_owned` is the one copy this path makes, and it is the same copy
        // the coordinator used to make with `std::fs::read`. The document
        // arrives as a read-only mapping and `lopdf` needs an owned buffer, so
        // the copy is `IncrementalDocument::create_from`'s requirement rather
        // than a choice made here --- see the note beside it, including what
        // `worker-probe` measured when this was moved from there to here and
        // nothing changed.
        crate::save::append_update(bytes.into_owned(), plan, self.password())
            .map_err(|why| why.message)
    }

    /// Every link in the document, read at most once.
    ///
    /// A failure is kept as a failure, for the reason above: a document whose
    /// links could not be read is a document whose cross-references silently do
    /// nothing, and the reader is better told than left clicking.
    pub fn links(&self) -> Result<Links, String> {
        self.links
            .get_or_init(|| {
                let bytes = self
                    .source_bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                links::scan(&bytes, self.page_count() as usize, self.password())
            })
            .clone()
    }

    /// What the document says about itself, read at most once.
    ///
    /// # Errors
    ///
    /// The bytes not being readable, or `lopdf` refusing to parse them.
    pub fn properties(&self) -> Result<Properties, String> {
        self.properties
            .get_or_init(|| {
                let bytes = self
                    .source_bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                docinfo::scan(&bytes, self.page_count(), self.password())
            })
            .clone()
    }

    /// The box a page is displayed from: PDFium's reading, or the page tree's.
    ///
    /// **PDFium is asked first and believed wherever it answers.** It is the
    /// engine that renders, so a box it agrees with is a box every downstream
    /// number is already consistent with, and a second opinion could only
    /// introduce a disagreement. The page tree is consulted for exactly one
    /// case: a page PDFium has no `/MediaBox` for.
    ///
    /// That case is not obscure and it is not benign. `FPDFPage_GetMediaBox`
    /// does not walk `/Parent`, so a page inheriting its box from an ancestor
    /// --- what any producer emitting uniform pages writes --- gets no answer;
    /// `FPDF_GetPageWidthF` then reports `width x width` for one that also
    /// carries a quarter turn, which is what a scanner writes. `docs/TRAPS.md`
    /// has the crossed measurements.
    ///
    /// The fallback is `crop_pt` again rather than a refusal, which is
    /// deliberate: a document whose bytes cannot be re-read, or which `lopdf`
    /// and PDFium disagree about the length of, is a document this makes no
    /// worse than it was. Nothing here can *fail*; it can only decline to
    /// improve.
    fn original_box(&self, page: &RawPage<'_>, index: u32) -> [f32; 4] {
        let media = page.media_pt();
        // The short-circuit, and it is the whole cost story: a document every
        // page of which states its own box never parses anything here. Asking
        // `sheet` first would be the same answer for a whole `lopdf` parse of
        // every file opened. `consulted_page_tree` is what makes that
        // observable, and `geometry-probe` reads it in both directions.
        let tree = if media.is_none() {
            self.sheet(index)
        } else {
            None
        };
        box_to_use(media, page.crop_pt(), tree)
    }

    /// Whether the page tree has been parsed for this document yet.
    ///
    /// **An accounting observable.** [`original_box`](Self::original_box) is
    /// meant to reach `lopdf` only for a document PDFium cannot give a
    /// `/MediaBox` for, and "it never happened" is invisible from outside ---
    /// every number a caller can see is identical either way, because the two
    /// agree wherever both answer. So the property that would silently be lost
    /// is the one this exists to let a check assert.
    pub fn consulted_page_tree(&self) -> bool {
        self.sheets.get().is_some()
    }

    /// One page's box out of the page tree, parsing the document at most once.
    ///
    /// See [`RawDocument::sheets`] for why this is lazy and why most documents
    /// never reach it.
    fn sheet(&self, index: u32) -> Option<[f32; 4]> {
        self.sheets
            .get_or_init(|| {
                let bytes = self
                    .source_bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                pagetree::displayed_boxes(&bytes, self.page_count() as usize, self.password())
            })
            .as_ref()
            .ok()
            .and_then(|boxes| boxes.get(index as usize).copied())
    }

    /// The password this document was opened with, for a parser that needs it.
    ///
    /// Every caller is a `lopdf` parse of the same bytes PDFium already holds
    /// open, in the same process. See the field.
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// The document's bytes, however it was opened.
    ///
    /// A worker holds the mapping and can borrow it; a probe opened a path and
    /// has to read it back, because PDFium keeps no copy anything here can
    /// reach. `None` is a file that has gone or become unreadable since it was
    /// opened --- which is a real state, not a defect: see `docs/PLAN.md` §5 on
    /// external modification.
    fn source_bytes(&self) -> Option<std::borrow::Cow<'_, [u8]>> {
        match &self.source {
            Source::Bytes(bytes) => Some(std::borrow::Cow::Borrowed(*bytes)),
            Source::Path(path) => std::fs::read(path).ok().map(std::borrow::Cow::Owned),
        }
    }

    /// Returns one page by zero-based index, loading it if it is not cached.
    ///
    /// **Private, and [`page`](Self::page) is the door.** A page handed straight
    /// out of the cache carries whatever crop box the last caller set on it, so
    /// this is only correct for a caller that then sets one --- which is exactly
    /// the two that do.
    fn load_page(&self, index: u32) -> Result<RawPage<'_>, String> {
        if let Some(&handle) = self.pages.borrow().0.get(&index) {
            return Ok(self.borrow_page(handle));
        }

        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        let handle = unsafe { self.bindings.FPDF_LoadPage(self.handle, index as c_int) };
        if handle.is_null() {
            return Err(format!("no such page: {index}"));
        }
        if let Some(form) = &self.form {
            // SAFETY: both handles are live and owned by this document.
            unsafe { self.bindings.FORM_OnAfterLoadPage(handle, form.handle) };
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
            self.close_page(old);
        }

        Ok(self.borrow_page(handle))
    }

    /// One page with the file's own crop box, whatever a previous caller set.
    ///
    /// The safe door, and it is the *default* one rather than the careful one on
    /// purpose. Pages are cached (see [`PAGE_CACHE`]), so a crop set on a handle
    /// stays set: a caller that simply took the cached page would see whichever
    /// crop the **previous** request left --- a tile of page 3 rendered cropped
    /// because a text extraction two seconds earlier asked for it that way.
    /// Making that state unreachable beats writing down that callers must avoid
    /// it, which `docs/TRAPS.md` records as a rule you wrote down and do not
    /// enforce.
    pub fn page(&self, index: u32) -> Result<RawPage<'_>, String> {
        self.page_cropped(index, None)
    }

    /// One page with the reader's crop applied, or with the file's own restored.
    ///
    /// `to` is `[llx, lly, urx, ury]` in the page's own space, y upwards, the
    /// same convention `/CropBox` uses. `None` restores the file's own box.
    ///
    /// Nothing here validates the rectangle: `docmodel::Rect::is_proper`
    /// refused a degenerate one before it reached the model, and PDFium is not
    /// this layer's place to re-litigate that. What it does refuse is a
    /// non-finite corner, for the reason [`normalised`] gives --- a `NaN` in a
    /// crop box poisons every measurement taken from the page.
    pub fn page_cropped(&self, index: u32, to: Option<[f32; 4]>) -> Result<RawPage<'_>, String> {
        let known = self.original_crops.borrow().contains_key(&index);
        let mut page = self.load_page(index)?;
        if !known {
            // Read on the first load of this page and never again --- after an
            // override there is nothing left to read.
            let original = self.original_box(&page, index);
            self.original_crops.borrow_mut().insert(index, original);
        }
        let want = match to {
            Some(box_pt) if box_pt.iter().all(|value| value.is_finite()) => box_pt,
            Some(_) => return Err(format!("page {index}: the crop box is not a number")),
            None => *self
                .original_crops
                .borrow()
                .get(&index)
                .expect("recorded on the load above"),
        };
        page.set_crop_pt(want);
        Ok(page)
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
            self.close_page(handle);
        }
    }

    /// Notifies the form environment before closing one loaded page.
    fn close_page(&self, handle: FPDF_PAGE) {
        if let Some(form) = &self.form {
            // SAFETY: this page received the matching OnAfterLoadPage call and
            // both handles are still live.
            unsafe { self.bindings.FORM_OnBeforeClosePage(handle, form.handle) };
        }
        // SAFETY: loaded by `page`, removed from the cache before this call,
        // and closed exactly once.
        unsafe { self.bindings.FPDF_ClosePage(handle) };
    }

    fn borrow_page(&self, handle: FPDF_PAGE) -> RawPage<'_> {
        RawPage {
            bindings: self.bindings,
            handle,
            form: self.form.as_ref().map(|form| form.handle),
            _document: std::marker::PhantomData,
        }
    }
}

impl Drop for RawDocument {
    fn drop(&mut self) {
        let pages: Vec<FPDF_PAGE> = self.pages.borrow().0.values().copied().collect();
        for handle in pages {
            self.close_page(handle);
        }
        // Explicitly before CloseDocument. `RawForm::drop` exits the form
        // environment while its pinned callback table is still alive.
        drop(self.form.take());
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
    form: Option<FPDF_FORMHANDLE>,
    _document: std::marker::PhantomData<&'doc RawDocument>,
}

/// **Separated from the call so that something can test it.**
/// [`RawPage::crop_pt`] needs a live page, which needs a document and a loaded
/// PDFium --- so both rules below were reachable by no unit test, and a mutation
/// deleting either survived in the function every character, link and comment
/// position is measured from.
///
/// Two rules, and they are the same pair `links.rs` applies to an annotation
/// rectangle, for the same reasons. The box is **normalised**, because a
/// producer may write either corner first. A **non-finite** value is refused
/// outright, because it would otherwise poison every subtraction it reaches and
/// turn a page of text into a page of `NaN` boxes.
///
/// `ok` false is PDFium declining to answer, which is `None` for the same reason
/// a malformed box is: the caller has a fallback and this does not have to guess
/// one for it.
fn normalised(ok: bool, box_pt: [f32; 4]) -> Option<[f32; 4]> {
    if !ok || !box_pt.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some([
        box_pt[0].min(box_pt[2]),
        box_pt[1].min(box_pt[3]),
        box_pt[0].max(box_pt[2]),
        box_pt[1].max(box_pt[3]),
    ])
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

    /// The page's lower-left corner in its own coordinate space.
    ///
    /// **Zero for most documents and load-bearing for the rest.** PDFium lays a
    /// page out from its `/CropBox`, so [`width_pt`](Self::width_pt) is the
    /// *cropped* size --- while `FPDFText_GetCharBox` answers in the page's own
    /// space, whose origin is the `/MediaBox`'s. When the crop box starts
    /// somewhere other than (0, 0) the two are different spaces, and combining
    /// them puts every character, link and comment out by exactly this.
    ///
    /// Measured: a fixture with `/CropBox [50 50 545 742]` on `/MediaBox
    /// [0 0 595 842]` renders 495x692 and lands **0%** of its character boxes on
    /// ink, against 100% for the same page with no crop box. A crop box that
    /// merely *shrinks* the page from the origin is fine, which is why the
    /// origin rather than the size is what this returns.
    ///
    /// Falls back to (0, 0) when PDFium will not answer, which is the value that
    /// changes nothing --- a page whose crop box cannot be read is then treated
    /// exactly as it was before this existed.
    pub fn origin_pt(&self) -> (f32, f32) {
        // Through `crop_pt` rather than reading `FPDFPage_GetCropBox` a second
        // time. The two agreed on every fixture in the corpus and that was an
        // accident: this one takes only the corner, and the fallback box whose
        // *size* is in the wrong space happens to have the right corner. One
        // rule means a page where they would differ cannot have two answers.
        let box_pt = self.crop_pt();
        (box_pt[0], box_pt[1])
    }

    /// The page's visible box in the page's **own** space, y upwards.
    ///
    /// `/CropBox` intersected with `/MediaBox`, which is what §14.11.2 says a
    /// reader does and is the same rule `pagetree::displayed_page` applies to the
    /// same two boxes through `lopdf`. Two libraries, one rule, stated in both
    /// places: they must agree, or a rectangle is measured against one page and
    /// drawn on another.
    ///
    /// **The fallback is the dangerous part of this function**, and it is the
    /// last resort rather than the common path for a reason worth stating.
    /// `FPDFPage_GetCropBox` returns *false* for a page with no `/CropBox` ---
    /// correctly; there is none to report --- and the version of this that fell
    /// back to `[0, 0, width_pt(), height_pt()]` was handing back the page's
    /// **displayed** rectangle where a caller needed one in the page's own space.
    ///
    /// On an unrotated page those are the same four numbers, so thirteen of the
    /// fourteen corpora could not tell. On `testdata/rotated-90.pdf` ---
    /// `/MediaBox [0 0 612 792]`, `/Rotate 90` --- the sheet is 612 by 792 and
    /// the displayed page is 792 by 612, and writing the second back through
    /// `FPDFPage_SetCropBox`, which reads page space, made PDFium intersect the
    /// two and report the page as **612x612**: a size it never had, on a document
    /// nobody had cropped.
    ///
    /// That fallback was harmless for months because its only consumer was
    /// [`origin_pt`](Self::origin_pt), which takes the *corner* --- and the
    /// corner is `(0, 0)` in both frames. A fallback is in the coordinate system
    /// of whoever wrote it, and the second consumer is where that stops being
    /// invisible.
    pub fn media_pt(&self) -> Option<[f32; 4]> {
        let (mut l, mut b, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
        let ok = unsafe {
            self.bindings
                .FPDFPage_GetMediaBox(self.handle, &mut l, &mut b, &mut r, &mut t)
        };
        normalised(ok != 0, [l, b, r, t])
    }

    pub fn crop_pt(&self) -> [f32; 4] {
        let (mut left, mut bottom, mut right, mut top) = (0f32, 0f32, 0f32, 0f32);
        // SAFETY: `self.handle` is non-null for the lifetime of `self`, and the
        // four out-parameters are live for the call.
        let ok = unsafe {
            self.bindings.FPDFPage_GetCropBox(
                self.handle,
                &mut left,
                &mut bottom,
                &mut right,
                &mut top,
            )
        };
        let crop = normalised(ok != 0, [left, bottom, right, top]);
        let Some(media) = self.media_pt() else {
            // No sheet to measure against. The displayed rectangle is the only
            // number left and it is the wrong space on a rotated page --- see the
            // note above --- so it is the last resort rather than a fallback.
            return crop.unwrap_or([0.0, 0.0, self.width_pt(), self.height_pt()]);
        };
        let Some(crop) = crop else { return media };
        let shown = [
            crop[0].max(media[0]),
            crop[1].max(media[1]),
            crop[2].min(media[2]),
            crop[3].min(media[3]),
        ];
        // An intersection can be empty where the two boxes do not overlap, which
        // is a malformed document rather than a page of no size. The sheet is
        // the honest answer, and it is the same one `pagetree::displayed_page`
        // gives --- the two read different libraries and must agree, or a
        // rectangle is placed against one page and drawn on another.
        if shown[2] > shown[0] && shown[3] > shown[1] {
            shown
        } else {
            media
        }
    }

    /// Replaces the page's `/CropBox` for as long as this page stays loaded.
    ///
    /// **Not a document edit.** It changes the loaded page, so every PDFium
    /// answer taken from it afterwards --- the size, the origin, the render, the
    /// character boxes --- is in the cropped page's terms, which is the whole
    /// mechanism. Nothing is written to the file; what the reader saves comes
    /// from the model through `save.rs`, and the two are deliberately different
    /// paths for the same number.
    pub fn set_crop_pt(&mut self, box_pt: [f32; 4]) {
        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        unsafe {
            self.bindings.FPDFPage_SetCropBox(
                self.handle,
                box_pt[0],
                box_pt[1],
                box_pt[2],
                box_pt[3],
            );
        }
    }

    /// Quarter-turns clockwise the page is displayed rotated by: 0, 1, 2 or 3.
    ///
    /// `/Rotate` on the page dictionary, which a scanner sets routinely. Note
    /// what it does *not* affect: [`width_pt`](Self::width_pt) and
    /// [`height_pt`](Self::height_pt) already report the rotated size, and a
    /// render already comes out rotated. The one thing left in the page's own
    /// unrotated space is `FPDFText_GetCharBox`, which is why this is needed at
    /// all --- see `text.rs`.
    ///
    /// PDFium returns -1 when it cannot say. Treated as no rotation, because a
    /// page whose rotation cannot be read is far more likely to have none than
    /// to have one nobody can name.
    pub fn quarter_turns(&self) -> u8 {
        // SAFETY: as above.
        let turns = unsafe { self.bindings.FPDFPage_GetRotation(self.handle) };
        if (0..=3).contains(&turns) {
            turns as u8
        } else {
            0
        }
    }

    /// The raw handle, for the PDFium APIs the safe wrapper does not reach.
    ///
    /// Crate-private on purpose. This is the same reason `progressive.rs` exists
    /// at all --- text extraction takes an `FPDF_PAGE` and `pdfium-render` keeps
    /// its own accessor `pub(crate)` --- but a handle escaping the crate would
    /// outlive the borrow that makes using it sound.
    pub(crate) fn handle(&self) -> FPDF_PAGE {
        self.handle
    }

    /// The bindings this page was loaded through.
    pub(crate) fn bindings(&self) -> Bindings {
        self.bindings
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
    /// Width of the whole scaled page in device pixels, as displayed.
    pub size_x: i32,
    /// Height of the whole scaled page in device pixels, as displayed.
    pub size_y: i32,
    /// Quarter-turns clockwise to display the page by, 0 to 3.
    ///
    /// This is the *view* rotation and composes on top of the page's own
    /// `/Rotate`: PDFium's display matrix applies the page's rotation first and
    /// then this, which is what makes "rotate the view" mean the same thing on a
    /// scanned page as on an upright one.
    pub turns: u8,
}

impl Placement {
    /// The placement for a tile at `(x, y)` of a page scaled by `scale` and
    /// displayed under `turns` quarter-turns clockwise.
    ///
    /// `size_x`/`size_y` are the *displayed* dimensions, so a quarter turn swaps
    /// them: PDFium fits the page into the rect it is given and rotates inside
    /// it, so passing the upright size would squeeze a landscape page into a
    /// portrait box rather than turning it.
    pub fn tile(page: &RawPage<'_>, scale: f32, turns: u8, x: i32, y: i32) -> Self {
        let width = (page.width_pt() * scale).round() as i32;
        let height = (page.height_pt() * scale).round() as i32;
        let (size_x, size_y) = match turns % 2 {
            0 => (width, height),
            _ => (height, width),
        };
        Self {
            start_x: -x,
            start_y: -y,
            size_x,
            size_y,
            turns: turns % 4,
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
/// exactly, including the form-widget overlay, so an uncancelled render here is
/// byte-identical to the safe path.
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
            placement.turns as c_int,
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

    if outcome.is_done() {
        if let Some(form) = page.form {
            // The form pass is deliberately after a complete base render, as in
            // pdfium-render. A cancelled tile is discarded by every production
            // caller; painting a complete widget over a partial page would make
            // that incomplete buffer look more authoritative than it is.
            // SAFETY: the form, bitmap and page belong to this live document;
            // placement and flags are the same ones used for the base render.
            unsafe {
                bindings.FPDF_FFLDraw(
                    form,
                    bitmap.handle,
                    page.handle,
                    placement.start_x,
                    placement.start_y,
                    placement.size_x,
                    placement.size_y,
                    placement.turns as c_int,
                    RENDER_FLAGS,
                )
            };
        }
    }

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
    /// Quarter-turns clockwise to display the page by, 0 to 3.
    pub turns: u8,
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
        turns,
        x,
        y,
        width,
        height,
    } = spec;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let placement = Placement::tile(page, scale, turns, x, y);

    let progress = {
        let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
        render(&mut bitmap, page, placement, slice, cancel)
    };

    Ok((buffer, progress))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page tree is preferred exactly where PDFium has no sheet to offer.
    ///
    /// Three cases and each is a different decision, which is why this is a
    /// function rather than a branch beside two FFI calls: as written inline it
    /// would sit where `media_pt` and `crop_pt` are called, and no test can
    /// reach that without a loaded PDFium.
    ///
    /// The middle case is the one with teeth. PDFium answered, so its reading
    /// wins **even though** the page tree also has one --- it is the engine that
    /// renders, and overriding a box it already agrees with could only make the
    /// size a page reports disagree with the pixels it produces.
    #[test]
    fn the_page_tree_decides_only_where_pdfium_has_no_media_box() {
        let crop = [0.0, 0.0, 612.0, 792.0];
        let tree = [10.0, 20.0, 410.0, 620.0];

        assert_eq!(
            box_to_use(None, crop, Some(tree)),
            tree,
            "an inherited box is the one case the page tree answers"
        );
        assert_eq!(
            box_to_use(Some(crop), crop, Some(tree)),
            crop,
            "PDFium answered, so its own reading wins over a second opinion"
        );
        assert_eq!(
            box_to_use(None, crop, None),
            crop,
            "nothing answered, so the document is left exactly as it was"
        );
    }

    /// A crop box written corner-first is normalised before its corner is taken.
    ///
    /// A producer may write either corner first and both are legal, so the
    /// lower-left is whichever numbers are smaller rather than whichever come
    /// first. Without this a backwards box hands back the *upper right*, and
    /// every character on the page is shifted by the page's own size.
    #[test]
    fn a_crop_box_written_backwards_still_yields_its_lower_left() {
        assert_eq!(
            normalised(true, [545.0, 742.0, 50.0, 50.0]),
            Some([50.0, 50.0, 545.0, 742.0])
        );
        // The control: written the ordinary way round, the same box gives the
        // same answer. Without it this would pass for an implementation that
        // always returned the *smaller* pair by accident of the input order.
        assert_eq!(
            normalised(true, [50.0, 50.0, 545.0, 742.0]),
            Some([50.0, 50.0, 545.0, 742.0])
        );
    }

    /// A non-finite coordinate falls back to the origin rather than spreading.
    ///
    /// Every character, link and comment rectangle is measured *from* this, so
    /// one `NaN` here is a whole page of `NaN` boxes --- and a `NaN` box fails
    /// every comparison silently rather than loudly, which is the shape that
    /// reads as "the text layer is empty".
    #[test]
    fn a_crop_box_with_a_non_finite_corner_is_refused() {
        assert_eq!(normalised(true, [f32::NAN, 50.0, 545.0, 742.0]), None);
        assert_eq!(normalised(true, [50.0, f32::INFINITY, 545.0, 742.0]), None);
        assert_eq!(
            normalised(true, [50.0, 50.0, f32::NEG_INFINITY, 742.0]),
            None
        );
        // The control, and it is the one that matters: an ordinary box is *not*
        // refused. A guard written as an unconditional `return None` would
        // satisfy all three assertions above.
        assert_eq!(
            normalised(true, [50.0, 50.0, 545.0, 742.0]),
            Some([50.0, 50.0, 545.0, 742.0])
        );
    }

    /// PDFium declining to answer is (0, 0), the value that changes nothing.
    ///
    /// The out-parameters are left holding whatever they held, so trusting a
    /// refused call reads uninitialised intent rather than a crop box.
    #[test]
    fn a_crop_box_pdfium_would_not_answer_for_is_the_origin() {
        assert_eq!(normalised(false, [50.0, 50.0, 545.0, 742.0]), None);
    }

    /// Every code PDFium documents, and what a reader is told for it.
    ///
    /// Enumerated rather than spot-checked, because the value of this mapping is
    /// entirely in the cases it *distinguishes*: a version that answered the
    /// same sentence for all of them would pass any test asking only whether the
    /// password case mentions a password.
    #[test]
    fn each_reason_says_something_different() {
        // 0 is FPDF_ERR_SUCCESS and is deliberately in the list: PDFium can hand
        // back a null handle with no error set, and "no error" is not something
        // to tell somebody whose document did not open.
        let codes: [std::os::raw::c_ulong; 8] = [0, 1, 2, 3, 4, 5, 6, 99];
        let said: Vec<String> = codes.iter().map(|code| open_failure(*code)).collect();

        for message in &said {
            assert!(
                message.ends_with('.') && message.len() > 20,
                "a reason must be a sentence: {message:?}"
            );
            assert!(
                !message.contains("FPDF") && !message.contains("error code"),
                "a reason is for a reader, not a log: {message:?}"
            );
        }

        // The four PDFium distinguishes and this acts on must be four different
        // sentences. The rest collapse into one on purpose.
        let named: std::collections::BTreeSet<&String> = said[2..6].iter().collect();
        assert_eq!(
            named.len(),
            4,
            "file, format, password and security must differ"
        );
    }

    #[test]
    fn a_password_is_named_as_a_password() {
        let said = open_failure(err::PASSWORD);
        assert!(said.contains("password"), "{said:?}");
        // And it does not imply the file is broken, which is the whole point of
        // the wording: a reader told "damaged" goes looking for another copy of
        // a file that is fine.
        assert!(!said.contains("damaged"), "{said:?}");
        // It also must not claim tpdf cannot ask, which it did until the prompt
        // existed. A message that tells a reader to give up in front of a dialog
        // asking them not to is worse than either alone.
        assert!(!said.contains("cannot ask"), "{said:?}");
    }

    /// Which refusals a reader can answer, over every code PDFium documents.
    ///
    /// The flag decides whether the app shows a password prompt or an error, so
    /// both directions are defects a reader meets: set too widely, a file that is
    /// not a PDF asks for a password it has none for; set too narrowly, a locked
    /// document is reported as damaged and there is nothing to type into.
    ///
    /// Enumerated rather than spot-checked, for the reason
    /// [`each_reason_says_something_different`] is: a version that answered
    /// `true` for everything passes any test that only asks about the password.
    #[test]
    fn only_a_password_refusal_is_one_a_reader_can_answer() {
        for code in [0, 1, 2, 3, 5, 6, 99] {
            let refusal = Refusal::of(code);
            assert!(
                !refusal.locked,
                "code {code} is not a password problem: {refusal:?}"
            );
            // And the wording travels with the flag rather than beside it, so
            // the two cannot disagree about the same code.
            assert_eq!(refusal.reason, open_failure(code));
        }
        let locked = Refusal::of(err::PASSWORD);
        assert!(locked.locked, "{locked:?}");
        assert_eq!(locked.reason, open_failure(err::PASSWORD));
    }

    /// A failure that arrived as prose is never one to prompt about.
    ///
    /// The `From` impls exist so `?` works on the ordinary failures inside an
    /// open --- page geometry, a path that is not UTF-8. None of those is
    /// answerable, and a widening that guessed otherwise would put a password
    /// dialog in front of a document that is simply broken.
    #[test]
    fn a_refusal_widened_from_prose_is_not_locked() {
        assert!(!Refusal::from("page 3 has no size".to_string()).locked);
        assert!(!Refusal::from("page 3 has no size").locked);
        assert_eq!(
            String::from(Refusal::from("page 3 has no size")),
            "page 3 has no size",
            "the prose survives the round trip"
        );
    }

    /// The control: an unknown code must not read as a working document.
    #[test]
    fn an_unrecognised_code_admits_it_does_not_know() {
        let said = open_failure(999);
        assert!(said.contains("did not say why"), "{said:?}");
        assert_ne!(said, open_failure(err::PASSWORD));
        assert_ne!(said, open_failure(err::FORMAT));
    }

    /// Success is not a reason a document failed to open.
    ///
    /// Reachable, and the tempting shape --- `if code == 0 { "no error" }` ---
    /// produces a message that reads as though the open worked.
    #[test]
    fn success_is_not_reported_as_a_reason() {
        let said = open_failure(0);
        assert!(!said.to_lowercase().contains("no error"), "{said:?}");
        assert!(!said.to_lowercase().contains("success"), "{said:?}");
    }
}
