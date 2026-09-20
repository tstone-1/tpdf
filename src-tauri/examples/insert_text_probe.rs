//! Does a replacement typed on an inserted page end up on *that* page of the
//! saved file, with every other page untouched?
//!
//! `save::import_tests` proves the writer, and it proves it by reading back
//! what `lopdf` wrote with `lopdf`. That is self-consistent by construction:
//! the same parser on both sides cannot notice a file it agrees with itself
//! about. So this probe writes the file and then says nothing about what is in
//! it --- it prints where the file is and what was asked for, and an
//! independent reader (`scripts/insert_text_check.py`, which uses `pypdf` and
//! `qpdf --check`) is what decides.
//!
//! **The page number is the whole point.** Page `n` of the opened document and
//! page `n` of the file its pages were inserted from are two different pages,
//! and every stage between the reader's keystroke and the written bytes has to
//! keep them apart. A probe that inserted from a file with the same words on
//! every page would pass with them confused, so the check script reads the
//! untouched pages back as well as the edited one.
//!
//! **In-process (`save::Here`) rather than through the sandboxed worker**,
//! which is what `save::import_tests` and `merge-probe` do for the same reason:
//! what is under test here is the document that comes out. `worker-probe`
//! drives the out-of-process writer, and the two paths run the same
//! `save::rewrite`.
//!
//! Usage:
//!   insert-text-probe <base.pdf> <other.pdf> <out.pdf>
//!                     [--page N] [--after N] [--replacement TEXT]
//!
//! `--page` is the zero-based page of the *other* file to insert (default 0),
//! `--after` the zero-based slot of the opened document it lands behind
//! (default 0), and `--replacement` the words to put in place of that page's
//! first text run. Without one the probe shortens the original to its first
//! word, which fits by construction --- a longer replacement is refused by the
//! writer, and that refusal is a correct answer rather than a probe failure.
//!
//! It prints one JSON object on stdout and nothing else there, so a caller can
//! read it; every remark goes to stderr.

use std::path::PathBuf;

use lopdf::Document;
use tpdf_lib::docmodel::{PageSource, SourceId};
use tpdf_lib::edits::{PageView, Plan, PlannedSource};
use tpdf_lib::fingerprint::Fingerprint;
use tpdf_lib::{save, textedit};

fn main() {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut page = 0u32;
    let mut after = 0usize;
    let mut replacement: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" => page = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--after" => after = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--replacement" => replacement = args.next(),
            other => files.push(PathBuf::from(other)),
        }
    }
    let [base, other, out] = files.as_slice() else {
        eprintln!(
            "usage: insert-text-probe <base.pdf> <other.pdf> <out.pdf> \
             [--page N] [--after N] [--replacement TEXT]"
        );
        std::process::exit(2);
    };

    let opened = match Document::load(base) {
        Ok(document) => document,
        Err(why) => {
            eprintln!("[FAIL] could not read {}: {why}", base.display());
            std::process::exit(1);
        }
    };
    let baseline = tpdf_lib::pagetree::ordered_pages(&opened).len() as u32;
    if after >= baseline as usize {
        eprintln!("[FAIL] --after {after} is past the opened document's {baseline} page(s)");
        std::process::exit(1);
    }

    // Read through the *other* file, addressed by its page number there ---
    // which is exactly what the reader's own edit does, through that file's
    // own worker. A scan of the opened document would produce a change that
    // the writer refuses on its digest, which is the point of the split.
    let source = match Document::load(other) {
        Ok(document) => document,
        Err(why) => {
            eprintln!("[FAIL] could not read {}: {why}", other.display());
            std::process::exit(1);
        }
    };
    let runs = match textedit::scan(&source, page) {
        Ok(runs) => runs,
        Err(why) => {
            eprintln!(
                "[FAIL] no editable text on page {} of the other file: {why}",
                page + 1
            );
            std::process::exit(1);
        }
    };
    let Some(run) = runs.runs.iter().find(|run| !run.text.trim().is_empty()) else {
        eprintln!(
            "[FAIL] page {} of the other file has no text to edit",
            page + 1
        );
        std::process::exit(1);
    };
    let replacement = replacement.unwrap_or_else(|| {
        run.text
            .split_whitespace()
            .next()
            .unwrap_or("EDITED")
            .to_string()
    });

    let mut pages: Vec<PageView> = (0..baseline)
        .map(|index| PageView {
            id: u64::from(index) + 1,
            source: PageSource::Baseline(index),
            turns: 0,
            crop: None,
        })
        .collect();
    pages.insert(
        after + 1,
        PageView {
            id: u64::from(baseline) + 1,
            source: PageSource::Imported {
                source: SourceId::from_raw(1),
                page,
            },
            turns: 0,
            crop: None,
        },
    );
    let fingerprint = match Fingerprint::of(other) {
        Ok(fingerprint) => fingerprint,
        Err(why) => {
            eprintln!("[FAIL] could not fingerprint the other file: {why}");
            std::process::exit(1);
        }
    };
    let plan = Plan {
        text_edits: vec![textedit::Edit::imported(
            1,
            textedit::Change {
                layout: None,
                page,
                revision: runs.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: replacement.clone(),
            },
        )],
        forms: Vec::new(),
        baseline,
        opened_as: None,
        pages,
        marks: Vec::new(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        sources: vec![PlannedSource {
            id: 1,
            path: other.clone(),
            opened_as: Some(fingerprint),
        }],
    };

    if let Err(why) = save::write_copy(base, &plan, out, None, &save::Here) {
        eprintln!("[FAIL] the save was refused: {why}");
        std::process::exit(1);
    }

    let escape = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    println!(
        "{{\"out\": \"{}\", \"baseline\": {baseline}, \"inserted_at\": {}, \
         \"other_page\": {page}, \"original\": \"{}\", \"replacement\": \"{}\"}}",
        escape(&out.display().to_string()),
        after + 1,
        escape(&run.text),
        escape(&replacement),
    );
    eprintln!(
        "[OK] wrote {} --- page {} is page {} of {}, with its first run replaced",
        out.display(),
        after + 2,
        page + 1,
        other.display()
    );
}
