//! Synthetic proof that a black finish follows real content removal.
//! cargo run --manifest-path src-tauri/Cargo.toml --example redaction-fill-probe
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use tpdf_lib::docmodel::PageSource;
use tpdf_lib::document::OpenDocument;
use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::fingerprint::Fingerprint;
use tpdf_lib::progressive::{self, Bindings, CancelToken, TileSpec};
use tpdf_lib::save::{self, InWorker};

fn source() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let zero = doc.add_object(dictionary! {"Type"=>"ExtGState","ca"=>0,"CA"=>0,"BM"=>"Multiply"});
    let mut kids = Vec::new();
    for slot in 0..2 {
        let words = if slot == 0 {
            "BT /F 8 Tf 12 25 Td (TARGET) Tj ET BT /F 8 Tf 70 95 Td (RETAIN) Tj ET"
        } else {
            "BT /F 8 Tf 20 95 Td (OTHER) Tj ET"
        };
        // Any ordinary stream appended afterwards inherits this hostile state:
        // a shifted/shrunken CTM, tiny clip and zero alpha. Annotation AP must
        // paint independently of all three.
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("{words} q 0.1 0 0 0.1 200 200 cm 0 0 1 1 re W n /Zero gs").into_bytes(),
        ));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),144.into(),144.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F"=>font},"ExtGState"=>dictionary!{"Zero"=>zero}},"Contents"=>content});
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>2}.into(),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
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
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: vec![0],
            text_objects: 2,
            areas: vec![[10.0, 20.0, 60.0, 40.0]],
            taking: vec!["TARGET".into()],
            images: vec![],
            image_objects: 0,
            form_shows: vec![],
            form_text_objects: vec![],
        }],
    }
}

fn text(document: &OpenDocument, slot: u32) -> Result<String, String> {
    let page = document.page(slot)?;
    Ok(tpdf_lib::text::extract(&page)?
        .codes
        .into_iter()
        .filter_map(char::from_u32)
        .collect())
}

fn pixel(
    bindings: Bindings,
    document: &OpenDocument,
    slot: u32,
    x: i32,
    y: i32,
) -> Result<[u8; 3], String> {
    let page = document.page(slot)?;
    let (bytes, done) = progressive::render_tile(
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
        return Err("pixel readback did not complete".into());
    }
    Ok([bytes[0], bytes[1], bytes[2]])
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
    let worker = InWorker::at(library);
    let room = std::env::temp_dir().join(format!("tpdf-fill-probe-{}", std::process::id()));
    std::fs::create_dir(&room).map_err(|e| e.to_string())?;
    let original = room.join("synthetic.pdf");
    source().save(&original).map_err(|e| e.to_string())?;
    let before = OpenDocument::open(bindings, &original, None).map_err(|e| e.reason)?;
    if !text(&before, 0)?.contains("TARGET") {
        return Err("target absent from input control".into());
    }
    drop(before);
    for turns in 0..4 {
        let path = room.join(format!("turn-{turns}.pdf"));
        let plan = plan(turns);
        save::write_copy(&original, &plan, &path, None, &worker).map_err(|e| e.message)?;
        let uncovered = OpenDocument::open(bindings, &path, None).map_err(|e| e.reason)?;
        let extracted = text(&uncovered, 1)?;
        if extracted.contains("TARGET") || !extracted.contains("RETAIN") {
            return Err("surgical removal did not preserve only outside text".into());
        }
        // Independent crop/quarter-turn locations for source point (25,30).
        let (x, y) = [(20, 105), (20, 20), (105, 20), (105, 105)][turns as usize];
        if pixel(bindings, &uncovered, 1, x, y)? != [255, 255, 255] {
            return Err("uncovered removal was not white".into());
        }
        drop(uncovered);
        let expected = Fingerprint::of(&path)?;
        save::fill_redactions(&path, &plan, &expected, None, &worker).map_err(|e| e.message)?;
        let filled = OpenDocument::open(bindings, &path, None).map_err(|e| e.reason)?;
        if pixel(bindings, &filled, 1, x, y)? != [0, 0, 0] {
            return Err(format!(
                "fill not black under hostile graphics state at turn {turns}"
            ));
        }
        let extracted = text(&filled, 1)?;
        if extracted.contains("TARGET")
            || !extracted.contains("RETAIN")
            || !text(&filled, 0)?.contains("OTHER")
        {
            return Err("fill changed selectable outside text".into());
        }
        drop(filled);
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if save::fill_redactions(&path, &plan, &expected, None, &worker).is_ok() {
            return Err("stale verification fingerprint accepted".into());
        }
        if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
            return Err("stale fill changed output".into());
        }
        println!("[PASS] turn{turns}: removed text absent before fill, white->black, outside text selectable, crop/reorder, hostile CTM/clip/alpha, stale fingerprint refused");
    }
    let mut encrypted = Document::load_with_options(
        root.join("testdata/incr-encrypted-pw.pdf"),
        lopdf::LoadOptions {
            password: Some("swordfish".into()),
            max_decompressed_size: Some(16 * 1024 * 1024),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let encryption = encrypted
        .encryption_state
        .take()
        .ok_or("synthetic fixture encryption")?;
    let mut original_doc = source();
    original_doc
        .encrypt(&encryption)
        .map_err(|e| e.to_string())?;
    let encrypted_original = room.join("encrypted-source.pdf");
    original_doc
        .save(&encrypted_original)
        .map_err(|e| e.to_string())?;
    let encrypted_out = room.join("encrypted-filled.pdf");
    let plan = plan(1);
    save::write_copy(
        &encrypted_original,
        &plan,
        &encrypted_out,
        Some("swordfish"),
        &worker,
    )
    .map_err(|e| e.message)?;
    let expected = Fingerprint::of(&encrypted_out)?;
    save::fill_redactions(&encrypted_out, &plan, &expected, Some("swordfish"), &worker)
        .map_err(|e| e.message)?;
    if OpenDocument::open(bindings, &encrypted_out, None).is_ok() {
        return Err("fill lost encryption".into());
    }
    let filled =
        OpenDocument::open(bindings, &encrypted_out, Some("swordfish")).map_err(|e| e.reason)?;
    if pixel(bindings, &filled, 1, 20, 20)? != [0, 0, 0] || !text(&filled, 1)?.contains("RETAIN") {
        return Err("encrypted fill content mismatch".into());
    }
    println!("[PASS] encrypted worker fill keeps password required, black region and selectable outside text");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == tpdf_lib::worker::WORKER_ARGV) {
        tpdf_lib::worker_child::main(&args)
    }
    if let Err(error) = run() {
        eprintln!("[FAIL] {error}");
        std::process::exit(1)
    }
}
