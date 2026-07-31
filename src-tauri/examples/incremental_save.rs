//! Spike 0.6: can tpdf append a PDF update section that other readers accept?
//!
//! `docs/PLAN.md` §5 classifies every edit command as incrementally saveable,
//! full-rewrite-required, or forbidden, and rests two claims on the first of
//! those: that appending to a large file is near-instant where a rewrite is not,
//! and that a prior revision stays byte-for-byte intact. Phase 0 lists the spike
//! as "a real appended update section that other readers accept". The operative
//! word is *other* --- a writer and reader from the same library agreeing with
//! each other proves nothing about the file.
//!
//! So the update is written by `lopdf` and judged by four independent parsers:
//!
//!   pdfium         Chrome's, and tpdf's own renderer.
//!   qpdf           An independent C++ implementation with a real structural
//!                  checker, which is the only one here that reports *why*.
//!   poppler        A third lineage entirely, via `pdftotext`.
//!   coregraphics   Apple's, i.e. what Preview and Quick Look use. Linked
//!                  directly rather than shelled out to, so it is macOS-only.
//!
//! A reader that merely opens the file is not evidence either --- spikes 0.3 and
//! 0.5 both found PDFium returning success while producing wrong output. Every
//! reader is therefore asked for something falsifiable: the page count, the
//! extracted text, or the pixels of an edited *and* an unedited page.
//!
//! The edit is the one Phase 2 leads with: append a content stream drawing a
//! visible mark on page 1, and add a `/Square` annotation next to it. The signed
//! fixtures also get an annotation-only variant, because a DocMDP permission
//! level turns on exactly that distinction and running only the combined edit
//! would show every signed document refusing everything.
//!
//! Whichever variant runs, only the objects that actually change are written.
//! Rewriting an object to hold the value it already had is still a change to a
//! signed structural object, and no difference analysis can tell an identical
//! rewrite from a real one.
//!
//! Corpus: `testdata/make_incremental_pdf.py`, plus the existing text and
//! hostile fixtures for the encrypted and multi-revision cases.
//!
//! Usage:
//!     incremental-save [--dir DIR] [--mode all|append|speed|signed|encrypted]
//!                      [--outdir DIR] [--rounds N] [--only NAME]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use lopdf::{
    dictionary, Dictionary, Document, IncrementalDocument, LoadOptions, Object as LoObject,
    ObjectId, Stream,
};
use pdfium_render::prelude::*;

/// Renders are compared at this scale. Same value as spikes 0.3 and 0.4.
const RENDER_SCALE: f32 = 1.5;

/// PDFium's rasterisation is not bit-deterministic once object order changes, so
/// a channel has to move by more than this to count. Same value as spike 0.4.
const CHANNEL_TOLERANCE: i16 = 8;

/// Where the appended mark is drawn, in PDF points from the page's bottom-left.
/// Chosen to sit inside every fixture's media box, all of which are US Letter.
const MARK: [f32; 4] = [72.0, 300.0, 300.0, 380.0];

/// Drawn inside the mark, so a text extractor can be asked whether it read the
/// update section or the revision underneath it.
const NEEDLE: &str = "TPDF-SPIKE-0-6-NEEDLE";

// ---------------------------------------------------------------------------
// The edit
// ---------------------------------------------------------------------------

/// What an append produced, beyond the bytes themselves.
struct Appended {
    /// The whole updated file. Empty unless the caller asked for it --- on a
    /// 336 MB fixture materialising it costs more than the edit does.
    bytes: Vec<u8>,
    /// Just the appended update section.
    update: Vec<u8>,
    /// Parse.
    load_ms: f64,
    /// Build the update section in memory.
    edit_ms: f64,
    /// Serialize the update section, with the previous revision's bytes sunk
    /// rather than copied. This is what an append costs if the file it is
    /// appending to is left where it already is.
    serialize_ms: f64,
    /// Object ids written into the update section.
    touched: Vec<ObjectId>,
}

impl Appended {
    /// Bytes added past the end of the original.
    fn added(&self) -> usize {
        self.update.len()
    }

    /// Load, edit and serialize together.
    fn total_ms(&self) -> f64 {
        self.load_ms + self.edit_ms + self.serialize_ms
    }
}

/// What to append, and what to keep afterwards.
#[derive(Clone, Copy)]
struct EditSpec<'a> {
    password: Option<&'a str>,
    /// Materialise the whole updated file, not just the update section.
    want_whole_file: bool,
    /// Add only the annotation, leaving the page's content stream alone.
    ///
    /// This is the distinction a DocMDP permission level turns on: level 3
    /// permits annotations and forbids page-content changes, so an edit that
    /// does both is refused for a reason that has nothing to do with the
    /// annotation. Without this variant the corpus could only ever show that
    /// every edit to a signed document is rejected.
    annotation_only: bool,
}

impl<'a> EditSpec<'a> {
    /// The default edit, keeping the whole file: what the reader panel needs.
    fn whole_file() -> Self {
        Self {
            password: None,
            want_whole_file: true,
            annotation_only: false,
        }
    }

    /// Update section only, for the timing runs.
    fn measuring() -> Self {
        Self {
            password: None,
            want_whole_file: false,
            annotation_only: false,
        }
    }

    fn with_password(mut self, password: &'a str) -> Self {
        self.password = Some(password);
        self
    }

    fn annotation_only(mut self) -> Self {
        self.annotation_only = true;
        self
    }
}

/// A `Write` that throws away the first `skip` bytes and keeps the rest.
///
/// `IncrementalDocument::save_to` writes the previous revision through to the
/// target before appending. That is correct for producing a new file, but it
/// makes an append cost a copy of the whole document --- and the plan's claim is
/// that appending is cheap *because* the document is not rewritten. Sinking the
/// prefix measures the update section on its own, which is what a save that
/// appends to the file in place would actually pay.
struct TailSink {
    skip: usize,
    seen: usize,
    tail: Vec<u8>,
}

impl std::io::Write for TailSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let start = self.skip.saturating_sub(self.seen).min(buf.len());
        self.tail.extend_from_slice(&buf[start..]);
        self.seen += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Loads `original`, adds a visible mark and an annotation to page 1, and
