//! `tpdf images` through the shipped tool.
//!
//! The picture goes in, the document comes out, and the tool's own renderer
//! draws the page: at one pixel a point the drawing must be the picture.

use std::path::Path;
use std::process::Command;

use super::{fixture, scratch, tool, Report};

const JPEG: &[u8] = include_bytes!("../../src/textedit/images/synthetic-rgb.jpg");

/// A PNG's size and its pixels as RGBA.
fn pixels(path: &str) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).expect("the PNG opens"),
    ));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().expect("a PNG");
    let mut data = vec![0; reader.output_buffer_size().expect("a size")];
    let frame = reader.next_frame(&mut data).expect("a frame");
    data.truncate(frame.buffer_size());
    (frame.width, frame.height, data)
}

/// `jpeg` with an EXIF segment saying `orientation`.
fn oriented(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut exif = b"Exif\0\0II*\0".to_vec();
    exif.extend_from_slice(&8u32.to_le_bytes());
    exif.extend_from_slice(&1u16.to_le_bytes());
    exif.extend_from_slice(&0x0112u16.to_le_bytes());
    exif.extend_from_slice(&3u16.to_le_bytes());
    exif.extend_from_slice(&1u32.to_le_bytes());
    exif.extend_from_slice(&orientation.to_le_bytes());
    exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(&exif);
    out.extend_from_slice(&jpeg[2..]);
    out
}

pub(super) fn makes_a_document_from_pictures(report: &mut Report) {
    let Some(source) = fixture("text-base14.pdf") else {
        report.skip("images", "testdata/text-base14.pdf is not generated");
        return;
    };
    let dir = scratch("images");
    let at = |name: &str| dir.join(name).display().to_string();
    let (picture, photo, turned, out, back) = (
        at("page.png"),
        at("photo.jpg"),
        at("turned.jpg"),
        at("out.pdf"),
        at("back.png"),
    );
    let source = source.display().to_string();
    let (code, _, stderr) = tool(&["render", &source, "-o", &picture, "--dpi", "72"], &[]);
    if code != 0 {
        report.check(
            "the source renders, to make the picture from",
            false,
            &stderr,
        );
        return;
    }
    std::fs::write(&photo, JPEG).unwrap();
    std::fs::write(&turned, oriented(JPEG, 6)).unwrap();
    let (width, height, original) = pixels(&picture);

    let (code, json, stderr) = tool(
        &["images", &picture, &photo, &turned, "-o", &out, "--json"],
        &[],
    );
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let size = |page: usize| {
        (
            result["pages"][page]["width_pt"].as_f64().unwrap_or(0.0),
            result["pages"][page]["height_pt"].as_f64().unwrap_or(0.0),
        )
    };
    report.check(
        "images reports one page for each picture, at the picture's size",
        code == 0
            && result["command"] == "images"
            && result["pages"].as_array().is_some_and(|p| p.len() == 3)
            && size(0) == (f64::from(width), f64::from(height))
            && result["pages"][1]["source"] == photo.as_str(),
        &format!("exit {code}; {stderr}; {json}"),
    );
    // The fixture is a gradient: red grows to the right and green downwards.
    // Turned a quarter clockwise, its bottom-left corner is shown top-left,
    // so the top-left pixel goes from dark to green.
    let corner = |page: &str| {
        let shown = at(&format!("corner-{page}.png"));
        let (code, _, _) = tool(
            &["render", &out, "--page", page, "--dpi", "72", "-o", &shown],
            &[],
        );
        let (_, _, data) = if code == 0 {
            pixels(&shown)
        } else {
            (0, 0, vec![0; 4])
        };
        (data[0], data[1])
    };
    let (upright, quarter) = (corner("2"), corner("3"));
    report.check(
        "a photograph is turned the way its EXIF orientation says",
        upright.0 < 60 && upright.1 < 60 && quarter.0 < 60 && quarter.1 > 150,
        &format!("top-left red and green: {upright:?} upright, {quarter:?} turned"),
    );

    // At one pixel a point, the page drawn again is the picture.
    let (code, _, stderr) = tool(
        &["render", &out, "--page", "1", "--dpi", "72", "-o", &back],
        &[],
    );
    let (again_w, again_h, again) = if code == 0 {
        pixels(&back)
    } else {
        (0, 0, Vec::new())
    };
    let differing = original
        .iter()
        .zip(&again)
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    report.check(
        "the page rendered at 72 DPI is the picture, pixel for pixel",
        code == 0
            && (again_w, again_h) == (width, height)
            && again.len() == original.len()
            && differing == 0
            // The control: the picture is not blank, so equal is not empty.
            && original.chunks_exact(4).any(|p| p[0] < 128),
        &format!("exit {code}; {stderr}; {differing} samples differ"),
    );

    match Command::new("qpdf").args(["--check", &out]).output() {
        Ok(checked) => report.check(
            "qpdf finds nothing to warn about in the document",
            checked.status.code() == Some(0),
            &String::from_utf8_lossy(&checked.stderr),
        ),
        Err(_) => report.skip("qpdf reads the document", "qpdf is unavailable"),
    }

    let paper = at("paper.pdf");
    let (code, json, stderr) = tool(
        &["images", &picture, "-o", &paper, "--paper", "a4", "--json"],
        &[],
    );
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "--paper a4 puts the picture on an A4 page",
        code == 0
            && result["pages"][0]["width_pt"] == 595.0
            && result["pages"][0]["height_pt"] == 842.0,
        &format!("exit {code}; {stderr}; {json}"),
    );

    let kept = std::fs::read(&out).unwrap();
    let refused = at("refused.pdf");
    for (args, status, sentence) in [
        (vec!["images", &picture, "-o", &out], 3, "--force"),
        (
            vec!["images", &picture, &source, "-o", &refused],
            3,
            "text-base14.pdf cannot be used: it is not a PNG or JPEG",
        ),
        (
            vec!["images", &picture, "-o", &picture],
            2,
            "names one of the pictures",
        ),
        (vec!["images", "-o", &refused], 2, "at least one picture"),
        (
            vec!["images", &picture, "-o", &refused, "--dpi", "9000"],
            2,
            "9000 DPI",
        ),
    ] {
        let (code, _, stderr) = tool(&args, &[]);
        report.check(
            "a refused images says why, writes nothing and leaves an existing file alone",
            code == status
                && stderr.contains(sentence)
                && std::fs::read(&out).unwrap() == kept
                && !Path::new(&refused).exists(),
            &format!("{args:?}: exit {code}; {stderr}"),
        );
    }
    report.check(
        "images leaves no staging directories",
        std::fs::read_dir(&dir).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tpdf-cli-")
        }),
        "a staging directory was left beside the output",
    );
}
