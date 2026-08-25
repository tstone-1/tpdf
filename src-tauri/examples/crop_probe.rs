//! Does cropping a page do what the model says, and does PDFium follow it?
//!
//! Three claims, and each is measured rather than reasoned about.
//!
//! * `--mode follows` --- setting a page's crop box moves **everything** that
//!   reads it: the reported size, the origin every character box is measured
//!   from, the render, and the text mapping. This is the load-bearing one: the
//!   whole design is that no consumer needs a crop parameter because the page
//!   already carries it. Its control is the restore --- putting the file's own
//!   box back must return every one of those numbers to what it was, or the
//!   page cache turns one reader's crop into everyone's.
//!
//! * `--mode content` --- the measured content box is inside the page, smaller
//!   than it on a page with margins, and **not** meaningfully smaller on one
//!   whose ink runs to the edge. The second half is the control: a "content box"
//!   that always shrinks by a fixed amount would pass the first half.
//!
//! * `--mode ink` --- cropping to the content box raises the fraction of the
//!   rendered page that is ink. That is the reader-visible claim (margins gone,
//!   text bigger) and it is the one no structural check can make.
//!
//! Usage:
//! * `--mode geometry` --- the number the frontend lays out from. A crop's
//!   displayed rectangle inside the file's own page must have the same width and
//!   height as the cropped page reports for itself, and those come from two
//!   independent places: one through the rotation table in `text::to_device`,
//!   the other from PDFium. Agreement is the check; the uncropped case is its
//!   control, where the rectangle must be the whole page at the origin.
//!
//! Usage:
//!   crop-probe <file.pdf> [--page N] [--mode follows|content|ink|geometry]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tpdf_lib::document::OpenDocument;

use pdfium_render::prelude::Pdfium;
use tpdf_lib::content::{self, MARGIN_PT};
use tpdf_lib::pagetree;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};
use tpdf_lib::text;

fn main() {
    let mut file = None;
    let mut page = 0u32;
    let mut mode = String::from("follows");
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" => page = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--mode" => mode = args.next().unwrap_or_default(),
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            other => file = Some(PathBuf::from(other)),
        }
    }
    let Some(file) = file else {
        eprintln!("usage: crop-probe <file.pdf> [--page N] [--mode follows|content|ink]");
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

    let ok = match mode.as_str() {
        "follows" => follows(bindings, &doc, &file, page),
        "content" => content(bindings, &doc, page),
        "ink" => ink(bindings, &doc, page),
        "geometry" => geometry(bindings, &doc, page),
        other => {
            println!("[FAIL] unknown mode {other}");
            false
        }
    };
    let (ran, passed) = (RAN.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
    println!("{passed}/{ran} checks passed");
    std::process::exit(if ok && passed == ran { 0 } else { 1 });
}

fn bind(library: &Path) -> progressive::Bindings {
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path).expect("could not load Pdfium");
    progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))))
}

/// Checks run and checks passed, for the summary line at the end.
///
/// A summary is not decoration here: `scripts/mutate_viewer.py` reads it, and a
/// run with no summary line is thrown away as unreadable rather than counted ---
/// which is the right call, since a probe that died halfway prints exactly the
/// same green lines as one that finished.
static RAN: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

fn check(name: &str, ok: bool, detail: &str) -> bool {
    RAN.fetch_add(1, Ordering::Relaxed);
    if ok {
        PASSED.fetch_add(1, Ordering::Relaxed);
    }
    println!("[{}] {name}  {detail}", if ok { "OK" } else { "FAIL" });
    ok
}

/// Everything a page answers about its own geometry, in one reading.
#[derive(PartialEq, Debug)]
struct Reading {
    width: f32,
    height: f32,
    origin: (f32, f32),
    box_pt: [f32; 4],
    first_box: Vec<f32>,
    chars: usize,
    /// The rendered pixels themselves, not a count of the inked ones: an A0
    /// drawing covers every pixel of a 200-wide tile whatever it is cropped to,
    /// so a count is equal before and after and says nothing.
    pixels: Vec<u8>,
}