/// returns the appended update section.
///
/// The mark goes in as a *second* content stream rather than by rewriting the
/// first, because that is what makes the edit incremental: the original content
/// stream object is never touched, so its bytes in the prior revision stay both
/// intact and still referenced.
fn append_edit(original: &[u8], spec: EditSpec<'_>) -> Result<Appended, String> {
    let EditSpec {
        password,
        want_whole_file,
        annotation_only,
    } = spec;
    let start = Instant::now();

    let prev = match password {
        Some(pw) => Document::load_mem_with_options(original, LoadOptions::with_password(pw))
            .map_err(|e| format!("load (with password): {e}"))?,
        None => Document::load_mem(original).map_err(|e| format!("load: {e}"))?,
    };
    // Loading with a password leaves the document decrypted in memory but keeps
    // the encryption state, which is what lets the appended objects be
    // re-encrypted with the original key.
    if password.is_none() && prev.is_encrypted() {
        return Err("document is encrypted and no password was supplied".to_string());
    }

    let page_id = *prev
        .get_pages()
        .get(&1)
        .ok_or_else(|| "document has no page 1".to_string())?;

    // Everything that has to be *read* out of the previous revision is read
    // here, into owned values, because `create_from` takes the document by
    // value. Cloning it instead would put a full copy of the object graph into
    // every measurement below, which on the scan fixtures is most of the number.
    let mut resources = effective_resources(&prev, page_id)?;
    let mut fonts = match resources.get(b"Font") {
        Ok(LoObject::Dictionary(existing)) => existing.clone(),
        Ok(LoObject::Reference(id)) => match prev.get_object(*id) {
            Ok(LoObject::Dictionary(existing)) => existing.clone(),
            _ => Dictionary::new(),
        },
        _ => Dictionary::new(),
    };
    let mut contents = existing_items(&prev, page_id, b"Contents")?;
    let annots_site = list_site(&prev, page_id, b"Annots")?;
    let mut annots = existing_items(&prev, page_id, b"Annots")?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;

    let edit_start = Instant::now();
    let mut incremental = IncrementalDocument::create_from(original.to_vec(), prev);

    // Bring across whichever objects are actually being modified. Everything
    // else --- the original content stream, the parent tree, the resources ---
    // stays in the previous revision and is referenced from there.
    //
    // The page dictionary is *only* cloned when it has to change. Rewriting it
    // to hold an identical value is still a change to a signed structural
    // object, and no difference analysis can tell an identical rewrite from a
    // real one.
    let touch_page = !annotation_only || matches!(annots_site, ListSite::Inline);
    if touch_page {
        incremental
            .opt_clone_object_to_new_document(page_id)
            .map_err(|e| format!("clone page {page_id:?}: {e}"))?;
    }
    if let ListSite::ArrayObject(array_id) = annots_site {
        incremental
            .opt_clone_object_to_new_document(array_id)
            .map_err(|e| format!("clone /Annots array {array_id:?}: {e}"))?;
    }

    // A font of our own, so the overlay can carry a text needle. A reader that
    // merely opened the file has proved nothing, and text is the only edit
    // `pdftotext` can be asked about.
    let font = incremental.new_document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    fonts.set("TpdfF", LoObject::Reference(font));
    resources.set("Font", fonts);

    let overlay = (!annotation_only).then(|| {
        incremental.new_document.add_object(Stream::new(
            dictionary! {},
            format!(
                "q 1 0 0 RG 0.85 0.85 0.2 rg 4 w {} {} {} {} re B Q\n\
                 BT /TpdfF 14 Tf 0 0 0 rg {} {} Td ({NEEDLE}) Tj ET\n",
                MARK[0],
                MARK[1],
                MARK[2] - MARK[0],
                MARK[3] - MARK[1],
                MARK[0] + 6.0,
                MARK[1] + 30.0,
            )
            .into_bytes(),
        ))
    });

    let annotation = incremental.new_document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Square",
        "Rect" => vec![
            (MARK[0] - 8.0).into(),
            (MARK[1] - 8.0).into(),
            (MARK[2] + 8.0).into(),
            (MARK[3] + 8.0).into(),
        ],
        "F" => 4,
        "C" => vec![1.into(), 0.into(), 0.into()],
        "Contents" => LoObject::string_literal("tpdf spike 0.6"),
    });

    annots.push(LoObject::Reference(annotation));

    match annots_site {
        // The page says `/Annots 12 0 R`, so the array is its own object and
        // extending it leaves the page dictionary untouched.
        ListSite::ArrayObject(array_id) => {
            incremental
                .new_document
                .set_object(array_id, LoObject::Array(annots));
        }
        // The array is written inline in the page dictionary, so there is no
        // way to add an annotation without rewriting the page.
        ListSite::Inline => {
            let page = incremental
                .new_document
                .get_object_mut(page_id)
                .and_then(LoObject::as_dict_mut)
                .map_err(|e| format!("page {page_id:?} is not a dictionary: {e}"))?;
            page.set("Annots", LoObject::Array(annots));
        }
    }

    if let Some(overlay) = overlay {
        contents.push(LoObject::Reference(overlay));
        let page = incremental
            .new_document
            .get_object_mut(page_id)
            .and_then(LoObject::as_dict_mut)
            .map_err(|e| format!("page {page_id:?} is not a dictionary: {e}"))?;
        page.set("Contents", LoObject::Array(contents));
        // Set the merged resources directly on the page rather than editing the
        // dictionary in place. An inherited /Resources is shared with every
        // sibling page, and rewriting a shared object to add a font to one page
        // is how an edit quietly becomes a document-wide change.
        page.set("Resources", resources);
    }

    let mut touched: Vec<ObjectId> = incremental.new_document.objects.keys().copied().collect();
    touched.sort_unstable();
    let edit_ms = edit_start.elapsed().as_secs_f64() * 1000.0;

    let serialize_start = Instant::now();
    let mut sink = TailSink {
        skip: original.len(),
        seen: 0,
        tail: Vec::with_capacity(4096),
    };
    incremental
        .save_to(&mut sink)
        .map_err(|e| format!("incremental save: {e}"))?;
    let serialize_ms = serialize_start.elapsed().as_secs_f64() * 1000.0;

    let bytes = if want_whole_file {
        let mut whole = Vec::with_capacity(original.len() + sink.tail.len());
        whole.extend_from_slice(original);
        whole.extend_from_slice(&sink.tail);
        whole
    } else {
        Vec::new()
    };

    Ok(Appended {
        bytes,
        update: sink.tail,
        load_ms,
        edit_ms,
        serialize_ms,
        touched,
    })
}

