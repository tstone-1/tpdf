//! Does the redaction gate certify a clean file and refuse a dirty one?
//!
//! `docs/PLAN.md` §6 step 4 is wired into `redact_copy` and `redact_document`,
//! and neither is reachable from a unit test: they are Tauri commands, and the
//! join between them and [`tpdf_lib::ocr_gate`] is exactly the layer
//! `docs/TRAPS.md` records as *a feature can be inert in the application while
//! three layers of tests pass*. This drives the real function against a real
//! render service, a real worker and a real engine.
//!
//! **The control is the same gate run against the file that was not redacted.**
//! A gate that certifies everything passes "the redacted file has no reasons"
//! perfectly, so that row on its own is worth nothing; the source file, with the
//! same regions and the same words, has to come back *legible*. One variable
//! between the two runs --- which file --- and it is the one under test.
//!
//! A third row asks whether the control rule is live at all, by handing the gate
//! a page it knows no words for: a control it cannot choose must be *not
//! verified* rather than a clean answer, since a page nothing was read on is
//! also a page nothing survived on.

use std::path::{Path, PathBuf};
use std::time::Instant;

use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::ocr_gate::{self, GatePage};
use tpdf_lib::render::{Backend, RenderService};
use tpdf_lib::{save, worker, worker_child};

#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
            println!("[OK]   {name}   {}", detail.as_ref());
        } else {
            self.failed += 1;
            println!("[FAIL] {name}   {}", detail.as_ref());
        }
    }

    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.skipped += 1;
        println!("[SKIP] {name}   {}", why.as_ref());
    }

    fn finish(&self) -> ! {
        println!(
            "\n{}/{} checks passed, {} skipped",
            self.passed,
            self.passed + self.failed,
            self.skipped
        );
        std::process::exit(i32::from(self.failed != 0));
    }
}

pub fn main() {
    // This binary is also both workers: each spawn re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }
    #[cfg(target_os = "macos")]
    if args
        .iter()
        .any(|a| a == tpdf_lib::ocr_worker::OCR_WORKER_ARGV)
    {
        tpdf_lib::ocr_worker::child_main();
    }

    let mut file: Option<PathBuf> = None;
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--lib" => library = PathBuf::from(rest.next().cloned().unwrap_or_default()),
            other if !other.starts_with("--") && file.is_none() => {
                file = Some(PathBuf::from(other))
            }
            _ => {}
        }
    }
    let Some(file) = file else {
        eprintln!("usage: redact-gate-probe <file.pdf> [--lib <dir>]");
        std::process::exit(2);
    };
    if let Err(e) = run(&file, &library) {
        eprintln!("[ERROR] {e}");
        std::process::exit(2);
    }
}

