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
use tpdf_lib::recognition::{
    ocr_copy_asked, offered_languages, Progress, Recognised, CANCELLED, UNSAVED,
};
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
    scan_with(picture, to, None, 0);
}

/// [`scan`], with `typed` written as real text in Helvetica along the top
/// margin, over the picture: a scanner's or a fax's header line. `forms` is
/// how many Form XObjects the picture is drawn inside: one as some scanners
/// write a page, two as an imposing or merging tool wraps such a page again.
fn scan_with(picture: &Path, to: &Path, typed: Option<&str>, forms: usize) {
    let after = typed
        .map(|line| {
            format!(
                " BT /F1 7 Tf 20 {} Td ({line}) Tj ET",
                page_height(picture) - 9.0
            )
        })
        .unwrap_or_default();
    scan_drawing(picture, to, after.as_bytes(), forms);
}

/// The height in points of a page made from `picture` at [`DPI`].
fn page_height(picture: &Path) -> f64 {
    f64::from(rgba(picture).1) * 72.0 / f64::from(DPI)
}

/// A scan another program already gave a text layer: the picture, and over it
/// `source`'s own first page drawn in render mode 3, neither filled nor
/// stroked, with each `(from, to)` in `misreads` replaced in its text.
///
/// The source is the document the picture was rendered from, so its words sit
/// exactly where the picture shows them, as an engine's layer would. The
/// misreads are what a second engine does not agree with: `rn` for `m`, `1` for
/// `l`, `0` for `O`.
fn scan_with_layer(picture: &Path, source: &Path, to: &Path, misreads: &[(&str, &str)]) {
    let doc = Document::load(source).unwrap();
    let page = *doc.get_pages().get(&1).unwrap();
    // Its text objects only: the source also paints a filled box and lines,
    // which drawn over the picture would hide the words under them.
    let mut content: String = String::from_utf8(doc.get_page_content(page))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("BT "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(
        content.lines().count(),
        4,
        "the source's four lines of text"
    );
    for (from, to) in misreads {
        assert!(content.contains(from), "{from} is not in the source's text");
        content = content.replacen(from, to, 1);
    }
    scan_drawing(picture, to, format!(" 3 Tr {content}").as_bytes(), 0);
}

/// A one-page document whose content draws the picture, inside `forms` Form
/// XObjects, and then `after`, with Helvetica as `/F1`.
fn scan_drawing(picture: &Path, to: &Path, after: &[u8], forms: usize) {
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
    // Where each level puts what it draws, innermost first: the picture in its
    // own space, then each form's content around the next level in, then the
    // page around the outermost form. Their product is the page, so the scan
    // looks the same at any depth. Unwrapped, the picture fills the page.
    //
    // **Two deep, the transforms do not cancel level by level**, and that is
    // the point: the page scales the outer form by two, the outer form moves
    // the inner three quarters of a page to the right, and the inner form
    // draws the picture three quarters of a page to the left at half size. A
    // reader that leaves out a form's transform, or applies two in the wrong
    // order, puts at most a quarter of the picture on the page.
    let shift = 0.75 * w_pt;
    let (half_w, half_h) = (w_pt / 2.0, h_pt / 2.0);
    let levels: Vec<String> = match forms {
        0 => vec![format!("{w_pt} 0 0 {h_pt} 0 0")],
        1 => vec![format!("{half_w} 0 0 {half_h} 0 0"), "2 0 0 2 0 0".into()],
        2 => vec![
            format!("{half_w} 0 0 {half_h} {} 0", -shift),
            format!("1 0 0 1 {shift} 0"),
            "2 0 0 2 0 0".into(),
        ],
        _ => panic!("a scan is made at most two forms deep"),
    };
    let mut drawn = doc.add_object(image);
    let mut name = "Im0";
    let mut draw = format!("q {} cm /Im0 Do Q", levels[0]);
    for around in &levels[1..] {
        // A box wide enough for any of the placements above: it clips
        // nothing.
        let wide = |v: f64| -> Object { v.into() };
        drawn = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![wide(-4.0 * w_pt), wide(-4.0 * h_pt), wide(4.0 * w_pt), wide(4.0 * h_pt)],
                "Resources" => dictionary! { "XObject" => dictionary! { name => drawn } },
            },
            draw.into_bytes(),
        ));
        name = "Fm0";
        draw = format!("q {around} cm /Fm0 Do Q");
    }
    let mut drawn_bytes = draw.into_bytes();
    drawn_bytes.extend_from_slice(after);
    let image = drawn;
    let content = doc.add_object(Stream::new(dictionary! {}, drawn_bytes));
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => tree,
        "MediaBox" => vec![0.into(), 0.into(), w_pt.into(), h_pt.into()],
        "Contents" => content,
        "Resources" => dictionary! {
            "XObject" => dictionary! { name => image },
            "Font" => dictionary! { "F1" => font },
        },
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
/// The share of the source's words a recognised copy has to read back.
///
/// Nine in ten, except on Windows. `Windows.Media.Ocr` reads 24 of this
/// fixture's 30 words: it drops the whole last line, `Widths matter: iiiii
/// WWWWW 11111 00000`, which sits directly under a rule. Measured 2026-10-03 on
/// Windows 11 by handing the engine the rendered page directly, with the German
/// and with the English recogniser, at 144 and at 300 pixels an inch: the same
/// 24 words four times. So that is the engine on this page and not the layer
/// tpdf writes, and the mark there is the 24, just under 0.8. CI never showed
/// it, because a runner does not generate `text-base14.pdf` and skips this.
const ENOUGH: f64 = if cfg!(windows) { 0.79 } else { 0.9 };

