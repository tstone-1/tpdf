//! The document's object graph: everything PDFium cannot answer, read at most once.
//!
//! ## Why this is a module rather than five fields on the handle
//!
//! `progressive.rs` exists to own `FPDF_DOCUMENT`, `FPDF_PAGE` and `FPDF_BITMAP`
//! so that rendering can be cancelled, and its header says so in as many words:
//! *"the RAII types below are that ownership, and nothing more."* By August 2026
//! that sentence was false. The `use` block under it imported `annots`,
//! `docinfo`, `encoding`, `links` and `pagetree`; `RawDocument` carried five
//! `OnceCell` caches, the document's bytes and its password, and called
//! `save::append_update`. An outside review measured its fan-in at 14, the
//! highest in the crate, and named the closed loop: `save -> progressive`, so
//! the renderer's handle type depended on the writer.
//!
//! Nothing about *what* those five do was wrong --- each is documented, measured
//! and lazy for a stated reason, and they are moved here unchanged. What was
//! wrong is where they lived. This is the half of that finding that is a move;
//! the other half, making a worker-side `Document` own both this and the PDFium
//! handle so that 38 signatures stop naming `RawDocument`, is a separate piece
//! and is deliberately not started here.
//!
//! ## The coupling that is real, and is now visible
//!
//! Four of these need the page count, and the page count is PDFium's. They take
//! it as an argument rather than reaching for it, which is the honest shape: a
//! caller that has a handle passes what the handle knows. Hiding it behind a
//! back-reference would put the arrow back the way it was.
//!
//! ## What every one of them has in common
//!
//! A failure is kept as a failure. *This document has no comments* and *this
//! document could not be read* are different things to tell a reader and only
//! one of them is reassuring, so the `Result` is cached rather than collapsed to
//! an empty list --- a document that could not be parsed does not become
//! parseable on a second attempt, and re-parsing to rediscover that is the same
//! work for the same answer.

use std::cell::OnceCell;
use std::path::PathBuf;

use crate::annots::{self, Comments};
use crate::docinfo::{self, Properties};
use crate::encoding::{self, PageMapping};
use crate::links::{self, Links};
use crate::pagetree;

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
pub enum Source {
    /// The mapping the worker was handed. Already in memory; no re-read.
    Bytes(&'static [u8]),
    /// A path the in-process backend opened. Read on demand, once.
    Path(PathBuf),
}

/// Every question about a document that is answered by parsing it rather than
/// by rendering it.
#[derive(Default)]
pub struct DocumentGraph {
    /// Where to find the bytes again. `None` before one is set, which no
    /// constructed graph is --- it exists so this can derive `Default` for a
    /// document that was opened from neither.
    source: Option<Source>,
    /// The password this document was opened with, if it needed one.
    ///
    /// **Held because `lopdf` needs it too, and it is the same key to the same
    /// bytes.** Every question below is a second parse of `source`, and a parse
    /// without the password reads *no objects at all*: `lopdf` returns a
    /// `Document` that loads cleanly and reports zero pages. So a locked
    /// document would open, render and search while its comments, links and
    /// properties came back empty, and the save path would refuse it.
    ///
    /// What this costs is one more copy of the password in a process that
    /// already has it: `Workers::open` holds one for the document's lifetime so
    /// a pool growing under contention can unlock its new workers, which
    /// `docs/THREAT-MODEL.md` §T6.9 states. This is the worker's own copy, in
    /// the process that is sandboxed.
    password: Option<String>,
    /// Per-page character-mapping verdicts, computed at most once.
    ///
    /// Lazy, and **not for the reason first written for it**. The original
    /// comment said this costs a full `lopdf` parse and that on a 337 MB
    /// document that is the dominant cost of opening one --- a guess, and wrong.
    /// Measured in release: 0.1 ms small, 5.8 ms on the 775-page document,
    /// 11.9 ms on the 337 MB scan, because `lopdf` reads the xref and object
    /// headers rather than every stream and the cost tracks object count, not
    /// bytes.
    ///
    /// It is still lazy, because warm startup is ~276 ms against a 300 ms target
    /// (`docs/PLAN.md` §4) and 6--12 ms is a quarter of the whole margin. Off the
    /// critical path that is free; on it, it is expensive.
    mapping: OnceCell<Vec<PageMapping>>,
    /// Every comment in the document, read at most once.
    ///
    /// Lazy for the same reason [`DocumentGraph::mapping`] is, and cached for the
    /// same one: it costs a second `lopdf` parse of the whole file, nothing on
    /// the startup path asks for it, and a reader who opens the comments panel
    /// twice should pay for it once.
    comments: OnceCell<Result<Comments, String>>,
    /// Every link in the document, read at most once.
    ///
    /// Lazy and cached for the same reasons as [`DocumentGraph::comments`], with
    /// one difference in when it is asked for: the viewer wants links as soon as
    /// a page is on screen rather than when a panel opens, so this is warmed
    /// just after first paint instead of on demand.
    links: OnceCell<Result<Links, String>>,
    /// What the document says about itself, read at most once.
    ///
    /// Lazy and cached like [`DocumentGraph::comments`], and the laziest of the
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
    /// Lazy and cached like [`DocumentGraph::comments`], and for one more reason
    /// than the others: it is read on the path that loads a page, which is a path
    /// a reader waits on. A document that needs it pays once.
    sheets: OnceCell<Result<Vec<[f32; 4]>, String>>,
}

impl DocumentGraph {
    /// A graph over bytes that can be reached again, and the key to read them.
    #[must_use]
    pub fn new(source: Source, password: Option<String>) -> Self {
        Self {
            source: Some(source),
            password,
            ..Default::default()
        }
    }

