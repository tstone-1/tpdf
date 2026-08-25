//! Does the page PDFium lays out match the page the document describes?
//!
//! `FPDFPage_GetMediaBox` does not walk `/Parent`, so a page that inherits its
//! box from an ancestor gets no answer from PDFium --- and `FPDF_GetPageWidthF`
//! then reports `width x width` for one that also carries a quarter turn.
//! `docs/TRAPS.md` has the crossed measurements. That is not an exotic
//! document: one `/MediaBox` on the page-tree root is what any producer
//! emitting uniform pages writes, and `/Rotate 90` is what a scanner writes.
//!
//! `RawDocument::page` repairs it by handing PDFium the box
//! `pagetree::displayed_boxes` derived. Four checks per run say whether that
//! happened, and each of them can fail:
//!
//! * **size** --- the displayed width and height PDFium reports equal the page
//!   tree's. This is the claim; on `inherited.pdf` before the repair it was
//!   `400.0 x 400.0` against `600.0 x 400.0`.
//!
//! * **box** --- `crop_pt`, which every coordinate on the page is measured
//!   from, is the rectangle the page tree derived, *in the page's own space*.
//!   Stronger than the size check and not implied by it: an origin can be wrong
//!   while both dimensions are right, and before the repair this page answered
//!   `[0 0 600 400]` --- the right size in the wrong convention.
//!
//! * **ink** --- the page draws something. The reader-visible half, and the one
//!   no structural check makes: the render is clipped to PDFium's idea of the
//!   sheet, so a page of text came out **1, 3 and 0** inked pixels of ~26,600
//!   against **1013, 1062 and 1317** after. The floor is 0.1% of the rendered
//!   pixels, which separates those by a factor of 10 on the failing side and 38
//!   on the passing one; the margin is printed either way, because a run that
//!   passed by a hundredth looks exactly like one that passed by a mile.
//!
//! * **cost** --- the page tree was parsed **iff** some page needed it. An
//!   accounting check: every number above is identical whether the parse
//!   happened or not, so a repair that parsed every document would be invisible
//!   here and would put a whole `lopdf` pass on the path a reader waits on.
//!   Run it over a corpus that inherits nothing and this is the check that goes
//!   red.
//!
//! **The ink check has one precondition it cannot assert: the page's glyphs
//! have to be drawable on this machine.** Measured 2026-08-24: page 2 of
//! `encodings.pdf` extracts fourteen Japanese characters and renders **0 of
//! 56,600 pixels**, while its size and box checks pass. That page is
//! `/UniJIS-UCS2-H` over a *non-embedded* `KozMinPro-Regular`, so drawing it
//! needs a substituted font; the text is there and the glyphs are not. Which
//! font is missing was not established, and the cause is deliberately not
//! guessed at here.
//!
//! No guard is written for it, and that is the decision rather than an
//! omission. The obvious one --- skip where a page has text and no ink --- would
//! have skipped page 2 of `inherited.pdf`, which drew **0** pixels before the
//! repair for the real reason. A guard that hides the defect this probe exists
//! to catch is worse than a red line naming a corpus, so run it on the corpora
//! `BUILD.md` names and read `0 of 56600` for what it says.
//!
//! Usage:
//!   geometry-probe <file.pdf> [--lib DIR]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tpdf_lib::document::OpenDocument;

use pdfium_render::prelude::Pdfium;
use tpdf_lib::pagetree;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};

/// The fraction of a rendered page that has to be ink for the page to count as
/// drawn. See the module note for where the number comes from.
const INK_FLOOR: f64 = 0.001;

