//! Runs *Copy a graphic*'s export (`graphic.rs`) on a real document, in
//! process, and writes what it makes: for checking the feature against a PDF
//! a reader has, which `tests/cli/graphic.rs` cannot carry.
//!
//! ```text
//! cargo run --example graphic_probe -- <file.pdf> <page from 1> <left> <bottom> <right> <top> [out-dir]
//! ```
//!
//! The region is in page space, points from the page's lower-left corner. It
//! prints the description and writes `graphic.png`, `graphic.svg` (when one
//! can be made) and the stored picture (when the region holds one) to
//! `out-dir`, the current directory by default.
use std::path::PathBuf;

use tpdf_lib::document::OpenDocument;
use tpdf_lib::graphic::{export, GraphicFormat, GraphicRequest};
use tpdf_lib::progressive;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 6 {
        eprintln!("usage: graphic_probe <file.pdf> <page> <left> <bottom> <right> <top> [out-dir]");
        std::process::exit(2);
    }
    if let Err(why) = run(&args) {
        eprintln!("[FAIL] {why}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let number = |at: usize| {
        args[at]
            .parse::<f64>()
            .map_err(|e| format!("{}: {e}", args[at]))
    };
    let file = PathBuf::from(&args[0]);
    let page = args[1]
        .parse::<u32>()
        .map_err(|e| e.to_string())?
        .saturating_sub(1);
    let (left, bottom, right, top) = (number(2)?, number(3)?, number(4)?, number(5)?);
    let out = PathBuf::from(args.get(6).map_or(".", String::as_str));
    let library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let bindings = progressive::bindings_of(progressive::bind(&library)?);
    let document = OpenDocument::open(bindings, &file, None).map_err(|e| format!("{e:?}"))?;
    let height = f64::from(
        document
            .pdfium()
            .page_cropped(page, None, &|_| None)?
            .height_pt(),
    );
    let request = |format| GraphicRequest {
        page,
        crop: None,
        turns: 0,
        scale: 4.0,
        rect: [
            (left * 4.0).floor() as i32,
            ((height - top) * 4.0).floor() as i32,
            ((right - left) * 4.0).ceil() as i32,
            ((top - bottom) * 4.0).ceil() as i32,
        ],
        format,
    };
    let (_, found) = export(&document, &request(GraphicFormat::Describe))?;
    println!("[INFO] {found:?}");
    let (png, _) = export(&document, &request(GraphicFormat::Png))?;
    std::fs::write(out.join("graphic.png"), &png).map_err(|e| e.to_string())?;
    println!("[OK] graphic.png, {} bytes", png.len());
    if found.no_svg.is_none() {
        let (svg, _) = export(&document, &request(GraphicFormat::Svg))?;
        std::fs::write(out.join("graphic.svg"), &svg).map_err(|e| e.to_string())?;
        println!("[OK] graphic.svg, {} bytes", svg.len());
    }
    if found.original.is_some() {
        let (bytes, made) = export(&document, &request(GraphicFormat::Original))?;
        let name = format!("graphic-stored.{}", made.extension);
        std::fs::write(out.join(&name), &bytes).map_err(|e| e.to_string())?;
        println!("[OK] {name}, {} bytes", bytes.len());
    }
    Ok(())
}
