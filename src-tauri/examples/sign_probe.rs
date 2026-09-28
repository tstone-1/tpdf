//! Does what tpdf signs verify under readers that did not write it?
//!
//! Phase 6 step 2's instrument. It signs a PDF through the production path ---
//! `sign_prepare::prepare`, the function the worker runs, then
//! `sign_cms::finish` and `save::write_signed` --- with one piece swapped: the
//! key is not the OS's but an `openssl`-generated one, and **`openssl pkeyutl`
//! makes the signature value**. That keeps the key out of tpdf entirely, as the
//! OS keeps it on the shipped path, and makes the signer a program that shares
//! no code with the verifier either.
//!
//! Then three readers judge the file:
//!
//! 1. **tpdf's own** `integrity.rs`, through `docinfo::scan`, as the properties
//!    dialog reads it: every signature `intact`, one more than the input had.
//! 2. **pyHanko**, through `testdata/check_signature.py --json`: every signature
//!    `intact` and `valid`, ours covering the entire file and every earlier one
//!    an entire revision --- which is what "the earlier signature is still
//!    intact, now with a revision appended" means in its words.
//! 3. **OpenSSL**: `openssl cms -verify -binary -noverify` over the two covered
//!    pieces joined, with the CMS blob cut out of the hole. `-noverify` skips the
//!    certificate chain, which is trust and not this step's question.
//!
//! And three controls, each of which must turn every reader that can see it:
//!
//! - **the value spliced two digits late**: the hole no longer holds the value,
//!   so tpdf must not call it intact and OpenSSL must refuse the blob it cuts;
//! - **a signature over the wrong digest**, written past `finish`'s own check:
//!   tpdf `broken`, pyHanko `valid=no`, OpenSSL a verification failure;
//! - **one byte changed inside the signed range**: tpdf `altered`, pyHanko
//!   `intact=no`, OpenSSL a digest mismatch.
//!
//! Without those, three readers agreeing would say nothing: a reader that
//! answered "fine" to everything would agree too.
//!
//! ## `--visible`
//!
//! Signs through `sign_prepare::prepare_visible` instead, with a rectangle on
//! page 1 (`--rect left,top,right,bottom`, display points; `40,40,280,120` by
//! default) and a synthetic signature image, so the appearance, its image and
//! its font are inside the signed revision. Every check above still runs over
//! that file --- the appearance must not cost any reader its verdict --- and two
//! renderers that share no code are asked **where the ink went**:
//!
//! - **PDFium**, through `progressive::render` as the viewer renders a tile,
//!   and **PDFKit**, through `scripts/sign_visible_pdfkit.swift` (macOS only;
//!   elsewhere it is reported as not run, never as a pass). Each renders the
//!   original and the signed file at 2 px per point and counts the pixels that
//!   changed: inside the rectangle there must be ink --- at least 2% of it ---
//!   and outside it, past a one-pixel antialiasing band, none at all.
//! - **The control**: the same document signed *invisibly*, rendered by
//!   PDFium, must differ from the original by no pixel. Without it, "the
//!   signed page differs inside the rectangle" could be a renderer that
//!   differs everywhere.
//! - **The preview agrees with the page**: `render::run_signature_preview` ---
//!   the route the panel's live preview takes --- is asked for the same
//!   appearance, its PNG decoded, and compared pixel for pixel with the signed
//!   page inside the rectangle, wherever the original page is paper there.
//!
//! The appearance's options: `--lines` a comma list of `label`, `name` and
//! `date` (all three by default), `--reason TEXT` and `--location TEXT` (none by
//! default), and `--no-image`. A reason or location is also asserted as what
//! **pyHanko** reads out of `/Reason` and `/Location`, and their absence as its
//! reading none.
//!
//! ## `--timestamp <digicert | sectigo | globalsign | URL>`
//!
//! Phase 6 step 3, increment B, against a **real** timestamp authority: the
//! signature is stamped through `tsa::stamp` and `tsa::ask_blocking` --- the
//! path the window and `tpdf sign --timestamp` take, network and all --- and
//! sealed by `Made::seal`, which refuses a token its own reader would not call
//! intact. Then the token is judged by three readers again:
//!
//! - **tpdf**: the new signature's timestamp `intact` and attested, its time
//!   within five minutes of this machine's clock, and the authority's standing
//!   through the system store printed (and required to be `trusted`: all three
//!   listed authorities chain to roots macOS ships);
//! - **pyHanko**: the token `intact` and `valid`;
//! - **OpenSSL**: `openssl ts -verify -data <signature value octets>` over the
//!   token cut out of the written file, with this Mac's system roots as
//!   `-CAfile` --- and, the control, the same token refused over the value
//!   with one byte changed.
//!
//! A measurement, run by hand: nothing a gate runs reaches the network.
//!
//! Usage:
//!   sign-probe <input.pdf> <scratch-dir> [--key rsa|p256|p384]
//!       [--visible [--rect l,t,r,b] [--lines label,name,date] [--reason TEXT]
//!        [--location TEXT] [--no-image]]
//!       [--timestamp digicert|sectigo|globalsign|URL]
//!
//! Needs `openssl` (3.x) and `uv`. Either missing is `[FAIL]`, never a pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use tpdf_lib::document::OpenDocument;
use tpdf_lib::integrity::Verdict;
use tpdf_lib::progressive::{self, Placement, RawBitmap};
use tpdf_lib::sign_cms::{self, Key, KeyKind};
use tpdf_lib::sign_prepare::{self, Options, Visible};

struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn check(&mut self, what: &str, ok: bool, detail: &str) {
        if ok {
            self.passed += 1;
            println!("[PASS] {what}");
        } else {
            self.failed += 1;
            println!("[FAIL] {what}: {detail}");
        }
    }
}

/// A key `openssl` holds, asked to sign one digest.
struct Openssl {
    key: PathBuf,
    scratch: PathBuf,
    /// Flip a bit of the digest before signing: the wrong-digest control.
    misdirect: bool,
}

impl Key for Openssl {
    fn sign_digest(&self, _kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let mut digest = *digest;
        if self.misdirect {
            digest[0] ^= 0x01;
        }
        let input = self.scratch.join("digest.bin");
        let output = self.scratch.join("value.bin");
        std::fs::write(&input, digest).map_err(|e| e.to_string())?;
        // `-pkeyopt digest:sha256` makes RSA wrap the digest in a SHA-256
        // DigestInfo (PKCS#1 v1.5) and ECDSA answer in DER --- the two forms
        // `sign_cms::Key` promises.
        run(Command::new("openssl")
            .args(["pkeyutl", "-sign", "-pkeyopt", "digest:sha256", "-inkey"])
            .arg(&self.key)
            .arg("-in")
            .arg(&input)
            .arg("-out")
            .arg(&output))?;
        std::fs::read(&output).map_err(|e| e.to_string())
    }
}

fn run(command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| format!("{command:?}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{command:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (input, scratch) = match args.as_slice() {
        [input, scratch, ..] => (PathBuf::from(input), PathBuf::from(scratch)),
        _ => {
            eprintln!("usage: sign-probe <input.pdf> <scratch-dir> [--key rsa|p256|p384]");
            std::process::exit(2);
        }
    };
    let kind = match args
        .iter()
        .position(|a| a == "--key")
        .map(|at| args.get(at + 1))
    {
        None => "rsa",
        Some(Some(kind)) => kind.as_str(),
        Some(None) => {
            eprintln!("--key needs a value");
            std::process::exit(2);
        }
    };
    let rect = match args
        .iter()
        .position(|a| a == "--rect")
        .map(|at| args.get(at + 1))
    {
        None => [40.0, 40.0, 280.0, 120.0],
        Some(Some(text)) => {
            let numbers: Vec<f32> = text
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            match numbers.as_slice() {
                [l, t, r, b] => [*l, *t, *r, *b],
                _ => {
                    eprintln!("--rect needs four numbers: left,top,right,bottom");
                    std::process::exit(2);
                }
            }
        }
        Some(None) => {
            eprintln!("--rect needs a value");
            std::process::exit(2);
        }
    };
    let value = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        match args.get(at + 1) {
            Some(value) => Some(value.clone()),
            None => {
                eprintln!("{flag} needs a value");
                std::process::exit(2);
            }
        }
    };
    let lines = value("--lines").unwrap_or_else(|| "label,name,date".into());
    let lines: Vec<&str> = lines.split(',').map(str::trim).collect();
    if let Some(unknown) = lines
        .iter()
        .find(|l| !["label", "name", "date", ""].contains(l))
    {
        eprintln!("--lines takes label, name and date, not {unknown}");
        std::process::exit(2);
    }
    let options = Options {
        label: lines.contains(&"label"),
        name: lines.contains(&"name"),
        date: lines.contains(&"date"),
        reason: value("--reason").unwrap_or_default(),
        location: value("--location").unwrap_or_default(),
    };
    let image = (!args.iter().any(|a| a == "--no-image")).then(raster);
    let visible = args.iter().any(|a| a == "--visible").then(|| Visible {
        page: 0,
        rect,
        name: "tpdf sign-probe".into(),
        image,
        options,
    });
    let timestamp = value("--timestamp").map(|text| {
        tpdf_lib::tsa::authority(&text).unwrap_or_else(|why| {
            eprintln!("--timestamp: {}", why.sentence(""));
            std::process::exit(2);
        })
    });
    match probe(&input, &scratch, kind, visible.as_ref(), timestamp.as_ref()) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            println!("[FAIL] {e}");
            std::process::exit(2);
        }
    }
}

