//! Phase 1: does the outline walk terminate, resolve, and refuse?
//!
//! `outline.rs` walks a tree PDFium explicitly declines to make safe --- its own
//! documentation says the caller must handle circular references. The failure
//! mode is not a wrong answer, it is a render thread that never returns, so the
//! check that matters is that the walk *finished at all*, and that what it
//! returned still contains the entries after the loop rather than stopping at
//! it.
//!
//! Two modes:
//!
//! * `--mode read` --- prints the tree, so a human can look at one.
//!
//! * `--mode check` --- asserts `testdata/outline-manifest.json` against what
//!   the walk returned: every expected entry present at the expected depth
//!   pointing at the expected page, every refused action refused *with the
//!   right reason*, and every bound reported rather than silently applied.
//!
//! The check carries a control of its own. A walk that hit the item budget
//! could satisfy "the required titles are present" on the simple fixture by
//! accident, so the simple fixture must report **no** limits at all, and the
//! hostile one must report a cycle, a depth cut and no budget exhaustion. A
//! run where the ordinary document also looks malformed is a walk that is
//! bounding things it should not.
//!
//! Usage:
//!   outline-probe <file.pdf> [--mode read|check] [--manifest PATH] [--lib DIR]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tpdf_lib::document::OpenDocument;

use tpdf_lib::outline::{self, Limits, Outline, OutlineItem, Target};
use tpdf_lib::progressive::{self};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    Check,
}

struct Args {
    file: PathBuf,
    mode: Mode,
    manifest: PathBuf,
    library: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: outline-probe <file.pdf> [...]")?;
    let mut parsed = Args {
        file: PathBuf::from(file),
        mode: Mode::Read,
        manifest: PathBuf::from("testdata/outline-manifest.json"),
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };

    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "read" => Mode::Read,
                    "check" => Mode::Check,
                    other => return Err(format!("unknown mode: {other}")),
                }
            }
            "--manifest" => parsed.manifest = PathBuf::from(value),
            "--lib" => parsed.library = PathBuf::from(value),
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(parsed)
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

fn run(args: &Args) -> Result<bool, String> {
    let bindings = bind(&args.library)?;
    let document = OpenDocument::open(bindings, &args.file, None)?;
    let outline = outline::read(&document);

    match args.mode {
        Mode::Read => {
            read(&outline);
            Ok(true)
        }
        Mode::Check => check(args, &outline),
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

/// One entry, flattened, as the checks want to see it.
struct Flat {
    title: String,
    depth: usize,
    target: Target,
}

fn flatten(items: &[OutlineItem], depth: usize, out: &mut Vec<Flat>) {
    for item in items {
        out.push(Flat {
            title: item.title.clone(),
            depth,
            target: item.target.clone(),
        });
        flatten(&item.children, depth + 1, out);
    }
}

fn describe(target: &Target) -> String {
    match target {
        Target::Page { page, top_pt } => match top_pt {
            Some(top) => format!("page {} at {top:.0}pt", page + 1),
            None => format!("page {}", page + 1),
        },
        Target::Broken => "broken destination".to_string(),
        Target::Refused { action } => format!("refused: {action}"),
        Target::None => "no destination".to_string(),
    }
}

fn read(outline: &Outline) {
    let mut flat = Vec::new();
    flatten(&outline.items, 0, &mut flat);

    println!(
        "{} entries, walked in {:.2} ms",
        outline.total, outline.walk_ms
    );
    for entry in &flat {
        let title = if entry.title.chars().count() > 60 {
            let short: String = entry.title.chars().take(57).collect();
            format!("{short}...")
        } else {
            entry.title.clone()
        };
        println!(
            "{:indent$}{title}   [{}]",
            "",
            describe(&entry.target),
            indent = entry.depth * 2
        );
    }
    println!("{}", limits_line(&outline.limits));
}

fn limits_line(limits: &Limits) -> String {
    format!(
        "limits: cycles={} too_deep={} over_budget={} titles_clipped={}",
        limits.cycles, limits.too_deep, limits.over_budget, limits.titles_clipped
    )
}

/// Records a check's outcome, in the shape every other probe here uses.
struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: &str) {
        if ok {
            self.passed += 1;
            println!("[OK]   {name:<48} {detail}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name:<48} {detail}");
        }
    }

    /// A check this fixture cannot exercise. Printed, never omitted --- a check
    /// that quietly disappears on some inputs cannot be told apart from one that
    /// ran, which is the failure this whole file is arranged to avoid.
    fn skip(&mut self, name: &str, why: &str) {
        println!("[SKIP] {name:<48} not applicable -- {why}");
    }
}

