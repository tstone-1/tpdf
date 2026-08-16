//! Does the structure tree give a reading order, and is it the *document's* one?
//!
//! `structure.rs` maps a tagged element's marked-content ids to ranges of the
//! same character indices everything else uses. That mapping is a sequence of
//! PDFium calls, and a unit test over a fake would only prove the fake --- so it
//! is asserted here, against `testdata/tagged.pdf` and the manifest a *different
//! program* wrote beside it.
//!
//! ## The assertion that carries the weight
//!
//! Not "an order came back". An order always comes back, and a geometric reader
//! produces a perfectly plausible one. The fixture exists so that the tagged
//! answer and the geometric answer **differ**, and the manifest states both: page
//! 1 puts a margin note last where geometry puts it third. So this checks that
//! the order matches the tagged one *and* that it does not match the geometric
//! one, which is the half that can fail when the tags are ignored.
//!
//! Page 2 is the control and matters as much: the same layout tagged in the order
//! geometry would infer anyway, which a tagged reader must leave alone. Without
//! it, "the tags are read" and "the tags are read and everything is scrambled"
//! look the same.
//!
//! An untagged fixture is the other control: it must report **no** runs, so that
//! a caller can tell "fall back to geometry" from "this document says its reading
//! order is nothing".
//!
//! ```text
//! cargo run --release --example structure-probe -- \
//!     --file ../testdata/tagged.pdf --untagged ../testdata/text-base14.pdf
//! ```

use std::path::{Path, PathBuf};

use tpdf_lib::progressive::{self, RawDocument};
use tpdf_lib::structure::{self, PageStructure};
use tpdf_lib::text;

/// The check-name roll and the prefix rule, shared with `search_probe`.
///
/// `#[path]` rather than a crate module: example scaffolding, which must not
/// ship in the binary.
#[path = "../src/probes/checkroll.rs"]
mod checkroll;

struct Args {
    library: PathBuf,
    file: PathBuf,
    manifest: PathBuf,
    untagged: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut library = PathBuf::from("../vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut file = PathBuf::from("../testdata/tagged.pdf");
    let mut manifest: Option<PathBuf> = None;
    let mut untagged = Some(PathBuf::from("../testdata/text-base14.pdf"));

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--library" => library = PathBuf::from(value()?),
            "--file" => file = PathBuf::from(value()?),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            "--untagged" => untagged = Some(PathBuf::from(value()?)),
            "--no-untagged" => untagged = None,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    // Beside the PDF, which is where the generator puts it. Derived rather than
    // required so the usual invocation is one flag.
    let manifest = manifest.unwrap_or_else(|| {
        let mut path = file.clone();
        path.set_extension("");
        PathBuf::from(format!("{}-manifest.json", path.display()))
    });
    Ok(Args {
        library,
        file,
        manifest,
        untagged,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    }
}

/// Records a check's outcome, in the shape every other probe here uses.
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
    /// Every name printed, so the set can be read without parsing the padded
    /// column. See `checkroll` for why that column cannot be parsed back.
    names: Vec<String>,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: &str) {
        self.names.push(name.to_string());
        if ok {
            self.passed += 1;
            println!("[OK]   {name:<52} {detail}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name:<52} {detail}");
        }
    }

    /// A check this fixture cannot exercise. Printed, never omitted.
    fn skip(&mut self, name: &str, why: &str) {
        self.names.push(name.to_string());
        self.skipped += 1;
        println!("[SKIP] {name:<52} not applicable -- {why}");
    }
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

/// The text a run covers, taken from the page rather than from the run.
///
/// This is the point of resolving through the page: a run reports indices, and
/// asking the page what is at those indices is what ties the reported order to
/// actual content. Comparing the runs' own metadata against the manifest would
/// compare two descriptions and never touch a character.
fn words_of(codes: &[u32], start: u32, end: u32) -> String {
    let text: String = codes[start as usize..end as usize]
        .iter()
        .filter_map(|c| char::from_u32(*c))
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn run(args: &Args) -> Result<bool, String> {
    let bindings = bind(&args.library)?;
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&args.manifest)
            .map_err(|e| format!("could not read {}: {e}", args.manifest.display()))?,
    )
    .map_err(|e| format!("manifest is not JSON: {e}"))?;
    let pages = manifest["pages"]
        .as_array()
        .ok_or("manifest has no pages array")?;

    let mut report = Report {
        passed: 0,
        failed: 0,
        skipped: 0,
        names: Vec::new(),
    };

    let document = RawDocument::open(bindings, &args.file)?;
    println!(
        "file: {} ({} pages), manifest: {}",
        args.file.display(),
        document.page_count(),
        args.manifest.display()
    );

    for (index, expected) in pages.iter().enumerate() {
        let page = document.page(index as u32)?;
        let structure = structure::read(&page)?;
        let extracted = text::extract(&page)?;
        check_page(&mut report, index, expected, &structure, &extracted.codes);
    }

