//! Run `cargo run --example text-edit-probe -- <scratch-directory> [fixture.pdf]`.
//! The optional fixture must contain the same two synthetic lines and page geometry.
//! Add `--latin1` for the accented variant or `--page=N` to edit another page.
//! `--cid-latin1` uses only the accented glyphs present in the browser fixture.
//! Add `--wrapped` for the naturally wrapped fixture with literal source spaces.
//! Add `--spacers` when each line is followed by a separate single-space show.
//! Every other page must remain unchanged; page indices are zero based.
//! `--inspect <fixture.pdf>` only discovers first-page runs through the worker;
//! Add `--all-pages` to inspect every page (at most 128), including refusals.
//! It prints JSON without document text and never creates or saves a PDF.
//! Exit 0 means inspection completed (read `status`); infrastructure errors exit 1.
//! `--w3c-dummy <source.pdf> <new-output-directory>` edits the unchanged public
//! W3C test fixture; every other writing mode uses synthetic fixtures.
//! The example re-execs as its contained worker.

#[path = "../src/probes/text_edit_public.rs"]
mod public;

use std::{fs::File, path::PathBuf};

use lopdf::{dictionary, Dictionary, Document, Stream};
use tpdf_lib::{
    docmodel::PageSource,
    edits::{PageView, Plan},
    save::{self, Rewriter},
    textedit,
    worker::{self, Reply, Request, Worker},
    worker_child,
};

fn fixture() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let content = doc.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\nBT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET".to_vec()));
    let page = doc.add_object(
        dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 240.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        },
    );
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    doc
}

fn runs(worker: &mut Worker, page: u32) -> Result<textedit::PageRuns, String> {
    let reply = worker.call(&Request::TextRuns {
        page,
        changes: Vec::new(),
    })?;
    match reply.reply {
        Some(Reply::TextRuns(runs)) if reply.ok => Ok(runs),
        _ => Err(format!("text run discovery failed: {}", reply.error)),
    }
}

fn inspect_page(worker: &mut Worker, page: u32) -> Result<serde_json::Value, String> {
    let reply = worker.call(&Request::TextRuns {
        page,
        changes: Vec::new(),
    })?;
    if !reply.ok {
        Ok(serde_json::json!({"page": page, "status": "refused", "reason": reply.error}))
    } else if let Some(Reply::TextRuns(runs)) = reply.reply {
        if runs.page != page {
            return Err("text inspection returned a different page".into());
        }
        let status = if runs.runs.is_empty() {
            "no_runs"
        } else {
            "editable"
        };
        Ok(serde_json::json!({"page": page, "status": status, "runs": runs.runs.len()}))
    } else {
        Err("unexpected text inspection reply".into())
    }
}

