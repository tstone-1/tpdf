//! Does the link scan find every link, resolve it, and refuse what it must?
//!
//! `links.rs` is unit-tested against documents it builds itself, which is right
//! for the resolvers and wrong for everything else: a document written by the
//! process that reads it agrees with itself about the parts both got wrong.
//! `testdata/links.pdf` is written by a generator that knows nothing about the
//! scan, and this asserts the scan against the manifest that generator emitted.
//!
//! ## The mode that matters is `--mode agree`
//!
//! tpdf has **two** destination resolvers. `outline.rs` asks PDFium, because a
//! bookmark is a PDFium object; `links.rs` reads the object graph with `lopdf`,
//! because asking PDFium would cost a page load per page. Two implementations of
//! one rule is the trap this repository records as *"Two copies of a distinction
//! drift, and a mutation of one survives"*, and the usual mitigations --- share
//! the type, share the wording --- do nothing about the resolution itself.
//!
//! So `links.pdf` gives its outline entries the same destinations as its links,
//! and this mode puts the two answers side by side. It is the only check here
//! that can fail for a reason neither module's own tests can reach.
//!
//! Four modes:
//!
//! * `--mode read` --- prints every link, so a human can look at one.
//!
//! * `--mode check` --- asserts `testdata/links-corpus.json`: both named
//!   destination mechanisms, each fit taking its top from its own position, the
//!   four refusals, the malformed shapes, the hidden and zero-area links that
//!   must not be listed, and every bound reported rather than silently applied.
//!
//! * `--mode agree` --- the two resolvers, on one document, compared.
//!
//! * `--mode clean` --- the control, and it is not optional. Against a document
//!   with no links at all it asserts zero links and zero limits: without it, a
//!   scan that returned nothing for everything would pass "the hidden link is
//!   not listed" and "the crowd is not cut" perfectly.
//!
//! Usage:
//!   links-probe <file.pdf> [--mode read|check|agree|clean] [--manifest PATH] [--lib DIR]

use std::path::{Path, PathBuf};

use tpdf_lib::links::Links;
use tpdf_lib::outline::Target;
use tpdf_lib::progressive::{self, RawDocument};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    Check,
    Agree,
    Clean,
}

struct Args {
    file: PathBuf,
    mode: Mode,
    manifest: PathBuf,
    library: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: links-probe <file.pdf> [...]")?;
    let mut parsed = Args {
        file: PathBuf::from(file),
        mode: Mode::Read,
        manifest: PathBuf::from("testdata/links-corpus.json"),
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };

    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "read" => Mode::Read,
                    "check" => Mode::Check,
                    "agree" => Mode::Agree,
                    "clean" => Mode::Clean,
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
    // Opened through PDFium as well as scanned: the viewer only ever asks for
    // links on a document it has open, and the page count that bounds the scan
    // comes from there. `--mode agree` needs the handle for a second reason ---
    // it is what the outline is read through.
    let bindings = bind(&args.library)?;
    let document = RawDocument::open(bindings, &args.file)?;
    let links = document.links()?;