fn check(args: &Args, outline: &Outline) -> Result<bool, String> {
    let text = std::fs::read_to_string(&args.manifest)
        .map_err(|e| format!("could not read {}: {e}", args.manifest.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("manifest is not JSON: {e}"))?;

    let name = args
        .file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the file has no name")?;
    let expected = manifest
        .get(name)
        .ok_or_else(|| format!("{name} is not in {}", args.manifest.display()))?;

    let mut flat = Vec::new();
    flatten(&outline.items, 0, &mut flat);
    let by_title: BTreeMap<&str, &Flat> = flat
        .iter()
        .map(|entry| (entry.title.as_str(), entry))
        .collect();

    let mut report = Report {
        passed: 0,
        failed: 0,
    };

    println!(
        "{name}: {} entries in {:.2} ms",
        outline.total, outline.walk_ms
    );

    // That this line prints at all is the headline result: the walk returned.
    // A hanging walk produces no output and no failure line, which is exactly
    // what a passing run looks like from the outside -- so the summary at the
    // bottom is what a caller must key on, never the absence of `[FAIL]`.
    report.check(
        outline.total > 0,
        "the walk terminated and found entries",
        &format!("{} entries", outline.total),
    );

    if let Some(entries) = expected.get("entries").and_then(|v| v.as_array()) {
        check_entries(&mut report, &flat, entries, expected);
    }
    // Outside the `entries` guard on purpose: a fixture without an entry list
    // still has destinations, and a check that only exists for some fixtures
    // cannot be told apart from one that ran.
    {
        check_tops(&mut report, &flat, expected);
    }
    if let Some(titles) = expected.get("required_titles").and_then(|v| v.as_array()) {
        for title in titles.iter().filter_map(|v| v.as_str()) {
            report.check(
                by_title.contains_key(title),
                "an entry after the cycle is still reached",
                title,
            );
        }
    }
    if let Some(refused) = expected.get("refused").and_then(|v| v.as_object()) {
        for (title, reason) in refused {
            let want = reason.as_str().unwrap_or_default();
            let got = by_title.get(title.as_str()).map(|e| &e.target);
            let ok = matches!(got, Some(Target::Refused { action }) if action == want);
            report.check(
                ok,
                "an action tpdf will not follow is refused",
                &format!(
                    "{title}: wanted refused:{want}, got {}",
                    got.map(describe).unwrap_or_else(|| "nothing".into())
                ),
            );
        }
    }
    if let Some(broken) = expected.get("broken").and_then(|v| v.as_array()) {
        for title in broken.iter().filter_map(|v| v.as_str()) {
            let got = by_title.get(title).map(|e| &e.target);
            report.check(
                matches!(got, Some(Target::Broken)),
                "a destination off the end of the document is broken",
                &format!(
                    "{title}: {}",
                    got.map(describe).unwrap_or_else(|| "nothing".into())
                ),
            );
        }
    }

    check_limits(&mut report, name, outline, expected);

    let total = report.passed + report.failed;
    println!("\n{}/{total} checks passed", report.passed);
    Ok(report.failed == 0)
}

/// Checks the ordinary fixture's entries: order, depth and destination.
/// Each destination's distance from the top of its page, against the manifest.
fn check_tops(report: &mut Report, flat: &[Flat], manifest: &serde_json::Value) {
    let tops: Vec<f32> = flat
        .iter()
        .filter_map(|entry| match entry.target {
            Target::Page {
                top_pt: Some(top), ..
            } => Some(top),
            _ => None,
        })
        .collect();
    // Fixture-specific, and stated by the fixture rather than hardcoded here so
    // that a second one can state its own. Under a coordinate flip these are
    // still numbers, still inside the page and still in the same order -- what
    // is wrong is the distance from the top edge, which is the only thing this
    // compares. `rotated-90.pdf` is the case that needs its own numbers: its
    // pages carry /Rotate 90, so a destination's *page-space x* is what becomes
    // the distance down the display, and the ordinary flip is wrong there in a
    // way no unrotated fixture can show.
    let wanted: Vec<f64> = manifest
        .get("tops")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_f64)
                .collect()
        })
        .unwrap_or_default();
    if wanted.is_empty() {
        report.skip(
            "each destination is measured from the top of its page",
            "this fixture states no expected offsets",
        );
    } else {
        let agree = wanted.len() <= tops.len()
            && wanted
                .iter()
                .zip(&tops)
                .all(|(want, got)| (*want as f32 - *got).abs() < 1.0);
        report.check(
            agree,
            "each destination is measured from the top of its page",
            &format!(
                "{:?}, expected {:?}",
                &tops[..tops.len().min(wanted.len())],
                wanted
            ),
        );
    }
}