/// A 64 x 32 signature raster: a dark blue bar across the middle, transparent
/// elsewhere, so the image has both ink and the soft mask to honour.
fn raster() -> tpdf_lib::signature::Image {
    let (width, height) = (64u32, 32u32);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for _ in 0..width {
            if (12..20).contains(&y) {
                rgba.extend_from_slice(&[20, 30, 120, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    tpdf_lib::signature::Image {
        width,
        height,
        rgba,
    }
}

/// Page 1 of `file`, rendered at `scale` pixels per point by PDFium as the
/// viewer renders a tile: RGBA pixels, width, height.
fn render(
    bindings: progressive::Bindings,
    file: &Path,
    scale: f32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let document = OpenDocument::open(bindings, file, None)?;
    let page = document.page(0)?;
    let width = (page.width_pt() * scale).round() as u16;
    let height = (page.height_pt() * scale).round() as u16;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
    let placement = Placement::tile(&page, scale, 0, 0, 0);
    let progress = progressive::render(
        &mut bitmap,
        &page,
        placement,
        None,
        &progressive::CancelToken::new(),
    );
    if !progress.outcome.is_done() {
        return Err(format!("render did not complete: {:?}", progress.outcome));
    }
    Ok((
        bitmap.pixels().to_vec(),
        u32::from(width),
        u32::from(height),
    ))
}

/// Changed pixels between two renders: inside `rect` (display points), in the
/// one-pixel band round it, and everywhere else.
fn changed(
    before: &[u8],
    after: &[u8],
    width: u32,
    height: u32,
    rect: [f32; 4],
    scale: f32,
) -> (usize, usize, usize) {
    let edge = |v: f32| (v * scale).round() as i64;
    let (x0, y0, x1, y1) = (edge(rect[0]), edge(rect[1]), edge(rect[2]), edge(rect[3]));
    let (mut inside, mut band, mut outside) = (0, 0, 0);
    for y in 0..i64::from(height) {
        for x in 0..i64::from(width) {
            let at = ((y * i64::from(width) + x) * 4) as usize;
            let moved = (0..3).any(|c| before[at + c].abs_diff(after[at + c]) > 8);
            if !moved {
                continue;
            }
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                inside += 1;
            } else if x >= x0 - 1 && x <= x1 && y >= y0 - 1 && y <= y1 {
                band += 1;
            } else {
                outside += 1;
            }
        }
    }
    (inside, band, outside)
}

/// The two renderers' verdicts on where a visible signature's ink went.
fn where_the_ink_went(
    report: &mut Report,
    input: &Path,
    out: &Path,
    invisible: &Path,
    visible: &Visible,
    now: u64,
) -> Result<(), String> {
    const SCALE: f32 = 2.0;
    let area =
        ((visible.rect[2] - visible.rect[0]) * SCALE * (visible.rect[3] - visible.rect[1]) * SCALE)
            as usize;
    let library = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let bindings = progressive::bindings_of(progressive::bind(&library)?);
    let (before, width, height) = render(bindings, input, SCALE)?;
    let (after, aw, ah) = render(bindings, out, SCALE)?;
    if (width, height) != (aw, ah) {
        return Err(format!("PDFium renders {width}x{height} and {aw}x{ah}"));
    }
    let (inside, band, outside) = changed(&before, &after, width, height, visible.rect, SCALE);
    println!(
        "PDFium: {inside} changed px inside ({area} px), {band} on the edge, {outside} outside"
    );
    report.check(
        "PDFium: the appearance puts ink inside the rectangle",
        inside * 50 >= area,
        &format!("{inside} of {area} px"),
    );
    report.check(
        "PDFium: and none outside it",
        outside == 0,
        &format!("{outside} px changed outside"),
    );
    let (control, _, _) = render(bindings, invisible, SCALE)?;
    let (a, b, c) = changed(&before, &control, width, height, visible.rect, SCALE);
    report.check(
        "control, invisible signature: PDFium draws the page unchanged",
        a + b + c == 0,
        &format!("{a} inside, {b} edge, {c} outside"),
    );
    preview_agrees(
        report, bindings, &before, &after, width, visible, now, SCALE,
    )?;

    if !cfg!(target_os = "macos") {
        println!("[SKIP] PDFKit: not run, it needs macOS");
        return Ok(());
    }
    let written = std::fs::read(out).map_err(|e| e.to_string())?;
    let turns = {
        let document = lopdf::Document::load_mem(&written).map_err(|e| e.to_string())?;
        let first = *tpdf_lib::pagetree::ordered_pages(&document)
            .first()
            .ok_or("the signed file has no page")?;
        tpdf_lib::pagetree::displayed_page(&document, first).turns
    };
    let [inside, band, outside, area] = pdfkit(input, out, visible.rect)?;
    println!(
        "PDFKit: {inside} changed px inside ({area} px), {band} on the edge, {outside} outside"
    );
    if turns == 0 {
        report.check(
            "PDFKit: the appearance puts ink inside the rectangle",
            inside * 50 >= area,
            &format!("{inside} of {area} px"),
        );
        report.check(
            "PDFKit: and none outside it",
            outside == 0,
            &format!("{outside} px changed outside"),
        );
        return Ok(());
    }
    // A turned page. PDFKit's rasteriser draws no `/Sig` widget there at all
    // --- measured: the same appearance under `/Subtype /Stamp` is drawn, a
    // `/Tx` widget is drawn, a `/Sig` widget is not, and neither `/MK /R` nor
    // NoRotate changes that (`docs/TRAPS.md`). So the limitation is asserted,
    // to expire loudly if PDFKit changes, and the appearance itself is checked
    // through the one door PDFKit leaves open: the same file with the new
    // widget's subtype renamed, byte for byte the same length.
    report.check(
        "PDFKit: draws no /Sig widget on a turned page (a PDFKit limitation; \
         if this fails, it now does, and the check above applies instead)",
        inside + band + outside == 0,
        &format!("{inside} inside, {band} edge, {outside} outside"),
    );
    let original = std::fs::read(input).map_err(|e| e.to_string())?;
    let tail = &written[original.len()..];
    let needle: &[u8] = b"/Subtype/Widget";
    let at: Vec<usize> = tail
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(at, _)| at)
        .collect();
    let [at] = at[..] else {
        return Err(format!(
            "the new revision spells /Subtype/Widget {} times",
            at.len()
        ));
    };
    let mut stamped = written.clone();
    let from = original.len() + at;
    stamped[from..from + needle.len()].copy_from_slice(b"/Subtype/Stamp ");
    let stamped_path = out.with_extension("stamp.pdf");
    std::fs::write(&stamped_path, &stamped).map_err(|e| e.to_string())?;
    let [inside, band, outside, area] = pdfkit(input, &stamped_path, visible.rect)?;
    println!("PDFKit, as a stamp: {inside} changed px inside ({area} px), {band} on the edge, {outside} outside");
    report.check(
        "PDFKit, the same appearance as a stamp: ink inside the rectangle",
        inside * 50 >= area,
        &format!("{inside} of {area} px"),
    );
    report.check(
        "PDFKit, the same appearance as a stamp: none outside it",
        outside == 0,
        &format!("{outside} px changed outside"),
    );
    Ok(())
}

/// The panel's preview against the signed page, inside the rectangle.
///
/// The preview is asked for through `render::run_signature_preview`, the
/// function a worker runs for the panel, with the very `Visible` the signing
/// was given and the same time, so the date line is the same. Its PNG is
/// decoded and laid over the signed page's render at the rectangle; a pixel is
/// compared where the original page is paper (every channel above 247), since
/// elsewhere the page's own ink is under the appearance and the preview's
/// blank page is not.
///
/// The control: the same comparison against the *original* page must
/// disagree, or agreement would say nothing --- a preview of nothing agrees
/// with a page where the appearance drew nothing.
#[allow(clippy::too_many_arguments)]
fn preview_agrees(
    report: &mut Report,
    bindings: progressive::Bindings,
    before: &[u8],
    after: &[u8],
    width: u32,
    visible: &Visible,
    now: u64,
    scale: f32,
) -> Result<(), String> {
    let preview = tpdf_lib::render::run_signature_preview(bindings, now, visible)?;
    let decoder = png::Decoder::new(std::io::Cursor::new(&preview.png));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut pixels = vec![0u8; reader.output_buffer_size().ok_or("no PNG size")?];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    let (pw, ph) = (frame.width, frame.height);
    let [l, t, r, b] = visible.rect.map(|v| (v * scale).round() as u32);
    report.check(
        "preview: drawn at the rectangle's size",
        (pw, ph) == (r - l, b - t) && (pw, ph) == (preview.width, preview.height),
        &format!("{pw}x{ph} for a {}x{} rectangle", r - l, b - t),
    );
    let compare = |page: &[u8]| {
        let (mut compared, mut differ) = (0usize, 0usize);
        for y in 0..ph.min(b - t) {
            for x in 0..pw.min(r - l) {
                let at = (((t + y) * width + (l + x)) * 4) as usize;
                if (0..3).any(|c| before[at + c] <= 247) {
                    continue;
                }
                let from = ((y * pw + x) * 4) as usize;
                compared += 1;
                if (0..3).any(|c| page[at + c].abs_diff(pixels[from + c]) > 8) {
                    differ += 1;
                }
            }
        }
        (compared, differ)
    };
    let (compared, differ) = compare(after);
    let area = ((r - l) * (b - t)) as usize;
    println!(
        "preview: {differ} of {compared} paper pixels differ from the signed page ({area} px)"
    );
    report.check(
        "preview: the signed page shows what the preview showed",
        compared * 2 >= area && differ * 1000 <= compared,
        &format!("{differ} of {compared} compared pixels differ ({area} px in the rectangle)"),
    );
    let (compared, differ) = compare(before);
    report.check(
        "control, preview against the unsigned page: they disagree",
        differ * 50 >= compared,
        &format!("{differ} of {compared} differ"),
    );
    Ok(())
}

/// `scripts/sign_visible_pdfkit.swift` on page 1: changed pixels inside the
/// rectangle, on its edge, outside it, and the rectangle's area.
fn pdfkit(input: &Path, out: &Path, rect: [f32; 4]) -> Result<[usize; 4], String> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/sign_visible_pdfkit.swift");
    let [l, t, r, b] = rect.map(|v| v.to_string());
    let said = run(Command::new("swift")
        .arg(&script)
        .arg(input)
        .arg(out)
        .arg("0")
        .args([l, t, r, b]))?;
    let numbers: Vec<usize> = said
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    match numbers[..] {
        [inside, band, outside, area] => Ok([inside, band, outside, area]),
        _ => Err(format!("PDFKit's check said {said:?}")),
    }
}

/// A key and a self-issued certificate for it, made by `openssl`.
fn credentials(scratch: &Path, kind: &str) -> Result<(PathBuf, Vec<u8>), String> {
    let key = scratch.join("key.pem");
    let pem = scratch.join("cert.pem");
    let der = scratch.join("cert.der");
    let algorithm: &[&str] = match kind {
        "rsa" => &["-newkey", "rsa:2048"],
        "p256" => &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256"],
        "p384" => &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-384"],
        other => return Err(format!("unknown key kind {other}")),
    };
    run(Command::new("openssl")
        .args(["req", "-x509", "-nodes", "-days", "2", "-sha256"])
        .args(algorithm)
        .args([
            "-subj",
            "/CN=tpdf sign-probe",
            "-addext",
            "keyUsage=critical,digitalSignature",
        ])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&pem))?;
    run(Command::new("openssl")
        .args(["x509", "-outform", "DER", "-in"])
        .arg(&pem)
        .arg("-out")
        .arg(&der))?;
    Ok((key, std::fs::read(&der).map_err(|e| e.to_string())?))
}

