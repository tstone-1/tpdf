//! `tpdf compress` through the shipped tool.
//!
//! The document goes in, a smaller one comes out, and the tool's own renderer
//! and text reader say what a reader of the copy gets: the same pixels when no
//! picture was asked to change, the same words always, and a coarser picture
//! when one was. `qpdf`, where it is installed, reads the object streams back
//! as a second opinion that shares no code with the writer.

use std::path::Path;
use std::process::Command;

use lopdf::{dictionary, Dictionary, Document, Object, Stream};

use super::{fixture, scratch, tool, Report};

/// One page: a line of text, and a 600 by 600 photograph two inches square,
/// stored without a filter so the file starts large.
fn photo_pdf() -> Vec<u8> {
    let mut state = 0x2545_f491u32;
    let pixels: Vec<u8> = (0..600 * 600 * 3)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            // Smooth in rows, so it looks like a picture and not like static.
            (state >> 26) as u8 * 4
        })
        .collect();
    let mut doc = Document::with_version("1.4");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let picture = doc.add_object(
        Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 600, "Height" => 600,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
            },
            pixels,
        )
        .with_compression(false),
    );
    let body = "BT /F1 14 Tf 72 700 Td (COMPRESS-KEEP these words) Tj ET\n\
                q 144 0 0 144 72 400 cm /Im Do Q\n";
    let content = doc.add_object(
        Stream::new(Dictionary::new(), body.as_bytes().to_vec()).with_compression(false),
    );
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Im" => picture },
        },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }
            .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialises");
    bytes
}

/// The picture in `path`: its width in pixels and its filter.
fn picture_of(path: &str) -> (i64, String) {
    let doc = Document::load(path).expect("the copy loads");
    let stream = doc
        .objects
        .values()
        .filter_map(|object| object.as_stream().ok())
        .find(|stream| stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
        .expect("the picture");
    let width = stream
        .dict
        .get(b"Width")
        .and_then(Object::as_i64)
        .expect("a width");
    let filter = match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => String::from_utf8_lossy(name).into_owned(),
        _ => "none".to_string(),
    };
    (width, filter)
}

fn size(path: &str) -> u64 {
    std::fs::metadata(path).map(|data| data.len()).unwrap_or(0)
}

/// The page as the tool draws it at one pixel a point.
fn drawn(path: &str, into: &str) -> Vec<u8> {
    let (code, _, stderr) = tool(&["render", path, "--dpi", "72", "-o", into, "--force"], &[]);
    assert_eq!(code, 0, "{path} renders: {stderr}");
    super::images::pixels(into).2
}

fn parsed(json: &str) -> serde_json::Value {
    serde_json::from_str(json).unwrap_or_default()
}

