//! Does search work on text that is not English?
//!
//! Every fixture the search code had ever been pointed at before this one was
//! Latin, written by us, in one script --- so "search works" meant "search works
//! on the documents we generated ourselves", which is a weaker claim than it
//! reads as. `docs/PLAN.md` Phase 1 names a multilingual corpus as one of two
//! items not to be estimated as viewer polish, and this is the harness for it.
//!
//! It runs `testdata/multilingual.pdf` through the **production** path ---
//! `text::extract` then `search::find_in`, the same two calls a reader's Ctrl-F
//! makes --- and compares against `multilingual-manifest.json`, which a different
//! program wrote from the strings it laid out. Nothing here computes an expected
//! answer by folding and matching: that would be comparing an implementation with
//! itself.
//!
//! ## What it checks, and why each one can fail
//!
//! **The text arrived at all.** Per page, the extracted characters against the
//! manifest's lines. This is the check that found the defect the corpus was built
//! for: `FPDFText_GetUnicode` is a UTF-16 API, so a code point above the BMP came
//! back as two lone surrogates, `char::from_u32` refused both, and an Extension B
//! ideograph was invisible to search while being perfectly visible on the page.
//!
//! **Each query finds the number of hits the manifest states.** Counts, not
//! presence: a fold that matched too eagerly would satisfy "at least one" on
//! every query here. Five of the queries record a *decision* rather than a fact
//! --- whole-word inside Japanese, NFC against NFD --- so a change in behaviour
//! turns this red and has to be argued for rather than absorbed.
//!
//! **A hit's indices still address the characters it claims.** For every hit, the
//! code points at `[start, end)` are compared against the `hit` string the match
//! carries. That is the assertion an off-by-one in the surrogate join lands on,
//! and it is the one that could not be made from the frontend: JavaScript
//! reassembles two adjacent lone surrogates into the right character by accident,
//! so the *same* broken array reads as correct there and wrong here.
//!
//! **Every hit is paintable.** A hit whose range contains a character with no box
//! cannot be highlighted, which is how a correct search result becomes an
//! invisible one.
//!
//! ## The one thing it does not assert
//!
//! The astral page's glyphs. It draws a stand-in --- no font on either platform
//! has an Extension B ideograph --- so the manifest marks it `standin` and a pixel
//! comparison there would be asserting that we drew the wrong character
//! correctly. The check reads that flag rather than knowing the page number.
//!
//! ```text
//! cargo run --release --example search-probe -- \
//!     --file ../testdata/multilingual.pdf
//! ```

use std::path::{Path, PathBuf};

use tpdf_lib::progressive::{self, RawDocument};
use tpdf_lib::search::{self, Options};
use tpdf_lib::text::{self, PageText};