/// Every signed field's verdict, as the properties dialog reads it.
fn tpdf_verdicts(bytes: &[u8]) -> Result<Vec<(String, Verdict)>, String> {
    Ok(tpdf_lib::docinfo::scan(bytes, 1, None)?
        .signatures
        .into_iter()
        .filter(|s| s.signed)
        .map(|s| (s.field, s.integrity.unwrap_or_default().verdict))
        .collect())
}

/// What pyHanko concludes about each signature in `file`.
fn pyhanko(file: &Path) -> Result<Vec<serde_json::Value>, String> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/check_signature.py");
    let out = run(Command::new("uv")
        .args(["run", "--with", "pyhanko", "--quiet", "python3"])
        .arg(&script)
        .arg("--json")
        .arg(file))?;
    let found: Vec<serde_json::Value> = out
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{e}: {line}")))
        .collect::<Result<_, _>>()?;
    if let Some(reason) = found.iter().find_map(|v| v.get("unreadable")) {
        return Err(format!("pyHanko cannot read {}: {reason}", file.display()));
    }
    Ok(found)
}

/// pyHanko's one-line summary, for the modification level it reports.
fn pyhanko_summary(file: &Path) -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/check_signature.py");
    run(Command::new("uv")
        .args(["run", "--with", "pyhanko", "--quiet", "python3"])
        .arg(&script)
        .arg(file))
    .unwrap_or_else(|e| e)
}

