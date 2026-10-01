//! `sign --image <path>`: the image drawn is the file's, read from the signed
//! document's own image XObject, and a file that cannot be used ends the run
//! before the store is asked for anything.
use super::{
    certificate, key, now, plain_pdf, read_back, scratch, signs, strings, Report, SUBJECT,
};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use tpdf_lib::cli::{Held, Store};
use tpdf_lib::sign_cms::{Key, KeyKind};
use tpdf_lib::signature::Image;

/// What the store was asked for.
#[derive(Default)]
struct Asked {
    identities: Cell<usize>,
    signed: Cell<usize>,
    saved_image: Cell<usize>,
}

struct Counted(p256::ecdsa::SigningKey, Rc<Asked>);

impl Key for Counted {
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        self.1.signed.set(self.1.signed.get() + 1);
        super::Soft(self.0.clone()).sign_digest(kind, digest)
    }
}

/// A store that counts, holding `saved` as the reader's saved image.
struct Counting {
    certificate: Vec<u8>,
    saved: Option<Image>,
    asked: Rc<Asked>,
}

impl Store for Counting {
    fn identities(&self) -> Result<Vec<Held>, String> {
        self.asked.identities.set(self.asked.identities.get() + 1);
        Ok(vec![Held {
            certificate: self.certificate.clone(),
            chain: Vec::new(),
            key: Box::new(Counted(key(), self.asked.clone())),
        }])
    }

    fn saved_image(&self) -> Result<Option<Image>, String> {
        self.asked.saved_image.set(self.asked.saved_image.get() + 1);
        Ok(self.saved.clone())
    }
}

/// A solid image of one colour, as the saved image or as a file's pixels.
fn solid(width: u32, height: u32, colour: [u8; 4]) -> Image {
    Image {
        width,
        height,
        rgba: colour.repeat(width as usize * height as usize),
    }
}

fn png_file(path: &Path, image: &Image) {
    let file = std::fs::File::create(path).expect("png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("header");
    writer.write_image_data(&image.rgba).expect("pixels");
    writer.finish().expect("end");
}

/// Every image the signed document draws, as straight RGBA: the colour image
/// with its soft mask. A plain document has none of its own.
fn drawn(path: &Path) -> Vec<Image> {
    let doc = lopdf::Document::load(path).expect("signed document");
    let number = |dict: &lopdf::Dictionary, key: &[u8]| {
        dict.get(key).and_then(lopdf::Object::as_i64).unwrap_or(0) as u32
    };
    let mut images = Vec::new();
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        let Ok(mask) = stream
            .dict
            .get(b"SMask")
            .and_then(lopdf::Object::as_reference)
        else {
            continue;
        };
        let rgb = stream.decompressed_content().expect("colour");
        let alpha = doc
            .get_object(mask)
            .and_then(lopdf::Object::as_stream)
            .expect("mask")
            .decompressed_content()
            .expect("alpha");
        images.push(Image {
            width: number(&stream.dict, b"Width"),
            height: number(&stream.dict, b"Height"),
            rgba: rgb
                .chunks_exact(3)
                .zip(&alpha)
                .flat_map(|(p, a)| [p[0], p[1], p[2], *a])
                .collect(),
        });
    }
    images
}

/// Whether the signed document's appearance shows any text.
fn shows_text(path: &Path) -> bool {
    let doc = lopdf::Document::load(path).expect("signed document");
    doc.objects.values().any(|object| {
        object.as_stream().is_ok_and(|stream| {
            stream
                .dict
                .get(b"Subtype")
                .and_then(lopdf::Object::as_name)
                .is_ok_and(|name| name == b"Form")
                && stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone())
                    .windows(2)
                    .any(|w| w == b"Tj")
        })
    })
}

