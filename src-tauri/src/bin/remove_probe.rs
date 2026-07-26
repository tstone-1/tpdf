//! Minimal repro for the `FPDFPage_RemoveObject` ownership crash (spike 0.3).
//!
//! `fpdf_edit.h` states that removal transfers ownership to the caller and that
//! `FPDFPageObj_Destroy()` frees the object. `pdfium-render`'s `Drop` does
//! exactly that, and it segfaults inside `FPDFPageObj_Destroy` -- for text and
//! path objects alike, whether the destroy happens immediately, after content
//! regeneration, or after the save. Leaking the handle is the only safe option
//! through this binding.
//!
//! Kept rather than deleted because removal is redaction's primitive and this
//! is the cheapest possible regression: run it after any `pdfium-render` or
//! PDFium bump. Case `c` must pass; if case `a` starts passing too, the
//! upstream bug is fixed and the `std::mem::forget` in `text_roundtrip.rs` can
//! go.
//!
//! Usage: remove-probe <file.pdf> [a|b|c|d|e]

use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

fn main() {
    let file = std::env::args().nth(1).expect("usage: remove-probe <pdf>");
    let case = std::env::args().nth(2).unwrap_or_else(|| "a".to_string());

    let dir = pdfium_dir();
    let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
    let pdfium = Pdfium::new(Pdfium::bind_to_library(&path).expect("bind"));

    let doc = pdfium.load_pdf_from_file(&file, None).expect("open");
    let mut page = doc.pages().get(0).expect("page");
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

    // Find the first text object.
    let index = page
        .objects()
        .iter()
        .position(|o| o.object_type() == PdfPageObjectType::Text)
        .expect("no text object") as PdfPageObjectIndex;
    eprintln!("case {case}: removing text object at index {index}");

    match case.as_str() {
        // Destroy immediately, nothing else touched first.
        "a" => {
            let removed = page
                .objects_mut()
                .remove_object_at_index(index)
                .expect("remove");
            eprintln!("  removed");
            drop(removed);
            eprintln!("  destroyed [OK]");
        }
        // Load a text page first, as anything that reads text does, then
        // destroy immediately. The header warns every FPDF_TEXTPAGE for the
        // page is invalidated by the removal.
        "b" => {
            let text = page.text().expect("text page");
            eprintln!("  text page loaded, {} chars", text.all().chars().count());
            drop(text);
            let removed = page
                .objects_mut()
                .remove_object_at_index(index)
                .expect("remove");
            eprintln!("  removed");
            drop(removed);
            eprintln!("  destroyed [OK]");
        }
        // Leak the handle instead of destroying it.
        "c" => {
            let removed = page
                .objects_mut()
                .remove_object_at_index(index)
                .expect("remove");
            eprintln!("  removed");
            std::mem::forget(removed);
            eprintln!("  leaked [OK]");
        }
        // Destroy only after regenerating content and saving.
        "d" => {
            let removed = page
                .objects_mut()
                .remove_object_at_index(index)
                .expect("remove");
            page.regenerate_content().expect("regenerate");
            eprintln!("  regenerated");
            drop(removed);
            eprintln!("  destroyed [OK]");
        }
        // The same removal, but of a path object. Distinguishes a text-specific
        // fault (the header warns only about invalidated FPDF_TEXTPAGE handles)
        // from removal being unsafe in general.
        "e" => {
            let path_index = page
                .objects()
                .iter()
                .position(|o| o.object_type() == PdfPageObjectType::Path)
                .expect("no path object") as PdfPageObjectIndex;
            eprintln!("  path object at index {path_index}");
            let removed = page
                .objects_mut()
                .remove_object_at_index(path_index)
                .expect("remove");
            eprintln!("  removed");
            drop(removed);
            eprintln!("  destroyed [OK]");
        }
        other => panic!("unknown case {other}"),
    }

    eprintln!("case {case}: survived");
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