    /// The password this document was opened with, for a parser that needs it.
    ///
    /// Every caller is a `lopdf` parse of the same bytes PDFium already holds
    /// open, in the same process. See the field.
    #[must_use]
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
    fn bytes(&self) -> Option<std::borrow::Cow<'_, [u8]>> {
        match self.source.as_ref()? {
            Source::Bytes(bytes) => Some(std::borrow::Cow::Borrowed(*bytes)),
            Source::Path(path) => std::fs::read(path).ok().map(std::borrow::Cow::Owned),
        }
    }

    /// Per-page verdicts on whether the text means anything, computed once.
    ///
    /// Always exactly `pages` long, so index `n` is page `n`.
    ///
    /// **Every failure is "unknown", never "clean".** Bytes that cannot be read,
    /// a document `lopdf` refuses, a page it cannot account for --- all produce a
    /// `PageMapping` with `truncated` set and `certain()` false. That is the rule
    /// `docs/PLAN.md` §6 states for a redaction verification, and it applies here
    /// for the same reason: this exists so a reader is not told "no matches" on a
    /// page nobody could search, and a scan that failed silently reporting clean
    /// would reinstate exactly that.
    pub fn mapping(&self, pages: usize) -> &[PageMapping] {
        self.mapping.get_or_init(|| {
            let unknown = || {
                vec![
                    PageMapping {
                        truncated: true,
                        ..PageMapping::default()
                    };
                    pages
                ]
            };
            let Some(bytes) = self.bytes() else {
                return unknown();
            };
            encoding::scan(&bytes, pages, self.password()).unwrap_or_else(|_| unknown())
        })
    }

    /// Every comment in the document, read at most once.
    ///
    /// # Errors
    ///
    /// The bytes not being readable, or `lopdf` refusing them. See
    /// `crate::annots`.
    pub fn comments(&self, pages: usize) -> Result<Comments, String> {
        self.comments
            .get_or_init(|| {
                let bytes = self
                    .bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                annots::scan(&bytes, pages, self.password())
            })
            .clone()
    }

    /// Every link in the document, read at most once.
    ///
    /// # Errors
    ///
    /// As [`DocumentGraph::comments`]. A document whose links could not be read
    /// is a document whose cross-references silently do nothing, and the reader
    /// is better told than left clicking.
    pub fn links(&self, pages: usize) -> Result<Links, String> {
        self.links
            .get_or_init(|| {
                let bytes = self
                    .bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                links::scan(&bytes, pages, self.password())
            })
            .clone()
    }

