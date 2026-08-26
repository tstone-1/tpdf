//! The whole redaction, end to end, on a real file.
//!
//! `redact_probe` proves the primitive: given ordinals, the show operators go.
//! This proves the **path** --- a rectangle a reader dragged becomes a plan
//! against PDFium's own object list, becomes ordinals in a save plan, becomes a
//! written file, and the words are not in it. Every step between the drag and
//! the file is here except the dialog and the Tauri command's own glue.
//!
//! ## Two readers, and the control is the point
//!
//! The needle must be **gone** and a word on another line must **survive**, and
//! both are asserted through two independent readers: `verify::scan` over the
//! bytes, and PDFium re-extracting the written file. A scan that finds nothing
//! because it cannot look is the failure this repository has recorded from
//! several directions, and a survivor it can still find is what says it can.
//!
//! Route B removes the whole operation, so the survivor is on a **different
//! line**: a word beside the target goes with it, and asserting otherwise would
//! fail for a redaction that is working correctly.
//!
//! ## Usage
//!
//!     cargo run --release --example redact-apply-probe [-- --library DIR]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpdf_lib::document::OpenDocument;
use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::{progressive, redact, render, save, text, verify};

/// The fixture, what to remove, and what has to survive.
const FILE: &str = "text-base14.pdf";
const REMOVE: &str = "4711-0815";
/// On another line, so route B's collateral does not take it.
const KEEP: &str = "Sphinx of black quartz";

fn main() -> ExitCode {
    let library = std::env::args()
        .skip_while(|a| a != "--library")
        .nth(1)
        .map_or_else(default_library, PathBuf::from);
    if std::env::args().any(|a| a == "--survey") {
        return match survey(&library) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("[FAIL] {e}");
                ExitCode::from(2)
            }
        };
    }
    match run(&library) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            ExitCode::from(2)
        }
    }
}

fn run(library: &Path) -> Result<bool, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repo root")?
        .to_path_buf();
    let source = root.join("testdata").join(FILE);
    if !source.exists() {
        println!(
            "[SKIP] {} is not built; run scripts/make_fixtures.sh",
            source.display()
        );
        return Ok(true);
    }
    let bindings = bind(library)?;
    let document = OpenDocument::open(bindings, &source, None).map_err(|why| why.reason)?;

    // The rectangle a reader would have dragged. Built from the character boxes
    // rather than typed in, so the probe is about the redaction and not about
    // whether somebody transcribed a coordinate correctly.
    let region = {
        let page = document.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        box_of(&extracted, REMOVE).ok_or_else(|| format!("{REMOVE} is not on page 1 of {FILE}"))?
    };
    println!(
        "[..] region {:?} around {REMOVE:?} in the file's display space",
        region.map(|v| (v * 10.0).round() / 10.0)
    );

    let plans = render::redaction_plans_of(&document, 0, &[region])?;
    let plan = plans.first().ok_or("no plan came back for one region")?;
    let mut ok = true;
    ok &= check(
        "the plan names at least one show operator",
        !plan.shows.is_empty(),
    );
    // **Asserted as present, not as absent**, and that is what this fixture is
    // for: `make_text_pdf.py` draws four unrelated non-text objects and one of
    // them is a path across the region. So the plan is deliberately
    // *incomplete*, which is the ordinary case rather than the exotic one --- a
    // rule under a line of text is what almost every real document has. The
    // removal still happens and the file still cannot be called clean, which is
    // the whole shape of `redact::Applied`.
    ok &= check(
        "the region covers a path, so the plan reports it and is not complete",
        plan.unhandled.iter().any(|object| object.kind == "path"),
    );
    for object in &plan.unhandled {
        println!("    unhandled: {}", object.sentence());
    }
    ok &= check(
        &format!("what it would take contains {REMOVE:?}"),
        plan.taking.contains(REMOVE),
    );
    println!("    it would take {:?}", plan.taking.trim());

    let out = std::env::temp_dir().join("tpdf-redact-apply-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let count = document.page_count();
    save::write_copy(&source, &plan_for(count, plan), &out).map_err(|why| why.message)?;
    let bytes = std::fs::read(&out).map_err(|why| why.to_string())?;
    println!("[..] wrote {} bytes to {}", bytes.len(), out.display());

    // Reader one: the bytes. The control is `KEEP` --- a scan that reports the
    // needle absent and the survivor absent has told you nothing at all.
    let report = verify::scan(&bytes, &[REMOVE.to_string(), KEEP.to_string()], None);
    ok &= check(
        &format!("the byte scan no longer finds {REMOVE:?}"),
        !report.found.contains(REMOVE),
    );
    ok &= check(
        &format!("and still finds {KEEP:?}, so it can see this file at all"),
        report.found.contains(KEEP),
    );

    // Reader two: PDFium, which shares no code with the scan and reads the
    // written file rather than the buffer that produced it.
    let after = OpenDocument::open(bindings, &out, None).map_err(|why| why.reason)?;
    let read_back = {
        let page = after.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        as_string(&extracted)
    };
    ok &= check(
        &format!("PDFium no longer extracts {REMOVE:?}"),
        !read_back.contains(REMOVE),
    );
    ok &= check(
        &format!("and still extracts {KEEP:?}"),
        read_back.contains(KEEP),
    );

    println!(
        "{}",
        if ok {
            "[OK] the region was removed, and both readers agree"
        } else {
            "[FAIL] see above"
        }
    );
    Ok(ok)
}