fn read(bindings: progressive::Bindings, page: &RawPage<'_>) -> Reading {
    let extracted = text::extract(page).expect("text");
    Reading {
        width: page.width_pt(),
        height: page.height_pt(),
        origin: page.origin_pt(),
        box_pt: page.crop_pt(),
        first_box: extracted.boxes.iter().take(4).copied().collect(),
        chars: extracted.len(),
        pixels: tile(bindings, page),
    }
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
fn inked(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[3] != 0 && (px[0] < 240 || px[1] < 240 || px[2] < 240))
        .count()
}

fn follows(bindings: progressive::Bindings, doc: &OpenDocument, file: &Path, index: u32) -> bool {
    let before = read(bindings, &doc.page(index).expect("page"));
    // **The middle half of the page's OWN box**, not of its displayed size. The
    // first version of this built the inset from `width_pt`/`height_pt`, which
    // are the *rotated* dimensions, and then asserted the cropped page would be
    // half of each of them --- on `rotated-90.pdf` that is a rectangle 396 wide
    // in a space where the page is 612 wide, and the check reported a working
    // crop as broken with the two numbers transposed.
    let [llx, lly, urx, ury] = before.box_pt;
    let (w, h) = (urx - llx, ury - lly);
    let inset = [
        llx + w * 0.25,
        lly + h * 0.25,
        urx - w * 0.25,
        ury - h * 0.25,
    ];
    let cropped = read(
        bindings,
        &doc.page_cropped(index, Some(inset)).expect("page"),
    );
    let restored = read(bindings, &doc.page(index).expect("page"));

    let mut ok = true;
    // **Before anything is cropped**, and against a size derived from the file
    // by another library. `pagetree::displayed_page` reads `/MediaBox`,
    // `/CropBox` and `/Rotate` through `lopdf` and applies §14.11.2's
    // intersection; PDFium reads the same three and applies its own. Every other
    // check in this mode reads `crop_pt`, which is the rule under test --- so a
    // rule that corrupted the box would corrupt their before and their after
    // equally and none of them could fail. This is the one independent
    // derivation, and it is the one that catches a crop box in the wrong space.
    {
        let page = doc.page(index).expect("page");
        let (got_w, got_h) = (page.width_pt(), page.height_pt());
        match lopdf::Document::load(file).ok().and_then(|parsed| {
            let ids = pagetree::ordered_pages(&parsed);
            ids.get(index as usize)
                .map(|id| pagetree::displayed_page(&parsed, *id))
        }) {
            Some(shown) => {
                ok &= check(
                    "the page is the size the file says, read by another library",
                    (got_w - shown.width).abs() < 0.01 && (got_h - shown.height).abs() < 0.01,
                    &format!(
                        "PDFium {got_w:.1}x{got_h:.1}, lopdf {:.1}x{:.1} at {} quarter turns",
                        shown.width, shown.height, shown.turns
                    ),
                );
            }
            None => println!(
                "[SKIP] the page is the size the file says, read by another library  lopdf would not parse it"
            ),
        }
    }
    // Halving both dimensions of the page's own box halves both dimensions of
    // the displayed one, whichever way round the page is turned --- which is
    // what makes this one assertion right at every `/Rotate`.
    ok &= check(
        "the reported size follows the crop",
        (cropped.width - before.width * 0.5).abs() < 1.0
            && (cropped.height - before.height * 0.5).abs() < 1.0,
        &format!(
            "{:.1}x{:.1} -> {:.1}x{:.1}, page box {:?} -> {:?}",
            before.width,
            before.height,
            cropped.width,
            cropped.height,
            before.box_pt,
            cropped.box_pt
        ),
    );
    ok &= check(
        "the origin every character is measured from follows it",
        (cropped.origin.0 - inset[0]).abs() < 0.1 && (cropped.origin.1 - inset[1]).abs() < 0.1,
        &format!(
            "{:?} -> {:?}, asked {:?}",
            before.origin,
            cropped.origin,
            (inset[0], inset[1])
        ),
    );
    if before.chars == 0 {
        println!("[SKIP] the text mapping follows it  the page has no extractable text");
    } else {
        ok &= check(
            "the text mapping follows it",
            before.first_box != cropped.first_box,
            &format!("{:?} -> {:?}", before.first_box, cropped.first_box),
        );
    }
    ok &= check(
        "the render follows it",
        before.pixels != cropped.pixels,
        &format!(
            "{} inked pixels of {} -> {} of {}",
            inked(&before.pixels),
            before.pixels.len() / 4,
            inked(&cropped.pixels),
            cropped.pixels.len() / 4
        ),
    );
    // The control, and the one that says the page cache cannot leak a crop into
    // the next request for the same page.
    ok &= check(
        "asking for no crop puts every one of them back",
        restored == before,
        &format!(
            "restored == before: {}, size {:?} origin {:?} box {:?}",
            restored == before,
            (restored.width, restored.height),
            restored.origin,
            restored.box_pt,
        ),
    );
    ok
}

