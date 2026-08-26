//! Does a merged document read back as the two documents that went into it?
//!
//! `save::write_merged`'s unit tests are `lopdf` reading back what `lopdf`
//! wrote, plus a page count from the OS parser. Both say the *tree* is right.
//! Neither says PDFium --- the engine tpdf renders with --- draws page seven, or
//! that the page it draws is the page that was merged in.
//!
//! So every check here compares the merged file against its **sources**, read
//! through PDFium, and the strong ones are the last three:
//!
//! * **Every page renders with ink.** A page grafted into a tree whose
//!   resources it cannot reach renders blank, which no structural check sees ---
//!   `lopdf` is perfectly happy with a `/Contents` naming a stream whose fonts
//!   are gone.
//! * **Each page keeps the size it had in its own file.** This is the
//!   inheritance claim (`pagetree::detached_page`) measured through a renderer
//!   rather than by reading the dictionary we just wrote. A page that inherited
//!   its `/MediaBox` from the node above it, and lost that on the way across,
//!   comes out at the *destination's* size --- so it lays out wrong rather than
//!   failing.
//! * **Each page's text is the text it had in its own file.** The one check
//!   that says the content stream and its font resources arrived together: a
//!   page whose `/Font` dictionary went missing extracts nothing, or extracts
//!   the wrong code points, while still rendering something.
//!
//! The control is the first document. Its pages are not moved by a merge and
//! must read back identically --- without it, every assertion above is satisfied
//! by a "merge" that wrote one of the two files out unchanged.
//!
//! **In-process rather than through the worker pool**, deliberately: what is
//! under test is the document, and `backend-probe` already drives the pool over
//! whatever file it is given. Point that at the emitted file for the other half.
//!
//! Usage:
//!   merge-probe <first.pdf> <second.pdf> [--lib DIR] [--emit PATH]
//!
//! `--emit` keeps the merged file instead of deleting it, which is how
//! `testdata/merged.pdf` is made: it is a window-sweep corpus that no Python
//! generator can write, because the thing that has to produce it is this
//! repository's own merge.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tpdf_lib::document::OpenDocument;

use lopdf::Document;
use pdfium_render::prelude::Pdfium;
use tpdf_lib::edits::{PageView, Plan};
use tpdf_lib::pagetree;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};
use tpdf_lib::save;
use tpdf_lib::text;

