//! `ocr` against a page that is a picture of a page whose text is known.
//!
//! The scan is made here: `testdata/text-base14.pdf` is rendered by the tool
//! and the pixels are put on a page of the same size as its only content. So
//! the words the recogniser should find, and where, are the source's own ---
//! read through the same tool --- rather than anything this file states.
//!
//! The control is the scan before `ocr`: it must have no text and a search of
//! it must find nothing, or every check after it is satisfied by text that was
//! already there.
//!
//! The recogniser is the operating system's, so its boxes are detections and
//! the comparison of places has an allowance. The text is compared by word
//! overlap, with a tenth allowed for a misread.

use super::{fixture, library_dir, overlap, scratch, tool, words, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use tpdf_lib::recognition::{ocr_copy_asked, Progress, Recognised, CANCELLED, UNSAVED};
use tpdf_lib::render::{Backend, RenderService};

/// The resolution the scan is rendered at.
const DPI: u32 = 200;

/// How far the centre of a recognised word's box may be from the centre of the
/// source's, in points.
///
/// **Centres, because the engine's boxes are a different shape.** Measured with
/// Vision on this fixture at 200 DPI, over the 29 words that occur once: its
/// boxes are up to 8.7 pt taller and 5.8 pt wider than the type, since it boxes
/// the line's height rather than the glyphs' ink, and their corners are up to
/// 4.5 pt from the source's. Compared edge by edge, a correct hit fails or
/// passes on which word was asked for. The centres stay within 4.2 pt. Six leaves
/// room for the other engine and still fails a hit drawn on a neighbouring
/// word, which is 20 pt away or more.
const NEAR_PT: f64 = 6.0;

/// The middle of an `[x, y, width, height]` box.
fn centre(rect: [f64; 4]) -> (f64, f64) {
    (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0)
}

fn rgba(path: &Path) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .read_info()
        .unwrap();
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    data.truncate(info.buffer_size());
    (info.width, info.height, data)
}

/// A one-page document whose only content is the picture.
fn scan(picture: &Path, to: &Path) {
    let (width, height, data) = rgba(picture);
    let rgb: Vec<u8> = data
        .chunks_exact(4)
        .flat_map(|px| [px[0], px[1], px[2]])
        .collect();
    let (w_pt, h_pt) = (
        f64::from(width) * 72.0 / f64::from(DPI),
        f64::from(height) * 72.0 / f64::from(DPI),
    );
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let mut image = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => i64::from(width), "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        rgb,
    );
    image.compress().unwrap();
    let image = doc.add_object(image);
    let content = doc.add_object(Stream::new(
        dictionary! {},
        format!("q {w_pt} 0 0 {h_pt} 0 0 cm /Im0 Do Q").into_bytes(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => tree,
        "MediaBox" => vec![0.into(), 0.into(), w_pt.into(), h_pt.into()],
        "Contents" => content,
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
    });
    doc.objects.insert(
        tree,
        dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }
            .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);
    doc.save(to).unwrap();
}

/// Every match of `query` in `file`, as `[x, y, width, height]`.
fn found(file: &str, query: &str) -> Vec<[f64; 4]> {
    let (_, json, _) = tool(&["search", file, "--text", query, "--json"], &[]);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
        return Vec::new();
    };
    value["files"][0]["matches"]
        .as_array()
        .map(|matches| {
            matches
                .iter()
                .filter_map(|m| {
                    let rect = m["rects"][0]["rect"].as_array()?;
                    Some([
                        rect.first()?.as_f64()?,
                        rect.get(1)?.as_f64()?,
                        rect.get(2)?.as_f64()?,
                        rect.get(3)?.as_f64()?,
                    ])
                })
                .collect()
        })
        .unwrap_or_default()
}

fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .unwrap_or_else(|_| Err(E::from("the render service did not answer".to_string())))
}

/// What the window is asked to do differently from a plain recognition.
#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Plain,
    /// The reader turned the first page and has not saved.
    Unsaved,
    /// The reader pressed Stop before the first page.
    Stopped,
    /// The reader pressed Stop while the last page was being read.
    StoppedReading,
}