/// The page's effective `/Resources`, copied so it can be modified.
///
/// `/Resources` is an inheritable attribute: a page may not carry one at all and
/// take the parent's instead. Creating an empty one on the page in that case
/// does not add resources, it *removes* every inherited one, and the page then
/// renders blank while every check that only counts pages still passes.
fn effective_resources(prev: &Document, page_id: ObjectId) -> Result<Dictionary, String> {
    let mut node = page_id;
    for _ in 0..64 {
        let dict = prev
            .get_object(node)
            .and_then(LoObject::as_dict)
            .map_err(|e| format!("object {node:?}: {e}"))?;
        match dict.get(b"Resources") {
            Ok(LoObject::Dictionary(found)) => return Ok(found.clone()),
            Ok(LoObject::Reference(id)) => {
                return prev
                    .get_object(*id)
                    .and_then(LoObject::as_dict)
                    .cloned()
                    .map_err(|e| format!("/Resources {id:?}: {e}"))
            }
            _ => {}
        }
        match dict.get(b"Parent").and_then(LoObject::as_reference) {
            Ok(parent) => node = parent,
            Err(_) => return Ok(Dictionary::new()),
        }
    }
    Err("/Parent chain did not terminate".to_string())
}

/// Where a page's list-valued entry actually lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListSite {
    /// Written inline in the page dictionary, so extending it means rewriting
    /// the page.
    Inline,
    /// A reference to an array object, which can be extended on its own.
    ArrayObject(ObjectId),
}

/// Which of the two shapes `key` is stored in on this page.
fn list_site(prev: &Document, page_id: ObjectId, key: &[u8]) -> Result<ListSite, String> {
    let page = prev
        .get_object(page_id)
        .and_then(LoObject::as_dict)
        .map_err(|e| format!("page {page_id:?}: {e}"))?;
    Ok(match page.get(key) {
        Ok(LoObject::Reference(id)) if matches!(prev.get_object(*id), Ok(LoObject::Array(_))) => {
            ListSite::ArrayObject(*id)
        }
        _ => ListSite::Inline,
    })
}

/// The page's existing `/Contents` or `/Annots` as a list, whatever shape it is
/// stored in: absent, a single reference, an inline array, or a reference to an
/// array object.
///
/// All four shapes occur in this corpus, and getting it wrong is silent: a
/// `/Contents` that replaces rather than extends still renders, just without the
/// original page on it.
fn existing_items(prev: &Document, page_id: ObjectId, key: &[u8]) -> Result<Vec<LoObject>, String> {
    let page = prev
        .get_object(page_id)
        .and_then(LoObject::as_dict)
        .map_err(|e| format!("page {page_id:?}: {e}"))?;
    Ok(match page.get(key) {
        Err(_) => Vec::new(),
        Ok(LoObject::Array(items)) => items.clone(),
        Ok(LoObject::Reference(id)) => {
            // A reference to an array --- the usual `/Contents` shape once a
            // producer has split a stream --- is flattened, because the array
            // object itself stays in the previous revision.
            match prev.get_object(*id) {
                Ok(LoObject::Array(items)) => items.clone(),
                _ => vec![LoObject::Reference(*id)],
            }
        }
        Ok(other) => {
            return Err(format!(
                "unexpected /{} of type {}",
                String::from_utf8_lossy(key),
                other.type_name().unwrap_or(b"?").escape_ascii()
            ))
        }
    })
}

/// Writes the whole document out again from the parsed object graph.
///
/// This is the comparison point for the speed claim, and deliberately the
/// cheapest possible rewrite: no collection, no renumbering, no re-encoding.
/// Anything a real full-rewrite save must also do only widens the gap.
fn full_rewrite(original: &[u8]) -> Result<(Vec<u8>, f64), String> {
    let start = Instant::now();
    let mut doc = Document::load_mem(original).map_err(|e| format!("load: {e}"))?;
    let mut bytes = Vec::with_capacity(original.len() + 4096);
    doc.save_to(&mut bytes).map_err(|e| format!("save: {e}"))?;
    Ok((bytes, start.elapsed().as_secs_f64() * 1000.0))
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

/// The structural facts about an update section that make it an update section
/// rather than a second file glued on the end.
struct Structure {
    /// Byte offsets named by each `startxref` in the file, in order.
    startxrefs: Vec<usize>,
    /// `%%EOF` markers.
    eofs: usize,
    /// Whether each `startxref` target begins a classic table or an object.
    kinds: Vec<&'static str>,
    /// `/Prev` values found in the appended trailer, in order.
    prevs: Vec<usize>,
}

/// Reads the cross-reference chain out of the raw bytes.
///
/// Deliberately a byte scan rather than a parse: the question is what a reader
/// coming to this file cold would find, and asking the library that wrote it
/// would answer a different question.
fn structure(bytes: &[u8]) -> Structure {
    let mut startxrefs = Vec::new();
    let mut kinds = Vec::new();
    let mut at = 0;
    while let Some(found) = find(bytes, b"startxref", at) {
        let mut cursor = found + b"startxref".len();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let digits_from = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if let Ok(offset) = std::str::from_utf8(&bytes[digits_from..cursor])
            .unwrap_or("")
            .parse::<usize>()
        {
            kinds.push(if bytes.get(offset..offset + 4) == Some(b"xref") {
                "table"
            } else {
                "stream"
            });
            startxrefs.push(offset);
        }
        at = found + 1;
    }

    // `/Prev` appears in trailers and in cross-reference stream dictionaries
    // alike, so one scan covers both forms.
    let mut prevs = Vec::new();
    let mut at = 0;
    while let Some(found) = find(bytes, b"/Prev", at) {
        let mut cursor = found + b"/Prev".len();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let digits_from = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if let Ok(offset) = std::str::from_utf8(&bytes[digits_from..cursor])
            .unwrap_or("")
            .parse::<usize>()
        {
            prevs.push(offset);
        }
        at = found + 1;
    }

    Structure {
        eofs: count(bytes, b"%%EOF"),
        startxrefs,
        kinds,
        prevs,
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    let mut at = 0;
    let mut total = 0;
    while let Some(found) = find(haystack, needle, at) {
        total += 1;
        at = found + 1;
    }
    total
}

// ---------------------------------------------------------------------------
// Readers
// ---------------------------------------------------------------------------

/// One independent parser's opinion of a file.
struct Verdict {
    reader: &'static str,
    ok: bool,
    detail: String,
}

impl Verdict {
    fn ok(reader: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reader,
            ok: true,
            detail: detail.into(),
        }
    }

    fn bad(reader: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reader,
            ok: false,
            detail: detail.into(),
        }
    }
}

