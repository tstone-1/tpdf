//! Does a region marked on the reader's own page survive a document that also
//! holds pages of another file --- and does the report stay honest about it?
//!
//! `save::import_tests` proves the writer with `lopdf` on both sides of the
//! comparison, which cannot notice a file it agrees with itself about. This
//! runs the same removal through the **sandboxed worker** (`save::InWorker`,
//! the writer the application uses) and then says nothing about what came out:
//! it prints where the file is and what was asked for, and an independent
//! reader --- `scripts/redact_import_check.py`, which uses `pypdf` and
//! `qpdf --check` --- is what decides.
//!
//! ## The two runs, and the second is the one worth having
//!
//! * **plain**: the other file carries none of the removed words. The scan
//!   finds nothing, the file verifies, and no note is added --- which is the
//!   property that keeps this from being the old document-wide refusal wearing
//!   a different coat.
//! * **`--echo`**: the other file is synthesised here and *prints the very word
//!   the region covers*. The scan finds it, the file does not verify, and the
//!   report says which page it is on. Nothing is silenced to make it look
//!   clean, which is `docs/PLAN.md` §6's rule.
//!
//! ⚠ **The second run's answer changed on 2026-09-21 and the sentence above
//! used to end differently.** It read *"the report carries
//! `redact::inserted_pages_note` beside the finding"* --- a note saying the
//! scan could not tell which page the word was on. `verify::scan` now walks the
//! written file per page, so it can: the hit is placed on slot 0, the inserted
//! page, and that note stays quiet because its own first clause is no longer
//! true. `redact::marked_pages_note` is what speaks instead, and it compares
//! the walk's answer against the slot the region was on. The verdict is the
//! same either way --- a word still in the file is still *not verified*.
//!
//! The echo file is built rather than tracked because it has to agree with
//! `--needle`, and a fixture that has to agree with an argument is a fixture
//! that will one day not.
//!
//! ## What this does not exercise
//!
//! The ask step runs here in-process: `render::redaction_plans_of` needs a
//! PDFium document in hand, and in the application that call is already in a
//! worker (`worker_child.rs`). What crosses the boundary here is the write and
//! the verification, which is where an inserted page changes what happens.
//! The OCR gate is not run --- it needs an engine and a render of the output;
//! `redact::gate_at_output_slots` has the unit tests for the remapping this
//! increment gave it.
//!
//! Usage:
//!   redact-import-probe <base.pdf> <other.pdf> <out.pdf>
//!                       [--library DIR] [--needle W] [--keep W] [--echo]
//!
//! It prints one JSON object on stdout and nothing else there; every remark
//! goes to stderr. Exit 0 when the removal ran, 1 when it did not, 2 on usage.

use std::path::{Path, PathBuf};

use lopdf::{dictionary, Document, Object, Stream};
use tpdf_lib::docmodel::{PageSource, SourceId};
use tpdf_lib::document::OpenDocument;
use tpdf_lib::edits::{PageView, Plan, PlannedRedaction, PlannedSource};
use tpdf_lib::fingerprint::Fingerprint;
use tpdf_lib::{progressive, redact, render, save, text, worker, worker_child};

/// The word the marked region covers, on page 1 of the base document.
const NEEDLE: &str = "4711-0815";
/// On another line of the same page, so route B's collateral does not take it.
/// Without it a scan that found nothing would have told us nothing.
const KEEP: &str = "Sphinx of black quartz";