fn run(file: &Path, library: &Path) -> Result<(), String> {
    let mut r = Report::default();
    let service = RenderService::start_with(library.to_path_buf(), Backend::Worker);
    let opened = wait(|reply| service.open(file.to_path_buf(), false, None, reply))
        .map_err(|e: tpdf_lib::progressive::Refusal| e.reason)?;
    let text = wait(|reply| service.text(opened.id, 0, None, reply))?;
    let words = ocr_gate::words_from(&text);
    println!("document   {}", file.display());
    println!(
        "page 0     {:.0} x {:.0} pt, {} characters, {} words",
        text.width_pt,
        text.height_pt,
        text.codes.len(),
        words.len()
    );

    // The longest word on the page, which leaves the most for a control to be
    // chosen from and gives the engine something substantial to fail to read.
    let Some(target) = words
        .iter()
        .filter(|w| w.text.chars().count() >= 6)
        .max_by_key(|w| w.text.chars().count())
        .cloned()
    else {
        r.skip(
            "this page has a word long enough to redact",
            "no word of six characters or more",
        );
        r.finish();
    };
    // Inflated by a point, so the region covers the word rather than tracing it.
    // A region flush to a glyph box is the boundary `redact::overlaps` is most
    // likely to be wrong at, and this probe is not about that boundary.
    let region = [
        target.rect[0] - 1.0,
        target.rect[1] - 1.0,
        target.rect[2] + 1.0,
        target.rect[3] + 1.0,
    ];
    println!("redacting  {:?} at {:?} pt", target.text, region);

    let plans = wait(|reply| service.redaction_plans(opened.id, 0, vec![region], reply))?;
    let Some(plan_for_region) = plans.first().cloned() else {
        r.skip("the region has a removal plan", "the worker returned none");
        r.finish();
    };
    if plan_for_region.shows.is_empty() {
        r.skip(
            "the region has a removal plan",
            format!("nothing to remove under {:?}", target.text),
        );
        r.finish();
    }

    let out = std::env::temp_dir().join(format!("tpdf-gate-probe-{}.pdf", std::process::id()));
    let plan = Plan {
        opened_as: None,
        baseline: opened.page_count as u32,
        pages: (0..opened.page_count)
            .map(|at| PageView {
                id: at as u64 + 1,
                source: at as u32,
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: plan_for_region.shows.clone(),
            text_objects: plan_for_region.text_objects,
            areas: vec![plan_for_region.area],
            taking: vec![plan_for_region.taking.clone()],
        }],
    };
    save::write_copy(file, &plan, &out).map_err(|e| e.message)?;

    let page = GatePage {
        page: 0,
        regions: vec![region],
        words: words.clone(),
        taking: plan_for_region.taking.clone(),
        width_pt: text.width_pt,
        height_pt: text.height_pt,
    };

    // Whether the gate can speak about this page at all, asked before anything
    // is measured. A page every region covers has nothing left to read a control
    // back from, so *not verified* is the correct answer and there is no gate
    // behaviour here to check --- `encodings.pdf` is one text object for the
    // whole page and is exactly that case.
    let survivors = ocr_gate::surviving(&page.words, &page.regions, &page.taking);
    if let Err(why) = tpdf_lib::ocr::control_from_page(&survivors, &page.regions) {
        r.skip("this page leaves a control to read back", format!("{why}"));
        let _ = std::fs::remove_file(&out);
        r.finish();
    }

    // Independent of the gate, and about the *region* rather than the file: the
    // pixels there are not the pixels they were. Without it, "the redacted file
    // has no reasons" could be satisfied by a write that changed nothing.
    //
    // **Not a byte scan for the words**, which is the check this had first and
    // which `redact_apply_probe` records making the same mistake a day earlier:
    // `text-marked.pdf` carries the same line four times and one of the copies
    // is an annotation the removal is right to keep, so the needle is still in
    // the file and nothing is wrong.
    let before = render_region(&service, opened.id, region, &text)?;
    let reopened = wait(|reply| service.open(out.clone(), false, None, reply))
        .map_err(|e: tpdf_lib::progressive::Refusal| e.reason)?;
    let after = render_region(&service, reopened.id, region, &text)?;
    let _: Result<(), String> = wait(|reply| service.close(reopened.id, reply));
    r.check(
        before != after && before.len() == after.len(),
        "the removal changed the pixels inside the region",
        format!("{} bytes rendered either side", before.len()),
    );

    // ------------------------------------------------------ no engine at all
    if let Err(why) = tpdf_lib::ocr_worker::OcrWorker::spawn() {
        let reasons = ocr_gate::run(&service, &out.to_string_lossy(), None, &[page]);
        r.check(
            reasons.len() == 1 && reasons[0].contains("OCR engine"),
            "a platform with no engine says so once, not once per region",
            format!("{} reason(s): {:?}", reasons.len(), reasons.first()),
        );
        let _ = std::fs::remove_file(&out);
        let _ = why;
        r.finish();
    }

    // ---------------------------------------------------------- the control
    // The same gate, the same regions, the same words --- against the file that
    // was never redacted. It has to come back legible, or the row below means
    // nothing.
    let started = Instant::now();
    let dirty = ocr_gate::run(
        &service,
        &file.to_string_lossy(),
        None,
        std::slice::from_ref(&page),
    );
    let control_ms = started.elapsed().as_secs_f32() * 1000.0;
    r.check(
        dirty.iter().any(|w| w.contains("still reads")),
        "the control: the unredacted file is reported legible",
        format!(
            "{} reason(s) in {control_ms:.0} ms: {:?}",
            dirty.len(),
            dirty.first()
        ),
    );
    let quoted = target.text.trim();
    r.check(
        dirty.iter().any(|w| w.contains(quoted)),
        "the control names the word that is still there",
        format!("looking for {quoted:?}"),
    );

    // ------------------------------------------------------ the written file
    let started = Instant::now();
    let clean = ocr_gate::run(
        &service,
        &out.to_string_lossy(),
        None,
        std::slice::from_ref(&page),
    );
    let clean_ms = started.elapsed().as_secs_f32() * 1000.0;
    r.check(
        clean.is_empty(),
        "the redacted file is certified",
        format!(
            "{} reason(s) in {clean_ms:.0} ms: {:?}",
            clean.len(),
            clean.first()
        ),
    );

    // -------------------------------------------------------- the control rule
    // A page the gate knows no words for cannot yield a control, and a region it
    // could not read a control back from must be *not verified* --- never clean,
    // which is what a page nothing was read on otherwise looks like.
    let blind = GatePage {
        words: Vec::new(),
        ..page.clone()
    };
    let refused = ocr_gate::run(&service, &out.to_string_lossy(), None, &[blind]);
    r.check(
        refused.len() == 1 && refused[0].contains("could not be shown unreadable"),
        "a page with no control is refused rather than certified",
        format!("{} reason(s): {:?}", refused.len(), refused.first()),
    );

    let _ = std::fs::remove_file(&out);
    let _: Result<(), String> = wait(|reply| service.close(opened.id, reply));
    r.finish();
}

/// The region's own rows, rendered from one open document.
fn render_region(
    service: &RenderService,
    doc: u32,
    region: [f32; 4],
    text: &tpdf_lib::text::PageText,
) -> Result<Vec<u8>, String> {
    const SCALE: f32 = 2.0;
    let height_px = (text.height_pt * SCALE).ceil() as u32;
    let (top, rows) = ocr_gate::rows_of(region, height_px, SCALE)
        .ok_or_else(|| "the region is not on the page".to_string())?;
    let request = tpdf_lib::render::TileRequest {
        rid: 0,
        doc,
        page: 0,
        scale: SCALE,
        turns: 0,
        invert: false,
        x: 0,
        y: i32::try_from(top).unwrap_or(i32::MAX),
        width: u16::try_from((text.width_pt * SCALE).ceil() as u32).unwrap_or(u16::MAX),
        height: u16::try_from(rows).unwrap_or(u16::MAX),
        format: tpdf_lib::render::TileFormat::Raw,
        crop: None,
    };
    match wait(|reply| service.tile(request, reply))? {
        tpdf_lib::render::TileOutcome::Rendered(tile) => Ok(tile.bytes),
        tpdf_lib::render::TileOutcome::Abandoned => Err("the render was abandoned".into()),
    }
}

/// Drives one of the render service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(result) => result,
        Err(_) => Err(E::from("the render service did not answer".to_string())),
    }
}