/// QPDF's structural checker. The only reader here that says *why* it objects.
fn read_qpdf(path: &Path) -> Verdict {
    match Command::new("qpdf").arg("--check").arg(path).output() {
        Err(e) => Verdict::bad("qpdf", format!("not run: {e}")),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            // Match qpdf's own `WARNING:` / `ERROR:` prefixes rather than the
            // words. Its clean output ends "...the file may still contain
            // errors that qpdf cannot detect", so a substring search for
            // "error" fails every file, including the ones it just passed.
            let complaints: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("WARNING") || line.starts_with("ERROR"))
                .collect();
            // Exit 0 is clean, 3 is warnings-only, 2 is an error.
            let code = out.status.code().unwrap_or(-1);
            if code == 0 && complaints.is_empty() {
                Verdict::ok("qpdf", "no warnings")
            } else {
                Verdict::bad(
                    "qpdf",
                    format!(
                        "exit {code}: {}",
                        complaints.first().unwrap_or(&"(no detail)")
                    ),
                )
            }
        }
    }
}

/// Poppler, asked for the page count *and* for the text of page 1.
///
/// The text is the part that matters. A reader can accept the file, report the
/// right page count and still be showing the previous revision --- which is the
/// specific failure an appended update invites, and the one that would look fine
/// in every check that only asks whether the file opened.
fn read_poppler(path: &Path, expect_pages: usize) -> Verdict {
    let info = match Command::new("pdfinfo").arg(path).output() {
        Err(e) => return Verdict::bad("poppler", format!("not run: {e}")),
        Ok(out) if !out.status.success() => {
            return Verdict::bad(
                "poppler",
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            )
        }
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
    };
    let pages = info
        .lines()
        .find_map(|line| line.strip_prefix("Pages:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if pages != expect_pages {
        return Verdict::bad("poppler", format!("{pages} pages, expected {expect_pages}"));
    }

    let text = match Command::new("pdftotext")
        .args(["-f", "1", "-l", "1"])
        .arg(path)
        .arg("-")
        .output()
    {
        Err(e) => return Verdict::bad("poppler", format!("pdftotext not run: {e}")),
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
    };
    if !text.contains(NEEDLE) {
        return Verdict::bad(
            "poppler",
            format!("{pages} pages, but page 1 text has no {NEEDLE}"),
        );
    }
    Verdict::ok("poppler", format!("{pages} pages, needle in page 1 text"))
}

/// Apple's PDF reader, linked directly. This is what Preview and Quick Look use,
/// so it is the one that decides whether the file looks broken on the user's own
/// desktop.
#[cfg(target_os = "macos")]
mod apple {
    use std::ffi::{c_char, c_void, CString};
    use std::path::Path;

    /// A patch of the page, in points from its bottom-left corner. Every fixture
    /// is at least this big, so one fixed window works for all of them and the
    /// `CGRect`-returning geometry calls stay out of the FFI surface.
    const PATCH: usize = 512;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDataProviderCreateWithFilename(filename: *const c_char) -> *mut c_void;
        fn CGDataProviderRelease(provider: *mut c_void);
        fn CGPDFDocumentCreateWithProvider(provider: *mut c_void) -> *mut c_void;
        fn CGPDFDocumentRelease(document: *mut c_void);
        fn CGPDFDocumentGetNumberOfPages(document: *mut c_void) -> usize;
        fn CGPDFDocumentGetPage(document: *mut c_void, page: usize) -> *mut c_void;
        fn CGColorSpaceCreateDeviceGray() -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *mut c_void,
            bitmap_info: u32,
        ) -> *mut c_void;
        fn CGContextRelease(context: *mut c_void);
        fn CGContextDrawPDFPage(context: *mut c_void, page: *mut c_void);
    }

    /// What CoreGraphics made of a file.
    pub struct Read {
        pub pages: usize,
        /// Grayscale pixels of the bottom-left `PATCH` square of page 1.
        pub patch: Vec<u8>,
    }

    /// Opens the file and renders a patch of page 1, or `None` if CoreGraphics
    /// rejected it. Grayscale with no alpha, so the buffer is one byte per pixel
    /// and a comparison needs no unpacking.
    pub fn read(path: &Path) -> Option<Read> {
        let c_path = CString::new(path.to_string_lossy().as_bytes()).ok()?;
        let mut patch = vec![0xFFu8; PATCH * PATCH];
        // SAFETY: `c_path` and `patch` outlive every call that borrows them;
        // each handle is released on all paths; and CoreGraphics returns null
        // rather than trapping on input it will not accept. `kCGImageAlphaNone`
        // is 0, which is the only legal alpha for a one-component space.
        let pages = unsafe {
            let provider = CGDataProviderCreateWithFilename(c_path.as_ptr());
            if provider.is_null() {
                return None;
            }
            let document = CGPDFDocumentCreateWithProvider(provider);
            CGDataProviderRelease(provider);
            if document.is_null() {
                return None;
            }
            let pages = CGPDFDocumentGetNumberOfPages(document);

            let page = CGPDFDocumentGetPage(document, 1);
            if !page.is_null() {
                let space = CGColorSpaceCreateDeviceGray();
                let context = CGBitmapContextCreate(
                    patch.as_mut_ptr().cast(),
                    PATCH,
                    PATCH,
                    8,
                    PATCH,
                    space,
                    0,
                );
                if !context.is_null() {
                    CGContextDrawPDFPage(context, page);
                    CGContextRelease(context);
                }
                CGColorSpaceRelease(space);
            }
            CGPDFDocumentRelease(document);
            pages
        };
        Some(Read { pages, patch })
    }
}

/// Apple's reader, asked for pixels as well as a page count, for the same
/// reason poppler is asked for text.
#[cfg(target_os = "macos")]
fn read_coregraphics(before: &Path, after: &Path, expect_pages: usize) -> Verdict {
    let Some(new) = apple::read(after) else {
        return Verdict::bad("coregraphics", "rejected the file");
    };
    if new.pages != expect_pages {
        return Verdict::bad(
            "coregraphics",
            format!("{} pages, expected {expect_pages}", new.pages),
        );
    }
    let Some(old) = apple::read(before) else {
        return Verdict::bad("coregraphics", "rejected the original");
    };
    let changed = old
        .patch
        .iter()
        .zip(&new.patch)
        .filter(|(a, b)| i16::from(**a).abs_diff(i16::from(**b)) > 8)
        .count();
    if changed == 0 {
        return Verdict::bad(
            "coregraphics",
            format!("{} pages, but page 1 is unchanged", new.pages),
        );
    }
    Verdict::ok(
        "coregraphics",
        format!("{} pages, {changed} px changed on page 1", new.pages),
    )
}