    match args.mode {
        Mode::Read => {
            read(&links);
            Ok(true)
        }
        Mode::Check => check(args, &links),
        Mode::Agree => agree(args, &links, &document),
        Mode::Clean => Ok(clean(&links)),
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

/// A target in one line, for a human and for an assertion message.
fn describe(target: &Target) -> String {
    match target {
        Target::Page { page, top_pt } => match top_pt {
            Some(top) => format!("page {} at {top:.1} pt", page + 1),
            None => format!("page {}", page + 1),
        },
        Target::Broken => "broken".into(),
        Target::Refused { action } => format!("refused ({action})"),
        Target::None => "no destination".into(),
    }
}

fn read(links: &Links) {
    println!(
        "{} links, scanned in {:.2} ms",
        links.items.len(),
        links.scan_ms
    );
    for item in &links.items {
        println!(
            "  #{:<4} p{:<3} [{:>6.1} {:>6.1} {:>6.1} {:>6.1}]  {}",
            item.id,
            item.page + 1,
            item.rect[0],
            item.rect[1],
            item.rect[2],
            item.rect[3],
            describe(&item.target),
        );
    }
    println!("{}", limits_line(links));
}

fn limits_line(links: &Links) -> String {
    let limits = &links.limits;
    format!(
        "limits: crowded_pages={} over_budget={} unreadable={} unresolved_names={}",
        limits.crowded_pages, limits.over_budget, limits.unreadable, limits.unresolved_names
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
            println!("[OK]   {name:<52} {detail}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name:<52} {detail}");
        }
    }

    /// A check this fixture cannot exercise. Printed, never omitted --- a check
    /// that quietly disappears on some inputs cannot be told from one that ran.
    fn skip(&mut self, name: &str, why: &str) {
        println!("[SKIP] {name:<52} not applicable -- {why}");
    }

    fn finish(&self) -> bool {
        println!(
            "\n{}/{} checks passed",
            self.passed,
            self.passed + self.failed
        );
        self.failed == 0
    }
}

/// The control: a document with no links reports none, and cuts nothing.
fn clean(links: &Links) -> bool {
    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    report.check(
        links.items.is_empty(),
        "a document with no links has none",
        &format!("{} found", links.items.len()),
    );
    report.check(
        !links.limits.any(),
        "and reports nothing cut",
        &limits_line(links),
    );
    report.finish()
}

/// Reads the manifest section for the file being probed.
fn section(args: &Args) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(&args.manifest)
        .map_err(|e| format!("could not read {}: {e}", args.manifest.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("manifest is not JSON: {e}"))?;
    let name = args
        .file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the file has no name")?;
    manifest
        .get(name)
        .cloned()
        .ok_or_else(|| format!("{} has no section for {name}", args.manifest.display()))
}

/// Turns a manifest target into the one the scan should have produced.
fn wanted(value: &serde_json::Value) -> Result<Target, String> {
    let kind = value
        .get("kind")
        .and_then(|kind| kind.as_str())
        .ok_or("a manifest target has no kind")?;
    Ok(match kind {
        "page" => Target::Page {
            page: value
                .get("page")
                .and_then(serde_json::Value::as_u64)
                .ok_or("a page target has no page")? as u32,
            top_pt: value
                .get("top_pt")
                .and_then(serde_json::Value::as_f64)
                .map(|top| top as f32),
        },
        "broken" => Target::Broken,
        "none" => Target::None,
        "refused" => Target::Refused {
            action: value
                .get("action")
                .and_then(|action| action.as_str())
                .ok_or("a refusal has no action")?
                .to_string(),
        },
        other => return Err(format!("unknown target kind in the manifest: {other}")),
    })
}

/// Two targets agree if they name the same place, within a point.
///
/// A tolerance rather than equality because the two sides of `--mode agree`
/// reach the offset by different arithmetic --- one flips it against a
/// `/MediaBox` read by `lopdf`, the other against a size PDFium reports as
/// `f32` --- and a disagreement of a tenth of a point is those two roundings,
/// not two different answers. A whole point is still far below anything a reader
/// could see and far below any real destination difference.
fn same(left: &Target, right: &Target) -> bool {
    match (left, right) {
        (
            Target::Page {
                page: a,
                top_pt: top_a,
            },
            Target::Page {
                page: b,
                top_pt: top_b,
            },
        ) => {
            a == b
                && match (top_a, top_b) {
                    (Some(x), Some(y)) => (x - y).abs() < 1.0,
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => left == right,
    }
}

fn check(args: &Args, links: &Links) -> Result<bool, String> {
    let spec = section(args)?;
    let mut report = Report {
        passed: 0,
        failed: 0,
    };

    let expected = spec
        .get("expected")
        .and_then(|value| value.as_array())
        .ok_or("the manifest section has no expected array")?;

    // The declared links come first in document order, then a crowd if the
    // fixture has one. Matched by (page, target) rather than by index, so that
    // adding a link to the fixture does not shift every assertion after it.
    for entry in expected {
        let page = entry
            .get("page")
            .and_then(serde_json::Value::as_u64)
            .ok_or("an expected link has no page")? as u32;
        let target = wanted(
            entry
                .get("target")
                .ok_or("an expected link has no target")?,
        )?;
        let note = entry
            .get("note")
            .and_then(|note| note.as_str())
            .unwrap_or("(no note)");

        let found = links
            .items
            .iter()
            .filter(|item| item.page == page)
            .find(|item| same(&item.target, &target));
        report.check(
            found.is_some(),
            note,
            &match found {
                Some(link) => format!("p{} -> {}", link.page + 1, describe(&link.target)),
                None => format!(
                    "wanted {} on p{}, page has [{}]",
                    describe(&target),
                    page + 1,
                    links
                        .items
                        .iter()
                        .filter(|item| item.page == page)
                        .map(|item| describe(&item.target))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
        );
    }

    // The total, which is what says the things that must *not* be listed are
    // not: a hidden link, a zero-area rectangle and a comment sharing the array
    // are each absent by being absent, and only a count can see that.
    match spec.get("total").and_then(serde_json::Value::as_u64) {
        Some(total) => report.check(
            links.items.len() as u64 == total,
            "every link is listed, and nothing else is",
            &format!("{} listed, {total} expected", links.items.len()),
        ),
        None => report.skip(
            "every link is listed, and nothing else is",
            "the manifest states no total",
        ),
    }

    // The crowd, which is what says a page of many links is read whole.
    match (
        spec.get("crowd_page").and_then(serde_json::Value::as_u64),
        spec.get("crowd").and_then(serde_json::Value::as_u64),
    ) {
        (Some(page), Some(crowd)) => {
            let on_page = links
                .items
                .iter()
                .filter(|item| item.page as u64 == page)
                .count() as u64;
            report.check(
                on_page == crowd,
                "a page of many links is read whole",
                &format!("{on_page} on page {}, {crowd} expected", page + 1),
            );
        }
        _ => report.skip(
            "a page of many links is read whole",
            "the fixture has no crowded page",
        ),
    }

    // Rectangles: every one inside its page, and none of them empty. Cheap, and
    // it is the assertion that would catch a coordinate flip applied twice.
    let bad: Vec<String> = links
        .items
        .iter()
        .filter(|item| {
            let [left, top, right, bottom] = item.rect;
            right <= left || bottom <= top || left < 0.0 || top < 0.0
        })
        .map(|item| format!("#{} {:?}", item.id, item.rect))
        .collect();
    report.check(
        bad.is_empty(),
        "every rectangle has area and sits on its page",
        &if bad.is_empty() {
            format!("{} checked", links.items.len())
        } else {
            bad.join(", ")
        },
    );

    // The limits, named one at a time: a single equality on the whole struct
    // would report "limits differ" and leave the reader to work out which.
    let limits = spec
        .get("limits")
        .ok_or("the manifest section has no limits")?;
    let want = |key: &str| {
        limits
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    report.check(
        links.limits.unreadable as u64 == want("unreadable"),
        "unreadable entries are counted, not dropped",
        &format!(
            "{} counted, {} expected",
            links.limits.unreadable,
            want("unreadable")
        ),
    );
    report.check(
        links.limits.crowded_pages as u64 == want("crowded_pages"),
        "no page was cut short",
        &limits_line(links),
    );
    report.check(
        links.limits.unresolved_names as u64 == want("unresolved_names"),
        "every named destination resolved or failed honestly",
        &format!(
            "{} unresolved, {} expected",
            links.limits.unresolved_names,
            want("unresolved_names")
        ),
    );
    report.check(
        !links.limits.over_budget,
        "the document was under the whole-document budget",
        &limits_line(links),
    );

    Ok(report.finish())
}

/// The two destination resolvers, on one document, compared.
fn agree(args: &Args, links: &Links, document: &RawDocument) -> Result<bool, String> {
    let spec = section(args)?;
    let mut report = Report {
        passed: 0,
        failed: 0,
    };

    let shared = match spec
        .get("shared_targets")
        .and_then(|value| value.as_array())
    {
        Some(shared) if !shared.is_empty() => shared,
        _ => {
            report.skip(
                "the two resolvers agree",
                "this fixture has no outline aimed at the links' destinations",
            );
            return Ok(report.finish());
        }
    };

    let outline = tpdf_lib::outline::read(document);
    // Flattened, because the fixture's entries are one level and a nested one
    // would still have to be found: a comparison that could not locate an entry
    // would report agreement by having nothing to disagree with.
    let mut entries: Vec<(String, Target)> = Vec::new();
    fn walk(items: &[tpdf_lib::outline::OutlineItem], into: &mut Vec<(String, Target)>) {
        for item in items {
            into.push((item.title.clone(), item.target.clone()));
            walk(&item.children, into);
        }
    }
    walk(&outline.items, &mut entries);

    // The instrument's own control: an outline that came back empty would make
    // every comparison below vacuous, and a vacuous run prints the same green
    // as a real one.
    report.check(
        entries.len() >= shared.len(),
        "PDFium read the outline this compares against",
        &format!("{} entries, {} expected", entries.len(), shared.len()),
    );

    for entry in shared {
        let title = entry
            .get("title")
            .and_then(|title| title.as_str())
            .ok_or("a shared target has no title")?;
        let expected = wanted(entry.get("target").ok_or("a shared target has none")?)?;

        let Some((_, from_pdfium)) = entries.iter().find(|(name, _)| name == title) else {
            report.check(false, title, "no outline entry with this title");
            continue;
        };

        // Both against the manifest, not against each other. Two resolvers that
        // are wrong in the same way agree perfectly, and the fixture's generator
        // is the only party here that knows what it wrote.
        let ours = links
            .items
            .iter()
            .map(|item| &item.target)
            .find(|target| same(target, &expected));
        report.check(
            same(from_pdfium, &expected) && ours.is_some(),
            title,
            &format!(
                "lopdf {} / PDFium {} / manifest {}",
                ours.map(describe).unwrap_or_else(|| "absent".into()),
                describe(from_pdfium),
                describe(&expected),
            ),
        );
    }

    Ok(report.finish())
}
