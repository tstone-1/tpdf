//! Does removing a region actually remove the words, and only those words?
//!
//! `src/redact.rs` is asserted against hand-built content streams, which is the
//! right shape for *which operator gets deleted* and says nothing about a real
//! document: a fixture agrees with whatever its author had in mind. This is the
//! corpus control. It drives the same two functions against real files, through
//! PDFium, and asks `verify::scan` --- the redaction verifier --- whether the
//! words are gone from the **file** rather than from the page.
//!
//! ## What it asserts, and the control is the harder half
//!
//! For each fixture and each target word:
//!
//! * the word is in the file to begin with (else the check cannot fail),
//! * after the removal `verify::scan` no longer finds it,
//! * **a word on another line is still there** --- the over-redaction control,
//!   without which a redaction that emptied the page would pass perfectly,
//! * PDFium still opens the result and still reports the page,
//! * and `verify::structure` accepts the bytes.
//!
//! ## Route B eats the line, and that is the measurement rather than a caveat
//!
//! `docs/PLAN.md` §6 route B removes the whole text-showing operation
//! containing any redacted glyph, so neighbouring words on the same operator go
//! with it. The control word is therefore chosen from a **different line**, and
//! the run prints how much of the page's text went so the cost is a number
//! rather than a footnote.
//!
//! Usage:
//!     cargo run --release --example redact-probe [-- --library DIR]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lopdf::Document;
use pdfium_render::prelude::*;
use tpdf_lib::{redact, verify};

/// One fixture, a word to remove, and a word that must survive.
///
/// The survivor is on a different line on purpose: route B removes the whole
/// operation, so a word beside the target goes with it and would make this
/// check fail for a redaction that was working correctly.
struct Case {
    file: &'static str,
    page: usize,
    remove: &'static str,
    keep: &'static str,
}

const CASES: &[Case] = &[
    // The fixture built for this: `make_text_pdf.py` writes a line reading
    // "REDACT ME: account 4711-0815 belongs to A. Beispiel." beside three
    // unrelated ones.
    Case {
        file: "text-base14.pdf",
        page: 0,
        remove: "4711-0815",
        keep: "Sphinx of black quartz",
    },
    // Several pages drawing the same words, which is why the needle names its
    // page: the first run of this probe marked a word that lives on all eight
    // pages, removed it from one, and correctly reported it still in the file.
    Case {
        file: "links.pdf",
        page: 0,
        remove: "Line 01 of page 1",
        keep: "Line 08 of page 1",
    },
    // Text inside an optional content group whose default state is OFF, which
    // reaches the one branch of the marked-content handler nothing else here
    // does: the span is written `/OC /MC0 BDC`, so its property list is a
    // **name** into the page's `/Properties` rather than an inline dictionary,
    // and `property_list` resolves it to a real `/Type /OCG` dictionary a parser
    // produced. Every other exercise of that branch builds its dictionary in
    // Rust. It is allowed through rather than refused because the OCG carries
    // `/Name` and none of `SHADOW_TEXT`, which is the distinction the refusal is
    // drawn on.
    //
    // **That this case can exist at all is a measurement, taken 2026-09-02.**
    // PDFium's text extraction returns the words of a group that is off --- 163
    // characters for this page, the needle among them --- so the region can be
    // built from character boxes exactly like every other case here. The
    // opposite answer was the one expected, and it would have meant a redaction
    // flow that finds words through PDFium cannot offer to remove text a reader
    // never sees. Worth keeping in view: extraction ignoring `/OC` while a
    // renderer honours it is what makes *searchable but not drawn* possible, and
    // §6's whole premise is that a file holds more than a page shows.
    //
    // **It is the only case here that reaches that branch, shown rather than
    // claimed.** Forcing the refusal --- `SHADOW_TEXT.into_iter().find(|_key|
    // true)`, which is a mutation `scripts/mutate_rust.py` already carries for
    // the unit test --- turns this case red and leaves `text-base14.pdf` and
    // `links.pdf` green. The two of them write their spans with an *inline*
    // dictionary, so neither ever reaches the name arm at all. A new case that
    // reddens with everything else would have added a document rather than
    // coverage.
    Case {
        file: "hostile-ocg.pdf",
        page: 0,
        remove: "TPDF-NEEDLE-OCGHIDDEN-4711-0815",
        keep: "no reader draws it",
    },
];

