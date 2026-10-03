//! Does a text layer read back through PDFium as the words it was written from?
//!
//! `textlayer`'s unit tests read the operators it wrote. They say the numbers
//! are the ones intended; they cannot say PDFium --- the engine tpdf searches
//! and selects with --- turns those operators into the same words at the same
//! places. This probe is that half.
//!
//! It needs no OCR engine, so it runs on every platform. The words come from a
//! document that already has text:
//!
//! 1. Read each page's words and their boxes through PDFium.
//! 2. Empty every page's content with `lopdf`. **The control**: that file must
//!    read no text at all, or everything after it is satisfied by the text that
//!    was already there.
//! 3. Write the words back as a layer and read the result through PDFium.
//!
//! The checks, per page: every written word reads back with its own text
//! within [`allowed`] of the box it was written at, and nothing else reads
//! back; on an upright page the words come back in the order written; every
//! character is upright as the page is displayed; and the page renders exactly
//! as the emptied page did, because the layer must paint nothing. One more
//! check uses a page of its own: words whose boxes touch read back as separate
//! words.
//!
//! Run it on a fixture with `/Rotate` as well as an upright one. The turn is the
//! part most likely to be wrong, and an upright page cannot show it.
//!
//! Usage:
//!   textlayer-probe <text.pdf> [--lib DIR] [--emit PATH]
//!
//! `--emit` keeps the layered file instead of deleting it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use pdfium_render::prelude::Pdfium;
use tpdf_lib::document::OpenDocument;
use tpdf_lib::ocr::ControlWord;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};
use tpdf_lib::textlayer::{self, Layer, Word};
use tpdf_lib::{ocr_gate, pagetree, text};

/// How far a word's box may be from where it was written, beyond rounding.
///
/// A twentieth of a point, on top of [`allowed`]'s share of the box's size.
const TOLERANCE_PT: f32 = 0.05;