/// How often the correspondence guard would refuse, across the whole corpus.
///
/// **The one number that decides whether this feature works on real files.**
/// `redact::remove_shows` refuses when the show operators `lopdf` decodes
/// disagree with the text objects PDFium counted, because nothing connects the
/// two lists but order and a mis-addressed removal deletes the wrong words while
/// reporting success. Spike 0.3 measured 4:4 on four fixtures built for it and
/// said plainly that a `TJ` split across objects or a Form XObject contributing
/// from another stream breaks it.
///
/// What that spike could not say is how often it breaks. This walks every page
/// of every fixture in `testdata/` and counts, so *"the guard may refuse on real
/// documents"* becomes a fraction instead of a worry. It asserts nothing --- a
/// page that disagrees is a fact about the corpus, not a defect --- and prints
/// the pages that do, because those are the ones worth reading.
fn survey(library: &Path) -> Result<bool, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repo root")?
        .to_path_buf();
    let bindings = bind(library)?;
    let mut agree = 0usize;
    let mut differ = 0usize;
    let mut unreadable = 0usize;
    let mut files = 0usize;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("testdata"))
        .map_err(|why| why.to_string())?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();

    for path in paths {
        let Ok(document) = OpenDocument::open(bindings, &path, None) else {
            // An encrypted or malformed fixture. Counted rather than skipped
            // silently: a survey that quietly drops what it cannot open reports
            // a cleaner corpus than it measured.
            unreadable += 1;
            continue;
        };
        let Ok(mut lopdf) = lopdf::Document::load(&path) else {
            unreadable += 1;
            continue;
        };
        files += 1;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let pages: Vec<lopdf::ObjectId> = lopdf.get_pages().values().copied().collect();
        for (at, page_id) in pages.iter().enumerate() {
            let Ok(page) = document.page(at as u32) else {
                continue;
            };
            let Ok(objects) = tpdf_lib::objects::read(&page) else {
                continue;
            };
            let shows = show_operators(&mut lopdf, *page_id);
            if shows == objects.text.len() {
                agree += 1;
            } else {
                differ += 1;
                println!(
                    "    {name} page {}: {} show operator(s), {} text object(s)",
                    at + 1,
                    shows,
                    objects.text.len()
                );
            }
        }
    }

    let total = agree + differ;
    println!("[..] {files} readable file(s), {unreadable} not opened; {total} page(s) compared");
    println!(
        "[..] {agree} agree, {differ} differ --- a removal is refused on {:.1}% of pages",
        if total == 0 {
            0.0
        } else {
            100.0 * differ as f64 / total as f64
        }
    );
    Ok(true)
}

/// How many text-showing operators one page's content stream holds.
///
/// The same count `redact::remove_shows` derives, and derived here through the
/// same decode rather than through a second rule: what is being measured is
/// whether the two *populations* agree, so a private notion of what counts as a
/// show operator would be measuring this file's opinion instead.
fn show_operators(doc: &mut lopdf::Document, page: lopdf::ObjectId) -> usize {
    let Ok(data) = doc.get_page_content_with_limit(page, redact::MAX_CONTENT_BYTES) else {
        return usize::MAX;
    };
    let Ok(content) = lopdf::content::Content::decode(&data) else {
        return usize::MAX;
    };
    content
        .operations
        .iter()
        .filter(|operation| matches!(operation.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
        .count()
}

/// The save plan the redact command builds: the file as it is, plus the removal.
///
/// Every page kept, unturned and uncropped, no marks --- which is a document a
/// reader has not edited at all. That is deliberate: it is the plan for which
/// `Plan::is_identity` would otherwise answer *true* and hand the file over
/// unchanged, so this probe is also the check that a redaction stops it.
fn plan_for(pages: u32, region: &redact::RegionPlan) -> Plan {
    Plan {
        baseline: pages,
        opened_as: None,
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at),
                source: at,
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: region.shows.clone(),
            text_objects: region.text_objects,
        }],
    }
}

/// The characters of a page as one string, in the file's own order.
///
/// Deliberately not the *reading* order: what this probe asks is whether a word
/// is still in the page at all, and reordering it could only make the answer
/// harder to read.
fn as_string(page: &text::PageText) -> String {
    page.codes
        .iter()
        .filter_map(|code| char::from_u32(*code))
        .collect()
}

/// The box around a needle's characters, in the page's display space.
fn box_of(page: &text::PageText, needle: &str) -> Option<[f32; 4]> {
    let text = as_string(page);
    let at = text.find(needle)?;
    // Character indices, not byte offsets: the fixture is ASCII, and counting
    // characters is what stays right when it is not.
    let from = text[..at].chars().count();
    let to = from + needle.chars().count();
    let mut found: Option<[f32; 4]> = None;
    for index in from..to {
        let base = index * 4;
        let quad = [
            *page.boxes.get(base)?,
            *page.boxes.get(base + 1)?,
            *page.boxes.get(base + 2)?,
            *page.boxes.get(base + 3)?,
        ];
        found = Some(match found {
            None => quad,
            Some(so_far) => [
                so_far[0].min(quad[0]),
                so_far[1].min(quad[1]),
                so_far[2].max(quad[2]),
                so_far[3].max(quad[3]),
            ],
        });
    }
    found
}

fn check(what: &str, held: bool) -> bool {
    println!("{} {what}", if held { "[OK]  " } else { "[FAIL]" });
    held
}

fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

fn default_library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join("vendor")
        .join("pdfium")
        .join(if cfg!(windows) { "bin" } else { "lib" })
}