fn inspect(source: &std::path::Path, all_pages: bool) -> Result<(), String> {
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(source, &library)?;
    let report = if all_pages {
        let opened = worker.call(&Request::Open {
            lazy_geometry: true,
        })?;
        let Some(Reply::Open { page_count, .. }) = opened.reply.filter(|_| opened.ok) else {
            return Err("could not read inspection page count".into());
        };
        let page_count = u32::try_from(page_count).map_err(|_| "page count exceeds u32")?;
        if page_count == 0 || page_count > 128 {
            return Err(
                "all-pages inspection requires 1 to 128 pages; no partial report produced".into(),
            );
        }
        let mut pages = Vec::with_capacity(page_count as usize);
        for page in 0..page_count {
            pages.push(inspect_page(&mut worker, page)?);
        }
        serde_json::json!({"page_count": page_count, "pages": pages})
    } else {
        inspect_page(&mut worker, 0)?
    };
    // Publish only after every requested page was inspected. An interrupted
    // worker must not leave a partial JSON report resembling a complete survey.
    println!("{report}");
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--w3c-dummy") {
        if args.len() != 3 {
            return Err(
                "usage: text-edit-probe --w3c-dummy <source.pdf> <new-output-directory>".into(),
            );
        }
        return public::run(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
        );
    }
    if args.first().is_some_and(|arg| arg == "--inspect") {
        if args.len() != 2 && !(args.len() == 3 && args[2] == "--all-pages") {
            return Err("usage: text-edit-probe --inspect <fixture.pdf> [--all-pages]".into());
        }
        return inspect(std::path::Path::new(&args[1]), args.len() == 3);
    }
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: text-edit-probe <scratch-directory>")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut dash = false;
    let mut latin1 = false;
    let mut cid_latin1 = false;
    let mut overhang = false;
    let mut default_encoding = false;
    let mut wrapped = false;
    let mut spacers = false;
    let mut page = 0;
    for option in std::env::args().skip(3) {
        if let Some(index) = option.strip_prefix("--page=") {
            page = index.parse::<u32>().map_err(|_| "invalid page index")?;
        } else if option == "--default-encoding" {
            default_encoding = true;
        } else if option == "--dash" {
            dash = true;
        } else if option == "--latin1" {
            latin1 = true;
        } else if option == "--overhang" {
            overhang = true;
        } else if option == "--cid-latin1" {
            cid_latin1 = true;
        } else if option == "--wrapped" {
            wrapped = true;
        } else if option == "--spacers" {
            spacers = true;
        } else {
            return Err(
                "expected --dash, --latin1, --cid-latin1, --overhang, --default-encoding, --wrapped, --spacers or --page=N after the fixture path".into(),
            );
        }
    }
    if [
        dash,
        latin1,
        cid_latin1,
        overhang,
        default_encoding,
        wrapped,
        spacers,
    ]
    .into_iter()
    .filter(|v| *v)
    .count()
        > 1
    {
        return Err("choose one fixture text variant".into());
    }
    let original = if dash {
        "SYNTHETIC\u{2013}FIRST"
    } else if default_encoding {
        "SYNTHETIC ' ` £ ß"
    } else if cid_latin1 || overhang {
        "SYNTHETIC ÄÖÜ äöü ß"
    } else if latin1 {
        "SYNTHETIC ÄÖÜ ß"
    } else {
        "SYNTHETIC FIRST"
    };
    let replacement = if dash {
        "EDITED\u{2013}FIRST"
    } else if default_encoding {
        "£ ' ` ß"
    } else if overhang {
        "ÖÄÜ äöü ß"
    } else if cid_latin1 {
        "ÄÖÜ äöü ß"
    } else if latin1 {
        "GEPRÜFT ß"
    } else {
        "EDITED FIRST"
    };
    let source = dir.join("synthetic-before.pdf");
    let target = dir.join("synthetic-after.pdf");
    // A failed run must not leave a previous run's success for PDFKit to read.
    let mut out = File::create(&target).map_err(|e| e.to_string())?;
    if let Some(input) = std::env::args().nth(2) {
        std::fs::copy(input, &source).map_err(|e| e.to_string())?;
    } else {
        fixture().save(&source).map_err(|e| e.to_string())?;
    }
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(&source, &library)?;
    let opened = worker.call(&Request::Open {
        lazy_geometry: false,
    })?;
    let Some(Reply::Open { page_count, .. }) = opened.reply.filter(|_| opened.ok) else {
        return Err("could not read source page count".into());
    };
    let page_count = u32::try_from(page_count).map_err(|_| "page count exceeds u32")?;
    if page_count == 0 || page_count > 128 || page >= page_count {
        return Err("fixture page index or count exceeds bounds".into());
    }
    let mut untouched = Vec::new();
    for index in 0..page_count {
        if index != page {
            untouched.push((index, runs(&mut worker, index)?.runs));
        }
    }
    let mapped = runs(&mut worker, page)?;
    let source_first = format!("{original}{}", if wrapped { " " } else { "" });
    let source_second = format!(
        "SYNTHETIC SECOND{}",
        if wrapped && page + 1 < page_count {
            " "
        } else {
            ""
        }
    );
    let expected = if spacers {
        vec![source_first.as_str(), " ", source_second.as_str(), " "]
    } else {
        vec![source_first.as_str(), source_second.as_str()]
    };
    if mapped
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        != expected
    {
        return Err("worker discovered incorrect text runs".into());
    }
    let mut plan = Plan {
        baseline: page_count,
        opened_as: Some(tpdf_lib::fingerprint::Fingerprint::of(&source)?),
        pages: (0..page_count)
            .map(|index| PageView {
                id: u64::from(index) + 1,
                source: PageSource::Baseline(index),
                turns: 0,
                crop: None,
            })
            .collect(),
        forms: vec![],
        marks: vec![],
        notes: vec![],
        discards: vec![],
        redactions: vec![],
        text_edits: vec![textedit::Change {
            page,
            revision: mapped.revision,
            operator: mapped.runs[0].operator,
            original: mapped.runs[0].text.clone(),
            replacement: replacement.into(),
        }],
    };
    let original_bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
    let tile = Request::Tile {
        rid: 91,
        page,
        scale: 1.0,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: 300,
        height: 240,
        png: false,
        crop: None,
    };
    let pixels = |worker: &mut Worker, request: &Request| -> Result<Vec<u8>, String> {
        let response = worker.call(request)?;
        if !response.ok {
            return Err(response.error);
        }
        Ok(worker.tile.as_slice()[..response.bytes].to_vec())
    };
    let before = pixels(&mut worker, &tile)?;
    let view = |request: Request| Request::TextView {
        changes: plan.text_edits.clone(),
        request: Box::new(request),
    };
    let preview = pixels(&mut worker, &view(tile.clone()))?;
    if before == preview {
        return Err("text preview did not change rendered pixels".into());
    }
    if before[110 * 300 * 4..] != preview[110 * 300 * 4..] {
        return Err("text preview changed the untouched second line".into());
    }
    let text = worker.call(&view(Request::Text { page, crop: None }))?;
    let Some(Reply::Text(text)) = text.reply.filter(|_| text.ok) else {
        return Err("preview extraction failed".into());
    };
    let extracted: String = text
        .codes
        .iter()
        .filter_map(|value| char::from_u32(*value))
        .collect();
    if !extracted.contains(replacement)
        || extracted.contains(original)
        || !extracted.contains("SYNTHETIC SECOND")
    {
        return Err(format!("preview extraction disagrees: {extracted}"));
    }
    for (query, count) in [(replacement, 1), (original, 0)] {
        let response = worker.call(&view(Request::Search {
            page,
            pages: vec![],
            query: query.into(),
            options: Default::default(),
            carry: None,
        }))?;
        let Some(Reply::Search(found)) = response.reply.filter(|_| response.ok) else {
            return Err("preview search failed".into());
        };
        if found.matches.len() != count {
            return Err("preview search retained stale text".into());
        }
    }
    for request in [
        Request::Open {
            lazy_geometry: false,
        },
        view(Request::Text { page, crop: None }),
    ] {
        if worker.call(&view(request))?.ok {
            return Err("text view accepted metadata or a nested wrapper".into());
        }
    }
    let mut invalid = plan.text_edits.clone();
    invalid[0].replacement = "S".repeat(80);
    if worker
        .call(&Request::TextRuns {
            page,
            changes: invalid,
        })?
        .ok
    {
        return Err("invalid draft preflight succeeded".into());
    }
    if runs(&mut worker, page)?.runs[0].text != source_first
        || pixels(&mut worker, &tile)? != before
        || std::fs::read(&source).map_err(|e| e.to_string())? != original_bytes
    {
        return Err("preview mutated the original or undo did not restore it".into());
    }
    println!("[PASS] preview pixels, extraction and search agree; undo restores source; invalid/nested views refused");
    let writer = save::InWorker::at(library.clone());
    let write = |plan: &Plan, out: &mut File, job: save::Job| -> Result<usize, String> {
        let mut input = File::open(&source).map_err(|e| e.to_string())?;
        let len = input.metadata().map_err(|e| e.to_string())?.len() as usize;
        writer
            .write(&mut input, len, out, plan, job, None)
            .map_err(|e| e.message)
    };
    let count = write(&plan, &mut out, save::Job::Save)?;
    if out.metadata().map_err(|e| e.to_string())?.len() != count as u64 {
        return Err("output length disagrees".into());
    }
    drop(out);
    let mut saved = Worker::spawn(&target, &library)?;
    let after = runs(&mut saved, page)?;
    if after.runs.len() != mapped.runs.len()
        || after.runs[0].text != replacement
        || after.runs[1..] != mapped.runs[1..]
    {
        return Err("worker rewrite changed the wrong text".into());
    }
    for (index, original_runs) in untouched {
        if runs(&mut saved, index)?.runs != original_runs {
            return Err("editing changed runs on another page".into());
        }
    }
    println!("[PASS] all other pages retain their original runs");
    println!("[PASS] contained discovery and replacement; second text block preserved");
    let mut invalid_replacements = vec![
        ("S".repeat(80), "exceed the original"),
        ("\u{03b1}".into(), "Latin-1 and en dash only"),
    ];
    if default_encoding {
        invalid_replacements.push(("Ä".into(), "no validated glyph"));
    }
    if overhang {
        invalid_replacements.push(("ÄÖÜ äöü ß".into(), "replacement ink"));
    }
    for (invalid_text, reason) in invalid_replacements {
        plan.text_edits[0].replacement = invalid_text;
        let mut rejected =
            File::create(dir.join("synthetic-refused.pdf")).map_err(|e| e.to_string())?;
        let error = write(&plan, &mut rejected, save::Job::Save)
            .expect_err("invalid replacement was accepted");
        if !error.contains(reason) || rejected.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err(format!("wrong refusal or partial output: {error}"));
        }
    }
    println!("[PASS] overflow and unsupported characters refused without output");
    plan.text_edits[0].replacement = replacement.into();
    plan.redactions.push(tpdf_lib::edits::PlannedRedaction {
        source: 0,
        shows: vec![],
        text_objects: 0,
        areas: vec![[0.0, 0.0, 10.0, 10.0]],
        taking: vec![],
        images: vec![],
        image_objects: 0,
        form_shows: vec![],
        form_text_objects: vec![],
    });
    let mut rejected =
        File::create(dir.join("synthetic-refused.pdf")).map_err(|e| e.to_string())?;
    let error = write(&plan, &mut rejected, save::Job::RasterRedact)
        .expect_err("mixed text/raster redaction accepted");
    if !error.contains("save text edits")
        || rejected.metadata().map_err(|e| e.to_string())?.len() != 0
    {
        return Err(format!("wrong raster refusal or partial output: {error}"));
    }
    println!("[PASS] raster redaction refuses pending text edits");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }
    if let Err(error) = run() {
        eprintln!("[FAIL] {error}");
        std::process::exit(1);
    }
}