/// The allowance for one word: [`TOLERANCE_PT`] plus 1/64 of its longer side.
///
/// **Relative, because what it allows for is.** PDFium reports the layer's
/// glyph taller by 1/64 of the ascent and its right edge further by 1/64 of the
/// advance --- measured here, with a fixed tolerance of 0.05 pt failing every
/// word by an amount that grew with the type's size and with nothing else. The
/// font size is the box's height and the ascent is 0.8 of it, so the top moves
/// by less than `height / 64`; the advances add up to the box's width, so the
/// right edge moves by less than `width / 64`.
///
/// A flat half point was tried first and passed a layer whose every word ran
/// 2.4% long.
fn allowed(rect: [f32; 4]) -> f32 {
    let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
    TOLERANCE_PT + w.max(h) / 64.0
}

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
    let [source] = files.as_slice() else {
        eprintln!("usage: textlayer-probe <text.pdf> [--lib DIR] [--emit PATH]");
        std::process::exit(2);
    };
    let bindings = bind(&library);

    let Some(original) = open(bindings, source) else {
        std::process::exit(1);
    };
    let pages = original.page_count();
    let wanted: Vec<Vec<ControlWord>> = (0..pages).map(|at| words(&original, at)).collect();
    let total: usize = wanted.iter().map(Vec::len).sum();
    if !check(
        "the source has words to write back",
        total > 0,
        &format!("{total} words on {pages} pages"),
    ) {
        finish(false);
    }

    let scratch = std::env::temp_dir();
    let emptied_at = scratch.join(format!("tpdf-textlayer-empty-{}.pdf", std::process::id()));
    let layered_at = emit.clone().unwrap_or_else(|| {
        scratch.join(format!("tpdf-textlayer-probe-{}.pdf", std::process::id()))
    });

    let mut doc = Document::load(source).expect("lopdf reads the source");
    for page in pagetree::ordered_pages(&doc) {
        let blank = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page dictionary")
            .set("Contents", blank);
    }
    doc.save(&emptied_at).expect("the emptied file is written");

    let layers: Vec<Layer> = wanted
        .iter()
        .enumerate()
        .map(|(at, words)| Layer {
            page: at as u32,
            words: words
                .iter()
                .map(|word| Word {
                    text: word.text.clone(),
                    rect: word.rect,
                })
                .collect(),
        })
        .collect();
    if let Err(why) = textlayer::write(&mut doc, &layers) {
        check("the layer is written", false, &why);
        finish(false);
    }
    doc.save(&layered_at).expect("the layered file is written");

    let (Some(emptied), Some(layered)) = (open(bindings, &emptied_at), open(bindings, &layered_at))
    else {
        finish(false);
    };

    let mut ok = true;
    for at in 0..pages {
        let n = at + 1;
        let none = words(&emptied, at);
        ok &= check(
            &format!("page {n}, emptied, reads no words (the control)"),
            none.is_empty(),
            &format!("{} words", none.len()),
        );

        let want = &wanted[at as usize];
        let got = words(&layered, at);

        // Paired by text and place rather than by position in the list: each
        // written word must find a word of its own, with the same text, whose
        // box is within the tolerance. A read-back word is used once.
        let mut free: Vec<&ControlWord> = got.iter().collect();
        let mut worst = 0.0_f32;
        let mut missing: Option<(&ControlWord, Option<[f32; 4]>)> = None;
        for word in want {
            let found = free
                .iter()
                .enumerate()
                .filter(|(_, got)| got.text == word.text)
                .map(|(i, got)| (i, apart(got.rect, word.rect), got.rect))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match found {
                Some((i, off, _)) if off <= allowed(word.rect) => {
                    worst = worst.max(off);
                    free.swap_remove(i);
                }
                other => {
                    missing.get_or_insert((word, other.map(|(_, _, rect)| rect)));
                }
            }
        }
        ok &= check(
            &format!("page {n}: every word reads back at the box it was written at"),
            missing.is_none() && free.is_empty(),
            &match (missing, free.first()) {
                (Some((word, nearest)), _) => format!(
                    "{:?} at {:?} has no word within {:.3} pt; the nearest is at {nearest:?}",
                    word.text,
                    word.rect,
                    allowed(word.rect)
                ),
                (None, Some(extra)) => {
                    format!("{:?} at {:?} was not written", extra.text, extra.rect)
                }
                (None, None) => format!("{} words, worst {worst:.3} pt", want.len()),
            },
        );

        // The order is a claim about an upright page only. The words here come
        // from type that is upright in the page's own space, so on a turned page
        // they are in an order no reader of the displayed page would give ---
        // and PDFium, reading the layer, puts words on one line back in the
        // order they sit, which is then a different order and the right one.
        let name = format!("page {n} reads the words in the order they were written");
        if turns(&original, at) == Some(0) {
            let same = got.len() == want.len()
                && got
                    .iter()
                    .zip(want)
                    .all(|(got, want)| got.text == want.text);
            ok &= check(
                &name,
                same,
                &format!("{} words against {}", got.len(), want.len()),
            );
        } else {
            skip(
                &name,
                "the page is displayed turned, so the source order is not a reading order",
            );
        }

        // **The one check that sees which way the type runs.** The glyph is a
        // box, so a word set along the page's own axis on a turned page fills the
        // same rectangle as one set the reader's way, and every check above
        // passes --- which is what they did with the turn taken out of the
        // writer. What differs is each character's own orientation.
        let sideways = sideways(&layered, at);
        ok &= check(
            &format!("page {n}: every character is upright as the page is displayed"),
            sideways == Some(0),
            &match sideways {
                Some(count) => format!("{count} characters are not"),
                None => "the page's text could not be read".to_string(),
            },
        );

        match (emptied.page(at), layered.page(at)) {
            (Ok(before), Ok(after)) => {
                let (before, after) = (tile(bindings, &before), tile(bindings, &after));
                ok &= check(
                    &format!("page {n} renders as the emptied page did"),
                    before == after,
                    &format!("{} bytes compared", before.len()),
                );
            }
            _ => {
                ok &= check(
                    &format!("page {n} renders as the emptied page did"),
                    false,
                    "one of the two pages could not be opened",
                );
            }
        }
    }

    ok &= touching(bindings, &scratch);

    let _ = std::fs::remove_file(&emptied_at);
    if emit.is_some() {
        println!(
            "[..]   kept the layered document at {}",
            layered_at.display()
        );
    } else {
        let _ = std::fs::remove_file(&layered_at);
    }
    finish(ok);
}