    /// What the document says about itself, read at most once.
    ///
    /// # Errors
    ///
    /// The bytes not being readable, or `lopdf` refusing to parse them.
    pub fn properties(&self, pages: u32) -> Result<Properties, String> {
        self.properties
            .get_or_init(|| {
                let bytes = self
                    .bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                docinfo::scan(&bytes, pages, self.password())
            })
            .clone()
    }

    /// The update section for a save that only adds marks.
    ///
    /// Deliberately **not** cached, where the four above are. Those are
    /// read-only facts about the document: asked for repeatedly, identical every
    /// time. This is a function of the *plan*, which differs on every save, so a
    /// cache keyed on the document would answer a second save with the first
    /// save's bytes --- and silently, because those bytes are a perfectly valid
    /// update section for a document that no longer matches them.
    ///
    /// # Errors
    ///
    /// The document's bytes are unreadable, or [`crate::save::append_update`]
    /// refuses --- see there for the reasons, all of which are about the document
    /// or the plan rather than about this process.
    pub fn append(&self, plan: &crate::edits::Plan) -> Result<crate::save::Update, String> {
        let bytes = self
            .bytes()
            .ok_or_else(|| "the document's bytes could not be read".to_string())?;
        // `into_owned` is the one copy this path makes, and it is the same copy
        // the coordinator used to make with `std::fs::read`. The document
        // arrives as a read-only mapping and `lopdf` needs an owned buffer, so
        // the copy is `IncrementalDocument::create_from`'s requirement rather
        // than a choice made here --- see the note beside it, including what
        // `worker-probe` measured when this was moved from the coordinator to
        // the worker and nothing changed.
        crate::save::append_update(bytes.into_owned(), plan, self.password())
            .map_err(|why| why.message)
    }

    /// Applies a plan to these bytes and serialises the whole document.
    ///
    /// The counterpart of [`DocumentGraph::append`] for every plan an append
    /// cannot express --- a deleted page, a move, a turn, a crop --- and uncached
    /// for the same reason: two saves of one document are two different answers,
    /// and the second one is not the first.
    ///
    /// **The bytes are not returned to the coordinator.** The caller writes them
    /// down the descriptor the worker was started with; see
    /// `worker_proto::Request::Rewrite`. What this function knows is the
    /// document and the plan, which is the whole of the split it exists for.
    ///
    /// **A [`crate::save::Refusal`] rather than a `String`**, alone among the
    /// methods here, and it is the one refusal a reader can act on that the
    /// others do not make: *this file is not the file your edits were made
    /// against*, which is the difference between offering Reload and not. Every
    /// other caller of this type reports a failure and stops.
    ///
    /// # Errors
    ///
    /// The document's bytes are unreadable, or [`crate::save::rewrite_update`]
    /// refuses --- see there for the reasons, all of which are about the document
    /// or the plan rather than about this process.
    pub fn rewrite(
        &self,
        plan: &crate::edits::Plan,
        job: crate::save::Job,
    ) -> Result<Vec<u8>, crate::save::Refusal> {
        let bytes = self
            .bytes()
            .ok_or_else(|| crate::save::Refusal::from("the document's bytes could not be read"))?;
        crate::save::rewrite_update(&bytes, plan, job, self.password())
    }

    /// Merges these bytes, under `plan`, with the documents in `inputs`.
    ///
    /// [`DocumentGraph::rewrite`]'s widest counterpart: the incoming documents
    /// are files tpdf never opened, and `incoming` names where each of them
    /// begins in the mapping handed over, and what to call it. See `crate::worker_proto::Request::Merge`.
    ///
    /// # Errors
    ///
    /// The bytes are unreadable, or anything `crate::save::merge_update`
    /// refuses.
    pub fn merge(
        &self,
        plan: &crate::edits::Plan,
        inputs: crate::save::Inputs<'_>,
    ) -> Result<(Vec<u8>, u32), crate::save::Refusal> {
        let bytes = self
            .bytes()
            .ok_or_else(|| crate::save::Refusal::from("the document's bytes could not be read"))?;
        crate::save::merge_update(&bytes, plan, inputs, self.password())
    }