/// The window's *Recognise text*, in this process: the document opened in a
/// render service with an edit model beside it, as the application holds it.
/// Returns the answer and the pages progress was reported for.
fn app_path(path: &Path, out: &Path, ask: Ask) -> (Result<Recognised, String>, Vec<Progress>) {
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let file = std::fs::File::open(path).unwrap();
    let hashing = file.try_clone().unwrap();
    let info = wait(|r| service.open_handed(path.to_path_buf(), Some(file), true, None, r))
        .map_err(|refusal| refusal.reason)
        .expect("the scan opens");
    let count = u32::try_from(info.page_count).unwrap();
    let edits = tpdf_lib::edits::Edits::default();
    edits.open(
        info.id,
        count,
        Some(tpdf_lib::fingerprint::Opened {
            file: hashing,
            what: path.to_path_buf(),
        }),
    );
    if ask == Ask::Unsaved {
        edits.rotate(info.id, 1, 1).expect("the page turns");
    }
    let seen = std::sync::Mutex::new(Vec::new());
    let stop = AtomicBool::new(ask == Ask::Stopped);
    let answer = ocr_copy_asked(
        &service,
        info.id,
        edits.plan(info.id).expect("a plan"),
        path,
        out,
        None,
        library_dir(),
        Vec::new(),
        &|| stop.load(std::sync::atomic::Ordering::Relaxed),
        &|at| {
            seen.lock().unwrap().push(at);
            if ask == Ask::StoppedReading {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    );
    (answer, seen.into_inner().unwrap())
}

fn text_of(file: &str) -> String {
    tool(&["text", file], &[]).1
}

/// The window's command on the same scan: the same words in the same places,
/// and its three refusals leave nothing behind.
fn window_path(report: &mut Report, source: &str, scanned: &str, out: &str, refused: &str) {
    let (answer, seen) = app_path(Path::new(scanned), Path::new(out), Ask::Plain);
    report.check(
        "the window's path gives the one page a layer and reports the page it read",
        answer.as_ref().is_ok_and(|read| {
            read.pages.len() == 1
                && read.pages[0].page == 1
                && read.pages[0].words > 0
                && read.already_text.is_empty()
                && !read.engine.is_empty()
        }) && seen == [Progress { page: 1, of: 1 }],
        &format!("{answer:?}; {seen:?}"),
    );
    if answer.is_err() {
        return;
    }
    let (want, got) = (words(&text_of(source)), words(&text_of(out)));
    let shared = overlap(&want, &got);
    let (theirs, ours) = (found(source, "quartz"), found(out, "quartz"));
    let near = theirs.len() == 1 && ours.len() == 1 && {
        let ((ax, ay), (bx, by)) = (centre(theirs[0]), centre(ours[0]));
        (ax - bx).abs() <= NEAR_PT && (ay - by).abs() <= NEAR_PT
    };
    report.check(
        "the window's copy reads the source's words and finds one where the source has it",
        shared >= 0.9 && near,
        &format!("{shared:.2}; {ours:?} against {theirs:?}"),
    );

    let staging_left = || {
        std::fs::read_dir(Path::new(refused).parent().unwrap())
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name();
                let name = name.to_string_lossy().into_owned();
                !name.ends_with(".pdf") && !name.ends_with(".png")
            })
            .count()
    };
    let before = staging_left();
    // `started` is how many pages were begun: a stop asked for before the
    // first page begins none, and one asked for during the only page begins it
    // and still writes nothing.
    for (what, from, ask, wording, started) in [
        ("with unsaved changes", scanned, Ask::Unsaved, UNSAVED, 0),
        (
            "stopped before its first page",
            scanned,
            Ask::Stopped,
            CANCELLED,
            0,
        ),
        (
            "stopped during its last page",
            scanned,
            Ask::StoppedReading,
            CANCELLED,
            1,
        ),
        (
            "whose pages all have text",
            out,
            Ask::Plain,
            "already has text",
            1,
        ),
    ] {
        let (answer, seen) = app_path(Path::new(from), Path::new(refused), ask);
        report.check(
            &format!("the window's path refuses a document {what} and writes nothing"),
            answer.as_ref().is_err_and(|why| why.contains(wording))
                && seen.len() == started
                && !Path::new(refused).exists()
                && staging_left() == before,
            &format!("{answer:?}; {seen:?}"),
        );
    }
}

