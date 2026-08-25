//! One open document: the PDFium handle that draws it, and the object graph that
//! answers everything drawing cannot.
//!
//! ## What this is for
//!
//! `RawDocument` owns `FPDF_DOCUMENT` so a render can be cancelled, and that is
//! all it should ever have owned --- its own header says *"the RAII types below
//! are that ownership, and nothing more."* By August 2026 it also carried five
//! `OnceCell` caches, the file's bytes, its password and a call to
//! `save::append_update`, giving it the highest fan-in in the crate and closing a
//! loop from the writer back to the renderer's handle type.
//!
//! [`crate::docgraph`] took the caches into a type of their own. That fixed the
//! *file* and left the *type*: `RawDocument` still held the graph as a field, so
//! the handle still owned both halves. This is the other half. Here they are
//! siblings, and `progressive.rs` names nothing from `docgraph` at all --- which
//! is checkable in its `use` block, where the problem was visible all along.
//!
//! ## Why it is not called `Document`
//!
//! `lopdf::Document` is imported in nine of the files this touches, and the
//! obvious name was tried first: a blanket rename produced thirty-two errors in
//! which two unrelated types shared a spelling, and the mechanical passes meant
//! to fix them started qualifying the wrong one. Reverted and redone under a name
//! nothing else uses. A refactor whose error count climbs rather than falls is
//! one to undo, not to push through --- and the collision was a design fact that
//! only trying it surfaced.
//!
//! ## Why `page_cropped` takes a closure
//!
//! One method genuinely needs both halves. A page stating no `/MediaBox`
//! inherits one, `FPDFPage_GetMediaBox` does not walk `/Parent`, and the answer
//! is only in the page tree --- an `lopdf` parse. So the PDFium side must be able
//! to ask the graph side a question on the path that loads a page.
//!
//! Three ways to arrange that, and two undo the split:
//!
//! - a back-reference from `RawDocument` to the graph is the arrangement being
//!   removed, wearing a pointer;
//! - computing the inherited box eagerly moves the *laziness decision* to every
//!   caller, and the laziness is the design: a document whose pages state their
//!   own boxes never parses the page tree at all, which `consulted_page_tree`
//!   exists to let a check assert;
//! - a closure keeps the decision where it is. `original_box` calls it only when
//!   PDFium answered `None`, exactly as before, and `progressive.rs` learns
//!   nothing about what is on the other end.
//!
//! ## Why nearly every signature moved
//!
//! Not only the five that read the object graph. `page()` means *the file's own
//! crop box*, and computing that is precisely the question that may need the page
//! tree --- so anything loading a page needs both halves even when everything it
//! then does is PDFium's. [`OpenDocument::pdfium`] is how a caller hands the
//! handle to something that genuinely wants only that.

use std::path::Path;

use crate::docgraph::{DocumentGraph, Source};
use crate::progressive::{Bindings, RawDocument, RawPage, Refusal};

/// A document, both halves of it.
pub struct OpenDocument {
    pdfium: RawDocument,
    graph: DocumentGraph,
}

impl OpenDocument {
    /// Opens a document from a path, for a caller that has one.
    ///
    /// The probe's route in. The viewer's is [`OpenDocument::open_bytes`] --- a
    /// mapped descriptor handed to a sandboxed worker, which has no path to open.
    ///
    /// # Errors
    ///
    /// Whatever PDFium refuses the file for, including a password that did not
    /// open it. See [`Refusal`].
    pub fn open(bindings: Bindings, path: &Path, password: Option<&str>) -> Result<Self, Refusal> {
        Ok(Self {
            pdfium: RawDocument::open(bindings, path, password)?,
            graph: DocumentGraph::new(
                Source::Path(path.to_path_buf()),
                password.map(str::to_string),
            ),
        })
    }

    /// Opens a document from bytes that outlive it.
    ///
    /// The worker's route in, and the reason the sandbox can exist: a mapped
    /// document needs no path, so the process holding it needs no authority to
    /// open one.
    ///
    /// # Errors
    ///
    /// As [`OpenDocument::open`].
    pub fn open_bytes(
        bindings: Bindings,
        bytes: &'static [u8],
        password: Option<&str>,
    ) -> Result<Self, Refusal> {
        Ok(Self {
            pdfium: RawDocument::open_bytes(bindings, bytes, password)?,
            graph: DocumentGraph::new(Source::Bytes(bytes), password.map(str::to_string)),
        })
    }

    /// The PDFium handle, for the things that need only it.
    #[must_use]
    pub fn pdfium(&self) -> &RawDocument {
        &self.pdfium
    }

    /// Everything about this document that is read rather than rendered.
    #[must_use]
    pub fn graph(&self) -> &DocumentGraph {
        &self.graph
    }

    /// How many pages, straight from PDFium.
    ///
    /// Forwarded rather than left to `pdfium().page_count()` because four of the
    /// graph's methods take it and every one of those calls sits here.
    #[must_use]
    pub fn page_count(&self) -> u32 {
        self.pdfium.page_count()
    }

    /// One page with the file's own crop box, whatever a previous caller set.
    ///
    /// The safe door, and the *default* one rather than the careful one on
    /// purpose. Pages are cached, so a crop set on a handle stays set: a caller
    /// that simply took the cached page would see whichever crop the **previous**
    /// request left --- a tile of page 3 rendered cropped because a text
    /// extraction two seconds earlier asked for it that way. Making that state
    /// unreachable beats writing down that callers must avoid it, which
    /// `docs/TRAPS.md` records as a rule you wrote down and do not enforce.
    ///
    /// # Errors
    ///
    /// The page not loading.
    pub fn page(&self, index: u32) -> Result<RawPage<'_>, String> {
        self.page_cropped(index, None)
    }

    /// One page, under the crop the reader has or the one the file states.
    ///
    /// The method that needs both halves --- see the module note for why the page
    /// tree is reached through a closure rather than a field or an eager
    /// argument.
    ///
    /// # Errors
    ///
    /// The page not loading, or a crop box that is not four finite numbers.
    pub fn page_cropped(&self, index: u32, to: Option<[f32; 4]>) -> Result<RawPage<'_>, String> {
        let pages = self.pdfium.page_count() as usize;
        self.pdfium
            .page_cropped(index, to, &|page| self.graph.sheet(page, pages))
    }
}