#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Plain,
    /// The reader turned the first page and has not saved.
    Unsaved,
    /// The reader pressed Stop before the first page.
    Stopped,
    /// The reader pressed Stop while the last page was being read.
    StoppedReading,
    /// The reader chose this language for recognised text.
    In(&'static str),
}

/// A tag no engine offers: Klingon.
const NOT_OFFERED: &str = "tlh-Latn";

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
    let language = match ask {
        Ask::In(tag) => Some(tag),
        _ => None,
    };
    let offered = offered_languages().unwrap_or_default();
    let answer = ocr_copy_asked(
        &service,
        info.id,
        edits.plan(info.id).expect("a plan"),
        path,
        out,
        None,
        library_dir(),
        language,
        &offered,
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
        shared >= ENOUGH && near,
        &format!("{shared:.2}; {ours:?} against {theirs:?}"),
    );

    // The reader's language. One the machine offers is asked of the engine and
    // the page still reads; one it does not offer is not an engine's error,
    // the engine chooses and the answer says which language was not there.
    let offered = offered_languages().unwrap_or_default();
    let beside = |name: &str| {
        Path::new(refused)
            .with_file_name(name)
            .display()
            .to_string()
    };
    if offered.iter().any(|tag| tag == "en-US") {
        let chosen = beside("chosen.pdf");
        let (answer, _) = app_path(Path::new(scanned), Path::new(&chosen), Ask::In("en-us"));
        let shared = overlap(&want, &words(&text_of(&chosen)));
        report.check(
            "the window's path reads the page in a language the machine offers",
            answer
                .as_ref()
                .is_ok_and(|read| read.language_unavailable.is_none() && read.pages.len() == 1)
                && shared >= ENOUGH,
            &format!("{answer:?}; {shared:.2}"),
        );
    } else {
        report.skip("ocr", "this machine's recogniser does not offer en-US");
    }
    let stale = beside("stale.pdf");
    let (answer, _) = app_path(Path::new(scanned), Path::new(&stale), Ask::In(NOT_OFFERED));
    let shared = overlap(&want, &words(&text_of(&stale)));
    report.check(
        "a language the machine does not offer is named, and the page is read all the same",
        answer.as_ref().is_ok_and(|read| {
            read.language_unavailable.as_deref() == Some(NOT_OFFERED) && read.pages.len() == 1
        }) && shared >= ENOUGH,
        &format!("{answer:?}; {shared:.2}"),
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

/// A Japanese page with no language named.
///
/// Left to its default, Vision reads this page as a few wrong Latin characters,
/// so the layer is asked for with the engine working out the script
/// (`OcrWorker::recognise_page`). Only on macOS: a stock Windows carries one
/// recogniser language, and it is not this one.
fn another_script(report: &mut Report, at: &dyn Fn(&str) -> String) {
    const WORDS: &str = "日本語";
    if !cfg!(target_os = "macos") {
        report.skip("ocr", "a page in another script needs a recogniser for it");
        return;
    }
    let Some(source) = fixture("multilingual.pdf") else {
        report.skip("ocr", "testdata/multilingual.pdf is not generated");
        return;
    };
    let (picture, scanned, out) = (
        at("script.png"),
        at("script-scan.pdf"),
        at("script-out.pdf"),
    );
    let source = source.display().to_string();
    let rendered = tool(
        &["render", &source, "-o", &picture, "--dpi", &DPI.to_string()],
        &[],
    );
    if rendered.0 != 0 {
        report.check(
            "the Japanese page renders, to make the scan from",
            false,
            &rendered.2,
        );
        return;
    }
    scan(Path::new(&picture), Path::new(&scanned));
    let (code, _, stderr) = tool(&["ocr", &scanned, "-o", &out], &[]);
    let read = if code == 0 {
        text_of(&out)
    } else {
        String::new()
    };
    report.check(
        "a Japanese page is read with no language named",
        text_of(&source).contains(WORDS)
            && text_of(&scanned).trim().is_empty()
            && read.contains(WORDS),
        &format!("exit {code}; {stderr}; read {read:?}"),
    );

    // The window's command is a second call into the recogniser.
    let window = at("script-window.pdf");
    let (answer, _) = app_path(Path::new(&scanned), Path::new(&window), Ask::Plain);
    let read = if answer.is_ok() {
        text_of(&window)
    } else {
        String::new()
    };
    report.check(
        "and the window's command reads it the same way",
        read.contains(WORDS),
        &format!("{:?}; read {read:?}", answer.err()),
    );
}

/// A scan with a typed header line: read, and the header not written twice.
///
/// Until 2026-10-15 one character of text kept a page from being read, so this
/// page was refused with *every selected page already has text*. A scan is
/// read whatever text it has (`ocr_layer::SCAN_SHARE`), and a word the page
/// already has in the same place is dropped (`ocr_layer::outcome_of`). The
/// header is a word the engine reads off the picture too, so a filter that
/// failed would make it findable twice.
fn a_scan_with_a_typed_line(
    report: &mut Report,
    at: &dyn Fn(&str) -> String,
    source: &str,
    picture: &str,
) {
    const TYPED: &str = "FAX 0401 RECEIVED";
    let (typed, out) = (at("typed-scan.pdf"), at("typed-out.pdf"));
    scan_with(Path::new(picture), Path::new(&typed), Some(TYPED), 0);
    report.check(
        "the control: the typed scan's only text is the header, found once",
        text_of(&typed).trim() == TYPED && found(&typed, "RECEIVED").len() == 1,
        &text_of(&typed),
    );
    let (code, json, stderr) = tool(&["ocr", &typed, "-o", &out, "--json"], &[]);
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "a scan with a typed line is read",
        code == 0
            && result["pages"].as_array().is_some_and(|p| p.len() == 1)
            && result["already_text"].as_array().is_some_and(Vec::is_empty),
        &format!("exit {code}; {stderr}; {json}"),
    );
    if code != 0 {
        return;
    }
    // The source's words and the typed ones: the copy holds both, and
    // `overlap` divides by the longer list.
    let mut want = words(&text_of(source));
    want.extend(words(TYPED));
    let got = words(&text_of(&out));
    let shared = overlap(&want, &got);
    report.check(
        "its copy reads the source's words and the typed line's",
        shared >= ENOUGH,
        &format!("{shared:.2} of {} words: {got:?}", want.len()),
    );
    report.check(
        "and the typed line is found once, not again from the picture",
        found(&out, "RECEIVED").len() == 1 && found(&out, "0401").len() == 1,
        &format!(
            "RECEIVED {:?}; 0401 {:?}",
            found(&out, "RECEIVED"),
            found(&out, "0401")
        ),
    );

    // The window's command walks the pages in code of its own.
    let window = at("typed-window.pdf");
    let (answer, _) = app_path(Path::new(&typed), Path::new(&window), Ask::Plain);
    let pages = answer.as_ref().map(|r| r.pages.len()).unwrap_or(0);
    report.check(
        "the window's command reads it too, and finds the typed line once",
        pages == 1 && found(&window, "RECEIVED").len() == 1 && found(&window, "quartz").len() == 1,
        &format!(
            "{:?}; RECEIVED {:?}",
            answer.err(),
            found(&window, "RECEIVED")
        ),
    );

    // A picture inside a Form XObject is a picture on the page all the same.
    let (formed, formed_out) = (at("form-scan.pdf"), at("form-out.pdf"));
    scan_with(Path::new(picture), Path::new(&formed), Some(TYPED), 1);
    let (code, json, stderr) = tool(&["ocr", &formed, "-o", &formed_out, "--json"], &[]);
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "a scan whose picture is inside a form is read the same",
        text_of(&formed).trim() == TYPED
            && code == 0
            && result["pages"].as_array().is_some_and(|p| p.len() == 1)
            && found(&formed_out, "quartz").len() == 1,
        &format!("exit {code}; {stderr}; {json}"),
    );
}