fn main() {
    let mut file = None;
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            other => file = Some(PathBuf::from(other)),
        }
    }
    let Some(file) = file else {
        eprintln!("usage: geometry-probe <file.pdf> [--lib DIR]");
        std::process::exit(2);
    };

    let bindings = bind(&library);
    let doc = match OpenDocument::open(bindings, &file, None) {
        Ok(doc) => doc,
        Err(why) => {
            println!("[FAIL] {}: {why}", file.display());
            std::process::exit(1);
        }
    };

    // The object graph's answer, read here rather than taken from the document
    // under test: an oracle that came out of the thing it is judging agrees with
    // it by construction.
    let bytes = std::fs::read(&file).expect("the file is readable");
    let parsed = lopdf::Document::load_mem(&bytes).expect("lopdf parses the file");
    let ids: Vec<_> = parsed.get_pages().values().copied().collect();
    if ids.len() != doc.page_count() as usize {
        println!(
            "[FAIL] the two parsers disagree about the page count: lopdf {} against pdfium {}",
            ids.len(),
            doc.page_count()
        );
        std::process::exit(1);
    }

    println!("{}  {} pages", file.display(), doc.page_count());

    // Whether any page of this document is one PDFium has no sheet for, which
    // is what decides the cost check below. Read from a page handed out by the
    // door under test, since that is the only page any caller ever sees.
    let mut inherits = false;

    for index in 0..doc.page_count() {
        let page = doc.page(index).expect("page");
        let want = pagetree::displayed_page(&parsed, ids[index as usize]);
        inherits |= page.media_pt().is_none();

        let (width, height) = (page.width_pt(), page.height_pt());
        check(
            &format!("page {index}: the displayed size is the page tree's"),
            close(width, want.width) && close(height, want.height),
            &format!(
                "{width:.1} x {height:.1}, page tree says {:.1} x {:.1}",
                want.width, want.height
            ),
        );

        let box_pt = page.crop_pt();
        // **Transposed here rather than by `DisplayedPage::box_pt`, on purpose.**
        // That method is what the repair hands to PDFium, so comparing against
        // it compares the writer with itself: a mutation that stops
        // transposing moves both sides together and the check passes. Measured
        // -- it did, and the mutation was reported SURVIVED while six other
        // checks went red. Four duplicated lines in a probe is the price of a
        // reader that is actually independent of the writer.
        let (want_w, want_h) = if want.turns % 2 == 1 {
            (want.height, want.width)
        } else {
            (want.width, want.height)
        };
        let want_box = [
            want.origin.0,
            want.origin.1,
            want.origin.0 + want_w,
            want.origin.1 + want_h,
        ];
        check(
            &format!("page {index}: coordinates are measured from that box"),
            box_pt
                .iter()
                .zip(want_box.iter())
                .all(|(a, b)| close(*a, *b)),
            &format!("{box_pt:.1?} against {want_box:.1?}"),
        );

        let pixels = tile(bindings, &page);
        let total = pixels.len() / 4;
        let ink = inked(&pixels);
        let fraction = ink as f64 / total.max(1) as f64;
        check(
            &format!("page {index}: the page draws something"),
            fraction >= INK_FLOOR,
            &format!(
                "{ink} of {total} pixels, {:.3}% against a {:.1}% floor",
                fraction * 100.0,
                INK_FLOOR * 100.0
            ),
        );
    }

    // Every page has been loaded by now, so this is the answer for the whole
    // document rather than for whichever page happened to be last.
    let parsed_tree = doc.graph().consulted_page_tree();
    check(
        "the page tree was parsed only because a page needed it",
        parsed_tree == inherits,
        &format!(
            "parsed: {parsed_tree}, and {} page inheriting its box",
            if inherits { "at least one" } else { "no" }
        ),
    );

    let (ran, passed) = (RAN.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
    println!("\n{passed}/{ran} checks passed");
    if passed != ran {
        std::process::exit(1);
    }
}

/// Points, compared at a tolerance no real box difference falls inside.
///
/// The two answers come from different libraries reading the same numbers out
/// of the same file, so they agree exactly or they disagree by a whole box
/// dimension. This exists for `f32` arithmetic, not for near-misses.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// Checks run and checks passed, for the summary line at the end.
///
/// A summary is not decoration: a probe that died halfway prints exactly the
/// same green lines as one that finished, so the count is the only thing that
/// distinguishes them.
static RAN: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

fn check(name: &str, ok: bool, detail: &str) -> bool {
    RAN.fetch_add(1, Ordering::Relaxed);
    if ok {
        PASSED.fetch_add(1, Ordering::Relaxed);
    }
    // Brackets in the literal and the name unpadded, which is what
    // `crop_probe` and `merge_probe` do. Padding the label puts the spaces
    // *inside* the brackets, and `mutate_viewer.py` matches `^\[(OK|FAIL|SKIP)\]`
    // -- so every line stops being a check line at all and every mutation
    // reports "matches 0 checks". Sibling of the trap about an interpolated
    // status label being two columns narrower when it passes.
    println!("[{}] {name}  {detail}", if ok { "OK" } else { "FAIL" });
    ok
}

fn bind(library: &Path) -> progressive::Bindings {
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path).expect("could not load Pdfium");
    progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))))
}

/// A 200-pixel-wide render of the whole page.
fn tile(bindings: progressive::Bindings, page: &RawPage<'_>) -> Vec<u8> {
    let w = page.width_pt().max(1.0);
    let h = page.height_pt().max(1.0);
    let scale = 200.0 / w;
    let spec = TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width: 200,
        height: (h * scale).round().max(1.0) as u16,
    };
    progressive::render_tile(bindings, page, spec, None, &CancelToken::default())
        .expect("render")
        .0
}

/// How many of a buffer's pixels are neither white nor transparent.
fn inked(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[3] != 0 && (px[0] < 240 || px[1] < 240 || px[2] < 240))
        .count()
}