pub(super) fn writes_a_smaller_copy(report: &mut Report) {
    let dir = scratch("compress");
    let at = |name: &str| dir.join(name).display().to_string();
    let source = at("photo.pdf");
    std::fs::write(&source, photo_pdf()).expect("the input");
    let before = size(&source);
    let original = drawn(&source, &at("source.png"));
    let (_, words, _) = tool(&["text", &source], &[]);
    report.check(
        "control: the input has its words and a 600 pixel picture stored plainly",
        words.contains("COMPRESS-KEEP these words") && picture_of(&source) == (600, "none".into()),
        &words,
    );

    // ---- Without --pictures: smaller, and not one pixel different.
    let lossless = at("lossless.pdf");
    let (code, json, stderr) = tool(&["compress", &source, "-o", &lossless, "--json"], &[]);
    let said = parsed(&json);
    report.check(
        "compress reports a smaller copy and that no picture was asked to change",
        code == 0
            && said["command"] == "compress"
            && said["written"] == true
            && said["output"] == lossless.as_str()
            && said["preset"].is_null()
            && said["dpi"].is_null()
            && said["bytes_before"] == before
            && said["bytes_after"] == size(&lossless)
            && size(&lossless) < before
            && said["pictures_total"].is_null(),
        &format!("exit {code}: {stderr}; {json}"),
    );
    report.check(
        "the copy draws the page pixel for pixel as the input does, with the picture at 600",
        drawn(&lossless, &at("lossless.png")) == original && picture_of(&lossless).0 == 600,
        &format!("{:?}", picture_of(&lossless)),
    );
    let raw = std::fs::read(&lossless).unwrap_or_default();
    report.check(
        "and is written with object streams",
        raw.windows(7).any(|window| window == b"/ObjStm"),
        "no /ObjStm",
    );
    match Command::new("qpdf").args(["--check", &lossless]).output() {
        Ok(out) => report.check(
            "qpdf reads the copy with nothing to warn about",
            out.status.code() == Some(0),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(_) => report.skip("qpdf reads the smaller copy", "qpdf is unavailable"),
    }

    // ---- A copy that would not be smaller is not written.
    let again = at("again.pdf");
    let (code, _, stderr) = tool(&["compress", &lossless, "-o", &again], &[]);
    report.check(
        "a copy that would not be smaller is refused (3) and not written",
        code == 3 && stderr.contains("cannot be made smaller") && !Path::new(&again).exists(),
        &format!("exit {code}: {stderr}"),
    );

    // ---- A dry run and a preview: what it would come to, and nothing written.
    let preview = at("preview.png");
    let (code, json, stderr) = tool(
        &[
            "compress",
            &source,
            "--dry-run",
            "--pictures",
            "screen",
            "--preview",
            &preview,
            "--json",
        ],
        &[],
    );
    let estimated = parsed(&json);
    report.check(
        "a dry run writes no document and says what the copy would come to",
        code == 0
            && estimated["written"] == false
            && estimated["output"].is_null()
            && estimated["preset"] == "screen"
            && estimated["dpi"] == 110
            && estimated["quality"] == 60
            && estimated["jpeg"] == true
            && estimated["pictures_total"] == 1
            && estimated["pictures_changed"] == 1
            && estimated["bytes_after"]
                .as_u64()
                .is_some_and(|after| after * 4 < before),
        &format!("exit {code}: {stderr}; {json}"),
    );
    let (wide, high, shown) = if Path::new(&preview).exists() {
        super::images::pixels(&preview)
    } else {
        (0, 0, Vec::new())
    };
    // Before, eight white pixels, after: 320 + 8 + 320.
    let halves_differ = wide == 648
        && shown
            .chunks_exact(648 * 4)
            .any(|row| row[..320 * 4] != row[328 * 4..]);
    report.check(
        "the preview is the page before and after, side by side, and the two differ",
        estimated["preview"] == preview.as_str() && (wide, high) == (648, 320) && halves_differ,
        &format!("{wide}x{high}"),
    );

    // ---- For a screen: the picture is scaled and stored as JPEG.
    let screen = at("screen.pdf");
    let (code, json, stderr) = tool(
        &[
            "compress",
            &source,
            "-o",
            &screen,
            "--pictures",
            "screen",
            "--json",
        ],
        &[],
    );
    let said = parsed(&json);
    // Two inches at 110 pixels an inch.
    report.check(
        "--pictures screen scales the picture to 220 pixels and stores it as JPEG",
        code == 0 && said["preset"] == "screen" && picture_of(&screen) == (220, "DCTDecode".into()),
        &format!("exit {code}: {stderr}; {:?}", picture_of(&screen)),
    );
    report.check(
        "the copy is the size the dry run said, and a fraction of the lossless one",
        said["bytes_after"] == estimated["bytes_after"] && size(&screen) * 4 < size(&lossless),
        &format!(
            "{} against {}",
            said["bytes_after"], estimated["bytes_after"]
        ),
    );
    let (_, kept, _) = tool(&["text", &screen], &[]);
    let coarse = drawn(&screen, &at("screen.png"));
    // The text is drawn by the same operators: the rows above the picture are
    // the input's, and the page as a whole is not.
    let row = 612 * 4;
    report.check(
        "its words are the input's, the text is drawn as it was and the picture is not",
        kept == words && coarse[..row * 120] == original[..row * 120] && coarse != original,
        &kept,
    );

    // ---- The reader's own numbers.
    let own = at("own.pdf");
    let (code, json, stderr) = tool(
        &[
            "compress",
            &source,
            "-o",
            &own,
            "--dpi",
            "200",
            "--quality",
            "30",
            "--no-jpeg",
            "--json",
        ],
        &[],
    );
    let said = parsed(&json);
    report.check(
        "--dpi, --quality and --no-jpeg are followed and reported as custom",
        code == 0
            && said["preset"] == "custom"
            && said["dpi"] == 200
            && said["quality"] == 30
            && said["jpeg"] == false
            && picture_of(&own) == (400, "FlateDecode".into()),
        &format!("exit {code}: {stderr}; {json}; {:?}", picture_of(&own)),
    );
    let (code, _, stderr) = tool(
        &["compress", &source, "-o", &at("bad.pdf"), "--dpi", "5"],
        &[],
    );
    report.check(
        "a resolution outside what is offered is a malformed line (2)",
        code == 2 && stderr.contains("it may be 20 to 1200") && !Path::new(&at("bad.pdf")).exists(),
        &format!("exit {code}: {stderr}"),
    );

    // ---- An output that exists, and a document with a password.
    let (code, _, stderr) = tool(
        &["compress", &source, "-o", &screen, "--pictures", "print"],
        &[],
    );
    report.check(
        "an output that exists is refused (3) and left as it was",
        code == 3 && picture_of(&screen).0 == 220,
        &format!("exit {code}: {stderr}"),
    );
    match fixture("incr-encrypted-pw.pdf") {
        Some(locked) => {
            let locked = locked.display().to_string();
            let out = at("locked.pdf");
            let (code, _, stderr) = tool(
                &[
                    "compress",
                    &locked,
                    "-o",
                    &out,
                    "--password-env",
                    "KEY",
                    "--force",
                ],
                &[("KEY", "swordfish")],
            );
            // A small fixture may not shrink; either answer has to keep the lock.
            let (without, _, _) = tool(&["text", &out], &[]);
            let (with, _, _) = tool(
                &["text", &out, "--password-env", "KEY"],
                &[("KEY", "swordfish")],
            );
            report.check(
                "a smaller copy of a locked document still needs its password, or is not written",
                (code == 0 && without == 3 && with == 0)
                    || (code == 3 && !Path::new(&out).exists()),
                &format!("exit {code}, {without}, {with}: {stderr}"),
            );
        }
        None => report.skip("compress keeps a password", "testdata/ is not generated"),
    }
    let _ = std::fs::remove_dir_all(dir);
}