/// A scan another program already gave a layer, which reads some words
/// differently from this machine's engine, is left alone: no second layer, and
/// the misread words are not written again beside the first program's.
///
/// The control the rest of this file has, re-reading tpdf's own copy, cannot
/// tell: the same engine reads the same words, so every one matches. Here five
/// words of the layer are spelled as another engine might have read them.
fn a_scan_another_program_read(
    report: &mut Report,
    at: &dyn Fn(&str) -> String,
    source: &str,
    picture: &str,
) {
    const MISREADS: [(&str, &str); 5] = [
        ("jumps", "jurnps"),
        ("lazy", "1azy"),
        ("account", "acc0unt"),
        ("Beispiel", "Beispie1"),
        ("quartz", "qu4rtz"),
    ];
    let (layered, out) = (at("layered-scan.pdf"), at("layered-out.pdf"));
    scan_with_layer(
        Path::new(picture),
        Path::new(source),
        Path::new(&layered),
        &MISREADS,
    );
    report.check(
        "the control: the other program's layer reads the source's words, five misread",
        found(&layered, "jurnps").len() == 1
            && found(&layered, "quartz").is_empty()
            && found(&layered, "Sphinx").len() == 1,
        &text_of(&layered),
    );
    let (code, json, stderr) = tool(&["ocr", &layered, "-o", &out, "--json"], &[]);
    report.check(
        "a scan another program read is left alone, and nothing is written",
        code == 3 && stderr.contains("already has text") && !Path::new(&out).exists(),
        &format!("exit {code}; {stderr}; {json}"),
    );

    let window = at("layered-window.pdf");
    let (answer, _) = app_path(Path::new(&layered), Path::new(&window), Ask::Plain);
    report.check(
        "and the window's command leaves it alone too",
        answer
            .as_ref()
            .is_err_and(|why| why.contains("already has text"))
            && !Path::new(&window).exists(),
        &format!("{answer:?}"),
    );
}

