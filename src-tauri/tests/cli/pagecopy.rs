//! *Copy page for PowerPoint* through a sandboxed worker: the metafile holds
//! each visible word as a text record, in the weight the font names, and the
//! page's shapes as paths. Read from the record layout (`[MS-EMF]` 2.3.5.8,
//! 2.3.7.8, 2.2.13), not by GDI, which is what wrote it.
use super::{library_dir, scratch, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use tpdf_lib::worker::{Reply, Request, Worker};

const HEADER: u32 = 1;
const EXTCREATEFONTINDIRECTW: u32 = 82;
const EXTTEXTOUTW: u32 = 84;

/// What one metafile says: its words, the fonts' weights and escapements,
/// and how many records draw something other than text.
#[derive(Default, Debug)]
struct Read {
    words: Vec<String>,
    weights: Vec<i32>,
    escapements: Vec<i32>,
    shapes: usize,
    header: bool,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read(emf: &[u8]) -> Read {
    let mut seen = Read::default();
    let mut at = 0;
    while let (Some(kind), Some(size)) = (u32_at(emf, at), u32_at(emf, at + 4)) {
        let size = size as usize;
        if size < 8 || at + size > emf.len() {
            break;
        }
        let record = &emf[at..at + size];
        match kind {
            HEADER => seen.header = record.get(40..44) == Some(b" EMF"),
            EXTTEXTOUTW => {
                let chars = u32_at(record, 44).unwrap_or(0) as usize;
                let offset = u32_at(record, 48).unwrap_or(0) as usize;
                let units: Vec<u16> = record
                    .get(offset..offset + 2 * chars)
                    .unwrap_or_default()
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                seen.words.push(String::from_utf16_lossy(&units));
            }
            EXTCREATEFONTINDIRECTW => {
                seen.escapements
                    .push(u32_at(record, 20).unwrap_or(0) as i32);
                seen.weights.push(u32_at(record, 28).unwrap_or(0) as i32);
            }
            // Fills and strokes of paths, polygons and Béziers: the page's own
            // drawing. PDFium paints an upright filled rectangle as a brush
            // `EMR_BITBLT` (76) with no source, not as a path.
            2..=7 | 62..=64 | 76 | 85..=92 => seen.shapes += 1,
            _ => {}
        }
        at += size;
    }
    seen
}

/// Two pages: plain, bold and invisible words over a filled box, and the same
/// page shown turned a quarter clockwise by its own `/Rotate`.
fn fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let regular = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding",
    });
    let bold = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica-Bold", "Encoding"=>"WinAnsiEncoding",
    });
    let mut content = b"0.2 0.4 0.8 rg 50 600 300 120 re f ".to_vec();
    content
        .extend_from_slice(b"BT /F1 14 Tf 0 0 0 rg 60 680 Td (Fertigungsunterst\xfctzung) Tj ET ");
    content.extend_from_slice(b"BT /F2 12 Tf 1 0 0 rg 60 640 Td (Rainer Paschold) Tj ET ");
    content.extend_from_slice(b"BT 3 Tr /F1 10 Tf 60 400 Td (hidden layer) Tj ET");
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    let resources = dictionary! { "Font" => dictionary! { "F1" => regular, "F2" => bold } };
    let mut kids = Vec::new();
    for rotation in [0, 90] {
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
            "Rotate"=>rotation, "Contents"=>stream, "Resources"=>resources.clone(),
        })));
    }
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>2}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

fn copy(worker: &mut Worker, page: u32) -> Result<Vec<u8>, String> {
    let response = worker.call(&Request::PageCopy {
        page,
        crop: None,
        turns: 0,
    })?;
    match response.reply {
        Some(Reply::PageCopy) if response.ok => {
            Ok(worker.tile.as_slice()[..response.bytes].to_vec())
        }
        _ => Err(response.error),
    }
}

pub(super) fn writes_words_as_text(report: &mut Report) {
    let dir = scratch("pagecopy");
    let source = dir.join("words.pdf");
    fixture(&source);
    let mut worker = match Worker::spawn(&source, &library_dir()) {
        Ok(worker) => worker,
        Err(why) => return report.check("a worker opens the page copy fixture", false, &why),
    };
    let _ = worker.call(&Request::Open {
        lazy_geometry: false,
    });
    let flat = match copy(&mut worker, 0) {
        Ok(bytes) => read(&bytes),
        Err(why) => return report.check("the worker draws page 1 as a metafile", false, &why),
    };
    report.check(
        "the copy is an enhanced metafile",
        flat.header,
        &format!("{flat:?}"),
    );
    report.check(
        "each visible text object is one text record, umlaut included",
        flat.words == ["Fertigungsunterstützung", "Rainer Paschold"],
        &format!("{:?}", flat.words),
    );
    report.check(
        "the bold font is asked for bold and the regular one regular",
        flat.weights == [400, 700],
        &format!("{:?}", flat.weights),
    );
    report.check(
        "the filled box is drawn as a shape",
        flat.shapes > 0,
        &format!("{} shape records", flat.shapes),
    );
    let turned = copy(&mut worker, 1).map(|bytes| read(&bytes));
    report.check(
        "text on a page turned a quarter clockwise runs down the page",
        turned
            .as_ref()
            .is_ok_and(|seen| seen.escapements == [2700, 2700]),
        &format!("{turned:?}"),
    );
    let missing = copy(&mut worker, 9);
    report.check(
        "a page the document does not have is refused, and the worker keeps answering",
        missing.is_err() && copy(&mut worker, 0).is_ok(),
        &format!("{:?}", missing.map(|bytes| bytes.len())),
    );
    if let Some(from) = std::env::var_os("TPDF_PAGECOPY_FROM") {
        by_hand(report, Path::new(&from));
    }
}

/// For a check by hand in PowerPoint: page 1 of `TPDF_PAGECOPY_FROM` copied
/// through a worker, written to `TPDF_PAGECOPY_TO` and, with
/// `TPDF_PAGECOPY_CLIPBOARD=1`, put on the clipboard as the application does.
fn by_hand(report: &mut Report, from: &Path) {
    let copied = Worker::spawn(from, &library_dir()).and_then(|mut worker| {
        let _ = worker.call(&Request::Open {
            lazy_geometry: false,
        });
        copy(&mut worker, 0)
    });
    let emf = match copied {
        Ok(emf) => emf,
        Err(why) => return report.check("the named document's page 1 is copied", false, &why),
    };
    let seen = read(&emf);
    report.check(
        "the named document's page 1 is copied",
        seen.header,
        &format!(
            "{} bytes, {} text records, {} shape records",
            emf.len(),
            seen.words.len(),
            seen.shapes
        ),
    );
    if let Some(to) = std::env::var_os("TPDF_PAGECOPY_TO") {
        let written = std::fs::write(&to, &emf);
        report.check(
            "the copy is written where named",
            written.is_ok(),
            &format!("{written:?}"),
        );
    }
    if std::env::var_os("TPDF_PAGECOPY_CLIPBOARD").is_some_and(|on| on == "1") {
        // SAFETY: no arguments; a null answer is a process with no console.
        let owner = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() } as isize;
        let put = tpdf_lib::pagecopy::clipboard(owner, &emf);
        report.check(
            "the copy is on the clipboard",
            put.is_ok(),
            &format!("{put:?}"),
        );
    }
}