pub fn draws_the_file(report: &mut Report) {
    let now = now();
    let dir = scratch("sign-image");
    let s = |p: &Path| p.display().to_string();
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let stamp = solid(40, 20, [200, 30, 40, 255]);
    let saved = solid(64, 32, [20, 30, 120, 255]);
    let stamp_file = dir.join("stamp.png");
    png_file(&stamp_file, &stamp);
    let store = |saved: Option<&Image>| Counting {
        certificate: certificate(now),
        saved: saved.cloned(),
        asked: Rc::default(),
    };
    let line = |out: &Path, extra: &[&str]| {
        let mut args = strings(&[
            "sign",
            &s(&plain),
            "-o",
            &s(out),
            "--identity",
            SUBJECT,
            "--visible",
            "--rect",
            "40,40,200,80",
        ]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };

    // The file's image is drawn, whether or not an image is saved, and the
    // saved image is never read.
    for (what, held) in [("no saved image", None), ("a saved image", Some(&saved))] {
        let out = dir.join(format!("file-{}.pdf", held.is_some()));
        let store = store(held);
        let (code, _, stderr) = signs(&line(&out, &["--image", &s(&stamp_file)]), &store, now);
        report.check(
            &format!("--image with {what}: sign exits 0"),
            code == 0,
            &stderr,
        );
        report.check(
            &format!("--image with {what}: the document draws the file's pixels and no others"),
            out.exists() && drawn(&out) == [stamp.clone()],
            &format!(
                "{:?}",
                drawn(&out)
                    .iter()
                    .map(|i| (i.width, i.height, i.rgba[..4].to_vec()))
                    .collect::<Vec<_>>()
            ),
        );
        report.check(
            &format!("--image with {what}: the saved image is never read"),
            store.asked.saved_image.get() == 0 && store.asked.signed.get() == 1,
            &format!(
                "saved image read {} time(s), key used {}",
                store.asked.saved_image.get(),
                store.asked.signed.get()
            ),
        );
        report.check(
            &format!("--image with {what}: the signature reads back intact"),
            read_back(&out).is_ok_and(|(lines, _)| lines.len() == 1 && lines[0].1 == "intact"),
            &format!("{:?}", read_back(&out).map(|(lines, _)| lines)),
        );
    }

    // Control: without --image the saved image is what is drawn.
    let out = dir.join("saved.pdf");
    let held = store(Some(&saved));
    let (code, _, stderr) = signs(&line(&out, &[]), &held, now);
    report.check(
        "control: without --image the saved image is drawn, read once",
        code == 0 && drawn(&out) == [saved.clone()] && held.asked.saved_image.get() == 1,
        &format!(
            "exit {code}, read {}: {stderr}",
            held.asked.saved_image.get()
        ),
    );

    // The image alone: no line of text in the appearance. Control: with the
    // default lines there is text.
    let alone = dir.join("alone.pdf");
    let (code, _, stderr) = signs(
        &line(&alone, &["--image", &s(&stamp_file), "--lines", ""]),
        &store(None),
        now,
    );
    let worded = dir.join("file-false.pdf");
    report.check(
        "--image with --lines \"\": the image and no text; with the default lines, text",
        code == 0 && drawn(&alone) == [stamp.clone()] && !shows_text(&alone) && shows_text(&worded),
        &format!(
            "exit {code}, text {} / {}: {stderr}",
            shows_text(&alone),
            shows_text(&worded)
        ),
    );

    // A reason that is written and not drawn: the image stays the whole
    // appearance. Control: without --hide the same line draws it.
    for (what, extra, text) in [
        ("with --hide reason", &["--hide", "reason"][..], false),
        ("without --hide", &[][..], true),
    ] {
        let out = dir.join(format!("reason-{text}.pdf"));
        let mut args = line(
            &out,
            &[
                "--image",
                &s(&stamp_file),
                "--lines",
                "",
                "--reason",
                "Document approved",
            ],
        );
        args.extend(extra.iter().map(|a| (*a).to_string()));
        let (code, _, stderr) = signs(&args, &store(None), now);
        let reason = std::fs::read(&out).ok().and_then(|bytes| {
            let found = tpdf_lib::docinfo::scan(&bytes, 1, None).ok()?;
            found.signatures.first().map(|s| s.reason.clone())
        });
        report.check(
            &format!("--reason {what}: /Reason is set, and the appearance has text: {text}"),
            code == 0
                && reason.as_deref() == Some("Document approved")
                && shows_text(&out) == text
                && drawn(&out) == [stamp.clone()],
            &format!(
                "exit {code}, reason {reason:?}, text {}: {stderr}",
                shows_text(&out)
            ),
        );
    }

    // A larger picture arrives scaled, as the chooser would hand it on.
    let large = dir.join("large.png");
    png_file(&large, &solid(1200, 300, [5, 90, 5, 255]));
    let out = dir.join("large.pdf");
    let (code, _, stderr) = signs(&line(&out, &["--image", &s(&large)]), &store(None), now);
    report.check(
        "--image: a 1200 by 300 picture is drawn at 512 by 128",
        code == 0 && drawn(&out) == [solid(512, 128, [5, 90, 5, 255])],
        &format!("exit {code}: {stderr}"),
    );

    // Files that cannot be used: exit 3, nothing written, the path and the
    // reason said, and the store never asked for a certificate or a key.
    let missing = dir.join("missing.png");
    let text = dir.join("notes.png");
    std::fs::write(&text, b"not an image at all").expect("text");
    let empty = dir.join("empty.png");
    std::fs::write(&empty, b"").expect("empty");
    let cut = dir.join("cut.png");
    let whole = std::fs::read(&stamp_file).expect("stamp");
    std::fs::write(&cut, &whole[..whole.len() - 9]).expect("cut");
    let huge = dir.join("huge.png");
    let mut padded = whole.clone();
    padded.resize(10 * 1024 * 1024 + 1, 0);
    std::fs::write(&huge, &padded).expect("huge");
    let clear = dir.join("clear.png");
    png_file(&clear, &solid(8, 8, [0, 0, 0, 0]));
    let a_pdf = dir.join("a.png");
    std::fs::write(&a_pdf, plain_pdf()).expect("pdf");
    for (what, file, says) in [
        ("a missing file", &missing, "missing.png"),
        ("a text file", &text, "not a valid, still PNG or JPEG"),
        ("an empty file", &empty, "not a valid, still PNG or JPEG"),
        ("a PNG cut short", &cut, "not a valid, still PNG or JPEG"),
        ("a file over 10 MB", &huge, "not a valid, still PNG or JPEG"),
        ("a PDF", &a_pdf, "not a valid, still PNG or JPEG"),
        ("a transparent image", &clear, "transparent everywhere"),
    ] {
        let out = dir.join("refused.pdf");
        let store = store(Some(&saved));
        let (code, stdout, stderr) = signs(&line(&out, &["--image", &s(file)]), &store, now);
        let asked = &store.asked;
        report.check(
            &format!("--image, {what}: refused (3), names the file and why, nothing written"),
            code == 3
                && !out.exists()
                && stdout.is_empty()
                && stderr.contains(&s(file))
                && stderr.contains(says),
            &format!("exit {code}, exists {}: {stderr}", out.exists()),
        );
        report.check(
            &format!("--image, {what}: no certificate listed, no key used, no saved image read"),
            asked.identities.get() == 0 && asked.signed.get() == 0 && asked.saved_image.get() == 0,
            &format!(
                "identities {}, key {}, saved image {}",
                asked.identities.get(),
                asked.signed.get(),
                asked.saved_image.get()
            ),
        );
    }

    // A document that does not open is the document's refusal, not the image's.
    let junk = dir.join("junk.pdf");
    std::fs::write(&junk, b"this is not a PDF").expect("junk");
    let out = dir.join("junk-signed.pdf");
    let mut args = line(&out, &["--image", &s(&stamp_file)]);
    args[1] = s(&junk);
    let (code, _, stderr) = signs(&args, &store(None), now);
    report.check(
        "--image on a file that is not a PDF: the document is what is refused",
        code == 3 && !out.exists() && stderr.contains("not a PDF") && !stderr.contains("the image"),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