/// `openssl cms -verify` over the signature whose value sits at `range`.
///
/// The blob is cut out of the hole exactly as a verifier would: the hex
/// between `<` and `>`, decoded, and ended where its structure ends.
fn openssl_verifies(bytes: &[u8], range: [u64; 4], scratch: &Path) -> Result<(), String> {
    let [_, first, second, _] = range.map(|n| n as usize);
    let hex = &bytes[first + 1..second - 1];
    let raw: Vec<u8> = hex
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("zz"), 16))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("the hole is not hex: {e}"))?;
    let blob = tpdf_lib::ber::to_definite_length(&raw).ok_or("the hole holds no CMS value")?;
    let (blob_path, content) = (scratch.join("blob.der"), scratch.join("covered.bin"));
    std::fs::write(&blob_path, blob).map_err(|e| e.to_string())?;
    std::fs::write(&content, [&bytes[..first], &bytes[second..]].concat())
        .map_err(|e| e.to_string())?;
    run(Command::new("openssl")
        .args([
            "cms",
            "-verify",
            "-binary",
            "-noverify",
            "-inform",
            "DER",
            "-in",
        ])
        .arg(&blob_path)
        .arg("-content")
        .arg(&content)
        .args(["-out", "/dev/null"]))
    .map(|_| ())
}

/// The token the signature at `range` carries, DER, and the value octets of
/// that signature --- cut out of the written file as a reader would.
fn token_of(bytes: &[u8], range: [u64; 4]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use der::{Decode as _, Encode as _};
    let [_, first, second, _] = range.map(|n| n as usize);
    let raw: Vec<u8> = bytes[first + 1..second - 1]
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("zz"), 16))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("the hole is not hex: {e}"))?;
    let mut reader = der::SliceReader::new(&raw).map_err(|e| e.to_string())?;
    let info = cms::content_info::ContentInfo::decode(&mut reader).map_err(|e| e.to_string())?;
    let signed: cms::signed_data::SignedData =
        info.content.decode_as().map_err(|e| e.to_string())?;
    let signer = signed
        .signer_infos
        .0
        .as_slice()
        .first()
        .ok_or("no signer")?;
    let attribute = signer
        .unsigned_attrs
        .as_ref()
        .and_then(|attributes| {
            attributes
                .iter()
                .find(|a| a.oid.to_string() == "1.2.840.113549.1.9.16.2.14")
        })
        .ok_or("the signature carries no timestamp")?;
    let token = attribute
        .values
        .as_slice()
        .first()
        .ok_or("an empty timestamp attribute")?
        .to_der()
        .map_err(|e| e.to_string())?;
    Ok((token, signer.signature.as_bytes().to_vec()))
}

/// Which roots `openssl ts -verify` is handed.
#[derive(Clone, Copy, Debug)]
enum Anchors {
    /// Every root this Mac ships.
    System,
    /// Only the system root the token's own chain names at its top --- the
    /// issuer of the one certificate in the token nothing else there issued.
    ///
    /// Needed for Sectigo, measured 2026-09-28: its token's ESS attribute lists
    /// the signer, its CA **and the cross-signed** time-stamping root, and
    /// OpenSSL requires every listed certificate in the chain it builds. Given
    /// every system root it builds the shorter chain to the self-signed copy of
    /// that root, and refuses (`ess cert id not found`); given the root the
    /// token names, it builds the chain the token lists and accepts.
    Named,
}