pub(super) fn makes_a_scan_searchable(report: &mut Report) {
    let Some(source) = fixture("text-base14.pdf") else {
        report.skip("ocr", "testdata/text-base14.pdf is not generated");
        return;
    };
    let dir = scratch("ocr");
    let at = |name: &str| dir.join(name).display().to_string();
    let (source, picture, scanned, out) = (
        source.display().to_string(),
        at("page.png"),
        at("scan.pdf"),
        at("out.pdf"),
    );
    let (code, _, stderr) = tool(
        &["render", &source, "-o", &picture, "--dpi", &DPI.to_string()],
        &[],
    );
    if code != 0 {
        report.check("the source renders, to make the scan from", false, &stderr);
        return;
    }
    scan(Path::new(&picture), Path::new(&scanned));

    report.check(
        "the control: the scan has no text and a search of it finds nothing",
        text_of(&scanned).trim().is_empty() && found(&scanned, "quartz").is_empty(),
        &text_of(&scanned),
    );

    let (code, json, stderr) = tool(&["ocr", &scanned, "-o", &out, "--json"], &[]);
    if code == 3 && stderr.contains(tpdf_lib::ocr_worker::NO_ENGINE) {
        report.skip("ocr", "this platform has no recogniser");
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "ocr reports one page given a layer, and names the engine",
        code == 0
            && result["pages"].as_array().is_some_and(|p| p.len() == 1)
            && result["pages"][0]["page"] == 1
            && result["pages"][0]["words"].as_u64().is_some_and(|n| n > 0)
            && result["already_text"].as_array().is_some_and(Vec::is_empty)
            && result["engine"].as_str().is_some_and(|e| !e.is_empty()),
        &format!("exit {code}; {stderr}; {json}"),
    );
    if code != 0 {
        let _ = std::fs::remove_dir_all(dir);
        return;
    }

    let (want, got) = (words(&text_of(&source)), words(&text_of(&out)));
    let shared = overlap(&want, &got);
    report.check(
        "the copy reads the source's words, as separate words",
        shared >= 0.9,
        &format!("{shared:.2} of {} words: {got:?}", want.len()),
    );

    for query in ["quartz", "Beispiel", "black quartz"] {
        let (theirs, ours) = (found(&source, query), found(&out, query));
        let near = theirs.len() == 1 && ours.len() == 1 && {
            let ((ax, ay), (bx, by)) = (centre(theirs[0]), centre(ours[0]));
            (ax - bx).abs() <= NEAR_PT && (ay - by).abs() <= NEAR_PT
        };
        report.check(
            &format!("a search for {query:?} is found where the source has it"),
            near,
            &format!("{ours:?} against {theirs:?}"),
        );
    }

    window_path(
        report,
        &source,
        &scanned,
        &at("window.pdf"),
        &at("refused.pdf"),
    );

    let (before, after) = (at("before.png"), at("after.png"));
    let rendered = [(&scanned, &before), (&out, &after)]
        .iter()
        .all(|(file, png)| tool(&["render", file, "-o", png], &[]).0 == 0);
    report.check(
        "the layer paints nothing: the copy renders as the scan did",
        rendered && rgba(Path::new(&before)) == rgba(Path::new(&after)),
        "the two renders differ",
    );

    // A page that has text is left alone, and a document with nothing to add
    // is refused with nothing written.
    let again = at("again.pdf");
    let (code, _, stderr) = tool(&["ocr", &out, "-o", &again], &[]);
    report.check(
        "a document whose pages all have text is refused and nothing is written",
        code == 3 && stderr.contains("already has text") && !Path::new(&again).exists(),
        &format!("exit {code}; {stderr}"),
    );

    let mixed = at("mixed.pdf");
    let merged = tool(&["merge", &scanned, &source, "-o", &mixed], &[]).0 == 0;
    let both = at("both.pdf");
    let (code, json, stderr) = tool(&["ocr", &mixed, "-o", &both, "--json"], &[]);
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "of a scanned page and a text page, only the scanned one gets a layer",
        merged
            && code == 0
            && result["pages"].as_array().is_some_and(|p| p.len() == 1)
            && result["pages"][0]["page"] == 1
            && result["already_text"] == serde_json::json!([2])
            && found(&both, "quartz").len() == 2,
        &format!("exit {code}; {stderr}; {json}"),
    );

    let kept = std::fs::read(&out).unwrap();
    for (args, status) in [
        (vec!["ocr", &scanned, "-o", &out], 3),
        (vec!["ocr", &scanned, "-o", &scanned, "--force"], 2),
        (vec!["ocr", &scanned, "-o", &again, "--pages", "2"], 3),
        (vec!["ocr", &scanned, "-o", &again, "--language", "x"], 2),
    ] {
        let (code, _, stderr) = tool(&args, &[]);
        report.check(
            "a refused ocr writes nothing and leaves an existing file alone",
            code == status && std::fs::read(&out).unwrap() == kept && !Path::new(&again).exists(),
            &format!("{args:?}: exit {code}; {stderr}"),
        );
    }
    report.check(
        "ocr leaves no staging directories",
        std::fs::read_dir(&dir).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tpdf-cli-")
        }),
        "",
    );
    let _ = std::fs::remove_dir_all(dir);
}
