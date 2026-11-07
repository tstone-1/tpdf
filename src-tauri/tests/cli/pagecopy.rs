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
    /// Every record, of any kind: a page without one of its texts must have as
    /// many as the same page drawn without that text at all.
    records: usize,
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
        seen.records += 1;
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

/// One page whose words the page itself does not show, beside three that it
/// does. Each "secret" is hidden a different way: under a white box drawn
/// after it, outside its clip, inside a clip of no area, at fill alpha 0, and
/// under an opaque square annotation. The controls are an uncovered word, a
/// word inside a clip just inside the page (producers write one around
/// everything), and a
/// word with a thin line drawn through it afterwards, as a strike-through or a
/// table rule is: it crosses the glyphs (PDFium's bounds for this word start at
/// its baseline, so a line under it would touch nothing) and hides nothing.
fn hidden_fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding",
    });
    let mut content = Vec::new();
    content.extend_from_slice(b"BT /F1 14 Tf 0 0 0 rg 60 760 Td (Visible note) Tj ET ");
    content.extend_from_slice(
        b"q 10 10 575 822 re W n BT /F1 14 Tf 0 0 0 rg 60 730 Td (Inside page clip) Tj ET Q ",
    );
    content.extend_from_slice(
        b"q 60 470 0 0 re W n BT /F1 14 Tf 0 0 0 rg 60 470 Td (Zero clip secret) Tj ET Q ",
    );
    content.extend_from_slice(b"BT /F1 14 Tf 0 0 0 rg 60 700 Td (Struck words) Tj ET ");
    content.extend_from_slice(b"0 0 0 rg 60 704 120 0.8 re f ");
    content.extend_from_slice(b"BT /F1 14 Tf 0 0 0 rg 60 650 Td (Covered secret) Tj ET ");
    content.extend_from_slice(b"1 1 1 rg 55 640 200 30 re f ");
    content.extend_from_slice(
        b"q 60 600 12 20 re W n BT /F1 14 Tf 0 0 0 rg 60 605 Td (Clipped secret) Tj ET Q ",
    );
    content.extend_from_slice(
        b"q /GS0 gs BT /F1 14 Tf 0 0 0 rg 60 560 Td (Transparent secret) Tj ET Q ",
    );
    content.extend_from_slice(b"BT /F1 14 Tf 0 0 0 rg 60 505 Td (Annotated secret) Tj ET");
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    let look = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form",
            "BBox"=>vec![0.into(), 0.into(), 200.into(), 30.into()],
        },
        b"1 1 1 rg 0 0 200 30 re f".to_vec(),
    ));
    let square = doc.add_object(dictionary! {
        "Type"=>"Annot", "Subtype"=>"Square", "F"=>4,
        "Rect"=>vec![55.into(), 495.into(), 255.into(), 525.into()],
        "AP"=>dictionary! { "N"=>look },
    });
    let resources = dictionary! {
        "Font" => dictionary! { "F1" => font },
        "ExtGState" => dictionary! { "GS0" => dictionary! { "ca" => 0 } },
    };
    let page = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
        "Contents"=>stream, "Resources"=>resources.clone(), "Annots"=>vec![Object::Reference(square)],
    });
    // Page 2: nothing but a word its clip cuts after the first letter.
    let cut = doc.add_object(Stream::new(
        dictionary! {},
        b"q 60 600 12 20 re W n BT /F1 14 Tf 0 0 0 rg 60 605 Td (Clipped secret) Tj ET Q".to_vec(),
    ));
    let alone = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
        "Contents"=>cut, "Resources"=>resources.clone(),
    });
    // Pages 3 to 8, in pairs: a hidden word, then the same page without it.
    // Under a white box; under an opaque annotation; at fill alpha 0.
    let mut kids = vec![Object::Reference(page), Object::Reference(alone)];
    let pairs: [(&[u8], &[u8], bool); 3] = [
        (
            b"BT /F1 14 Tf 0 0 0 rg 60 650 Td (Covered secret) Tj ET 1 1 1 rg 55 640 200 30 re f",
            b"1 1 1 rg 55 640 200 30 re f",
            false,
        ),
        (
            b"BT /F1 14 Tf 0 0 0 rg 60 505 Td (Annotated secret) Tj ET",
            b"",
            true,
        ),
        (
            b"q /GS0 gs BT /F1 14 Tf 0 0 0 rg 60 560 Td (Transparent secret) Tj ET Q",
            b"",
            false,
        ),
    ];
    for (with, without, annotated) in pairs {
        for content in [with, without] {
            let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
            let mut sheet = dictionary! {
                "Type"=>"Page", "Parent"=>tree,
                "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
                "Contents"=>stream, "Resources"=>resources.clone(),
            };
            if annotated {
                let square = doc.add_object(dictionary! {
                    "Type"=>"Annot", "Subtype"=>"Square", "F"=>4,
                    "Rect"=>vec![55.into(), 495.into(), 255.into(), 525.into()],
                    "AP"=>dictionary! { "N"=>look },
                });
                sheet.set("Annots", vec![Object::Reference(square)]);
            }
            kids.push(Object::Reference(doc.add_object(sheet)));
        }
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        tree,
        dictionary! { "Type"=>"Pages", "Kids"=>kids, "Count"=>count }.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// What the page hides stays out of the copy's text: a pasted picture is
/// ungrouped in PowerPoint, and a word written as text above the box, clip or
/// annotation that hid it is then a word anyone can read.
fn keeps_hidden_words_hidden(report: &mut Report, dir: &Path) {
    let source = dir.join("hidden.pdf");
    hidden_fixture(&source);
    let copied = Worker::spawn(&source, &library_dir()).and_then(|mut worker| {
        let _ = worker.call(&Request::Open {
            lazy_geometry: false,
        });
        Ok((copy_counted(&mut worker, 0)?, copy_counted(&mut worker, 1)?))
    });
    let ((seen, left), (cut, left_alone)) = match copied {
        Ok(((page, left), (alone, left_alone))) => {
            ((read(&page), left), (read(&alone), left_alone))
        }
        Err(why) => return report.check("the worker draws the hidden-words pages", false, &why),
    };
    // The window says the copy lacks something when this is not 0. Page 1
    // has two texts a clip cuts, the one in a clip of no area among them, and
    // two that a box or an annotation covers. The one at alpha 0 shows nothing
    // on the page and is not counted, nor is the one inside a page-sized clip.
    report.check(
        "the copy counts the texts a clip or a cover hid: 4 on page 1, 1 on page 2",
        left == 4 && left_alone == 1,
        &format!("page 1: {left}, page 2: {left_alone}"),
    );
    // Drawn as outlines under its cover, a word looked hidden on the pasted
    // slide, but *Ungroup* makes the cover a shape of its own and moving it
    // showed the word (2026-11-07). So the hidden word must leave nothing at
    // all: its page draws exactly what the page without it draws.
    let pairs = Worker::spawn(&source, &library_dir()).and_then(|mut worker| {
        let _ = worker.call(&Request::Open {
            lazy_geometry: false,
        });
        (2..8)
            .map(|page| copy_counted(&mut worker, page))
            .collect::<Result<Vec<_>, _>>()
    });
    let pairs = match pairs {
        Ok(pairs) => pairs,
        Err(why) => return report.check("the worker draws the hidden-word pairs", false, &why),
    };
    for (name, at, counted) in [
        ("under a white box", 0, 1),
        ("under an annotation", 2, 1),
        ("at alpha 0", 4, 0),
    ] {
        let (with, left) = (read(&pairs[at].0), pairs[at].1);
        let without = read(&pairs[at + 1].0);
        report.check(
            &format!("a word {name} leaves no record in the copy, and the count is {counted}"),
            with.header
                && with.words.is_empty()
                && with.records == without.records
                && left == counted
                && pairs[at + 1].1 == 0,
            &format!(
                "{} records with the word, {} without; counted {left}",
                with.records, without.records
            ),
        );
    }
    // PowerPoint's Ungroup drops a metafile's clip, so a clipped word drawn
    // as outlines would show whole on the slide: it must not be drawn at all.
    report.check(
        "a word its clip cuts is left out of the copy, as text and as outlines",
        cut.header && cut.words.is_empty() && cut.shapes == 0,
        &format!("{cut:?}"),
    );
    report.check(
        "words the page shows stay text: uncovered, inside a page-sized clip, struck through",
        ["Visible note", "Inside page clip", "Struck words"]
            .iter()
            .all(|word| seen.words.iter().any(|w| w == word)),
        &format!("{:?}", seen.words),
    );
    report.check(
        "no covered, clipped, transparent or annotated word is written as text",
        !seen.words.iter().any(|w| w.contains("secret")),
        &format!("{:?}", seen.words),
    );
}

fn copy(worker: &mut Worker, page: u32) -> Result<Vec<u8>, String> {
    copy_counted(worker, page).map(|(emf, _)| emf)
}

/// The metafile, and how many texts a clip cut that the copy left out.
fn copy_counted(worker: &mut Worker, page: u32) -> Result<(Vec<u8>, u32), String> {
    let response = worker.call(&Request::PageCopy {
        page,
        crop: None,
        turns: 0,
    })?;
    match response.reply {
        Some(Reply::PageCopy(clipped)) if response.ok => {
            Ok((worker.tile.as_slice()[..response.bytes].to_vec(), clipped))
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
    let flat = match copy_counted(&mut worker, 0) {
        Ok((bytes, left)) => {
            report.check(
                "a page with no clip leaves nothing out",
                left == 0,
                &left.to_string(),
            );
            read(&bytes)
        }
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
    keeps_hidden_words_hidden(report, &dir);
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
