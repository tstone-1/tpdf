//! Synthetic raster replacement proof. Run from the repository root:
//! cargo run --manifest-path src-tauri/Cargo.toml --example raster-redact-probe
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use tpdf_lib::docmodel::PageSource;
use tpdf_lib::document::OpenDocument;
use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::progressive::{self, Bindings, CancelToken, TileSpec};

fn source() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>2,"Height"=>2,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8}, vec![230,30,40,230,30,40,230,30,40,230,30,40]));
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let metadata = doc
        .add_object(dictionary! {"Title"=>Object::string_literal("SYNTHETIC_ORIGINAL_METADATA")});
    let mut kids = Vec::new();
    for _ in 0..2 {
        let contents = doc.add_object(Stream::new(dictionary! {}, b"q 100 0 0 100 10 10 cm /I Do Q 0 1 0 rg 15 22 10 10 re f BT /F 8 Tf 12 25 Td (SYNTHETIC) Tj ET".to_vec()));
        let page = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),144.into(),144.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"I"=>image},"Font"=>dictionary!{"F"=>font}},"Contents"=>contents});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>2}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", metadata);
    doc
}

fn plan(turns: u8) -> Plan {
    Plan {
        baseline: 2,
        opened_as: None,
        pages: vec![
            PageView {
                id: 2,
                source: PageSource::Baseline(1),
                turns: 0,
                crop: None,
            },
            PageView {
                id: 1,
                source: PageSource::Baseline(0),
                turns,
                crop: Some([5.0, 10.0, 130.0, 135.0]),
            },
        ],
        marks: vec![],
        notes: vec![],
        discards: vec![],
        forms: Vec::new(),
        text_edits: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: vec![],
            text_objects: 0,
            areas: vec![[10.0, 20.0, 40.0, 45.0]],
            taking: vec![],
            images: vec![],
            image_objects: 0,
            form_shows: vec![],
            form_text_objects: vec![],
        }],
    }
}

fn pixel(
    bindings: Bindings,
    doc: &OpenDocument,
    page: u32,
    x: i32,
    y: i32,
) -> Result<Vec<u8>, String> {
    let page = doc.pdfium().page_cropped(page, None, &|_| None)?;
    let (pixels, done) = progressive::render_tile(
        bindings,
        &page,
        TileSpec {
            scale: 1.0,
            turns: 0,
            x,
            y,
            width: 1,
            height: 1,
        },
        None,
        &CancelToken::new(),
    )?;
    if !done.outcome.is_done() {
        return Err("control render incomplete".into());
    }
    Ok(pixels)
}