fn main() -> std::process::ExitCode {
    // **This binary is also the worker**, because `save::InWorker` spawns one by
    // re-executing `current_exe`. Without this the child lands in the argument
    // loop below, prints the usage line and exits --- which the parent sees as
    // *worker stopped answering*, a sentence about the boundary that is really
    // a sentence about this function. `worker-probe` and `text-edit-probe` open
    // the same way and for the same reason.
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|arg| arg == worker::WORKER_ARGV) {
        worker_child::main(&argv);
    }

    let mut files: Vec<PathBuf> = Vec::new();
    let mut library: Option<PathBuf> = None;
    let mut needle = NEEDLE.to_string();
    let mut keep = KEEP.to_string();
    let mut echo = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--library" => library = args.next().map(PathBuf::from),
            "--needle" => needle = args.next().unwrap_or(needle),
            "--keep" => keep = args.next().unwrap_or(keep),
            "--echo" => echo = true,
            other => files.push(PathBuf::from(other)),
        }
    }
    let [base, other, out] = files.as_slice() else {
        eprintln!(
            "usage: redact-import-probe <base.pdf> <other.pdf> <out.pdf> \
             [--library DIR] [--needle W] [--keep W] [--echo]"
        );
        return std::process::ExitCode::from(2);
    };
    let library = library.unwrap_or_else(default_library);
    match run(&library, base, other, out, &needle, &keep, echo) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("[FAIL] {why}");
            std::process::ExitCode::from(1)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    library: &Path,
    base: &Path,
    other: &Path,
    out: &Path,
    needle: &str,
    keep: &str,
    echo: bool,
) -> Result<(), String> {
    // The file the pages come from. With `--echo` it is written beside the
    // output rather than read, so that its text is the needle by construction.
    let (source_file, echoed) = if echo {
        let made = out.with_file_name("echo-source.pdf");
        // ⚠ **Refused rather than allowed to collide**, and the collision is
        // paid for once: with the output named `echo.pdf` this file *was* the
        // output, `let _ = remove_file(out)` below deleted it, and the save
        // then refused with *"could not read echo.pdf"* --- a sentence about a
        // missing insert source, from a probe that had just written one.
        if made == out {
            return Err(format!(
                "choose another name for the output: --echo writes its own file at {}",
                made.display()
            ));
        }
        std::fs::write(&made, echo_document(needle)).map_err(|why| why.to_string())?;
        (made, true)
    } else {
        (other.to_path_buf(), false)
    };

    let bindings = bind(library)?;
    let document = OpenDocument::open(bindings, base, None).map_err(|why| why.reason)?;
    let baseline = document.page_count();

    // The rectangle a reader would have dragged, built from the character
    // boxes rather than typed in --- `redact-apply-probe`'s reason: the probe
    // is about the removal, not about whether somebody transcribed a
    // coordinate correctly.
    let region = {
        let page = document.page(0).map_err(|why| why.to_string())?;
        let extracted = text::extract(&page).map_err(|why| why.to_string())?;
        box_of(&extracted, needle)
            .ok_or_else(|| format!("{needle} is not on page 1 of {}", base.display()))?
    };
    let plans = render::redaction_plans_of(&document, 0, &[region])?;
    let planned = plans.first().ok_or("no plan came back for one region")?;
    if planned.shows.is_empty() {
        return Err("the plan names no show operator, so nothing would be removed".into());
    }
    drop(document);

    // How many pages the other file has, read through `lopdf` because the plan
    // needs the count and not the pixels.
    let incoming = Document::load(&source_file)
        .map_err(|why| format!("could not read {}: {why}", source_file.display()))?;
    let incoming_pages = tpdf_lib::pagetree::ordered_pages(&incoming).len();
    if incoming_pages == 0 {
        return Err("the other file has no pages".into());
    }
    drop(incoming);

    // **In front of the marked page**, so the page's number in the base file
    // and its slot in the output are different numbers. A writer that
    // addressed the removal by output slot would take the inserted page's
    // text instead, and the file would still be valid and still have the right
    // number of pages.
    let mut pages: Vec<PageView> = vec![PageView {
        id: 1_000,
        source: PageSource::Imported {
            source: SourceId::from_raw(1),
            page: 0,
        },
        turns: 0,
        crop: None,
    }];
    pages.extend((0..baseline).map(|at| PageView {
        id: u64::from(at),
        source: PageSource::Baseline(at),
        turns: 0,
        crop: None,
    }));

    let plan = Plan {
        baseline,
        opened_as: Some(Fingerprint::of(base)?),
        pages,
        marks: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: planned.shows.clone(),
            text_objects: planned.text_objects,
            areas: vec![planned.area],
            taking: vec![planned.taking.trim().to_string()],
            form_shows: planned.form_shows.clone(),
            form_text_objects: planned.form_text_objects.clone(),
            images: planned.images.clone(),
            image_objects: planned.image_objects,
        }],
        notes: Vec::new(),
        discards: Vec::new(),
        sources: vec![PlannedSource {
            id: 1,
            path: source_file.clone(),
            opened_as: Some(Fingerprint::of(&source_file)?),
        }],
        forms: Vec::new(),
        text_edits: Vec::new(),
    };

    // **The shipped writer, across the process boundary.** `save::Here` is
    // what every other redaction probe passes, so this is the one place the
    // removal and the import cross it together.
    let writing = save::InWorker::at(library.to_path_buf());
    let _ = std::fs::remove_file(out);
    save::write_copy(base, &plan, out, None, &writing).map_err(|why| why.message)?;

    // And the shipped verifier, across the same boundary --- the command's own
    // read-back, which is what an inserted page changes the meaning of.
    let mut written = std::fs::File::open(out).map_err(|why| why.to_string())?;
    let len = usize::try_from(written.metadata().map_err(|why| why.to_string())?.len())
        .map_err(|why| why.to_string())?;
    let needles = vec![needle.to_string(), keep.to_string()];
    let report = {
        use save::Verifier as _;
        writing.scan(&mut written, len, &needles, None)?
    };
    let inserted = plan
        .pages
        .iter()
        .filter(|page| matches!(page.source, PageSource::Imported { .. }))
        .count();
    // **From the removal's own needles, never from the control.** The command
    // scans for the words a region covered and nothing else; `keep` is here so
    // that a scan finding nothing can be told apart from a scan that could not
    // look, and feeding it to the note would fire the sentence on every run.
    // Same shape as the probe's other controls: what proves the instrument must
    // not become part of what the instrument reports.
    let removed: std::collections::BTreeSet<String> = report
        .found
        .iter()
        .filter(|one| one.as_str() == needle)
        .cloned()
        .collect();
    // **The same report with the control taken out of it, and both notes read
    // that one.** ⚠ The comment above is the whole reason this exists, and it
    // was written before `marked_pages_note` did and then not applied to it:
    // `keep` is the probe's instrument, not one of the removal's needles, and
    // it survives *on the marked page* by construction. Handing it to a note
    // that asks whether a marked page still carries a reported word makes every
    // run report a removal that did not take --- which is what happened, in
    // both runs, the first time this was wired up. The command never scans for
    // it: `redact::aggregate` pushes only what the removal takes.
    let narrowed = {
        let mut narrowed = report.clone();
        narrowed.found = removed.clone();
        narrowed.located.retain(|word, _| removed.contains(word));
        narrowed
    };
    let note = redact::inserted_pages_note(inserted, &removed, narrowed.placed());
    // Where the walk put each surviving word, and how that reads against the
    // slot the region was on. `marked_slot` is 1 below: the marked page is
    // baseline page 0 with one inserted page in front of it.
    let marked = redact::marked_pages_note(&narrowed, &[1]);
    let placed = |word: &str| match report.located.get(word) {
        Some(tpdf_lib::verify::Located::Pages(where_)) => format!(
            "{{\"kind\": \"pages\", \"pages\": [{}], \"more\": {}}}",
            where_
                .pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            where_.more
        ),
        Some(tpdf_lib::verify::Located::Shared(_)) => "{\"kind\": \"shared\"}".to_string(),
        Some(tpdf_lib::verify::Located::Unplaced) => "{\"kind\": \"unplaced\"}".to_string(),
        None => "null".to_string(),
    };
    // The reason a reader is actually shown, which is what carries the page
    // number --- a JSON field the probe built itself would agree with the probe
    // rather than with `Report::verdict`.
    let reasons: Vec<String> = match report.verdict() {
        tpdf_lib::verify::Verdict::NotVerified(why) => why,
        tpdf_lib::verify::Verdict::Verified => Vec::new(),
    };

    let escape = |value: &str| value.replace('\\', "\\\\").replace('"', "\\\"");
    let strings = |values: &[String]| {
        values
            .iter()
            .map(|one| format!("\"{}\"", escape(one)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let found: Vec<String> = report.found.iter().cloned().collect();
    println!(
        "{{\"out\": \"{}\", \"other\": \"{}\", \"echoed\": {echoed}, \"baseline\": {baseline}, \
         \"inserted_at\": 0, \"inserted\": {inserted}, \"marked_slot\": 1, \
         \"needle\": \"{}\", \"keep\": \"{}\", \"found\": [{}], \
         \"needle_found\": {}, \"keep_found\": {}, \"objects\": {}, \
         \"placed\": {}, \"needle_at\": {}, \"keep_at\": {}, \
         \"reasons\": [{}], \"marked_note\": {}, \"note\": {}}}",
        escape(&out.display().to_string()),
        escape(&source_file.display().to_string()),
        escape(needle),
        escape(keep),
        strings(&found),
        !removed.is_empty(),
        report.found.contains(keep),
        report.objects,
        narrowed.placed(),
        placed(needle),
        placed(keep),
        strings(&reasons),
        match &marked {
            Some(said) => format!("\"{}\"", escape(said)),
            None => "null".to_string(),
        },
        match &note {
            Some(said) => format!("\"{}\"", escape(said)),
            None => "null".to_string(),
        },
    );
    eprintln!(
        "[OK] wrote {} --- slot 0 came from {}, slot 1 is page 1 of {} with a region removed",
        out.display(),
        source_file.display(),
        base.display()
    );
    Ok(())
}

/// A one-page document that prints `needle` and nothing else.
///
/// **Uncompressed, so the word is findable in the bytes**, which is how the
/// check script and `verify::scan` both see it. It exists to make the
/// interesting case reachable: a page nobody marked, carrying the very word a
/// region covered somewhere else, so the whole-file scan reports a hit that is
/// not a failed removal --- and the report has to say so without claiming it.
fn echo_document(needle: &str) -> Vec<u8> {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let content = document.add_object(Stream::new(
        dictionary! {},
        format!("BT /F1 12 Tf 72 720 Td ({needle}) Tj ET").into_bytes(),
    ));
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Contents" => content,
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::from(page)], "Count" => 1,
        }),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .expect("the echo file must save");
    bytes
}

/// The box around a needle's characters, in the page's display space.
///
/// `redact-apply-probe`'s, and deliberately a second copy rather than a shared
/// helper: an example cannot import another example, and a probe that took it
/// from the library would be measuring code the library could change under it.
fn box_of(page: &text::PageText, needle: &str) -> Option<[f32; 4]> {
    let text: String = page
        .codes
        .iter()
        .filter_map(|code| char::from_u32(*code))
        .collect();
    let at = text.find(needle)?;
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

fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = progressive::bind_library(&path)
        .map_err(|why| format!("could not load Pdfium from {}: {why}", path.display()))?;
    Ok(progressive::bindings_of(bound))
}

fn default_library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join("vendor")
        .join("pdfium")
        .join(if cfg!(windows) { "bin" } else { "lib" })
}