#[cfg(not(target_os = "macos"))]
fn read_coregraphics(_before: &Path, _after: &Path, _expect_pages: usize) -> Verdict {
    Verdict::ok("coregraphics", "skipped (not macOS)")
}

struct Render {
    rgba: Vec<u8>,
    width: i32,
    height: i32,
}

/// Renders one page to RGBA with annotations on, so an annotation that failed to
/// attach shows up as missing ink rather than passing silently.
fn render(doc: &PdfDocument<'_>, index: PdfPageIndex, scale: f32) -> Result<Render, String> {
    let page = doc
        .pages()
        .get(index)
        .map_err(|e| format!("no page {index}: {e}"))?;
    let width = (page.width().value * scale).round() as i32;
    let height = (page.height().value * scale).round() as i32;
    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .set_target_height(height)
        .render_annotations(true);
    let mut bitmap = PdfBitmap::empty(width as Pixels, height as Pixels, PdfBitmapFormat::BGRA)
        .map_err(|e| format!("could not allocate {width}x{height}: {e}"))?;
    page.render_into_bitmap_with_config(&mut bitmap, &config)
        .map_err(|e| format!("render failed: {e}"))?;
    Ok(Render {
        rgba: bitmap.as_rgba_bytes(),
        width,
        height,
    })
}

/// Device pixels that differ between two renders of the same page.
fn changed_pixels(before: &Render, after: &Render) -> Result<usize, String> {
    if before.width != after.width || before.height != after.height {
        return Err("page size changed".to_string());
    }
    Ok(before
        .rgba
        .chunks_exact(4)
        .zip(after.rgba.chunks_exact(4))
        .filter(|(a, b)| {
            (0..4).any(|c| (i16::from(a[c]) - i16::from(b[c])).abs() > CHANNEL_TOLERANCE)
        })
        .count())
}

/// What PDFium makes of the edited file, compared against the original.
struct PdfiumVerdict {
    verdict: Verdict,
    /// Pixels changed on the edited page. Must be non-zero: a reader that used
    /// the *previous* revision would render an unchanged page and look fine.
    edited_changed: usize,
    /// Pixels changed on an untouched page, if the document has one.
    untouched_changed: Option<usize>,
    annots_before: usize,
    annots_after: usize,
}

fn read_pdfium(
    pdfium: &Pdfium,
    before_path: &Path,
    after_path: &Path,
    password: Option<&str>,
) -> Result<PdfiumVerdict, String> {
    let before = pdfium
        .load_pdf_from_file(before_path, password)
        .map_err(|e| format!("original: {e}"))?;
    let after = pdfium
        .load_pdf_from_file(after_path, password)
        .map_err(|e| format!("updated: {e}"))?;

    let pages_before = before.pages().len();
    let pages_after = after.pages().len();
    if pages_before != pages_after {
        return Err(format!("{pages_before} pages became {pages_after}"));
    }

    let annots_before = before
        .pages()
        .get(0)
        .map(|p| p.annotations().len())
        .unwrap_or(0);
    let annots_after = after
        .pages()
        .get(0)
        .map(|p| p.annotations().len())
        .unwrap_or(0);

    let edited_changed = changed_pixels(
        &render(&before, 0, RENDER_SCALE)?,
        &render(&after, 0, RENDER_SCALE)?,
    )?;

    let untouched_changed = if pages_before > 1 {
        Some(changed_pixels(
            &render(&before, 1, RENDER_SCALE)?,
            &render(&after, 1, RENDER_SCALE)?,
        )?)
    } else {
        None
    };

    Ok(PdfiumVerdict {
        verdict: Verdict::ok("pdfium", format!("{pages_after} pages")),
        edited_changed,
        untouched_changed,
        annots_before,
        annots_after,
    })
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

struct Args {
    dir: PathBuf,
    outdir: PathBuf,
    mode: String,
    rounds: usize,
    only: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args {
        dir: PathBuf::from("testdata"),
        outdir: PathBuf::from("/tmp/tpdf-incremental"),
        mode: "all".to_string(),
        rounds: 5,
        only: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        match flag.as_str() {
            "--dir" => args.dir = PathBuf::from(argv.next().unwrap_or_default()),
            "--outdir" => args.outdir = PathBuf::from(argv.next().unwrap_or_default()),
            "--mode" => args.mode = argv.next().unwrap_or_default(),
            "--rounds" => args.rounds = argv.next().and_then(|v| v.parse().ok()).unwrap_or(5),
            "--only" => args.only = argv.next(),
            other => eprintln!("[WARN] ignoring unknown flag {other}"),
        }
    }
    args
}

/// Appends to each fixture and puts the result to all four readers.
fn mode_append(args: &Args, pdfium: &Pdfium) -> bool {
    let fixtures: Vec<(&str, &str)> = vec![
        ("text-base14.pdf", "one page, classic table, base-14 font"),
        ("text-truetype.pdf", "embedded TrueType subset"),
        ("text-cid.pdf", "Type0 / Identity-H"),
        ("text-marked.pdf", "marked content and /ActualText"),
        ("text-heavy.pdf", "775 pages, 1.3 MB"),
        ("vector-heavy.pdf", "one A0 sheet, ~200k path segments"),
        (
            "incr-xrefstream.pdf",
            "xref stream, page dict inside /ObjStm",
        ),
        ("hostile-stale.pdf", "already has two revisions"),
        ("hostile-objstm.pdf", "xref stream, one page"),
        ("hostile-attachment.pdf", "embedded file in the catalog"),
        ("hostile-metadata.pdf", "/Info and XMP /Metadata"),
        ("incr-scan-5p.pdf", "5 uncompressed 300-dpi scan pages"),
    ];

    println!("== append: one update section per fixture, judged by four readers ==\n");
    let mut all_ok = true;

    for (name, what) in fixtures {
        if let Some(only) = &args.only {
            if !name.contains(only.as_str()) {
                continue;
            }
        }
        let path = args.dir.join(name);
        if !path.exists() {
            println!("[SKIP] {name}: not present");
            continue;
        }
        let original = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("[FAIL] {name}: {e}");
                all_ok = false;
                continue;
            }
        };

        println!("-- {name} ({what}, {} KB)", original.len() / 1024);

        let appended = match append_edit(&original, EditSpec::whole_file()) {
            Ok(a) => a,
            Err(e) => {
                println!("   [FAIL] append: {e}\n");
                all_ok = false;
                continue;
            }
        };

        let mut ok = true;

        // The claim the whole mode exists for: the previous revision is still
        // there, byte for byte.
        let prefix_intact = appended.bytes.len() >= original.len()
            && appended.bytes[..original.len()] == original[..];
        println!(
            "   prefix   {} original {} B preserved, {} B appended ({:.2} ms)",
            status(prefix_intact),
            original.len(),
            appended.added(),
            appended.total_ms()
        );
        ok &= prefix_intact;

        let before = structure(&original);
        let after = structure(&appended.bytes);
        let chain_ok = after.eofs == before.eofs + 1
            && after.startxrefs.len() == before.startxrefs.len() + 1
            && after.kinds.last() == before.kinds.last()
            && after.prevs.len() == before.prevs.len() + 1
            && after.prevs.last() == before.startxrefs.last();
        println!(
            "   chain    {} {} -> {} revisions, appended xref is a {}, /Prev {:?} -> previous startxref {:?}",
            status(chain_ok),
            before.eofs,
            after.eofs,
            after.kinds.last().copied().unwrap_or("?"),
            after.prevs.last(),
            before.startxrefs.last(),
        );
        ok &= chain_ok;

        let out_path = args.outdir.join(name);
        if let Err(e) = fs::write(&out_path, &appended.bytes) {
            println!("   [FAIL] write {}: {e}\n", out_path.display());
            all_ok = false;
            continue;
        }

        match read_pdfium(pdfium, &path, &out_path, None) {
            Err(e) => {
                println!("   pdfium   [FAIL] {e}");
                ok = false;
            }
            Ok(v) => {
                let saw_edit = v.edited_changed > 0 && v.annots_after == v.annots_before + 1;
                let untouched_clean = v.untouched_changed.is_none_or(|n| n == 0);
                println!(
                    "   pdfium   {} {}, page 1 {} px changed, annots {} -> {}{}",
                    status(saw_edit && untouched_clean),
                    v.verdict.detail,
                    v.edited_changed,
                    v.annots_before,
                    v.annots_after,
                    match v.untouched_changed {
                        Some(n) => format!(", page 2 {n} px changed"),
                        None => String::new(),
                    }
                );
                ok &= saw_edit && untouched_clean;
            }
        }

        let expect_pages = pdfium
            .load_pdf_from_file(&path, None)
            .map(|d| d.pages().len() as usize)
            .unwrap_or(0);
        for verdict in [
            read_qpdf(&out_path),
            read_poppler(&out_path, expect_pages),
            read_coregraphics(&path, &out_path, expect_pages),
        ] {
            println!(
                "   {:<8} {} {}",
                verdict.reader,
                status(verdict.ok),
                verdict.detail
            );
            ok &= verdict.ok;
        }

        println!(
            "   objects  {} appended: {}",
            status(true),
            appended
                .touched
                .iter()
                .map(|(id, gen)| format!("{id}.{gen}"))
                .collect::<Vec<_>>()
                .join(" ")
        );

        println!("   {}\n", if ok { "[OK]" } else { "[FAIL]" });
        all_ok &= ok;
    }

    all_ok
}