fn run() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("repo root")?;
    let library = root
        .join("vendor")
        .join("pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let bindings = progressive::bindings_of(progressive::bind(&library)?);
    let room = std::env::temp_dir().join(format!("tpdf-raster-probe-{}", std::process::id()));
    std::fs::create_dir(&room).map_err(|e| e.to_string())?;
    let source_path = room.join("synthetic.pdf");
    let out_path = room.join("raster.pdf");
    source().save(&source_path).map_err(|e| e.to_string())?;
    let opened = OpenDocument::open(bindings, &source_path, None).map_err(|e| e.reason)?;
    let control = pixel(bindings, &opened, 0, 20, 110)?;
    if control[..3] != [230, 30, 40] {
        return Err(format!("control pixel is not image red: {control:?}"));
    }
    for turns in 0..4 {
        let out = tpdf_lib::raster_redact::rewrite(&opened, &plan(turns)).map_err(|e| e.message)?;
        std::fs::write(&out_path, &out).map_err(|e| e.to_string())?;
        let after = OpenDocument::open(bindings, &out_path, None).map_err(|e| e.reason)?;
        // The untouched duplicate moved to output page zero. Its visible image
        // must survive even though the original shared object is not retained.
        if pixel(bindings, &after, 0, 20, 110)?[..3] != control[..3] {
            return Err("unmarked shared image occurrence changed".into());
        }
        // Independent expected coordinates for source point (25,30), after
        // crop [5,10,130,135] and each quarter turn; no production mapper used.
        let (x, y) = [(20, 105), (20, 20), (105, 20), (105, 105)][turns as usize];
        if pixel(bindings, &after, 1, x, y)?[..3] != [0, 0, 0] {
            return Err(format!("marked point survived turn {turns}"));
        }
        let (x, y) = [(55, 75), (50, 55), (70, 50), (75, 70)][turns as usize];
        if pixel(bindings, &after, 1, x, y)?[..3] != [230, 30, 40] {
            return Err(format!("unmarked point changed at turn {turns}"));
        }
        let graph = Document::load_mem(&out).map_err(|e| e.to_string())?;
        if graph.trailer.has(b"Info") || out.windows(b"SYNTHETIC".len()).any(|w| w == b"SYNTHETIC")
        {
            return Err("original text/metadata survived".into());
        }
        println!(
            "[PASS] crop + turn {turns} + reordered pages + repeated image + vector/text carriers"
        );
    }
    let mut bad = plan(0);
    bad.redactions[0].areas = vec![[200.0, 200.0, 210.0, 210.0]];
    if tpdf_lib::raster_redact::rewrite(&opened, &bad).is_ok() {
        return Err("offpage region accepted".into());
    }
    println!("[PASS] offpage region refused");
    drop(opened);
    let mut inherited = source();
    let pages = inherited.get_pages();
    let first = *pages.values().next().ok_or("synthetic first page")?;
    let parent = inherited
        .get_dictionary(first)
        .and_then(|p| p.get(b"Parent"))
        .and_then(Object::as_reference)
        .map_err(|e| e.to_string())?;
    inherited
        .get_dictionary_mut(parent)
        .map_err(|e| e.to_string())?
        .set("MediaBox", vec![0.into(), 0.into(), 144.into(), 216.into()]);
    for id in pages.values() {
        let page = inherited
            .get_dictionary_mut(*id)
            .map_err(|e| e.to_string())?;
        page.remove(b"MediaBox");
        page.set("Rotate", 90);
    }
    inherited.save(&source_path).map_err(|e| e.to_string())?;
    let opened = OpenDocument::open(bindings, &source_path, None).map_err(|e| e.reason)?;
    let mut inherited_plan = plan(0);
    for page in &mut inherited_plan.pages {
        page.crop = None;
    }
    let out = tpdf_lib::raster_redact::rewrite(&opened, &inherited_plan).map_err(|e| e.message)?;
    std::fs::write(&out_path, &out).map_err(|e| e.to_string())?;
    let after = OpenDocument::open(bindings, &out_path, None).map_err(|e| e.reason)?;
    let page = after.page(0)?;
    if (page.width_pt(), page.height_pt()) != (216.0, 144.0) {
        return Err("inherited rotated page lost its dimensions".into());
    }
    drop(after);
    drop(opened);
    println!("[PASS] inherited non-square MediaBox with intrinsic quarter-turn");
    source().save(&source_path).map_err(|e| e.to_string())?;
    let mut worker_plan = plan(2);
    worker_plan.opened_as = Some(tpdf_lib::fingerprint::Fingerprint::of(&source_path)?);
    let worker_out = room.join("worker.pdf");
    tpdf_lib::save::write_raster_copy(
        &source_path,
        &worker_plan,
        &worker_out,
        None,
        &tpdf_lib::save::InWorker::at(library.clone()),
    )
    .map_err(|e| e.message)?;
    let worker_pdf = OpenDocument::open(bindings, &worker_out, None).map_err(|e| e.reason)?;
    if worker_pdf.pdfium().page_count() != 2 {
        return Err("worker output page count changed".into());
    }
    drop(worker_pdf);
    println!("[PASS] sandboxed worker raster job + fingerprint snapshot + output channel + saved PDF readback");
    // Reuse only the synthetic encryption fixture's policy, never its objects.
    let fixture = root.join("testdata/incr-encrypted-pw.pdf");
    let mut encrypted = Document::load_with_options(
        &fixture,
        lopdf::LoadOptions {
            password: Some("swordfish".into()),
            max_decompressed_size: Some(16 * 1024 * 1024),
            ..Default::default()
        },
    )
    .map_err(|e| format!("synthetic encryption fixture: {e}"))?;
    let encryption = encrypted
        .encryption_state
        .take()
        .ok_or("fixture has no encryption state")?;
    let mut own = source();
    own.encrypt(&encryption).map_err(|e| e.to_string())?;
    own.save(&source_path).map_err(|e| e.to_string())?;
    let opened =
        OpenDocument::open(bindings, &source_path, Some("swordfish")).map_err(|e| e.reason)?;
    let out = tpdf_lib::raster_redact::rewrite(&opened, &plan(1)).map_err(|e| e.message)?;
    std::fs::write(&out_path, &out).map_err(|e| e.to_string())?;
    if OpenDocument::open(bindings, &out_path, None).is_ok() {
        return Err("raster output lost password protection".into());
    }
    let after = OpenDocument::open(bindings, &out_path, Some("swordfish")).map_err(|e| e.reason)?;
    if after.pdfium().page_count() != 2 {
        return Err("encrypted output lost pages".into());
    }
    println!("[PASS] password preserved and required, independently reopened by PDFium");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == tpdf_lib::worker::WORKER_ARGV) {
        tpdf_lib::worker_child::main(&args);
    }
    if let Err(error) = run() {
        eprintln!("[FAIL] {error}");
        std::process::exit(1)
    }
}