struct Args {
    library: PathBuf,
    file: PathBuf,
    manifest: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut library = PathBuf::from("../vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut file = PathBuf::from("../testdata/multilingual.pdf");
    let mut manifest: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--lib" => library = PathBuf::from(value()?),
            "--file" => file = PathBuf::from(value()?),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown flag: {other}")),
        }
    }

    let manifest = manifest.unwrap_or_else(|| {
        let mut path = file.clone();
        path.set_extension("");
        PathBuf::from(format!("{}-manifest.json", path.display()))
    });
    Ok(Args {
        library,
        file,
        manifest,
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

    /// A check this fixture cannot exercise. Printed, never omitted.
    fn skip(&mut self, name: &str, why: &str) {
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

/// The page's characters as a string, exactly as `codes` holds them.
///
/// Deliberately not `search`'s own `exact_of`: that one filters out anything
/// `char::from_u32` refuses, which is precisely the behaviour under test. A
/// refused code becomes U+FFFD here so it is *visible* in a diff rather than
/// silently absent --- an assertion that cannot see the defect it is aimed at is
/// the failure this whole family of harnesses exists to avoid.
fn page_text(text: &PageText) -> String {
    text.codes
        .iter()
        .map(|code| char::from_u32(*code).unwrap_or('\u{FFFD}'))
        .collect()
}

/// The manifest's lines joined the way PDFium separates them.
///
/// PDFium synthesises `\r\n` between text objects, so the comparison has to
/// either insert them or strip them. Stripping is wrong: the page break is
/// whitespace and losing it merges the last word of one line with the first of
/// the next, which `search.rs` cares about. Inserting is what a producer's own
/// line structure means.
fn expected_text(lines: &[String]) -> String {
    lines.join("\r\n")
}

fn options_of(value: &serde_json::Value) -> Options {
    let flag = |name: &str| value.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
    Options {
        match_case: flag("matchCase"),
        whole_word: flag("wholeWord"),
        regex: flag("regex"),
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let bindings = bind(&args.library)?;
    let document = RawDocument::open(bindings, &args.file)?;
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&args.manifest)
            .map_err(|e| format!("could not read {}: {e}", args.manifest.display()))?,
    )
    .map_err(|e| format!("manifest is not JSON: {e}"))?;

    let described = manifest["pages"]
        .as_array()
        .ok_or("manifest has no pages array")?;
    let queries = manifest["queries"]
        .as_array()
        .ok_or("manifest has no queries array")?;

    let count = document.page_count();
    if count as usize != described.len() {
        return Err(format!(
            "document has {count} pages and the manifest describes {}",
            described.len()
        ));
    }

    println!(
        "{} : {count} pages, {} queries",
        args.file.display(),
        queries.len()
    );

    let mut report = Report {
        passed: 0,
        failed: 0,
        skipped: 0,
    };

    // Extract every page once. The queries below are keyed by page number, and
    // re-extracting per query would make the harness slower than the thing it is
    // measuring for no gain --- extraction has no state a query could disturb.
    let mut pages: Vec<PageText> = Vec::new();
    for number in 0..count {
        let page = document.page(number)?;
        pages.push(text::extract(&page)?);
    }

    for (number, described) in described.iter().enumerate() {
        let name = described["name"].as_str().unwrap_or("?");
        let lines: Vec<String> = described["lines"]
            .as_array()
            .ok_or_else(|| format!("page {number} has no lines"))?
            .iter()
            .map(|line| line.as_str().unwrap_or_default().to_string())
            .collect();
        let text = &pages[number];
        let got = page_text(text);
        let want = expected_text(&lines);
        report.check(
            got == want,
            &format!("page {number} {name}: text is what was written"),
            &if got == want {
                format!("{} characters", text.len())
            } else {
                format!("got {got:?}, want {want:?}")
            },
        );

        // A page whose characters are right can still have lost its geometry, and
        // a hit with no box cannot be highlighted. Marks are the exception that
        // matters --- PDFium gives a synthesised separator no box at all --- so
        // this counts rather than requiring all.
        let placed = (0..text.len())
            .filter(|index| {
                let quad = &text.boxes[index * 4..index * 4 + 4];
                quad[2] > quad[0] || quad[3] > quad[1]
            })
            .count();
        let visible = got.chars().filter(|ch| !ch.is_whitespace()).count();
        report.check(
            placed >= visible,
            &format!("page {number} {name}: every visible character is placed"),
            &format!("{placed} placed, {visible} visible"),
        );

        if described["standin"].as_bool().unwrap_or(false) {
            report.skip(
                &format!("page {number} {name}: glyphs are the characters"),
                "this page draws a stand-in glyph, so what it shows is not what it says",
            );
        } else {
            // Not a render comparison --- no probe here rasterises --- but the
            // weaker statement that nothing was re-labelled: every character the
            // manifest wrote is one the page's own font was chosen to cover.
            report.check(
                !got.contains('\u{FFFD}'),
                &format!("page {number} {name}: glyphs are the characters"),
                "no replacement character in the extracted text",
            );
        }
    }

    for query in queries {
        let name = query["name"].as_str().unwrap_or("?");
        let needle = query["query"].as_str().unwrap_or_default();
        let number = query["page"].as_u64().unwrap_or(0) as usize;
        let wanted = query["hits"].as_u64().unwrap_or(0) as usize;
        let options = options_of(&query["options"]);
        // A decision rather than a fact about the file. Printed so that a reader
        // of the output can see which counts are arguable, which is the whole
        // reason the manifest distinguishes them.
        let kind = if query.get("decided").is_some() {
            "decided"
        } else if query.get("measured").is_some() {
            "measured"
        } else {
            "stated"
        };

        let text = pages
            .get(number)
            .ok_or_else(|| format!("query {name} names page {number}"))?;
        let matches = search::find_in(text, number as u32, needle, options)?;
        // `: hit count` rather than the bare query name, so that no check name is
        // a *prefix* of another. `scripts/mutate_viewer.py` matches an
        // expectation as a substring and refuses one naming more than a single
        // check, so a bare `query astral-alone` matched this and both of the
        // per-hit checks below it --- and the mutation could not be aimed.
        report.check(
            matches.len() == wanted,
            &format!("query {name}: hit count"),
            &format!("{} hits, {kind} {wanted}", matches.len()),
        );

        let mut aligned = true;
        let mut paintable = true;
        for hit in &matches {
            // The hit's own text against the characters its indices address. An
            // off-by-one in the surrogate join lands here, and nowhere else: the
            // count above is satisfied by a hit in the wrong place.
            if hit.end_page.is_some() {
                continue;
            }
            let span: String = text.codes[hit.start as usize..hit.end as usize]
                .iter()
                .map(|code| char::from_u32(*code).unwrap_or('\u{FFFD}'))
                .collect();
            if span != hit.hit {
                aligned = false;
            }
            let boxed = (hit.start as usize..hit.end as usize).any(|index| {
                let quad = &text.boxes[index * 4..index * 4 + 4];
                quad[2] > quad[0] || quad[3] > quad[1]
            });
            if !boxed {
                paintable = false;
            }
        }
        if matches.is_empty() {
            report.skip(
                &format!("query {name}: indices address the hit"),
                "expected no hits, so there are no indices to check",
            );
            report.skip(
                &format!("query {name}: hit is paintable"),
                "expected no hits, so there is nothing to paint",
            );
        } else {
            report.check(
                aligned,
                &format!("query {name}: indices address the hit"),
                &format!("{} hits, each spanning what it says", matches.len()),
            );
            report.check(
                paintable,
                &format!("query {name}: hit is paintable"),
                "every hit covers at least one character with a box",
            );
        }
    }

    // Worded exactly as every other probe here words it, because
    // `scripts/mutate_viewer.py` requires a summary line before it will believe a
    // run happened at all --- a crash and a clean sweep both produce no failures.
    println!(
        "\n{}/{} checks passed, {} not applicable",
        report.passed,
        report.passed + report.failed,
        report.skipped
    );
    Ok(report.failed == 0)
}