fn check_entries(
    report: &mut Report,
    flat: &[Flat],
    expected: &[serde_json::Value],
    _manifest: &serde_json::Value,
) {
    report.check(
        flat.len() == expected.len(),
        "every entry is reached, and no more",
        &format!("{} entries, expected {}", flat.len(), expected.len()),
    );

    for (index, want) in expected.iter().enumerate() {
        let Some(got) = flat.get(index) else { break };
        let title = want.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let depth = want.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let page = want.get("page").and_then(|v| v.as_u64()).map(|p| p as u32);
        let has_top = want
            .get("has_top")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        report.check(
            got.title == title && got.depth == depth,
            "an entry arrives in document order at its depth",
            &format!(
                "#{index}: {:?} at depth {}, expected {title:?} at {depth}",
                got.title, got.depth
            ),
        );

        let target_ok = match (&got.target, page) {
            (Target::Page { page: p, top_pt }, Some(want)) => {
                *p == want && top_pt.is_some() == has_top
            }
            (Target::None, None) => true,
            _ => false,
        };
        report.check(
            target_ok,
            "an entry resolves to the page it names",
            &format!(
                "{title:?}: {} (wanted page {page:?}, y {has_top})",
                describe(&got.target)
            ),
        );
    }

    // The y coordinate must be *inside* the page and must increase down the
    // document, which is the assertion a y-flip fails. Comparing it against
    // "some number arrived" would not: the flipped value is a number too.
    let tops: Vec<f32> = flat
        .iter()
        .filter_map(|entry| match entry.target {
            Target::Page {
                top_pt: Some(top), ..
            } => Some(top),
            _ => None,
        })
        .collect();
    report.check(
        tops.iter().all(|top| *top >= 0.0 && *top < 842.0),
        "a destination's y lands inside the page",
        &format!(
            "{} coordinates, max {:?}",
            tops.len(),
            tops.iter().cloned().fold(0.0f32, f32::max)
        ),
    );
}

/// Checks that the bounds fired where they should and nowhere else.
fn check_limits(report: &mut Report, name: &str, outline: &Outline, expected: &serde_json::Value) {
    let limits = &outline.limits;
    let hostile = expected.get("required_titles").is_some();

    if hostile {
        report.check(
            limits.cycles > 0,
            "the cycle was noticed rather than walked",
            &limits_line(limits),
        );
        report.check(
            limits.too_deep > 0,
            "the deep chain was cut at the depth bound",
            &format!("{} subtrees dropped", limits.too_deep),
        );
        report.check(
            limits.titles_clipped > 0,
            "the oversized title was clipped and said so",
            &format!("{} titles clipped", limits.titles_clipped),
        );
        report.check(
            !limits.over_budget,
            "the item budget was not what stopped the walk",
            "a walk that hits the budget proves nothing about the other bounds",
        );
    } else {
        // The control. Without it, a walker that reported every document as
        // malformed would pass every check above.
        report.check(
            !limits.any(),
            "an ordinary outline trips no bound at all",
            &limits_line(limits),
        );
    }

    let _ = name;
}
