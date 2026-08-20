//! Checks `textbox::advance` against the width PDFium actually lays down.
//!
//! **The table in `textbox.rs` is 95 numbers written out by hand**, and a wrong
//! entry is invisible: text still draws, still wraps, and wraps in the wrong
//! place. No unit test can find that, because a test would compare the table
//! against itself — a writer agreeing with its own reader, which this repository
//! has an entry about.
//!
//! So the oracle is the engine that will draw it. This writes a PDF whose page
//! contains one Helvetica string at a known size and origin, renders it through
//! PDFium, measures how far the ink actually extends, and compares that against
//! what `advance` predicted.
//!
//! **What the comparison can and cannot say.** Rendered ink is measured from the
//! left edge of the first glyph to the right edge of the last, and an *advance*
//! includes the right side bearing after that last glyph — so the measurement is
//! expected to come in slightly *under* the prediction, never over. A string
//! ending in a letter with a wide bearing (`A`, `V`) under-runs by more than one
//! ending in `l`. The tolerance is therefore one-sided and stated per string,
//! and a measurement *exceeding* the advance is a hard failure: that is ink
//! outside the box the wrap arithmetic promised.
//!
//! Run:
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example helvetica-probe
//! ```

use std::path::PathBuf;

use lopdf::{dictionary, Document, Object, Stream};
use tpdf_lib::progressive::{self, Placement, RawBitmap, RawDocument};
use tpdf_lib::textbox;

/// Where the string is placed on the page, in points from the bottom-left.
const ORIGIN: (f64, f64) = (50.0, 400.0);
/// The size every string is set at. Large, so a one-pixel error in the ink scan
/// is a small fraction of the number being compared.
const SIZE: f64 = 48.0;
/// Pixels per point when rendering.
const SCALE: f32 = 4.0;

fn main() {
    let library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let bindings = match progressive::bind(&library).map(progressive::bindings_of) {
        Ok(bindings) => bindings,
        Err(why) => {
            eprintln!("[FAIL] {why}");
            std::process::exit(2);
        }
    };

    // Chosen to exercise the parts of the table most likely to be wrong: the
    // wide capitals, the narrow lowercase, the digits (all one width), the
    // punctuation, and a German line for the accented Latin-1 range whose widths
    // are claimed to equal their base letters'.
    let strings = [
        "Hamburgefonstiv",
        "WAVE Tokyo",
        "illiwilli",
        "0123456789",
        "Grüße aus München",
        "the quick brown fox jumps",
        "(punctuation!) @ 50%",
    ];

    let mut ok = true;
    let mut scratch = std::env::temp_dir();
    scratch.push("tpdf-helvetica-probe.pdf");
    for text in strings {
        match measure(bindings, &scratch, text) {
            Ok((measured, predicted)) => {
                // One-sided: ink may fall short of the advance by the last
                // glyph's right bearing, and may never exceed it.
                let short = predicted - measured;
                let good = (0.0..SIZE * 0.20).contains(&short);
                ok &= good;
                println!(
                    "[{}]   {text:<26} advance {predicted:7.2} pt, ink {measured:7.2} pt, short by {short:5.2}",
                    if good { "OK" } else { "FAIL" }
                );
            }
            Err(why) => {
                ok = false;
                println!("[FAIL] {text}: {why}");
            }
        }
    }
    let _ = std::fs::remove_file(&scratch);

    // The control, and it is not decoration: every reading above is a comparison
    // between two numbers this repository produced, and a scan that found no ink
    // at all would report every string as "short by its whole advance" — which
    // the one-sided test above would reject, but for the wrong reason. This says
    // the instrument can see a difference it is supposed to see.
    match measure(bindings, &scratch, "l") {
        Ok((narrow, _)) => match measure(bindings, &scratch, "W") {
            Ok((wide, _)) => {
                let good = wide > narrow * 3.0;
                ok &= good;
                println!(
                    "[{}]   the scan can tell a wide glyph from a narrow one (W {wide:.2} pt against l {narrow:.2} pt)",
                    if good { "OK" } else { "FAIL" }
                );
            }
            Err(why) => {
                ok = false;
                println!("[FAIL] control: {why}");
            }
        },
        Err(why) => {
            ok = false;
            println!("[FAIL] control: {why}");
        }
    }
    let _ = std::fs::remove_file(&scratch);

    if !ok {
        std::process::exit(1);
    }
}

/// Renders `text` alone on a page and returns `(measured ink width, advance)`.
fn measure(
    bindings: progressive::Bindings,
    path: &std::path::Path,
    text: &str,
) -> Result<(f64, f64), String> {
    write_page(path, text)?;
    let document = RawDocument::open(bindings, path)?;
    let page = document.page(0)?;
    let width = (page.width_pt() * SCALE).round() as u16;
    let height = (page.height_pt() * SCALE).round() as u16;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
    let placement = Placement::tile(&page, SCALE, 0, 0, 0);
    let progress = progressive::render(
        &mut bitmap,
        &page,
        placement,
        None,
        &progressive::CancelToken::new(),
    );
    if !progress.outcome.is_done() {
        return Err(format!("render did not complete: {:?}", progress.outcome));
    }

    // The leftmost and rightmost columns holding any non-white pixel. The page
    // is blank apart from the string, so there is nothing else it could find.
    let pixels = bitmap.pixels();
    let (mut first, mut last) = (None, None);
    for x in 0..width as u32 {
        let mut inked = false;
        for y in 0..height as u32 {
            let at = ((y * width as u32 + x) * 4) as usize;
            if pixels[at] < 200 || pixels[at + 1] < 200 || pixels[at + 2] < 200 {
                inked = true;
                break;
            }
        }
        if inked {
            first.get_or_insert(x);
            last = Some(x);
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        return Err("nothing was drawn on the page".to_string());
    };
    let measured = f64::from(last + 1 - first) / f64::from(SCALE);
    Ok((measured, textbox::advance(text, SIZE)))
}

/// A one-page PDF holding `text` in Helvetica at [`SIZE`], and nothing else.
fn write_page(path: &std::path::Path, text: &str) -> Result<(), String> {
    let mut doc = Document::with_version("1.7");
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
    });
    // WinAnsi is Latin-1 for everything these strings use, so one byte per
    // character. `escape` handles the three bytes a literal string cannot carry
    // raw.
    let mut bytes = Vec::new();
    for ch in text.chars() {
        let code = ch as u32;
        if code > 0xff {
            return Err(format!("{ch:?} is not encodable in WinAnsi"));
        }
        let byte = code as u8;
        if matches!(byte, b'(' | b')' | b'\\') {
            bytes.push(b'\\');
        }
        bytes.push(byte);
    }
    let mut content = format!("BT /F1 {SIZE} Tf {} {} Td (", ORIGIN.0, ORIGIN.1).into_bytes();
    content.extend_from_slice(&bytes);
    content.extend_from_slice(b") Tj ET");
    let contents = doc.add_object(Stream::new(dictionary! {}, content));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => contents,
        "Resources" => resources,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => 1i64,
            "Kids" => vec![Object::Reference(page)],
            "MediaBox" => vec![0.into(), 0.into(), 842.into(), 595.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", Object::Reference(catalog));
    doc.save(path).map_err(|why| why.to_string())?;
    Ok(())
}