/// Interleaved append-versus-rewrite across the size sweep.
///
/// A,B,A,B within each round and compared pairwise, per the measurement policy:
/// wall clock on this machine drifts several percent over minutes, which is
/// larger than most differences worth reporting. Here the difference is not
/// small, but the discipline is what makes that statement trustworthy.
fn mode_speed(args: &Args) -> bool {
    let candidates = [
        "text-base14.pdf",
        "text-heavy.pdf",
        "incr-scan-5p.pdf",
        "incr-scan-20p.pdf",
        "incr-scan-40p.pdf",
    ];

    println!(
        "== speed: append vs full rewrite, {} rounds interleaved ==\n",
        args.rounds
    );
    println!(
        "{:<18} {:>8} {:>9} {:>8} {:>10} {:>10} {:>10} {:>8} {:>10}",
        "fixture",
        "size MB",
        "parse ms",
        "edit ms",
        "append ms",
        "total ms",
        "rewrite ms",
        "ratio",
        "rewrote MB"
    );

    let mut all_ok = true;
    for name in candidates {
        let path = args.dir.join(name);
        if !path.exists() {
            continue;
        }
        let original = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("{name}: [FAIL] {e}");
                all_ok = false;
                continue;
            }
        };

        let mut loads = Vec::new();
        let mut edits = Vec::new();
        let mut serializes = Vec::new();
        let mut totals = Vec::new();
        let mut rewrites = Vec::new();
        let mut rewrote = 0usize;
        let mut failed = None;

        for _ in 0..args.rounds {
            match append_edit(&original, EditSpec::measuring()) {
                Ok(a) => {
                    loads.push(a.load_ms);
                    edits.push(a.edit_ms);
                    serializes.push(a.serialize_ms);
                    totals.push(a.total_ms());
                }
                Err(e) => failed = Some(format!("append: {e}")),
            }
            match full_rewrite(&original) {
                Ok((bytes, ms)) => {
                    rewrote = bytes.len();
                    rewrites.push(ms);
                }
                Err(e) => failed = Some(format!("rewrite: {e}")),
            }
        }

        if let Some(e) = failed {
            println!("{name}: [FAIL] {e}");
            all_ok = false;
            continue;
        }

        // Median, not mean: one page-cache miss should not decide the number.
        let total = median(&mut totals);
        let rewrite = median(&mut rewrites);
        println!(
            "{:<18} {:>8.1} {:>9.2} {:>8.2} {:>10.2} {:>10.2} {:>10.2} {:>7.1}x {:>10.1}",
            name.trim_end_matches(".pdf"),
            original.len() as f64 / 1e6,
            median(&mut loads),
            median(&mut edits),
            median(&mut serializes),
            total,
            rewrite,
            rewrite / total,
            rewrote as f64 / 1e6,
        );
    }
    println!(
        "\nappend ms is the update section alone, with the previous revision's bytes\n\
         sunk rather than copied -- what a save that appends in place would pay.\n"
    );

    all_ok &= speed_to_disk(args, &candidates);
    all_ok
}