/// The same line, in a document that keeps three copies of it.
///
/// `text-marked.pdf` is `text-base14.pdf` plus the carriers spike 0.3 was built
/// to find, and it holds the line **three** times: in a marked-content
/// property list as `/ActualText`, in a text annotation's `/Contents`, and in
/// `/Info /Title`. Removing the show operator takes the *drawing* away and
/// leaves all three, which is `docs/PLAN.md` §6's whole thesis --- redaction is
/// whole-graph sanitation and not a page edit --- stated as a measurement rather
/// than as a paragraph.
///
/// **Since 2026-08-27 the first of the three goes, and that is why this check
/// had to change shape.** It used to assert one thing: that `verify::scan` still
/// finds the word afterwards. Three carriers and one needle means that assertion
/// is satisfied by *any* of them surviving --- so it stayed green when
/// `/ActualText` started being cleared, and its own doc comment had promised it
/// would go red on exactly that day. A check whose observable several mechanisms
/// can produce cannot say which one it measured; `docs/TRAPS.md` records that
/// shape from three other directions.
///
/// So it now reads the carriers apart: the property list must be **gone from the
/// content stream**, with a control proving it was there, while the scan must
/// still find the word --- which by then can only be the annotation and `/Info`.
/// Both directions go red for their own reason, and the day the document-level
/// carriers are cleared too, the second one says so and this moves into `CASES`.
const CARRIED: (&str, usize, &str) = ("text-marked.pdf", 0, "4711-0815");

/// A page where the region covers something this cannot remove.
///
/// §6's deny-by-default rule: a picture in the region is a verification failure
/// rather than a shrug, because removing the words while leaving a picture of
/// the words is the confident lie the section opens by forbidding. The whole
/// page is marked, so the `/DCTDecode` image is certainly inside it.
const INCOMPLETE: (&str, usize) = ("hostile-scan.pdf", 0);

/// A document whose text no byte scan can find, and the reason it is here.
///
/// `text-cid.pdf` draws the same line as `text-base14.pdf` through an embedded
/// subsetted CIDFontType2 under Type0 / Identity-H, so the file holds glyph ids
/// and not characters and has **zero** literal show strings. `verify::scan` therefore cannot see a word PDFium
/// extracts perfectly well, which is spike 0.3's finding --- *a byte-level leak
/// scan cannot verify a CID document* --- arriving in the instrument this probe
/// is built on.
///
/// Asserted rather than left out, because a probe that quietly skips the case it
/// cannot handle reads as one that handles everything. What is checked is the
/// blindness itself: PDFium finds the word, the scan does not. If that ever
/// stops being true the scan has grown a capability and this probe's own limits
/// need re-reading.
const BLIND: (&str, &str) = ("text-cid.pdf", "4711-0815");