fn main() {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut emit: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            "--emit" => emit = args.next().map(PathBuf::from),
            other => files.push(PathBuf::from(other)),
        }
    }
    let [first, second] = files.as_slice() else {
        eprintln!("usage: merge-probe <first.pdf> <second.pdf> [--lib DIR] [--emit PATH]");
        std::process::exit(2);
    };

    let bindings = bind(&library);

    // Opened before the merge, so that a source this probe cannot read is
    // reported as such rather than as a merge that came out wrong.
    let (Some(a), Some(b)) = (open(bindings, first), open(bindings, second)) else {
        std::process::exit(1);
    };
    let (mine, theirs) = (a.page_count(), b.page_count());

    let out = emit.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("tpdf-merge-probe-{}.pdf", std::process::id()))
    });
    let plan = whole(mine);
    let merged = match save::write_merged(first, &plan, std::slice::from_ref(second), &out) {
        Ok(merged) => merged,
        Err(why) => {
            println!("[FAIL] the merge was refused: {why}");
            std::process::exit(1);
        }
    };
    check(
        "the merge reports both documents' pages",
        merged.pages as usize == mine as usize + theirs as usize,
        &format!("{} against {mine} + {theirs}", merged.pages),
    );

    let Some(both) = open(bindings, &out) else {
        println!("[FAIL] PDFium cannot open the merged document at all");
        std::process::exit(1);
    };
    check(
        "PDFium counts both documents' pages",
        both.page_count() == mine + theirs,
        &format!("{} against {mine} + {theirs}", both.page_count()),
    );

    // What each page of the merged document should measure, read out of the two
    // sources' object graphs before anything is compared. Built here rather than
    // per page so that a source this probe cannot parse fails once, loudly, and
    // not as a page-shaped mystery in the middle of the loop.
    let Some(sizes) = expected_sizes(first, second) else {
        std::process::exit(1);
    };

    // The first document's pages are at the front and the second's follow, which
    // is the order `merge::append` promises and the order every comparison below
    // is indexed by.
    let mut ok = true;
    for (at, (source, index)) in (0..mine)
        .map(|i| (&a, i))
        .chain((0..theirs).map(|i| (&b, i)))
        .enumerate()
    {
        let at = at as u32;
        let from = if at < mine { "first" } else { "second" };
        let Ok(page) = both.page(at) else {
            ok &= check(
                &format!("page {} of the merge exists", at + 1),
                false,
                "the merged document has no such page",
            );
            continue;
        };
        let Ok(original) = source.page(index) else {
            ok &= check(
                &format!("page {} of the merge exists", at + 1),
                false,
                "its source has no such page",
            );
            continue;
        };

        // Size first: everything after it is measured in this page's own space,
        // so a page that came across at the wrong size makes the rest of this
        // loop compare two different rectangles and report on the wrong thing.
        //
        // **The oracle is the object graph, not PDFium's reading of the
        // source**, and that is not fastidiousness. PDFium reports the wrong
        // size for a page whose `/MediaBox` is inherited *and* which is rotated
        // a quarter turn --- `width x width` instead of `height x width`,
        // measured at 90 and 270 and correct at 0 and 180. Comparing the merged
        // page against that reading would make this check demand that a merge
        // reproduce a defect, and `testdata/inherited.pdf` is exactly such a
        // document. `pagetree::displayed_page` walks the tree with `lopdf` and
        // is right; see docs/TRAPS.md.
        //
        // PDFium's own reading of the source is printed beside it, so the two
        // disagreeing is visible in the line rather than silently resolved.
        let want = sizes[at as usize];
        ok &= check(
            &format!(
                "page {} keeps the size it had in the {from} document",
                at + 1
            ),
            near(page.width_pt(), want.0) && near(page.height_pt(), want.1),
            &format!(
                "{:.1}x{:.1} against {:.1}x{:.1} from the page tree \
                 (PDFium reads the source as {:.1}x{:.1})",
                page.width_pt(),
                page.height_pt(),
                want.0,
                want.1,
                original.width_pt(),
                original.height_pt()
            ),
        );

        // **Two claims, because only one of them has a trustworthy baseline.**
        //
        // The first is unconditional and is the one that matters: a merged page
        // that draws *nothing* is what a lost resource dictionary looks like,
        // and no structural check sees it --- `lopdf` is content with a
        // `/Contents` naming a stream whose fonts are gone.
        //
        // The second compares against the source's own render, and that is only
        // a baseline where PDFium reads the source correctly. On a page whose
        // box is inherited and which is turned a quarter, it does not: the page
        // comes out `width x width`, so content above that line is off the sheet
        // and the render is nearly blank. Measured on `testdata/inherited.pdf`:
        // 0, 1 and 3 inked pixels against the merged pages' 1013, 1062 and 1317.
        // Comparing against that would report a correct merge as a defect --- so
        // it is skipped, out loud, with the reason.
        let drawn = inked(&tile(bindings, &page));
        ok &= check(
            &format!("page {} draws something at all", at + 1),
            drawn > 0,
            &format!("{drawn} inked pixels"),
        );
        let baseline_usable =
            near(original.width_pt(), want.0) && near(original.height_pt(), want.1);
        if baseline_usable {
            let before = inked(&tile(bindings, &original));
            ok &= check(
                &format!("page {} draws what it drew in the {from} document", at + 1),
                before > 0 && ratio(drawn, before) > 0.9 && ratio(drawn, before) < 1.1,
                &format!("{drawn} inked pixels against {before}"),
            );
        } else {
            skip(
                &format!("page {} draws what it drew in the {from} document", at + 1),
                &format!(
                    "PDFium reads that source page as {:.1}x{:.1} where the page tree says \
                     {:.1}x{:.1}, so its render is not a baseline",
                    original.width_pt(),
                    original.height_pt(),
                    want.0,
                    want.1
                ),
            );
        }

        // Text last, because it is the check that needs the fonts as well as the
        // stream. A page whose `/Font` went missing still renders --- PDFium
        // substitutes --- and extracts nothing, or the wrong code points.
        let (now, then) = (text::extract(&page), text::extract(&original));
        match (now, then) {
            (Ok(now), Ok(then)) => {
                ok &= check(
                    &format!("page {} reads as it read in the {from} document", at + 1),
                    now.codes == then.codes,
                    &format!("{} characters against {}", now.len(), then.len()),
                );
            }
            _ => {
                ok &= check(
                    &format!("page {} reads as it read in the {from} document", at + 1),
                    false,
                    "text could not be extracted from one of the two",
                );
            }
        }
    }

    if emit.is_some() {
        println!("[..]   kept the merged document at {}", out.display());
    } else {
        let _ = std::fs::remove_file(&out);
    }

    let (ran, passed) = (RAN.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
    let skipped = SKIPPED.load(Ordering::Relaxed);
    println!("{passed}/{ran} checks passed, {skipped} skipped");
    std::process::exit(if ok && passed == ran { 0 } else { 1 });
}