/// The same comparison, but landing on disk.
///
/// The in-memory table above is misleading on its own, and the direction it
/// misleads in is the one that matters: it says a full rewrite of a scanned
/// document costs about what an append does. That is true of the *computation*
/// and false of the *save*, because the append writes several hundred bytes to a
/// file that already exists while the rewrite writes the whole document out
/// again, flushes it, and atomically replaces the original --- and needs room for
/// both copies while it does.
fn speed_to_disk(args: &Args, candidates: &[&str]) -> bool {
    use std::io::Write as _;

    println!("== speed: the same save, landing on disk ==\n");
    println!(
        "{:<18} {:>8} {:>12} {:>12} {:>8} {:>12} {:>12}",
        "fixture", "size MB", "append ms", "rewrite ms", "ratio", "wrote B", "rewrote B"
    );

    let mut all_ok = true;
    for name in candidates {
        let path = args.dir.join(name);
        if !path.exists() {
            continue;
        }
        let Ok(original) = fs::read(&path) else {
            continue;
        };
        let target = args.outdir.join(format!("disk-{name}"));

        let mut appends = Vec::new();
        let mut rewrites = Vec::new();
        let mut wrote = 0usize;
        let mut rewrote = 0usize;
        let mut failed = None;

        for _ in 0..args.rounds {
            // The document is already on disk -- that is the premise, not part
            // of the cost. Restoring it between rounds is untimed setup, and it
            // has to be made *durable* before the timer starts: `sync_all` is
            // `F_FULLFSYNC` on macOS, a device-wide barrier, so leaving 336 MB
            // of staging dirty in the page cache makes the append's flush pay
            // for it. Without this the 40-page append measured slower than the
            // full rewrite, which is how the artifact announced itself.
            if stage(&target, &original).is_err() {
                failed = Some("could not stage the target".to_string());
                break;
            }

            match append_edit(&original, EditSpec::measuring()) {
                Ok(a) => {
                    let start = Instant::now();
                    let appended = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&target)
                        .and_then(|mut file| {
                            file.write_all(&a.update)?;
                            file.sync_all()
                        });
                    if appended.is_err() {
                        failed = Some("append to disk failed".to_string());
                        break;
                    }
                    wrote = a.update.len();
                    appends.push(a.total_ms() + start.elapsed().as_secs_f64() * 1000.0);
                }
                Err(e) => {
                    failed = Some(format!("append: {e}"));
                    break;
                }
            }

            if stage(&target, &original).is_err() {
                failed = Some("could not stage the target".to_string());
                break;
            }
            match full_rewrite(&original) {
                Ok((bytes, compute_ms)) => {
                    let start = Instant::now();
                    let temporary = target.with_extension("tpdf-part");
                    let written = fs::File::create(&temporary).and_then(|mut file| {
                        file.write_all(&bytes)?;
                        file.sync_all()
                    });
                    if written.is_err() || fs::rename(&temporary, &target).is_err() {
                        failed = Some("rewrite to disk failed".to_string());
                        break;
                    }
                    rewrote = bytes.len();
                    rewrites.push(compute_ms + start.elapsed().as_secs_f64() * 1000.0);
                }
                Err(e) => {
                    failed = Some(format!("rewrite: {e}"));
                    break;
                }
            }
        }
        fs::remove_file(&target).ok();

        if let Some(e) = failed {
            println!("{name}: [FAIL] {e}");
            all_ok = false;
            continue;
        }

        let append = median(&mut appends);
        let rewrite = median(&mut rewrites);
        println!(
            "{:<18} {:>8.1} {:>12.2} {:>12.2} {:>7.1}x {:>12} {:>12}",
            name.trim_end_matches(".pdf"),
            original.len() as f64 / 1e6,
            append,
            rewrite,
            rewrite / append,
            wrote,
            rewrote,
        );
    }
    println!();
    all_ok
}

/// Writes `bytes` to `path` and flushes them all the way to the device.
fn stage(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if values.is_empty() {
        return f64::NAN;
    }
    values[values.len() / 2]
}

/// What an appended update does to an existing signature.
///
/// Two edits are tried on each fixture, because the answer differs: the full
/// edit changes page content *and* adds an annotation, while the annotation-only
/// edit is the one a DocMDP level 3 certification is supposed to permit. Running
/// only the first would show every signed document refusing every edit, and make
/// "signed means forbidden" look proven.
fn mode_signed(args: &Args) -> bool {
    let fixtures = [
        ("incr-signed.pdf", "approval signature, no DocMDP"),
        ("incr-certified-1.pdf", "DocMDP 1 -- no changes permitted"),
        (
            "incr-certified-2.pdf",
            "DocMDP 2 -- form filling and signing",
        ),
        (
            "incr-certified-3.pdf",
            "DocMDP 3 -- annotations as well, /Annots inline",
        ),
        (
            "incr-certified-3-indirect.pdf",
            "DocMDP 3, /Annots as its own object",
        ),
    ];

    println!(
        "== signed: is the signature intact, is the document unmodified, was it permitted? ==\n"
    );
    let mut all_ok = true;

    for (name, what) in fixtures {
        let path = args.dir.join(name);
        if !path.exists() {
            println!("[SKIP] {name}: not present (needs pyhanko)");
            continue;
        }
        let original = fs::read(&path).unwrap_or_default();
        println!("-- {name} ({what})");
        match validate_signature(args, &path) {
            Ok(report) => println!("   {:<16} {report}", "as signed"),
            Err(e) => println!("   {:<16} [SKIP] {e}", "as signed"),
        }

        for (label, spec) in [
            ("content+annot", EditSpec::whole_file()),
            ("annotation only", EditSpec::whole_file().annotation_only()),
        ] {
            let appended = match append_edit(&original, spec) {
                Ok(a) => a,
                Err(e) => {
                    println!("   {label:<16} [FAIL] append: {e}");
                    all_ok = false;
                    continue;
                }
            };
            let out_path = args
                .outdir
                .join(format!("{}-{name}", label.replace(' ', "-")));
            if fs::write(&out_path, &appended.bytes).is_err() {
                println!("   {label:<16} [FAIL] could not write output");
                all_ok = false;
                continue;
            }

            // The claim the append is supposed to protect: the bytes the
            // signature covers are exactly the bytes it covered before.
            let intact = appended.bytes.len() >= original.len()
                && appended.bytes[..original.len()] == original[..];
            if !intact {
                println!("   {label:<16} [FAIL] signed bytes were disturbed");
                all_ok = false;
                continue;
            }
            match validate_signature(args, &out_path) {
                Ok(report) => println!("   {label:<16} {report}"),
                Err(e) => println!("   {label:<16} [SKIP] {e}"),
            }
        }
        println!();
    }
    all_ok
}