    // The untagged control. A document with no tree must report nothing rather
    // than reporting an order it inferred, because a caller distinguishes "use
    // geometry" from "the document says this" by exactly that emptiness.
    match &args.untagged {
        Some(path) => {
            let plain = RawDocument::open(bindings, path)?;
            let page = plain.page(0)?;
            let structure = structure::read(&page)?;
            report.check(
                structure.runs.is_empty()
                    && structure.chars > 0
                    && structure.untagged_chars == structure.chars,
                "an untagged page reports no structure, not an inferred one",
                &format!(
                    "{}: {} runs, {} of {} characters unclaimed",
                    path.display(),
                    structure.runs.len(),
                    structure.untagged_chars,
                    structure.chars
                ),
            );
        }
        None => report.skip(
            "an untagged page reports no structure, not an inferred one",
            "no untagged fixture was given",
        ),
    }

    let named = checkroll::finish(&report.names);
    println!(
        "\n{}/{} checks passed, {} not applicable",
        report.passed,
        report.passed + report.failed,
        report.skipped
    );
    Ok(report.failed == 0 && named)
}

/// Every assertion about one page.
fn check_page(
    report: &mut Report,
    index: usize,
    expected: &serde_json::Value,
    structure: &PageStructure,
    codes: &[u32],
) {
    let page = index + 1;
    let names = |key: &str| -> Vec<String> {
        expected[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let tagged = names("tagged");
    let geometric = names("geometric");
    let text_of = &expected["text"];
    let types = &expected["types"];

    // Each run resolved to the words the *page* has at its indices, then matched
    // against the manifest's text for a block. A run whose characters are not any
    // block's text is a mapping error, and is reported as one rather than being
    // dropped from the comparison.
    let mut order: Vec<String> = Vec::new();
    let mut unmatched: Vec<String> = Vec::new();
    for run in &structure.runs {
        if run.end as usize > codes.len() || run.start >= run.end {
            unmatched.push(format!("[{}, {}) out of range", run.start, run.end));
            continue;
        }
        let words = words_of(codes, run.start, run.end);
        match tagged
            .iter()
            .find(|name| text_of[name.as_str()].as_str() == Some(words.as_str()))
        {
            Some(name) => order.push(name.clone()),
            None => unmatched.push(format!("\"{words}\"")),
        }
    }

    report.check(
        unmatched.is_empty() && order.len() == tagged.len(),
        &format!("page {page}: every run's characters are a block's text"),
        &format!(
            "{} runs -> {} blocks, unmatched {:?}",
            structure.runs.len(),
            order.len(),
            unmatched
        ),
    );

    report.check(
        order == tagged,
        &format!("page {page}: the order is the one the tags give"),
        &format!("got {order:?}, tags say {tagged:?}"),
    );

    // The discriminating half. On the control page the two orders are the same by
    // construction, so there is nothing here to tell apart and it says so.
    if tagged == geometric {
        report.skip(
            &format!("page {page}: and not the one geometry would give"),
            "this page is the control -- the two orders agree by construction",
        );
    } else {
        report.check(
            order != geometric,
            &format!("page {page}: and not the one geometry would give"),
            &format!("geometry would say {geometric:?}"),
        );
    }

    // Types, because reading order is not the only thing a tree answers and a
    // consumer that flattened every element to text would pass everything above.
    let mut wrong: Vec<String> = Vec::new();
    for (run, name) in structure.runs.iter().zip(order.iter()) {
        let want = types[name.as_str()].as_str().unwrap_or("");
        if run.tag != want {
            wrong.push(format!("{name}: {} not {want}", run.tag));
        }
    }
    report.check(
        wrong.is_empty() && !order.is_empty(),
        &format!("page {page}: each run carries its element's type"),
        &if wrong.is_empty() {
            let tags: Vec<&str> = structure.runs.iter().map(|r| r.tag.as_str()).collect();
            format!("{tags:?}")
        } else {
            format!("{wrong:?}")
        },
    );

    // Not "every character is claimed", which is not true and should not be:
    // PDFium generates a separator between two text objects and one that falls
    // *between* two elements belongs to neither. What must hold is that nothing
    // **visible** is unclaimed --- a tagged reading order that silently dropped a
    // word would be worse than no tagged reading order at all.
    let mut claimed = vec![false; codes.len()];
    for run in &structure.runs {
        let upto = (run.end as usize).min(codes.len());
        for slot in claimed[run.start as usize..upto].iter_mut() {
            *slot = true;
        }
    }
    let lost: String = codes
        .iter()
        .enumerate()
        .filter(|(index, _)| !claimed[*index])
        .filter_map(|(_, code)| char::from_u32(*code))
        .filter(|ch| !ch.is_whitespace())
        .collect();
    report.check(
        lost.is_empty() && !structure.truncated,
        &format!("page {page}: no visible character is left out of the tree"),
        &format!(
            "{} of {} characters unclaimed, all whitespace unless named here: \"{lost}\"; \
             truncated={}",
            structure.untagged_chars, structure.chars, structure.truncated
        ),
    );
}