/// The common name of the issuer at the top of `token`'s certificate set.
fn top_issuer(token: &[u8]) -> Result<String, String> {
    use der::Decode as _;
    let info = cms::content_info::ContentInfo::from_der(token).map_err(|e| e.to_string())?;
    let signed: cms::signed_data::SignedData =
        info.content.decode_as().map_err(|e| e.to_string())?;
    let certificates: Vec<x509_cert::Certificate> = signed
        .certificates
        .map(|set| {
            set.0
                .into_vec()
                .into_iter()
                .filter_map(|c| match c {
                    cms::cert::CertificateChoices::Certificate(c) => Some(c),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let top = certificates
        .iter()
        .find(|c| {
            !certificates.iter().any(|other| {
                other.tbs_certificate.subject == c.tbs_certificate.issuer
                    && other.tbs_certificate.subject != c.tbs_certificate.subject
            })
        })
        .ok_or("the token carries no certificate")?;
    let issuer = &top.tbs_certificate.issuer;
    issuer
        .0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .find(|atv| atv.oid.to_string() == "2.5.4.3")
        .and_then(|atv| {
            der::asn1::PrintableStringRef::try_from(&atv.value)
                .map(|s| s.to_string())
                .or_else(|_| der::asn1::Utf8StringRef::try_from(&atv.value).map(|s| s.to_string()))
                .ok()
        })
        .ok_or_else(|| format!("no common name in {issuer}"))
}

/// `openssl ts -verify` over `token`, with `data` the bytes its imprint must be
/// of and the system roots `anchors` chooses as the trust anchors.
fn openssl_ts_verifies(
    token: &[u8],
    data: &[u8],
    scratch: &Path,
    anchors: Anchors,
) -> Result<String, String> {
    let (token_path, data_path, roots) = (
        scratch.join("ts-token.der"),
        // Not `value.bin`, which the openssl key above signs into.
        scratch.join("ts-data.bin"),
        scratch.join("roots.pem"),
    );
    std::fs::write(&token_path, token).map_err(|e| e.to_string())?;
    std::fs::write(&data_path, data).map_err(|e| e.to_string())?;
    let mut find = Command::new("security");
    find.args(["find-certificate", "-a", "-p"]);
    if let Anchors::Named = anchors {
        find.args(["-c", &top_issuer(token)?]);
    }
    let pem = run(find.arg("/System/Library/Keychains/SystemRootCertificates.keychain"))?;
    if !pem.contains("BEGIN CERTIFICATE") {
        return Err(format!("no system root for {anchors:?}"));
    }
    std::fs::write(&roots, pem).map_err(|e| e.to_string())?;
    run(Command::new("openssl")
        .args(["ts", "-verify", "-token_in", "-in"])
        .arg(&token_path)
        .arg("-data")
        .arg(&data_path)
        .arg("-CAfile")
        .arg(&roots))
}

fn probe(
    input: &Path,
    scratch: &Path,
    kind: &str,
    visible: Option<&Visible>,
    timestamp: Option<&url::Url>,
) -> Result<bool, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    let original = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let before = tpdf_verdicts(&original)?.len();
    let (key_path, certificate) = credentials(scratch, kind)?;
    let key = Openssl {
        key: key_path,
        scratch: scratch.to_path_buf(),
        misdirect: false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let unsigned = match visible {
        None => sign_prepare::prepare(original.clone(), now, None)?,
        Some(visible) => sign_prepare::prepare_visible(original.clone(), now, None, visible)?,
    };
    let range = unsigned.range;
    let field = unsigned.field.clone();
    let made = sign_cms::sign(
        original.clone(),
        unsigned.clone(),
        now,
        &certificate,
        &[],
        &key,
    )?;
    let asked = std::time::Instant::now();
    let stamped = tpdf_lib::tsa::stamp(&made, timestamp, |url, value| {
        tpdf_lib::tsa::ask_blocking(url, value, &tpdf_lib::tsa::LIMITS)
    })
    .map_err(|why| why.sentence(timestamp.and_then(url::Url::host_str).unwrap_or("")))?;
    if let (Some(url), Some(blob)) = (timestamp, stamped.as_ref()) {
        println!(
            "timestamped by {url} in {:.2} s: {} bytes of CMS with the token, of {} reserved",
            asked.elapsed().as_secs_f64(),
            blob.len(),
            sign_prepare::RESERVED
        );
    }
    let bytes = made.seal(stamped)?;
    let stem = input
        .file_stem()
        .map_or("document".into(), |s| s.to_string_lossy().into_owned());
    let out = scratch.join(format!("{stem}-signed.pdf"));
    tpdf_lib::save::write_signed(input, &out, &bytes).map_err(|why| why.message)?;
    let written = std::fs::read(&out).map_err(|e| e.to_string())?;
    println!(
        "signed {} as {field} with {kind}: {} -> {} bytes, {} earlier signature(s)",
        input.display(),
        original.len(),
        written.len(),
        before
    );

    // --------------------------------------------------------- the file
    report.check(
        "the written file is the original followed by one revision",
        written.starts_with(&original) && written.len() > original.len(),
        "the original's bytes are not the file's prefix",
    );
    let ours = tpdf_verdicts(&written)?;
    report.check(
        "tpdf: one signature more than before",
        ours.len() == before + 1,
        &format!("{} before, {} after", before, ours.len()),
    );
    for (name, verdict) in &ours {
        report.check(
            &format!("tpdf: {name} intact"),
            *verdict == Verdict::Intact,
            &format!("{verdict:?}"),
        );
    }
    let theirs = pyhanko(&out)?;
    report.check(
        "pyHanko: the same number of signatures",
        theirs.len() == ours.len(),
        &format!("pyHanko {}, tpdf {}", theirs.len(), ours.len()),
    );
    for entry in &theirs {
        let name = entry.get("field").and_then(|v| v.as_str()).unwrap_or("?");
        let flag = |key: &str| entry.get(key).and_then(serde_json::Value::as_bool) == Some(true);
        let coverage = entry.get("coverage").and_then(|v| v.as_str()).unwrap_or("");
        let want = if name == field {
            "ENTIRE_FILE"
        } else {
            "ENTIRE_REVISION"
        };
        report.check(
            &format!("pyHanko: {name} intact and valid, covering {want}"),
            flag("intact") && flag("valid") && coverage == want,
            &entry.to_string(),
        );
    }
    if let Some(visible) = visible {
        // What pyHanko reads out of /Reason and /Location: the text written,
        // trimmed as tpdf trims it, or nothing when there was none.
        let entry = theirs
            .iter()
            .find(|v| v.get("field").and_then(|f| f.as_str()) == Some(field.as_str()));
        for (key, wanted) in [
            ("reason", visible.options.reason()),
            ("location", visible.options.location()),
        ] {
            let read = entry.and_then(|v| v.get(key)).cloned();
            let expected = wanted.map_or(serde_json::Value::Null, |w| w.into());
            report.check(
                &format!("pyHanko: /{key} reads {expected}"),
                read.as_ref() == Some(&expected),
                &format!("{read:?}"),
            );
        }
    }
    let summary = pyhanko_summary(&out);
    println!("pyHanko's summary:\n{}", summary.trim_end());
    // The difference analysis, which `--json` does not carry: every signature
    // line must say its DocMDP is satisfied. A new visible field after a
    // certification is where this went red (`docs/TRAPS.md`).
    let judged: Vec<&str> = summary.lines().filter(|l| l.contains(" intact=")).collect();
    report.check(
        "pyHanko: every signature's difference analysis is satisfied (docmdp=ok)",
        judged.len() == theirs.len() && judged.iter().all(|l| l.contains("docmdp=ok")),
        summary.trim_end(),
    );
    let verified = openssl_verifies(&written, range, scratch);
    report.check(
        "openssl cms -verify -binary -noverify accepts the signature",
        verified.is_ok(),
        &verified.err().unwrap_or_default(),
    );

    // ------------------------------------------------------- the timestamp
    if timestamp.is_some() {
        let found = tpdf_lib::docinfo::scan(&written, 1, None)?;
        let ours = found
            .signatures
            .iter()
            .find(|s| s.field == field)
            .ok_or("our signature is not in the file")?;
        let stamp = ours.timestamp.as_ref().ok_or("no timestamp read back")?;
        let authority = stamp
            .authority
            .as_ref()
            .map_or(String::new(), |c| c.subject_cn.clone());
        println!(
            "tpdf reads: {} by {authority}, {:?}, attested={}, authority {:?}",
            stamp.when, stamp.integrity, stamp.attested, stamp.trust
        );
        report.check(
            "tpdf: the timestamp is intact and its time attested",
            stamp.integrity.as_ref().map(|i| i.verdict) == Some(Verdict::Intact) && stamp.attested,
            &format!("{stamp:?}"),
        );
        let clock = utc(now);
        report.check(
            "tpdf: the attested time is this machine's clock, give or take five minutes",
            stamp.when.get(..15) == clock.get(..15)
                || minutes_apart(&stamp.when, &clock).is_some_and(|m| m <= 5),
            &format!("attested {}, clock {clock}", stamp.when),
        );
        report.check(
            "tpdf: the system store trusts the authority, for timestamping",
            stamp.trust.as_ref().map(|t| t.standing) == Some(tpdf_lib::trust::Standing::Trusted),
            &format!("{:?}", stamp.trust),
        );
        let entry = theirs
            .iter()
            .find(|v| v.get("field").and_then(|f| f.as_str()) == Some(field.as_str()))
            .and_then(|v| v.get("timestamp"))
            .cloned()
            .unwrap_or_default();
        report.check(
            "pyHanko: the timestamp is intact and valid",
            entry.get("intact").and_then(serde_json::Value::as_bool) == Some(true)
                && entry.get("valid").and_then(serde_json::Value::as_bool) == Some(true),
            &entry.to_string(),
        );
        let (token, value) = token_of(&written, range)?;
        println!("the token is {} bytes", token.len());
        // Every system root first, printed and not counted: whether OpenSSL
        // accepts it there depends on which chain it builds (`Anchors::Named`).
        let everything = openssl_ts_verifies(&token, &value, scratch, Anchors::System);
        println!(
            "openssl ts -verify, every system root: {}",
            match &everything {
                Ok(_) => "OK".to_string(),
                Err(why) => why.lines().last().unwrap_or_default().to_string(),
            }
        );
        let accepted = openssl_ts_verifies(&token, &value, scratch, Anchors::Named);
        report.check(
            &format!(
                "openssl ts -verify over the signature's value octets, anchored at {}: OK",
                top_issuer(&token).unwrap_or_default()
            ),
            accepted
                .as_ref()
                .is_ok_and(|out| out.contains("Verification: OK")),
            &format!("{accepted:?}"),
        );
        let mut other = value.clone();
        other[0] ^= 0x01;
        report.check(
            "control: openssl ts -verify refuses the token over other bytes",
            openssl_ts_verifies(&token, &other, scratch, Anchors::Named).is_err(),
            "accepted a token over bytes it is not of",
        );
    }

    // ------------------------------------------------ control: wrong offset
    let mut late = unsigned.update.clone();
    let mut shifted = range;
    shifted[1] += 2;
    let digest = sign_cms::check(&original, &unsigned)?;
    let blob = sign_cms::build(&digest, &certificate, &[], &key)?;
    sign_cms::splice(&mut late, unsigned.built_against, shifted, &blob)?;
    let late = [original.as_slice(), late.as_slice()].concat();
    let found = tpdf_verdicts(&late)?;
    let theirs_late = found.iter().find(|(n, _)| *n == field).map(|(_, v)| *v);
    report.check(
        "control, value two digits late: tpdf does not call it intact",
        theirs_late.is_some_and(|v| v != Verdict::Intact),
        &format!("{theirs_late:?}"),
    );
    report.check(
        "control, value two digits late: openssl refuses",
        openssl_verifies(&late, range, scratch).is_err(),
        "openssl accepted a misplaced value",
    );

    // ------------------------------------------- control: the wrong digest
    let misdirected = Openssl {
        misdirect: true,
        ..Openssl {
            key: key.key.clone(),
            scratch: scratch.to_path_buf(),
            misdirect: false,
        }
    };
    let wrong_blob = sign_cms::build(&digest, &certificate, &[], &misdirected)?;
    let mut wrong = unsigned.update.clone();
    sign_cms::splice(&mut wrong, unsigned.built_against, range, &wrong_blob)?;
    let wrong_path = scratch.join(format!("{stem}-wrong-digest.pdf"));
    let wrong = [original.as_slice(), wrong.as_slice()].concat();
    std::fs::write(&wrong_path, &wrong).map_err(|e| e.to_string())?;
    let verdict = tpdf_verdicts(&wrong)?
        .into_iter()
        .find(|(n, _)| *n == field)
        .map(|(_, v)| v);
    report.check(
        "control, wrong digest: tpdf says broken",
        verdict == Some(Verdict::Broken),
        &format!("{verdict:?}"),
    );
    let theirs_wrong = pyhanko(&wrong_path)?;
    let entry = theirs_wrong
        .iter()
        .find(|v| v.get("field").and_then(|f| f.as_str()) == Some(field.as_str()));
    report.check(
        "control, wrong digest: pyHanko says valid=no",
        entry.is_some_and(|v| v.get("valid").and_then(serde_json::Value::as_bool) == Some(false)),
        &format!("{entry:?}"),
    );
    report.check(
        "control, wrong digest: openssl refuses",
        openssl_verifies(&wrong, range, scratch).is_err(),
        "openssl accepted a signature over the wrong digest",
    );
    let refused = sign_cms::finish(
        original.clone(),
        unsigned.clone(),
        now,
        &certificate,
        &[],
        &misdirected,
    );
    report.check(
        "control, wrong digest: finish refuses to hand it back",
        refused.as_ref().is_err_and(|why| why.contains("Broken")),
        &format!("{:?}", refused.map(|b| b.len())),
    );

    // ------------------------------------------ control: a changed byte
    let mut altered = written.clone();
    // A digit of the signing time in the new revision's `/M`: covered by the
    // range, and harmless to every parser, so the only thing that changes is
    // whether the covered bytes still hash to what was signed.
    let date = written[original.len()..]
        .windows(3)
        .position(|w| w == b"(D:")
        .ok_or("the revision has no /M date")?;
    let at = original.len() + date + 3;
    altered[at] = if altered[at] == b'1' { b'2' } else { b'1' };
    let altered_path = scratch.join(format!("{stem}-altered.pdf"));
    std::fs::write(&altered_path, &altered).map_err(|e| e.to_string())?;
    let verdict = tpdf_verdicts(&altered)
        .ok()
        .and_then(|v| v.into_iter().find(|(n, _)| *n == field).map(|(_, v)| v));
    report.check(
        "control, changed byte: tpdf says altered",
        verdict == Some(Verdict::Altered),
        &format!("{verdict:?}"),
    );
    match pyhanko(&altered_path) {
        Ok(found) => {
            let entry = found
                .iter()
                .find(|v| v.get("field").and_then(|f| f.as_str()) == Some(field.as_str()));
            report.check(
                "control, changed byte: pyHanko says intact=no",
                entry.is_some_and(|v| {
                    v.get("intact").and_then(serde_json::Value::as_bool) == Some(false)
                }),
                &format!("{entry:?}"),
            );
        }
        Err(e) => report.check("control, changed byte: pyHanko reads the file", false, &e),
    }
    report.check(
        "control, changed byte: openssl refuses",
        openssl_verifies(&altered, range, scratch).is_err(),
        "openssl accepted a changed document",
    );

    // ------------------------------------------- where a visible one's ink went
    if let Some(visible) = visible {
        let invisible = sign_cms::finish(
            original.clone(),
            sign_prepare::prepare(original.clone(), now, None)?,
            now,
            &certificate,
            &[],
            &key,
        )?;
        let invisible_path = scratch.join(format!("{stem}-invisible.pdf"));
        std::fs::write(&invisible_path, &invisible).map_err(|e| e.to_string())?;
        if let Err(why) =
            where_the_ink_went(&mut report, input, &out, &invisible_path, visible, now)
        {
            report.check("the renderers ran", false, &why);
        }
    }

    println!("\n{} passed, {} failed", report.passed, report.failed);
    Ok(report.failed == 0 && report.passed > 0)
}

/// Minutes between two `YYYY-MM-DD HH:MM:SS UTC` times on the same day.
fn minutes_apart(a: &str, b: &str) -> Option<i64> {
    let minutes = |t: &str| -> Option<i64> {
        if t.get(..10)? != a.get(..10)? {
            return None;
        }
        let hours: i64 = t.get(11..13)?.parse().ok()?;
        let mins: i64 = t.get(14..16)?.parse().ok()?;
        Some(hours * 60 + mins)
    };
    Some((minutes(a)? - minutes(b)?).abs())
}

/// `seconds` since the epoch as `YYYY-MM-DD HH:MM:SS UTC`, the form tpdf writes.
fn utc(seconds: u64) -> String {
    der::DateTime::from_unix_duration(std::time::Duration::from_secs(seconds)).map_or_else(
        |_| String::new(),
        |t| {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minutes(),
                t.seconds()
            )
        },
    )
}