    /// Builds a print job for the page range `job` names.
    ///
    /// [`DocumentGraph::rewrite`]'s counterpart for the print route that carries
    /// no plan. **It takes no password**, unlike every other reader here, and
    /// that is `crate::print::build_update`'s refusal rather than an omission:
    /// its writer emits every object in the clear, so an encrypted document is
    /// refused whether or not the key is held.
    ///
    /// # Errors
    ///
    /// The bytes are unreadable, or anything `crate::print::build_update`
    /// refuses.
    pub fn print_range(&self, job: &crate::print::Job) -> Result<Vec<u8>, String> {
        let bytes = self
            .bytes()
            .ok_or_else(|| "the document's bytes could not be read".to_string())?;
        crate::print::build_update(&bytes, job)
    }

    /// How many pages `lopdf` finds in these bytes.
    ///
    /// Uncached, like [`DocumentGraph::append`] and for a related reason: those
    /// above are facts about a document that does not change, and this is a
    /// question about a file that has *just* been written. A cached answer here
    /// would be the previous revision's, which is a perfectly plausible number
    /// and the wrong one.
    ///
    /// # Errors
    ///
    /// The bytes are unreadable, or `lopdf` refuses them --- which is what a
    /// mis-chained cross-reference produces and is the answer this is here for.
    pub fn reread_pages(&self) -> Result<usize, String> {
        let bytes = self
            .bytes()
            .ok_or_else(|| "the document's bytes could not be read".to_string())?;
        crate::save::reread_pages(&bytes, self.password())
    }

    /// What a redaction verification finds in these bytes.
    ///
    /// Uncached, like [`DocumentGraph::reread_pages`] and for the same reason:
    /// this is a question about a file that has *just* been written, and a
    /// cached answer would be about the revision before it.
    ///
    /// **The password is the graph's own**, which is the whole reason this is a
    /// method here rather than a free function the worker calls. A redacted copy
    /// of an encrypted document is re-encrypted, so a scan arriving without the
    /// key parses no objects at all and finds nothing --- and finding nothing is
    /// what a clean file looks like. `crate::verify::scan` refuses to certify
    /// that, so the failure is safe; handing it the key is what lets it answer
    /// the question instead of declining it.
    ///
    /// # Errors
    ///
    /// The bytes are unreadable. A file that cannot be *parsed* is not an error
    /// --- it is a blind spot, and `crate::verify::Report` is the type that says
    /// so.
    pub fn verify(&self, needles: &[String]) -> Result<crate::verify::Report, String> {
        let bytes = self
            .bytes()
            .ok_or_else(|| "the document's bytes could not be read".to_string())?;
        Ok(crate::verify::scan(&bytes, needles, self.password()))
    }

    /// Whether the page tree has been parsed for this document yet.
    ///
    /// **An accounting observable.** `RawDocument::original_box` is meant to
    /// reach `lopdf` only for a document PDFium cannot give a `/MediaBox` for,
    /// and "it never happened" is invisible from outside --- every number a
    /// caller can see is identical either way, because the two agree wherever
    /// both answer. So the property that would silently be lost is the one this
    /// exists to let a check assert. `geometry-probe` reads it in both
    /// directions.
    #[must_use]
    pub fn consulted_page_tree(&self) -> bool {
        self.sheets.get().is_some()
    }

    /// One page's box out of the page tree, parsing the document at most once.
    ///
    /// See [`DocumentGraph::sheets`] for why this is lazy and why most documents
    /// never reach it.
    #[must_use]
    pub fn sheet(&self, index: u32, pages: usize) -> Option<[f32; 4]> {
        self.sheets
            .get_or_init(|| {
                let bytes = self
                    .bytes()
                    .ok_or_else(|| "the document's bytes could not be read".to_string())?;
                pagetree::displayed_boxes(&bytes, pages, self.password())
            })
            .as_ref()
            .ok()
            .and_then(|boxes| boxes.get(index as usize).copied())
    }
}