/// A scan whose picture is two forms deep, with a typed line: a picture all
/// the same, so the page is read.
///
/// The picture's share was measured from the page's objects one level down,
/// so until 2026-10-15 this page counted as no picture at all, and the typed
/// line filed it under *already has text*.
fn a_scan_two_forms_deep(report: &mut Report, at: &dyn Fn(&str) -> String, picture: &str) {
    const TYPED: &str = "FAX 0401 RECEIVED";
    let (deep, out) = (at("deep-scan.pdf"), at("deep-out.pdf"));
    scan_with(Path::new(picture), Path::new(&deep), Some(TYPED), 2);
    let (code, json, stderr) = tool(&["ocr", &deep, "-o", &out, "--json"], &[]);
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "a scan whose picture is two forms deep is read, and the typed line kept once",
        text_of(&deep).trim() == TYPED
            && code == 0
            && result["pages"].as_array().is_some_and(|p| p.len() == 1)
            && found(&out, "quartz").len() == 1
            && found(&out, "RECEIVED").len() == 1,
        &format!("exit {code}; {stderr}; {json}"),
    );
}

/// A page the engine will not read, beside one it reads.
///
/// The second page is a picture of `multilingual.pdf`'s Arabic page. Vision in
/// the worker cannot load the model it picks for Arabic and rejects the image
/// (`docs/TRAPS.md`), and until 26.10.8 that failed the document: exit 4, no
/// copy, and the Latin page beside it got nothing.
///
/// **What is held on every machine is the property, not the refusal.** Whether
/// a recogniser reads Arabic is the machine's: a system that has the model
/// gives the page a layer, and that is as right as naming it. So the copy must
/// be written, page 1 must read as the source does, and page 2 must be
/// accounted for exactly once. The wording of a refusal is checked only where
/// one happened, and said to be skipped where none did.
///
/// **The tool and the window are held to each other**, because each walks the
/// pages in code of its own and "accounted for" alone would pass a walk that
/// filed a refused page under *nothing recognised*. They are given the same
/// picture and ask the same engine, so what one calls refused the other must.
fn a_page_the_engine_refuses(
    report: &mut Report,
    at: &dyn Fn(&str) -> String,
    source: &str,
    scanned: &str,
) {
    if !cfg!(target_os = "macos") {
        report.skip(
            "ocr",
            "a page the recogniser refuses is known for Vision only",
        );
        return;
    }
    let Some(other) = fixture("multilingual.pdf") else {
        report.skip("ocr", "testdata/multilingual.pdf is not generated");
        return;
    };
    let (picture, arabic, two) = (at("arabic.png"), at("arabic-scan.pdf"), at("two.pdf"));
    let other = other.display().to_string();
    let dpi = DPI.to_string();
    let rendered = tool(
        &[
            "render", &other, "-o", &picture, "--page", "2", "--dpi", &dpi,
        ],
        &[],
    );
    if rendered.0 != 0 {
        report.check(
            "the Arabic page renders, to make the scan from",
            false,
            &rendered.2,
        );
        return;
    }
    scan(Path::new(&picture), Path::new(&arabic));
    let merged = tool(&["merge", scanned, &arabic, "-o", &two], &[]);
    // The control: both pages are pictures, so neither is skipped for having
    // text, and the source of the second page is the Arabic one.
    report.check(
        "the control: a Latin scan and an Arabic scan, neither with any text",
        merged.0 == 0
            && text_of(&two).trim().is_empty()
            && tool(&["text", &other, "--pages", "2"], &[])
                .1
                .contains("البحر"),
        &merged.2,
    );
    let first_page = |file: &str| tool(&["text", file, "--pages", "1"], &[]).1;
    let second_page = |file: &str| tool(&["text", file, "--pages", "2"], &[]).1;
    let want = words(&text_of(source));

    let out = at("two-out.pdf");
    let (code, json, stderr) = tool(&["ocr", &two, "-o", &out, "--json"], &[]);
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let numbers = |key: &str| -> Vec<u64> {
        result[key]
            .as_array()
            .map(|pages| pages.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default()
    };
    let layered: Vec<u64> = result["pages"]
        .as_array()
        .map(|pages| pages.iter().filter_map(|p| p["page"].as_u64()).collect())
        .unwrap_or_default();
    let (refused, nothing) = (numbers("refused"), numbers("nothing_read"));
    let mut accounted: Vec<u64> = [&layered[..], &refused[..], &nothing[..]].concat();
    accounted.sort_unstable();
    let shared = if code == 0 {
        overlap(&want, &words(&first_page(&out)))
    } else {
        0.0
    };
    report.check(
        "a page the engine may refuse does not cost the other page its layer",
        code == 0
            && Path::new(&out).exists()
            && layered.contains(&1)
            && shared >= ENOUGH
            && accounted == [1, 2],
        &format!("exit {code}; {stderr}; {shared:.2}; {json}"),
    );
    if refused == [2] {
        let plain = at("two-plain.pdf");
        let (code, said, _) = tool(&["ocr", &two, "-o", &plain], &[]);
        report.check(
            "the refused page is named with what that usually means, and gets no text",
            code == 0
                && said.contains("text added to 1 of 2 pages")
                && said.contains(&format!(
                    "The recogniser refused page 2, {}",
                    tpdf_lib::ocr_layer::REFUSED_MEANS
                ))
                && second_page(&out).trim().is_empty(),
            &said,
        );
        // With no page left to give a layer to, nothing is written, and the
        // reason is the refusal and not an empty page.
        let none = at("arabic-out.pdf");
        let (code, _, stderr) = tool(&["ocr", &arabic, "-o", &none], &[]);
        report.check(
            "a document whose only page is refused is not written, and the reason says so",
            code == 3
                && stderr.contains("the recogniser refused page 1")
                && stderr.contains("no copy was written")
                && !Path::new(&none).exists(),
            &format!("exit {code}; {stderr}"),
        );
    } else {
        report.skip(
            "ocr",
            "this machine's recogniser did not refuse the Arabic page, so the refusal's wording was not checked",
        );
    }

    // The window's command, which walks the pages itself.
    let as_u32 = |pages: &[u64]| -> Vec<u32> { pages.iter().map(|n| *n as u32).collect() };
    let (tool_refused, tool_nothing) = (as_u32(&refused), as_u32(&nothing));
    let window = at("two-window.pdf");
    let (answer, seen) = app_path(Path::new(&two), Path::new(&window), Ask::Plain);
    let shared = if answer.is_ok() {
        overlap(&want, &words(&first_page(&window)))
    } else {
        0.0
    };
    report.check(
        "the window's command gives the other page its layer too, and files page 2 as the tool does",
        answer.as_ref().is_ok_and(|read| {
            let mut accounted: Vec<u32> = read
                .pages
                .iter()
                .map(|page| page.page)
                .chain(read.refused.iter().copied())
                .chain(read.nothing_read.iter().copied())
                .collect();
            accounted.sort_unstable();
            read.pages.iter().any(|page| page.page == 1 && page.words > 0)
                && accounted == [1, 2]
                && read.refused == tool_refused
                && read.nothing_read == tool_nothing
        }) && seen == [Progress { page: 1, of: 2 }, Progress { page: 2, of: 2 }]
            && shared >= ENOUGH,
        &format!("{answer:?}; {seen:?}; {shared:.2}; the tool refused {tool_refused:?}"),
    );
    if tool_refused == [2] {
        let none = at("arabic-window.pdf");
        let (answer, _) = app_path(Path::new(&arabic), Path::new(&none), Ask::Plain);
        report.check(
            "the window's command names a refused page when it writes nothing",
            second_page(&window).trim().is_empty()
                && answer.as_ref().is_err_and(|why| {
                    why.contains("The recogniser refused page 1")
                        && why.contains("No copy was written.")
                })
                && !Path::new(&none).exists(),
            &format!("{answer:?}"),
        );
    } else {
        report.skip(
            "ocr",
            "the recogniser did not refuse the Arabic page, so the window's refusal was not checked",
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
        shared >= ENOUGH,
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

    another_script(report, &at);
    a_page_the_engine_refuses(report, &at, &source, &scanned);

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

    a_scan_with_a_typed_line(report, &at, &source, &picture);
    a_scan_another_program_read(report, &at, &source, &picture);
    a_scan_two_forms_deep(report, &at, &picture);

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