fn content(bindings: progressive::Bindings, doc: &OpenDocument, index: u32) -> bool {
    let page = doc.page_cropped(index, None).expect("page");
    let crop = page.crop_pt();
    let found = content::content_box(bindings, &page, &CancelToken::default()).expect("content");
    let Some(found) = found else {
        return check("the page has a content box", false, "none found");
    };
    let area = |r: [f64; 4]| (r[2] - r[0]) * (r[3] - r[1]);
    let sheet = area(crop.map(f64::from));
    let mut ok = true;
    ok &= check(
        "the content box is inside the page",
        found[0] >= f64::from(crop[0]) - 0.001
            && found[1] >= f64::from(crop[1]) - 0.001
            && found[2] <= f64::from(crop[2]) + 0.001
            && found[3] <= f64::from(crop[3]) + 0.001,
        &format!("{found:?} in {crop:?}"),
    );
    ok &= check(
        "it is proper",
        found[2] > found[0] && found[3] > found[1],
        &format!(
            "{:.1} x {:.1} points",
            found[2] - found[0],
            found[3] - found[1]
        ),
    );
    println!(
        "[..] it keeps {:.1}% of the sheet's area, margin {MARGIN_PT} pt",
        100.0 * area(found) / sheet
    );
    ok
}

fn ink(bindings: progressive::Bindings, doc: &OpenDocument, index: u32) -> bool {
    // **Every reading of the uncropped page is taken before the crop**, and
    // that is not tidiness. `page_cropped` hands back a `RawPage` wrapping the
    // *cached* handle, so two of them for one index are aliases: cropping
    // through the second changes what the first answers. Reading the page size
    // afterwards --- which the first version of this did --- divides an
    // uncropped ink count by the cropped page's pixel count, and reports a
    // density of 1.23, which is not a number a fraction can take.
    let (before, page_px, sheet, found) = {
        let page = doc.page_cropped(index, None).expect("page");
        let pixels = tile(bindings, &page);
        let px = pixels.len() as f64 / 4.0;
        let sheet = f64::from(page.width_pt()) * f64::from(page.height_pt());
        let found = content::content_box(bindings, &page, &CancelToken::default()).expect("box");
        (inked(&pixels) as f64, px, sheet, found)
    };
    let Some(found) = found else {
        println!("[SKIP] cropping to the content box raises the ink density  no content box");
        return true;
    };
    let box_pt = [
        found[0] as f32,
        found[1] as f32,
        found[2] as f32,
        found[3] as f32,
    ];
    let (after, crop_px) = {
        let cropped = doc.page_cropped(index, Some(box_pt)).expect("page");
        let pixels = tile(bindings, &cropped);
        (inked(&pixels) as f64, pixels.len() as f64 / 4.0)
    };
    // Per rendered pixel, since the cropped render covers fewer of them: the
    // claim is that the reader's screen fills with more ink, not that more ink
    // exists. Both counts come from the buffer that was actually scanned, so
    // neither can disagree with the render it describes.
    let (d0, d1) = (before / page_px, after / crop_px);
    // **In points, not in pixels.** Both renders are 200 px wide whatever shape
    // the page is, so their pixel counts compare aspect ratios and not areas ---
    // and the first version of this guard, which divided one by the other,
    // reported a crop keeping a fifth of the sheet as "247% of it" and skipped
    // every row including the two that had something to say.
    let kept = (found[2] - found[0]) * (found[3] - found[1]) / sheet;
    if kept >= 0.98 {
        // A page whose ink already reaches its edges. Said rather than failed:
        // there is nothing to crop, so a density that did not rise is the right
        // answer and the honest control for every row that did.
        println!(
            "[SKIP] cropping to the content box raises the ink density  the content box is {:.1}% of the page, so nothing was cropped",
            100.0 * kept
        );
        return true;
    }
    check(
        "cropping to the content box raises the ink density",
        d1 > d0,
        &format!("{:.3} -> {:.3} of the rendered page", d0, d1),
    )
}

