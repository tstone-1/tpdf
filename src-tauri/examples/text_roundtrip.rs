//! Spike 0.3: can one existing text object be edited and everything else
//! reproduced faithfully?
//!
//! This is the gating spike. If it fails on both routes, surgical redaction and
//! in-place text editing are both off the table and the stack question reopens.
//!
//! Two routes are tested against each other, because they fail differently:
//!
//! * **Route A, PDFium.** Mutate the page object, `FPDFPage_GenerateContent()`,
//!   save. Easy to write; regenerates the whole content stream, so the question
//!   is what that regeneration destroys.
//! * **Route B, surgical.** Decode the content stream with `lopdf`, rewrite the
//!   one text-showing operator, re-encode. Preserves everything it does not
//!   touch by construction; the question is whether the target operator can be
//!   *found* and its encoding reproduced.
//!
//! The measurement that decides it is **collateral damage**: pixels that
//! changed outside the edited object's own bounds. An edit that reproduces the
//! rest of the page faithfully changes nothing there. Comparing the two routes
//! on the same file, same edit, same renderer is the only way to attribute a
//! difference to the route rather than to the document.
//!
//! Route A is also run once *without* regenerating content, to confirm the
//! pdfium-1051 trap recorded in AGENTS.md still bites this build: without it the
//! save silently emits the original stream and the edit vanishes. For redaction
//! that is not a cosmetic bug, it is a data leak, so it is worth a standing
//! regression rather than a comment.
//!
//! Usage:
//!   text-roundtrip <file.pdf> [--page N] [--needle STR] [--replacement STR]
//!                  [--scale S] [--outdir DIR] [--dump]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lopdf::content::Content;
use lopdf::{Document as LoDocument, Object as LoObject};
use pdfium_render::prelude::*;

/// Cap on any single decompressed content stream, in bytes.
///
/// AGENTS.md requires every `lopdf` stream decode to be bounded: a content
/// stream is attacker-controlled and Flate is a decompression-bomb carrier.
/// 64 MiB is far above any legitimate page and far below a denial of service.
const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;

/// Pixel channel difference below which two renders count as identical.
///
/// Not zero: PDFium's rasteriser is not bit-deterministic across two loads of
/// the same file once the object order in the content stream changes, and an
/// antialiased edge lands a channel or two apart. A threshold of 8/255 ignores
/// that while still catching a glyph that moved by a fraction of a pixel.
const CHANNEL_TOLERANCE: i16 = 8;

struct Args {
    file: PathBuf,
    page: PdfPageIndex,
    needle: String,
    replacement: String,
    scale: f32,
    outdir: PathBuf,
    dump: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: text-roundtrip <file.pdf> [options]")?;

    let mut parsed = Args {
        file: PathBuf::from(file),
        page: 0,
        needle: "REDACT ME".to_string(),
        replacement: "REDACTED".to_string(),
        scale: 2.0,
        outdir: PathBuf::from("target/spike-0.3"),
        dump: false,
    };