/// The displayed size of every page of `first` then `second`, from the graph.
///
/// `lopdf` plus `pagetree::displayed_page`, which resolves the four inheritable
/// attributes by walking `/Parent` --- an implementation this repository owns and
/// which shares nothing with PDFium. That independence is the point: it is what
/// lets the size check above be a statement about the merge rather than about
/// whichever reading the renderer happens to give.
///
/// `None` when a source cannot be parsed, having said which.
fn expected_sizes(first: &Path, second: &Path) -> Option<Vec<(f32, f32)>> {
    let mut out = Vec::new();
    for path in [first, second] {
        let document = match Document::load(path) {
            Ok(document) => document,
            Err(why) => {
                println!(
                    "[FAIL] {} could not be parsed for its page sizes: {why}",
                    path.display()
                );
                return None;
            }
        };
        for page in pagetree::ordered_pages(&document) {
            let shown = pagetree::displayed_page(&document, page);
            out.push((shown.width, shown.height));
        }
    }
    Some(out)
}

/// A plan that keeps every page of a `pages`-page document, unturned.
///
/// The identity plan, so that what this probe measures is the merge rather than
/// the edits `planned_bytes` would apply on the way through. A plan that dropped
/// or turned a page would make every comparison below a comparison against a
/// page that was deliberately changed.
fn whole(pages: u32) -> Plan {
    Plan {
        opened_as: None,
        baseline: pages,
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at) + 1,
                source: at,
                turns: 0,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        marks: Vec::new(),
    }
}

fn bind(library: &Path) -> progressive::Bindings {
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path).expect("could not load Pdfium");
    progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))))
}

fn open(bindings: progressive::Bindings, path: &Path) -> Option<OpenDocument> {
    match OpenDocument::open(bindings, path, None) {
        Ok(document) => Some(document),
        Err(why) => {
            println!("[FAIL] {}: {why}", path.display());
            None
        }
    }
}

/// Two point sizes that are the same page.
///
/// A tenth of a point: the two documents are serialised separately, so a
/// coordinate can differ in its last decimal without anything having moved.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

/// How much of `now` there is compared with `then`, guarding a zero denominator.
fn ratio(now: usize, then: usize) -> f64 {
    if then == 0 {
        return 0.0;
    }
    now as f64 / then as f64
}

/// A 200-pixel-wide render of the whole page, for comparing.
fn tile(bindings: progressive::Bindings, page: &RawPage<'_>) -> Vec<u8> {
    let w = page.width_pt();
    let h = page.height_pt();
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
///
/// `crop_probe`'s definition, and deliberately the same one: two probes
/// disagreeing about what counts as ink would make their numbers
/// incomparable, and the threshold has a measured reason there.
fn inked(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[3] != 0 && (px[0] < 240 || px[1] < 240 || px[2] < 240))
        .count()
}

/// Checks run and checks passed, for the summary line at the end.
///
/// A run with no summary line is thrown away as unreadable rather than counted:
/// a probe that died halfway prints exactly the same green lines as one that
/// finished.
static RAN: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

/// A check that could not be made, and why.
///
/// Counted apart from the passes so that the summary cannot read as coverage it
/// does not have --- a skip that is silently folded into "all checks passed" is
/// the shape this project has a trap about.
static SKIPPED: AtomicUsize = AtomicUsize::new(0);

fn skip(name: &str, why: &str) {
    SKIPPED.fetch_add(1, Ordering::Relaxed);
    println!("{:6} {name}  {why}", "[SKIP]");
}

fn check(name: &str, ok: bool, detail: &str) -> bool {
    RAN.fetch_add(1, Ordering::Relaxed);
    if ok {
        PASSED.fetch_add(1, Ordering::Relaxed);
    }
    // `[OK]` is four characters and `[FAIL]` six, so the label is padded rather
    // than interpolated --- see the trap about a status column that is two wide
    // when it passes.
    println!("{:6} {name}  {detail}", if ok { "[OK]" } else { "[FAIL]" });
    ok
}