/// Words whose boxes touch or overlap must still read back as separate words.
///
/// The fixture's own words cannot show this: their boxes are PDFium's, tight to
/// the type, with the page's real gaps between them. A recogniser's are looser
/// and meet edge to edge. So this page is made here --- three words on one
/// line, the second starting where the first ends and the third two points
/// inside the second.
fn touching(bindings: progressive::Bindings, scratch: &Path) -> bool {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 400.into(), 200.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page)],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let word = |text: &str, left: f32, right: f32| Word {
        text: text.into(),
        rect: [left, 100.0, right, 112.0],
    };
    let layer = Layer {
        page: 0,
        words: vec![
            word("left", 100.0, 150.0),
            word("middle", 150.0, 200.0),
            word("right", 198.0, 250.0),
        ],
    };
    let name = "three words whose boxes touch read back as three words";
    if let Err(why) = textlayer::write(&mut doc, &[layer]) {
        return check(name, false, &why);
    }
    let at = scratch.join(format!(
        "tpdf-textlayer-touching-{}.pdf",
        std::process::id()
    ));
    doc.save(&at).expect("the touching-words file is written");
    let read: Vec<String> = open(bindings, &at)
        .map(|document| words(&document, 0))
        .unwrap_or_default()
        .into_iter()
        .map(|word| word.text)
        .collect();
    let _ = std::fs::remove_file(&at);
    check(
        name,
        read == ["left", "middle", "right"],
        &format!("{read:?}"),
    )
}

/// The largest difference between two boxes' matching edges.
fn apart(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|i| (a[i] - b[i]).abs()).fold(0.0_f32, f32::max)
}

/// The quarter turns a page is displayed at, or `None` if it cannot be read.
fn turns(document: &OpenDocument, at: u32) -> Option<u8> {
    let page = document.page(at).ok()?;
    Some(text::extract(&page).ok()?.quarter_turns)
}

/// How many of a page's characters are not upright once the page is displayed.
///
/// A character's turn is relative to the unrotated page and the page's is
/// added by whoever displays it, so upright as displayed is the two summing to
/// a whole turn. White space is left out: PDFium makes the spaces between words
/// up, and they have no orientation of their own.
fn sideways(document: &OpenDocument, at: u32) -> Option<usize> {
    let page = document.page(at).ok()?;
    let read = text::extract(&page).ok()?;
    Some(
        read.codes
            .iter()
            .enumerate()
            .filter(|(_, code)| !char::from_u32(**code).is_some_and(char::is_whitespace))
            .filter(|(i, _)| {
                let own = read.char_turns.get(*i).copied().unwrap_or(0);
                (own + read.quarter_turns) % 4 != 0
            })
            .count(),
    )
}

/// One page's words through PDFium, or none if the page or its text cannot be read.
fn words(document: &OpenDocument, at: u32) -> Vec<ControlWord> {
    document
        .page(at)
        .ok()
        .and_then(|page| text::extract(&page).ok())
        .map(|page| ocr_gate::words_from(&page))
        .unwrap_or_default()
}

fn finish(ok: bool) -> ! {
    let (ran, passed) = (RAN.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
    let skipped = SKIPPED.load(Ordering::Relaxed);
    println!("{passed}/{ran} checks passed, {skipped} skipped");
    std::process::exit(if ok && passed == ran { 0 } else { 1 });
}

fn bind(library: &Path) -> progressive::Bindings {
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = progressive::bind_library(&path).expect("could not load Pdfium");
    progressive::bindings_of(bound)
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

/// A 400-pixel-wide render of the whole page, for comparing.
fn tile(bindings: progressive::Bindings, page: &RawPage<'_>) -> Vec<u8> {
    let w = page.width_pt();
    let h = page.height_pt();
    let scale = 400.0 / w;
    let spec = TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width: 400,
        height: (h * scale).round().max(1.0) as u16,
    };
    progressive::render_tile(bindings, page, spec, None, &CancelToken::default())
        .expect("render")
        .0
}

static RAN: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);
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
    println!("{:6} {name}  {detail}", if ok { "[OK]" } else { "[FAIL]" });
    ok
}