    while let Some(flag) = args.next() {
        if flag == "--dump" {
            parsed.dump = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--page" => parsed.page = value.parse().map_err(|_| "bad --page")?,
            "--needle" => parsed.needle = value,
            "--replacement" => parsed.replacement = value,
            "--scale" => parsed.scale = value.parse().map_err(|_| "bad --scale")?,
            "--outdir" => parsed.outdir = PathBuf::from(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }

    Ok(parsed)
}

/// A rendered page, kept as RGBA so two of them can be differenced directly.
struct Render {
    rgba: Vec<u8>,
    width: i32,
    height: i32,
}

/// The target object's bounds in device pixels, y-down.
#[derive(Clone, Copy, Debug)]
struct DeviceBox {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl DeviceBox {
    /// True if the pixel is inside the box, which is padded before this runs.
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// One way of applying the edit: writes the result to the given path, and
/// returns a note for the report rather than a value the caller inspects.
type VariantFn = fn(&Pdfium, &Args, &Target, &Path) -> Result<String, String>;

/// What a variant did to the page, measured against the untouched baseline.
struct DiffReport {
    changed_total: u64,
    changed_inside: u64,
    changed_outside: u64,
    outside_bounds: Option<DeviceBox>,
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    if let Err(e) = std::fs::create_dir_all(&args.outdir) {
        eprintln!("[FAIL] could not create {}: {e}", args.outdir.display());
        return std::process::ExitCode::FAILURE;
    }

    let library_dir = pdfium_dir();
    let path = Pdfium::pdfium_platform_library_name_at_path(&library_dir);
    let bindings = match Pdfium::bind_to_library(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[FAIL] could not load Pdfium from {}: {e}", path.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    let pdfium = Pdfium::new(bindings);

    match run(&pdfium, &args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(pdfium: &Pdfium, args: &Args) -> Result<(), String> {
    println!("file:        {}", args.file.display());
    println!("page:        {}", args.page);
    println!("scale:       {}x", args.scale);
    println!("needle:      {:?}", args.needle);
    println!("replacement: {:?}", args.replacement);
    println!();

    let baseline = render(pdfium, &args.file, args.page, args.scale)?;
    println!(
        "baseline render: {}x{} device px",
        baseline.width, baseline.height
    );

    let inventory = inventory(pdfium, &args.file, args.page, &args.needle)?;
    println!(
        "page objects:    {} total, {} text",
        inventory.total_objects, inventory.text_objects
    );
    println!(
        "text-showing operators in the content stream: {}",
        inventory.show_operators
    );
    // The 1:1 correspondence is what a surgical edit needs in order to address
    // "the operator that produced this page object" at all. It is not
    // guaranteed by the format -- one Tj can be split across objects, and a
    // Form XObject contributes objects from a stream that is not the page's.
    if inventory.text_objects == inventory.show_operators {
        println!("  [OK] counts agree, so ordinal mapping is usable on this file");
    } else {
        println!("  [WARN] counts differ; ordinal mapping is NOT usable on this file");
    }

    let target = inventory
        .target
        .as_ref()
        .ok_or_else(|| format!("no text object contains {:?}", args.needle))?;
    println!();
    println!("target object:   index {} of the page", target.object_index);
    println!("  text:          {:?}", target.text);
    println!(
        "  bounds (pt):   {:.1} {:.1} .. {:.1} {:.1}",
        target.left_pt, target.bottom_pt, target.right_pt, target.top_pt
    );
    println!("  ordinal among text objects: {}", target.text_ordinal);
    println!(
        "  font:          {}",
        inventory.font_report.as_deref().unwrap_or("unknown")
    );

    // Padded by two device pixels: an antialiased glyph edge bleeds outside the
    // bounds PDFium reports, and counting that bleed as collateral damage would
    // make every route look worse than it is.
    let target_box = to_device_box(target, &baseline, args.scale, 2);
    println!(
        "  bounds (px):   {} {} .. {} {} (padded 2 px)",
        target_box.left, target_box.top, target_box.right, target_box.bottom
    );

    if args.dump {
        write_ppm(&args.outdir.join("baseline.ppm"), &baseline)?;
    }

    println!();
    let header = format!(
        "{:<26} {:>10} {:>10} {:>10}  {:<9} {}",
        "variant", "changed", "inside", "OUTSIDE", "reparse", "notes"
    );
    println!("{header}");
    println!("{}", "-".repeat(header.len()));

    let variants: Vec<(&str, VariantFn)> = vec![
        ("A pdfium set_text (no regen)", route_a_set_text_no_regen),
        ("A pdfium set_text", route_a_set_text),
        ("A pdfium remove", route_a_remove),
        ("B surgical set_text", route_b_set_text),
        ("B surgical remove", route_b_remove),
    ];

    for (name, apply) in variants {
        let out = args.outdir.join(format!("{}.pdf", slug(name)));
        let note = match apply(pdfium, args, target, &out) {
            Ok(note) => note,
            Err(e) => {
                println!(
                    "{name:<26} {:>10} {:>10} {:>10}  {:<9} {e}",
                    "-", "-", "-", "-"
                );
                continue;
            }
        };

        let after = match render(pdfium, &out, args.page, args.scale) {
            Ok(r) => r,
            Err(e) => {
                println!(
                    "{name:<26} {:>10} {:>10} {:>10}  {:<9} unreadable: {e}",
                    "-", "-", "-", "[FAIL]"
                );
                continue;
            }
        };

        let reparse = match LoDocument::load(&out) {
            Ok(_) => "[OK]",
            Err(_) => "[FAIL]",
        };

        let diff = diff(&baseline, &after, target_box)?;
        println!(
            "{name:<26} {:>10} {:>10} {:>10}  {:<9} {note}",
            diff.changed_total, diff.changed_inside, diff.changed_outside, reparse
        );
        if let Some(bounds) = diff.outside_bounds {
            println!(
                "{:<26} {:>10} damage box {} {} .. {} {}",
                "", "", bounds.left, bounds.top, bounds.right, bounds.bottom
            );
        }

        // The pixel diff says the page *looks* right. For redaction that is not
        // the question -- black pixels over recoverable bytes is the failure
        // mode the whole subsystem exists to avoid. Extracted text is a second,
        // independent witness, and the two can disagree.
        match extracted_text(pdfium, &out, args.page) {
            Ok(text) => {
                let survives = text.contains(&args.needle);
                let applied = text.contains(&args.replacement);
                println!(
                    "{:<26} {:>10} text: needle {}, replacement {}",
                    "",
                    "",
                    if survives {
                        "STILL PRESENT"
                    } else {
                        "gone [OK]"
                    },
                    if applied { "present [OK]" } else { "absent" },
                );
            }
            Err(e) => println!("{:<26} {:>10} text: unreadable: {e}", "", ""),
        }

        // And a third: hunt the secret through the whole file, not just the
        // page. A viewer showing nothing proves nothing while a carrier
        // elsewhere still holds the text.
        match leak_scan(&out, args.needle.as_bytes()) {
            Ok(report) => {
                if !report.carriers.is_empty() {
                    println!(
                        "{:<26} {:>10} leak scan: SECRET SURVIVES in {}",
                        "",
                        "",
                        report.carriers.join(", ")
                    );
                }
                if !report.blind_spots.is_empty() {
                    println!(
                        "{:<26} {:>10} leak scan: NOT VERIFIED -- cannot decode {}",
                        "",
                        "",
                        report.blind_spots.join(", ")
                    );
                } else if report.carriers.is_empty() {
                    println!(
                        "{:<26} {:>10} leak scan: no carrier holds the needle [OK]",
                        "", ""
                    );
                }
            }
            Err(e) => println!("{:<26} {:>10} leak scan: NOT VERIFIED: {e}", "", ""),
        }

        if args.dump {
            write_ppm(&args.outdir.join(format!("{}.ppm", slug(name))), &after)?;
        }
    }

    println!();
    println!("`changed` counts device pixels differing from the baseline by more");
    println!("than {CHANNEL_TOLERANCE}/255 on any channel. OUTSIDE is the number that decides");
    println!("the spike: it is content the edit was never asked to touch.");
    Ok(())
}

/// A text object located on the page, described in the terms both routes need.
struct Target {
    /// Index into the page's object list, which is Route A's address.
    object_index: usize,
    /// Position among text objects only, which is Route B's address.
    text_ordinal: usize,
    text: String,
    left_pt: f32,
    bottom_pt: f32,
    right_pt: f32,
    top_pt: f32,
}

struct Inventory {
    total_objects: usize,
    text_objects: usize,
    show_operators: usize,
    target: Option<Target>,
    /// How the target's font resolved, which decides what the run proves.
    font_report: Option<String>,
}

/// Walks the page once, counting objects and locating the edit target.
fn inventory(
    pdfium: &Pdfium,
    file: &Path,
    page_index: PdfPageIndex,
    needle: &str,
) -> Result<Inventory, String> {
    let doc = pdfium
        .load_pdf_from_file(file, None)
        .map_err(|e| format!("could not open {}: {e}", file.display()))?;
    let page = doc
        .pages()
        .get(page_index)
        .map_err(|e| format!("no such page {page_index}: {e}"))?;

    let mut total_objects = 0usize;
    let mut text_objects = 0usize;
    let mut target = None;
    let mut font_report = None;

    for (index, object) in page.objects().iter().enumerate() {
        total_objects += 1;
        let Some(text_object) = object.as_text_object() else {
            continue;
        };
        let text = text_object.text();
        let ordinal = text_objects;
        text_objects += 1;

        if target.is_none() && text.contains(needle) {
            let bounds = object
                .bounds()
                .map_err(|e| format!("no bounds for object {index}: {e}"))?;
            let font = text_object.font();
            // Whether the *embedded* programme is in use decides what this
            // spike is actually testing. If Pdfium substituted a system font,
            // the subsetting constraint never comes into play and a clean
            // result would mean nothing.
            font_report = Some(format!(
                "{} (embedded: {}, built-in: {})",
                font.name(),
                font.is_embedded()
                    .map(|e| if e { "yes" } else { "NO" }.to_string())
                    .unwrap_or_else(|e| format!("unknown: {e}")),
                font.is_built_in(),
            ));
            target = Some(Target {
                object_index: index,
                text_ordinal: ordinal,
                text,
                left_pt: bounds.left().value,
                bottom_pt: bounds.bottom().value,
                right_pt: bounds.right().value,
                top_pt: bounds.top().value,
            });
        }
    }

    let show_operators = count_show_operators(file, page_index)?;

    Ok(Inventory {
        total_objects,
        text_objects,
        show_operators,
        target,
        font_report,
    })
}

/// Counts text-showing operators in the page's own content stream.
///
/// Deliberately does not descend into Form XObjects: the point of the count is
/// to test whether PDFium's page-object list can be addressed by ordinal in
/// *this* stream, and an XObject's operators live in a different one.
fn count_show_operators(file: &Path, page_index: PdfPageIndex) -> Result<usize, String> {
    let (_, content) = load_page_content(file, page_index)?;
    Ok(content
        .operations
        .iter()
        .filter(|op| is_show_operator(&op.operator))
        .count())
}

fn is_show_operator(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

/// Loads a page's decoded content stream, bounded, plus its object id.
fn load_page_content(
    file: &Path,
    page_index: PdfPageIndex,
) -> Result<(lopdf::ObjectId, Content), String> {
    let doc = LoDocument::load(file).map_err(|e| format!("lopdf could not load: {e}"))?;
    let pages = doc.get_pages();
    let page_id = *pages
        .get(&(page_index as u32 + 1))
        .ok_or_else(|| format!("lopdf: no page {page_index}"))?;
    let data = doc
        .get_page_content_with_limit(page_id, MAX_CONTENT_BYTES)
        .map_err(|e| format!("lopdf could not read content: {e}"))?;
    let content = Content::decode(&data).map_err(|e| format!("lopdf could not decode: {e}"))?;
    Ok((page_id, content))
}

/// Extracts all text from a page, as a reader's search or copy would see it.
fn extracted_text(
    pdfium: &Pdfium,
    file: &Path,
    page_index: PdfPageIndex,
) -> Result<String, String> {
    let doc = pdfium
        .load_pdf_from_file(file, None)
        .map_err(|e| format!("could not open: {e}"))?;
    let page = doc
        .pages()
        .get(page_index)
        .map_err(|e| format!("no such page: {e}"))?;
    let text = page.text().map_err(|e| format!("no text page: {e}"))?;
    Ok(text.all())
}

/// True if `needle` appears anywhere in `haystack`.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Hunts a secret through every carrier in the file, naming the ones that hold it.
///
/// The prototype of the Phase 3 verifier, and it exists because "the page no
/// longer draws it" is not the same claim as "the file no longer contains it".
/// A page's glyphs are one carrier among many: `/ActualText` on a marked-content
/// span, an annotation's `/Contents`, document `/Info`, an orphaned object left
/// behind by a rewrite, or bytes past `%%EOF` all outlive an edit to page
/// content.
///
/// Errors rather than returning "clean" when a stream cannot be decoded. A
/// verifier that cannot read a carrier has not verified it, and AGENTS.md is
/// explicit that the result is then "not verified", never "clean".
fn leak_scan(file: &Path, needle: &[u8]) -> Result<LeakReport, String> {
    let raw = std::fs::read(file).map_err(|e| format!("could not read: {e}"))?;
    let doc = LoDocument::load(file).map_err(|e| format!("lopdf could not load: {e}"))?;

    let mut carriers = Vec::new();
    let mut blind_spots = Vec::new();

    // A byte scan can only find text that is *stored* as those bytes. Under a
    // Type0 font with Identity encoding the content stream holds glyph ids, so
    // the secret is present and unfindable by this method -- verified here:
    // the CID fixture reported "clean" while extraction still returned the
    // needle. That is precisely the over-claim AGENTS.md forbids, so it is
    // reported as a blind spot rather than folded into a pass.
    for (id, object) in doc.objects.iter() {
        let LoObject::Dictionary(dictionary) = object else {
            continue;
        };
        if matches!(
            dictionary.get(b"Subtype").and_then(|o| o.as_name()),
            Ok(b"Type0")
        ) {
            blind_spots.push(format!("object {} is a Type0 font", id.0));
        }
    }

    // Uncompressed anywhere in the file. Catches trailing bytes and prior
    // revisions that the object graph no longer reaches.
    if find_bytes(&raw, needle) {
        carriers.push("raw file bytes".to_string());
    }

    for (id, object) in doc.objects.iter() {
        match object {
            LoObject::Stream(stream) => {
                // A stream that will not decode is the one case that must not
                // be scanned and passed. Falling back to the *compressed*
                // bytes would find nothing and report clean, which is the
                // over-claim this whole subsystem exists to avoid.
                let decoded = match stream.decompressed_content() {
                    Ok(bytes) => bytes,
                    Err(e) => return Err(format!("stream {} could not be decoded: {e}", id.0)),
                };
                if decoded.len() > MAX_CONTENT_BYTES {
                    return Err(format!("stream {} exceeds the decode bound", id.0));
                }
                if find_bytes(&decoded, needle) {
                    carriers.push(format!("stream {}", id.0));
                }
                // The stream's *dictionary* is a carrier in its own right.
                if find_bytes(
                    &flatten_strings(&LoObject::Dictionary(stream.dict.clone())),
                    needle,
                ) {
                    carriers.push(format!("stream {} dictionary", id.0));
                }
            }
            other => {
                if find_bytes(&flatten_strings(other), needle) {
                    carriers.push(format!("object {}", id.0));
                }
            }
        }
    }

    if let Ok(LoObject::Dictionary(info)) = doc
        .trailer
        .get(b"Info")
        .and_then(|r| doc.dereference(r).map(|(_, o)| o))
    {
        if find_bytes(
            &flatten_strings(&LoObject::Dictionary(info.clone())),
            needle,
        ) {
            carriers.push("/Info metadata".to_string());
        }
    }

    carriers.sort();
    carriers.dedup();
    blind_spots.sort();
    blind_spots.dedup();
    Ok(LeakReport {
        carriers,
        blind_spots,
    })
}

/// The outcome of a leak scan, which has three states rather than two.
///
/// "Clean" and "leaking" are not exhaustive: a scan can also be *unable to
/// look*, and collapsing that into "clean" is the failure mode redaction
/// verification exists to prevent.
struct LeakReport {
    carriers: Vec<String>,
    blind_spots: Vec<String>,
}

/// Concatenates every string buried anywhere in an object, however nested.
fn flatten_strings(object: &LoObject) -> Vec<u8> {
    let mut out = Vec::new();
    collect_strings(object, &mut out);
    out
}

fn collect_strings(object: &LoObject, out: &mut Vec<u8>) {
    match object {
        LoObject::String(bytes, _) => {
            out.extend_from_slice(bytes);
            // A separator, so two adjacent strings cannot spell out a needle
            // that neither of them contains.
            out.push(0);
        }
        LoObject::Array(items) => items.iter().for_each(|i| collect_strings(i, out)),
        LoObject::Dictionary(dictionary) => {
            dictionary.iter().for_each(|(_, v)| collect_strings(v, out))
        }
        LoObject::Stream(stream) => {
            collect_strings(&LoObject::Dictionary(stream.dict.clone()), out)
        }
        _ => {}
    }
}

/// Renders a page to RGBA at the given scale.
fn render(
    pdfium: &Pdfium,
    file: &Path,
    page_index: PdfPageIndex,
    scale: f32,
) -> Result<Render, String> {
    let doc = pdfium
        .load_pdf_from_file(file, None)
        .map_err(|e| format!("could not open {}: {e}", file.display()))?;
    let page = doc
        .pages()
        .get(page_index)
        .map_err(|e| format!("no such page {page_index}: {e}"))?;

    let width = (page.width().value * scale).round() as i32;
    let height = (page.height().value * scale).round() as i32;

    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .set_target_height(height);
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

/// Converts PDF-point bounds (y-up) to device-pixel bounds (y-down).
fn to_device_box(target: &Target, render: &Render, scale: f32, pad: i32) -> DeviceBox {
    let left = (target.left_pt * scale).floor() as i32 - pad;
    let right = (target.right_pt * scale).ceil() as i32 + pad;
    // PDF's origin is bottom-left, the bitmap's is top-left, so the top of the
    // box is derived from the *top* coordinate measured down from the page top.
    let top = render.height - (target.top_pt * scale).ceil() as i32 - pad;
    let bottom = render.height - (target.bottom_pt * scale).floor() as i32 + pad;
    DeviceBox {
        left: left.max(0),
        top: top.max(0),
        right: right.min(render.width),
        bottom: bottom.min(render.height),
    }
}

fn diff(before: &Render, after: &Render, target: DeviceBox) -> Result<DiffReport, String> {
    if before.width != after.width || before.height != after.height {
        return Err(format!(
            "size changed: {}x{} -> {}x{}",
            before.width, before.height, after.width, after.height
        ));
    }

    let mut report = DiffReport {
        changed_total: 0,
        changed_inside: 0,
        changed_outside: 0,
        outside_bounds: None,
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);

    for y in 0..before.height {
        for x in 0..before.width {
            let offset = ((y * before.width + x) * 4) as usize;
            let a = &before.rgba[offset..offset + 4];
            let b = &after.rgba[offset..offset + 4];
            let differs = (0..4).any(|c| (a[c] as i16 - b[c] as i16).abs() > CHANNEL_TOLERANCE);
            if !differs {
                continue;
            }
            report.changed_total += 1;
            if target.contains(x, y) {
                report.changed_inside += 1;
            } else {
                report.changed_outside += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }

    if report.changed_outside > 0 {
        report.outside_bounds = Some(DeviceBox {
            left: min_x,
            top: min_y,
            right: max_x,
            bottom: max_y,
        });
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// Route A: PDFium page-object mutation.
// ---------------------------------------------------------------------------

/// Saves after `set_text` but *without* regenerating content.
///
/// Expected to lose the edit entirely -- see pdfium issue 1051 and AGENTS.md.
/// A run where this variant reports a change is a signal that the upstream bug
/// was fixed and the mandatory-regeneration rule can be revisited.
fn route_a_set_text_no_regen(
    pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
) -> Result<String, String> {
    apply_pdfium(pdfium, args, target, out, false, |object, replacement| {
        let text_object = object
            .as_text_object_mut()
            .ok_or("target is not a text object")?;
        text_object
            .set_text(replacement)
            .map_err(|e| format!("set_text failed: {e}"))
    })
    .map(|()| "expected to lose the edit".to_string())
}

fn route_a_set_text(
    pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
) -> Result<String, String> {
    apply_pdfium(pdfium, args, target, out, true, |object, replacement| {
        let text_object = object
            .as_text_object_mut()
            .ok_or("target is not a text object")?;
        text_object
            .set_text(replacement)
            .map_err(|e| format!("set_text failed: {e}"))
    })
    .map(|()| String::new())
}

fn route_a_remove(
    pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
) -> Result<String, String> {
    let doc = pdfium
        .load_pdf_from_file(&args.file, None)
        .map_err(|e| format!("could not open: {e}"))?;
    let mut page = doc
        .pages()
        .get(args.page)
        .map_err(|e| format!("no such page: {e}"))?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

    // By index rather than by handle: taking the object out first borrows the
    // page immutably for as long as the object lives, which the removal itself
    // needs mutably.
    let removed = page
        .objects_mut()
        .remove_object_at_index(target.object_index as PdfPageObjectIndex)
        .map_err(|e| format!("remove_object failed: {e}"))?;
    // Deliberately not dropped. `fpdf_edit.h` says ownership transfers to the
    // caller and `FPDFPageObj_Destroy()` frees it, and pdfium-render's `Drop`
    // does exactly that -- and it segfaults, for text and path objects alike,
    // whether the destroy happens immediately or after regeneration and save.
    // See examples/remove_probe.rs and AGENTS.md. Leaking the handle is the only
    // safe option through this binding; the memory is reclaimed when the
    // document closes.
    std::mem::forget(removed);
    page.regenerate_content()
        .map_err(|e| format!("regenerate_content failed: {e}"))?;
    drop(page);

    doc.save_to_file(out)
        .map_err(|e| format!("save failed: {e}"))?;
    Ok(String::new())
}

/// Shared body of the Route A variants: open, mutate, optionally regenerate, save.
fn apply_pdfium(
    pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
    regenerate: bool,
    mutate: impl FnOnce(&mut PdfPageObject, &str) -> Result<(), String>,
) -> Result<(), String> {
    let doc = pdfium
        .load_pdf_from_file(&args.file, None)
        .map_err(|e| format!("could not open: {e}"))?;
    let mut page = doc
        .pages()
        .get(args.page)
        .map_err(|e| format!("no such page: {e}"))?;
    // Manual, so that "did regeneration happen" is a property of this variant
    // rather than of pdfium-render's default policy.
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

    {
        let objects = page.objects_mut();
        let mut object = objects
            .get(target.object_index as PdfPageObjectIndex)
            .map_err(|e| format!("no object {}: {e}", target.object_index))?;
        mutate(&mut object, &args.replacement)?;
    }

    if regenerate {
        page.regenerate_content()
            .map_err(|e| format!("regenerate_content failed: {e}"))?;
    }
    drop(page);

    doc.save_to_file(out)
        .map_err(|e| format!("save failed: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route B: surgical content-stream rewriting.
// ---------------------------------------------------------------------------

fn route_b_set_text(
    _pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
) -> Result<String, String> {
    surgical(args, target, out, Some(&args.replacement))
}

fn route_b_remove(
    _pdfium: &Pdfium,
    args: &Args,
    target: &Target,
    out: &Path,
) -> Result<String, String> {
    surgical(args, target, out, None)
}

/// Rewrites exactly one text-showing operator, leaving every other byte of the
/// content stream as it was.
///
/// `replacement` of `None` removes the operator entirely, which is redaction's
/// primitive: the glyphs are not covered up, the instruction that drew them is
/// gone.
fn surgical(
    args: &Args,
    target: &Target,
    out: &Path,
    replacement: Option<&str>,
) -> Result<String, String> {
    let mut doc = LoDocument::load(&args.file).map_err(|e| format!("lopdf could not load: {e}"))?;
    let pages = doc.get_pages();
    let page_id = *pages
        .get(&(args.page as u32 + 1))
        .ok_or_else(|| format!("lopdf: no page {}", args.page))?;
    let data = doc
        .get_page_content_with_limit(page_id, MAX_CONTENT_BYTES)
        .map_err(|e| format!("lopdf could not read content: {e}"))?;
    let mut content = Content::decode(&data).map_err(|e| format!("lopdf could not decode: {e}"))?;

    // Find the target by ordinal among show operators. The count check printed
    // earlier is what licenses this; on a file where the counts disagree the
    // ordinal addresses the wrong operator and the diff will say so loudly.
    let position = content
        .operations
        .iter()
        .enumerate()
        .filter(|(_, op)| is_show_operator(&op.operator))
        .map(|(index, _)| index)
        .nth(target.text_ordinal)
        .ok_or("no show operator at that ordinal")?;

    let note = match replacement {
        None => {
            content.operations.remove(position);
            String::new()
        }
        Some(text) => {
            let operation = &mut content.operations[position];
            let original = show_operand(operation)
                .ok_or("target operator carries no string operand")?
                .to_vec();
            let encoded = reencode(&original, &target.text, text)?;
            set_show_operand(operation, encoded);
            String::new()
        }
    };

    let encoded = content
        .encode()
        .map_err(|e| format!("lopdf could not encode: {e}"))?;
    doc.change_page_content(page_id, encoded)
        .map_err(|e| format!("lopdf could not replace content: {e}"))?;
    doc.save(out)
        .map_err(|e| format!("lopdf could not save: {e}"))?;
    Ok(note)
}

/// The string operand of a show operator, for the single-string forms.
///
/// `TJ` carries an array of alternating strings and kerning numbers; this spike
/// handles the single-string case and reports the array case rather than
/// silently mangling it.
fn show_operand(operation: &lopdf::content::Operation) -> Option<&[u8]> {
    match operation.operands.first() {
        Some(LoObject::String(bytes, _)) => Some(bytes),
        _ => None,
    }
}

fn set_show_operand(operation: &mut lopdf::content::Operation, bytes: Vec<u8>) {
    if let Some(LoObject::String(existing, _)) = operation.operands.first_mut() {
        *existing = bytes;
    }
}

/// Re-encodes `replacement` into the same code units the original operand used.
///
/// This is the part that decides whether surgical editing is possible at all.
/// The content stream carries *codes*, not characters: one byte per character
/// for a simple font, two bytes of glyph id for Identity-H. Nothing in the
/// stream says which. What is available is the original operand alongside the
/// text PDFium extracted from it, and aligning those two gives a code table --
/// but only for characters the object already draws.
///
/// A character outside that table cannot be encoded here, which is not a defect
/// of this function: it is the subsetted-font constraint from AGENTS.md showing
/// up at the first place it can. Reporting it is correct; guessing a code would
/// draw the wrong glyph.
fn reencode(original: &[u8], original_text: &str, replacement: &str) -> Result<Vec<u8>, String> {
    let characters: Vec<char> = original_text.chars().collect();
    if characters.is_empty() {
        return Err("original text is empty".to_string());
    }
    if original.len() % characters.len() != 0 {
        return Err(format!(
            "operand of {} bytes does not divide into {} characters",
            original.len(),
            characters.len()
        ));
    }
    let width = original.len() / characters.len();
    if width != 1 && width != 2 {
        return Err(format!("unsupported code width of {width} bytes"));
    }

    let mut table: HashMap<char, &[u8]> = HashMap::new();
    for (index, character) in characters.iter().enumerate() {
        table
            .entry(*character)
            .or_insert(&original[index * width..(index + 1) * width]);
    }

    let mut encoded = Vec::with_capacity(replacement.chars().count() * width);
    for character in replacement.chars() {
        let code = table.get(&character).ok_or_else(|| {
            format!("{character:?} is not drawn by this object, so its code is unknown")
        })?;
        encoded.extend_from_slice(code);
    }
    Ok(encoded)
}

// ---------------------------------------------------------------------------

/// Writes a binary PPM, so a render can be eyeballed without an image crate.
fn write_ppm(path: &Path, render: &Render) -> Result<(), String> {
    let mut out = format!("P6\n{} {}\n255\n", render.width, render.height).into_bytes();
    for pixel in render.rgba.chunks_exact(4) {
        out.extend_from_slice(&pixel[0..3]);
    }
    std::fs::write(path, out).map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
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