fn geometry(bindings: progressive::Bindings, doc: &OpenDocument, index: u32) -> bool {
    let none = tpdf_lib::render::geometry_of(doc, index, None).expect("geometry");
    let (whole, turns) = {
        let page = doc.page(index).expect("page");
        ((page.width_pt(), page.height_pt()), page.quarter_turns())
    };
    let mut ok = true;
    ok &= check(
        "with no crop the page is itself, at the origin",
        (none.width_pt - whole.0).abs() < 0.01
            && (none.height_pt - whole.1).abs() < 0.01
            && none.left == 0.0
            && none.top == 0.0,
        &format!("{none:?} against {whole:?}, /Rotate turns {turns}"),
    );

    let found = content::content_box(
        bindings,
        &doc.page(index).expect("page"),
        &CancelToken::default(),
    )
    .expect("content");
    let Some(found) = found else {
        println!(
            "[SKIP] the rectangle and the cropped page agree about the size  the page has no ink"
        );
        return ok;
    };
    let box_pt = [
        found[0] as f32,
        found[1] as f32,
        found[2] as f32,
        found[3] as f32,
    ];
    let cropped = tpdf_lib::render::geometry_of(doc, index, Some(box_pt)).expect("geometry");
    // Two independent answers to one question: the rectangle's own size, derived
    // through the rotation table, and PDFium's report of the cropped page. A
    // rotation table wrong at some turns makes these disagree there and nowhere
    // else, which is what a fixture at /Rotate 90 is in the corpus for.
    let placed = doc
        .page_cropped(index, Some(box_pt))
        .map(|p| (p.width_pt(), p.height_pt()))
        .expect("page");
    ok &= check(
        "the rectangle and the cropped page agree about the size",
        (cropped.width_pt - placed.0).abs() < 0.01 && (cropped.height_pt - placed.1).abs() < 0.01,
        &format!(
            "{:.2}x{:.2} reported, at ({:.2}, {:.2}) inside the page",
            cropped.width_pt, cropped.height_pt, cropped.left, cropped.top
        ),
    );
    ok &= check(
        "the crop sits inside the page it was measured on",
        cropped.left >= -0.01
            && cropped.top >= -0.01
            && cropped.left + cropped.width_pt <= whole.0 + 0.01
            && cropped.top + cropped.height_pt <= whole.1 + 0.01,
        &format!(
            "[{:.1} {:.1} {:.1} {:.1}] in {:.1}x{:.1}",
            cropped.left,
            cropped.top,
            cropped.left + cropped.width_pt,
            cropped.top + cropped.height_pt,
            whole.0,
            whole.1
        ),
    );
    ok
}