/// Asks pyhanko what it thinks of the signatures in `file`.
///
/// pyhanko is the oracle here for the same reason QPDF is above: it is the only
/// implementation present that both wrote the signature and can judge it, and a
/// judgement from the writer alone would be circular for everything *except*
/// the question of whether our append disturbed it.
fn validate_signature(args: &Args, file: &Path) -> Result<String, String> {
    let script = args.dir.join("check_signature.py");
    if !script.exists() {
        return Err(format!("{} not present", script.display()));
    }
    let out = Command::new("uv")
        .args(["run", "--with", "pyhanko", "--quiet", "python3"])
        .arg(&script)
        .arg(file)
        .output()
        .map_err(|e| format!("uv run: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What an appended update does to an encrypted document.
///
/// Spike 0.4 recorded that `lopdf` silently drops encryption on a full rewrite,
/// which is its own security failure and invisible to any check that looks at
/// content rather than structure. An incremental save must not repeat it, and
/// the appended objects have to be encrypted with the *original* key --- a
/// plaintext object appended to an encrypted file is both a leak and a file no
/// reader can decode.
fn mode_encrypted(args: &Args, pdfium: &Pdfium) -> bool {
    println!("== encrypted: does the append preserve encryption, or quietly drop it? ==\n");
    let cases = [
        (
            "hostile-encrypted.pdf",
            "",
            "AES-256, empty user password (opens with no prompt)",
        ),
        (
            "incr-encrypted-pw.pdf",
            "swordfish",
            "AES-256 behind a real user password",
        ),
    ];

    let mut all_ok = true;
    for (name, password, what) in cases {
        let path = args.dir.join(name);
        if !path.exists() {
            println!("[SKIP] {name}: not present");
            continue;
        }
        let original = fs::read(&path).unwrap_or_default();
        println!("-- {name} ({what})");

        // Without the password. An empty user password is not a missing one:
        // the file opens unprompted in any reader, so the edit should go
        // through. A real password must instead be refused, not guessed past.
        let blind = append_edit(&original, EditSpec::whole_file());
        let blind_ok = if password.is_empty() {
            let ok = blind.is_ok();
            println!(
                "   no-password  {} {}",
                status(ok),
                match &blind {
                    Ok(_) => "opened and edited, as an empty user password should".to_string(),
                    Err(e) => format!("refused, but an empty password needs none: {e}"),
                }
            );
            ok
        } else {
            let ok = blind.is_err();
            println!(
                "   no-password  {} {}",
                status(ok),
                match &blind {
                    Err(e) => format!("refused: {e}"),
                    Ok(_) => "succeeded without the password".to_string(),
                }
            );
            ok
        };

        let appended = match append_edit(&original, EditSpec::whole_file().with_password(password))
        {
            Ok(a) => a,
            Err(e) => {
                println!("   password     [FAIL] {e}\n");
                all_ok = false;
                continue;
            }
        };
        let out_path = args.outdir.join(name);
        if fs::write(&out_path, &appended.bytes).is_err() {
            println!("   [FAIL] could not write output\n");
            all_ok = false;
            continue;
        }

        let intact = appended.bytes.len() >= original.len()
            && appended.bytes[..original.len()] == original[..];
        println!(
            "   prefix       {} original bytes preserved",
            status(intact)
        );

        let still = Command::new("qpdf")
            .arg("--is-encrypted")
            .arg(&out_path)
            .status()
            .map(|s| s.code() == Some(0))
            .unwrap_or(false);
        println!(
            "   encryption   {} qpdf --is-encrypted on the result",
            status(still)
        );

        // The sharp check. Our overlay stream carries a known needle; if the
        // appended objects went in as plaintext it is sitting in the update
        // section in the clear. `qpdf --is-encrypted` would still say yes,
        // because the /Encrypt dictionary is intact either way.
        let plain = find(&appended.update, NEEDLE.as_bytes(), 0).is_some();
        println!(
            "   ciphertext   {} appended objects {}",
            status(!plain),
            if plain {
                "contain the needle in the clear"
            } else {
                "are encrypted"
            }
        );

        let readable = pdfium
            .load_pdf_from_file(&out_path, Some(password))
            .map(|d| d.pages().len())
            .map_err(|e| e.to_string());
        match &readable {
            Ok(pages) => {
                println!("   pdfium       [OK]   opens with the same password, {pages} pages")
            }
            Err(e) => println!("   pdfium       [FAIL] {e}"),
        }
        println!();

        all_ok &= blind_ok && intact && still && !plain && readable.is_ok();
    }
    all_ok
}

fn pdfium_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TPDF_PDFIUM_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn status(ok: bool) -> &'static str {
    if ok {
        "[OK]  "
    } else {
        "[FAIL]"
    }
}

fn main() -> ExitCode {
    let args = parse_args();
    if let Err(e) = fs::create_dir_all(&args.outdir) {
        eprintln!("[FAIL] could not create {}: {e}", args.outdir.display());
        return ExitCode::FAILURE;
    }

    let dir = pdfium_dir();
    let library = Pdfium::pdfium_platform_library_name_at_path(&dir);
    let pdfium = match Pdfium::bind_to_library(&library) {
        Ok(bindings) => Pdfium::new(bindings),
        Err(e) => {
            eprintln!("[FAIL] could not bind PDFium at {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };

    let run = |name: &str| args.mode == "all" || args.mode == name;
    let mut ok = true;
    if run("append") {
        ok &= mode_append(&args, &pdfium);
    }
    if run("speed") {
        ok &= mode_speed(&args);
    }
    if run("signed") {
        ok &= mode_signed(&args);
    }
    if run("encrypted") {
        ok &= mode_encrypted(&args, &pdfium);
    }

    println!(
        "{}",
        if ok {
            "[OK] spike 0.6"
        } else {
            "[FAIL] spike 0.6"
        }
    );
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