fn main() -> ExitCode {
    let library = std::env::args()
        .skip_while(|arg| arg != "--library")
        .nth(1)
        .map_or_else(default_library, PathBuf::from);
    let bound =
        match Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&library)) {
            Ok(bound) => bound,
            Err(why) => {
                println!("[SKIP] no PDFium at {}: {why}", library.display());
                return ExitCode::from(2);
            }
        };
    let pdfium = Pdfium::new(bound);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let scratch = root.join("target").join("redact-probe");
    let _ = std::fs::remove_dir_all(&scratch);
    if std::fs::create_dir_all(&scratch).is_err() {
        println!("[FAIL] could not make a scratch directory");
        return ExitCode::FAILURE;
    }

    let mut ran = 0usize;
    let mut failures = 0usize;
    for case in CASES {
        let source = root.join("testdata").join(case.file);
        if !source.exists() {
            println!("[SKIP] {} not generated", case.file);
            continue;
        }
        match run(&pdfium, case, &source, &scratch) {
            Ok(note) => {
                ran += 1;
                println!("[OK]   {}: {note}", case.file);
            }
            Err(why) => {
                failures += 1;
                println!("[FAIL] {}: {why}", case.file);
            }
        }
    }

    match carried(&pdfium, &root, &scratch) {
        Ok(note) => println!("[OK]   {}: {note}", CARRIED.0),
        Err(why) => {
            failures += 1;
            println!("[FAIL] {}: {why}", CARRIED.0);
        }
    }
    match incomplete(&pdfium, &root) {
        Ok(note) => println!("[OK]   {}: {note}", INCOMPLETE.0),
        Err(why) => {
            failures += 1;
            println!("[FAIL] {}: {why}", INCOMPLETE.0);
        }
    }
    match blind_spot(&pdfium, &root) {
        Ok(note) => println!("[OK]   {}: {note}", BLIND.0),
        Err(why) => {
            failures += 1;
            println!("[FAIL] {}: {why}", BLIND.0);
        }
    }

    // A run that opened nothing reports success exactly like a clean one, and a
    // fresh checkout has no fixtures at all.
    if ran == 0 {
        println!("[FAIL] no case ran, so nothing was checked");
        failures += 1;
    }
    println!("[INFO] {ran} case(s) ran, {failures} failure(s)");
    if failures == 0 {
        println!("[OK] every marked word left the file and every control word stayed.");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Removing the drawing is not enough when the words are also somewhere else.
///
/// See [`CARRIED`]. Returns a note when the show operator went and the words
/// stayed; a failure when either half stops being true.
fn carried(pdfium: &Pdfium, root: &Path, scratch: &Path) -> Result<String, String> {
    let (file, page, needle) = CARRIED;
    let source = root.join("testdata").join(file);
    if !source.exists() {
        return Ok(format!("[SKIP] {file} not generated"));
    }
    let case = Case {
        file,
        page,
        remove: needle,
        keep: needle,
    };
    let region = region_of(pdfium, &source, &case)?;
    let objects = page_objects(pdfium, &source, page)?;
    let text_objects = objects.iter().filter(|o| o.kind == "text").count();
    let plan = redact::covered(&objects, &[], region);
    if plan.shows.is_empty() {
        return Err("the region covers no text object".to_string());
    }
    let mut doc = Document::load(&source).map_err(|why| why.to_string())?;
    let page_id = *doc
        .get_pages()
        .get(&(page as u32 + 1))
        .ok_or("no such page")?;

    // The control for the half below. Without it, "the property list is gone"
    // passes on a fixture that never had one --- which is the same property this
    // repository has been caught by whenever an assertion held by construction.
    let before = page_stream(&doc, page_id)?;
    if !before.contains("ActualText") || !before.contains(needle) {
        return Err(format!(
            "{file} page {page} does not carry the line in a marked-content property list, so \
             the carrier check below cannot fail. Read testdata/make_text_pdf.py."
        ));
    }

    let removed = redact::remove_shows(&mut doc, page_id, &plan.shows, text_objects)?;
    let after_stream = page_stream(&doc, page_id)?;
    if removed.struct_carriers == 0 {
        return Err(
            "the removal cleared no structure element, and this fixture has one on the \
             redacted line's /MCID"
                .to_string(),
        );
    }
    if removed.carriers == 0 {
        return Err("the removal reported clearing no carrier at all".to_string());
    }
    if after_stream.contains(needle) {
        return Err(format!(
            "{needle:?} is still in the page's content stream after {} carrier(s) were \
             reported cleared: {after_stream}",
            removed.carriers
        ));
    }

    let bytes = tpdf_lib::save::serialise(&mut doc, "the redacted document")?;
    std::fs::write(scratch.join(format!("{file}.redacted.pdf")), &bytes)
        .map_err(|why| why.to_string())?;

    let after = verify::scan(&bytes, &[needle.to_string()], None);
    if !after.found.contains(needle) {
        return Err(format!(
            "{needle:?} is gone from this file too. That would mean the document-level carriers \
             -- the annotation's /Contents and /Info /Title -- are being cleared as well; read \
             docs/PLAN.md §6 and move this into CASES."
        ));
    }
    Ok(format!(
        "{} show operator(s) removed, {} shadow-text key(s) cleared from the content stream, \
         and {needle:?} is STILL in the file -- the annotation and /Info keep it, which is why \
         §6 calls redaction whole-graph sanitation",
        removed.removed, removed.carriers
    ))
}

/// A page's content stream as stored, which is where a marked-content carrier lives.
///
/// The decoded operation list cannot answer this: a property list is an
/// *operand*, so a check reading operators sees nothing either way.
fn page_stream(doc: &Document, page: lopdf::ObjectId) -> Result<String, String> {
    let data = doc
        .get_page_content_with_limit(page, redact::MAX_CONTENT_BYTES)
        .map_err(|why| format!("the page's content stream could not be read: {why}"))?;
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// A region over a drawing is not redactable by this module, and says so ---
/// while the picture beside it now is.
///
/// **This checked an image until 2026-08-27**, when a region over a picture
/// stopped being a refusal and became a removal. It went on passing, on the two
/// **path** objects the same page carries: the check's subject moved and nothing
/// went red, which is the shape `docs/TRAPS.md` records more than once. So it
/// now asserts both halves --- the paths are still reported, and the image is
/// named as removable --- and a change to either direction reddens it.
fn incomplete(pdfium: &Pdfium, root: &Path) -> Result<String, String> {
    let (file, page) = INCOMPLETE;
    let source = root.join("testdata").join(file);
    if !source.exists() {
        return Ok(format!("[SKIP] {file} not generated"));
    }
    let objects = page_objects(pdfium, &source, page)?;
    if !objects.iter().any(|object| object.kind == "image") {
        return Err(format!(
            "{file} has no image object on page {page}, so it cannot demonstrate the refusal"
        ));
    }
    let whole = [
        f32::MIN / 4.0,
        f32::MIN / 4.0,
        f32::MAX / 4.0,
        f32::MAX / 4.0,
    ];
    let plan = redact::covered(&objects, &[], whole);
    if plan.is_complete() {
        return Err(
            "a region covering a drawing reports a complete plan, so a caller acting on it \
             would take the words and leave the picture of the words"
                .to_string(),
        );
    }
    if plan.unhandled.iter().any(|object| object.kind == "image") {
        return Err(
            "an image is still reported as unremovable, so the removal below is not being \
             asked for and this check is measuring the paths instead"
                .to_string(),
        );
    }
    if plan.images.is_empty() {
        return Err(format!(
            "{file} has a /DCTDecode image inside the region and the plan names none, so a \
             redaction over a scanned page would remove nothing"
        ));
    }
    Ok(format!(
        "{} image(s) are removable and the plan is still incomplete, naming why: {}",
        plan.images.len(),
        plan.unhandled
            .iter()
            .map(redact::Unhandled::sentence)
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

/// The documented blind spot, asserted in both directions.
///
/// Returns a note when PDFium extracts the word and `verify::scan` does not find
/// it. Either half failing is a finding: no extraction means the fixture changed,
/// and a scan that *does* find it means this instrument sees more than its
/// documentation says.
fn blind_spot(pdfium: &Pdfium, root: &Path) -> Result<String, String> {
    let (file, word) = BLIND;
    let source = root.join("testdata").join(file);
    if !source.exists() {
        return Ok(format!("[SKIP] {file} not generated"));
    }
    let doc = pdfium
        .load_pdf_from_file(&source, None)
        .map_err(|why| why.to_string())?;
    let page = doc.pages().get(0).map_err(|why| why.to_string())?;
    let extracted = page.text().map_err(|why| why.to_string())?.all();
    if !extracted.contains(word) {
        return Err(format!(
            "PDFium no longer extracts {word:?} from this fixture, so the blind spot cannot \
             be demonstrated with it"
        ));
    }
    let bytes = std::fs::read(&source).map_err(|why| why.to_string())?;
    let found = verify::scan(&bytes, &[word.to_string()], None);
    if found.found.contains(word) {
        return Err(format!(
            "verify::scan now FINDS {word:?} in a CID-encoded document. That contradicts what \
             this probe and docs/PLAN.md §6 both say about a byte scan -- read them before \
             believing it."
        ));
    }
    Ok(format!(
        "PDFium extracts {word:?} and verify::scan cannot see it -- the documented blind spot, \
         still exactly that"
    ))
}

fn run(pdfium: &Pdfium, case: &Case, source: &Path, scratch: &Path) -> Result<String, String> {
    let needles = vec![case.remove.to_string(), case.keep.to_string()];

    // The precondition, and it is not ceremony: a check that the word is gone
    // afterwards passes trivially for a word that was never there.
    let before = verify::scan(
        &std::fs::read(source).map_err(|why| why.to_string())?,
        &needles,
        None,
    );
    if !before.found.contains(case.remove) {
        return Err(format!(
            "{:?} is not in the file to begin with, so removing it could not fail",
            case.remove
        ));
    }
    if !before.found.contains(case.keep) {
        return Err(format!(
            "the control word {:?} is not in the file, so it cannot prove anything survived",
            case.keep
        ));
    }

    let region = region_of(pdfium, source, case)?;
    let objects = page_objects(pdfium, source, case.page)?;
    let text_objects = objects.iter().filter(|o| o.kind == "text").count();
    let plan = redact::covered(&objects, &[], region);
    if plan.shows.is_empty() {
        return Err(format!(
            "the region for {:?} covers no text object, so nothing would be removed",
            case.remove
        ));
    }

    let mut doc = Document::load(source).map_err(|why| why.to_string())?;
    let page_id = *doc
        .get_pages()
        .get(&(case.page as u32 + 1))
        .ok_or("no such page")?;
    let removed = redact::remove_shows(&mut doc, page_id, &plan.shows, text_objects)?;

    let out = scratch.join(format!("{}.redacted.pdf", case.file));
    let bytes = tpdf_lib::save::serialise(&mut doc, "the redacted document")?;
    std::fs::write(&out, &bytes).map_err(|why| why.to_string())?;

    let wrong = verify::structure(&bytes);
    if !wrong.is_empty() {
        return Err(format!("the result is malformed: {}", wrong.join("; ")));
    }

    let after = verify::scan(&bytes, &needles, None);
    if after.found.contains(case.remove) {
        return Err(format!(
            "{:?} is still in the file after being marked for removal",
            case.remove
        ));
    }
    // The control. A redaction that emptied the page passes the line above.
    if !after.found.contains(case.keep) {
        return Err(format!(
            "the control word {:?} went too, so this removed more than the region",
            case.keep
        ));
    }
    // And the file is still a document a reader can open.
    let opened = pdfium
        .load_pdf_from_file(&out, None)
        .map_err(|why| format!("PDFium will not open the result: {why}"))?;
    let pages = opened.pages().len();
    drop(opened);

    Ok(format!(
        "removed {} of {} show operator(s), {pages} page(s) still open, {:?} gone and {:?} kept",
        removed.removed, removed.shows_before, case.remove, case.keep
    ))
}

/// The bounding box of the first occurrence of `remove`, in page space.
///
/// Derived from PDFium's own character boxes rather than guessed, so the region
/// is where the word actually is --- and deliberately *not* padded: a region
/// that had to be inflated to hit anything would be testing the padding.
fn region_of(pdfium: &Pdfium, source: &Path, case: &Case) -> Result<redact::Rect, String> {
    let doc = pdfium
        .load_pdf_from_file(source, None)
        .map_err(|why| why.to_string())?;
    let page = doc
        .pages()
        .get(case.page as i32)
        .map_err(|why| why.to_string())?;
    let text = page.text().map_err(|why| why.to_string())?;
    let all = text.all();
    let at = all
        .find(case.remove)
        .ok_or_else(|| format!("PDFium does not extract {:?} from this page", case.remove))?;
    // Character indices, not byte indices: `find` answers in bytes.
    let start = all[..at].chars().count();
    let len = case.remove.chars().count();

    let mut region: Option<redact::Rect> = None;
    for index in start..start + len {
        let Ok(bounds) = text.chars().get(index).and_then(|c| c.tight_bounds()) else {
            continue;
        };
        let box_ = [
            bounds.left().value,
            bounds.bottom().value,
            bounds.right().value,
            bounds.top().value,
        ];
        region = Some(match region {
            None => box_,
            Some(seen) => [
                seen[0].min(box_[0]),
                seen[1].min(box_[1]),
                seen[2].max(box_[2]),
                seen[3].max(box_[3]),
            ],
        });
    }
    region.ok_or_else(|| format!("PDFium gave no character box for {:?}", case.remove))
}

/// Every object PDFium enumerates on the page, in its order.
fn page_objects(
    pdfium: &Pdfium,
    source: &Path,
    index: usize,
) -> Result<Vec<redact::PageObject>, String> {
    let doc = pdfium
        .load_pdf_from_file(source, None)
        .map_err(|why| why.to_string())?;
    let page = doc
        .pages()
        .get(index as i32)
        .map_err(|why| why.to_string())?;
    let mut out = Vec::new();
    for object in page.objects().iter() {
        let Ok(bounds) = object.bounds() else {
            // An object PDFium will not measure cannot be placed, so it cannot
            // be excluded from a region either. Reported as unhandled by name
            // rather than dropped -- the alternative is a redaction that passes
            // over whatever it could not see.
            out.push(redact::PageObject {
                bounds: [f32::MIN, f32::MIN, f32::MAX, f32::MAX],
                kind: "unmeasurable".to_string(),
            });
            continue;
        };
        out.push(redact::PageObject {
            bounds: [
                bounds.left().value,
                bounds.bottom().value,
                bounds.right().value,
                bounds.top().value,
            ],
            kind: match object.object_type() {
                PdfPageObjectType::Text => "text",
                PdfPageObjectType::Image => "image",
                PdfPageObjectType::Path => "path",
                PdfPageObjectType::Shading => "shading",
                PdfPageObjectType::XObjectForm => "form",
                PdfPageObjectType::Unsupported => "unsupported",
            }
            .to_string(),
        });
    }
    Ok(out)
}

fn default_library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join("vendor")
        .join("pdfium")
        .join(if cfg!(windows) { "bin" } else { "lib" })
}
