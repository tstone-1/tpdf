//! Run `cargo run --example text-edit-probe -- <scratch-directory>`.
//! Creates synthetic PDFs only. The example re-execs as its contained worker.

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

fn runs(worker: &mut Worker) -> Result<textedit::PageRuns, String> {
    let reply = worker.call(&Request::TextRuns { page: 0 })?;
    match reply.reply {
        Some(Reply::TextRuns(runs)) if reply.ok => Ok(runs),
        _ => Err(format!("text run discovery failed: {}", reply.error)),
    }
}

fn run() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: text-edit-probe <scratch-directory>")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let source = dir.join("synthetic-before.pdf");
    let target = dir.join("synthetic-after.pdf");
    // A failed run must not leave a previous run's success for PDFKit to read.
    let mut out = File::create(&target).map_err(|e| e.to_string())?;
    fixture().save(&source).map_err(|e| e.to_string())?;
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(&source, &library)?;
    let mapped = runs(&mut worker)?;
    if mapped.runs.len() != 2
        || mapped.runs[0].text != "SYNTHETIC FIRST"
        || mapped.runs[0].operator != 3
    {
        return Err("worker discovered incorrect text runs".into());
    }
    let mut plan = Plan {
        baseline: 1,
        opened_as: Some(tpdf_lib::fingerprint::Fingerprint::of(&source)?),
        pages: vec![PageView {
            id: 1,
            source: PageSource::Baseline(0),
            turns: 0,
            crop: None,
        }],
        forms: vec![],
        marks: vec![],
        notes: vec![],
        discards: vec![],
        redactions: vec![],
        text_edits: vec![textedit::Change {
            page: 0,
            revision: mapped.revision,
            operator: 3,
            original: mapped.runs[0].text.clone(),
            replacement: "EDITED FIRST".into(),
        }],
    };
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
    let after = runs(&mut saved)?;
    if after.runs.len() != 2
        || after.runs[0].text != "EDITED FIRST"
        || after.runs[1].text != "SYNTHETIC SECOND"
    {
        return Err("worker rewrite changed the wrong text".into());
    }
    println!("[PASS] contained discovery and replacement; second text block preserved");
    for (replacement, reason) in [
        ("Z".repeat(80), "exceed the original"),
        ("\u{03b1}".into(), "ASCII only"),
    ] {
        plan.text_edits[0].replacement = replacement;
        let mut rejected =
            File::create(dir.join("synthetic-refused.pdf")).map_err(|e| e.to_string())?;
        let error = write(&plan, &mut rejected, save::Job::Save)
            .expect_err("invalid replacement was accepted");
        if !error.contains(reason) || rejected.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err(format!("wrong refusal or partial output: {error}"));
        }
    }
    println!("[PASS] overflow and unsupported characters refused without output");
    plan.text_edits[0].replacement = "EDITED FIRST".into();
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
