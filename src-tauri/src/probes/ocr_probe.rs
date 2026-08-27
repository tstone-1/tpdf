//! Body of `examples/ocr_probe.rs`. See that file for what this probe is and why.
//!
//! It lives here rather than in a directory beside the example for the reason
//! `src/probes/backend_probe.rs` does: a directory next to a target source has no
//! manifest entry claiming it, and a bundler that enumerates such a directory has
//! already cost this repository a failed Windows installer. See `docs/TRAPS.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;
use tpdf_lib::document::OpenDocument;

use tpdf_lib::ocr::{
    adjudicate, control_from_page, Control, ControlWord, Legibility, Options, Pixels, Recogniser,
};
use tpdf_lib::ocr_vision::Vision;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};
use tpdf_lib::text;

/// Verdicts, with every label padded to seven at column 1 so the rows that pass line up with
/// the rows that do not --- `docs/TRAPS.md` has an entry about the alternative.
#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:44} {}", detail.as_ref());
    }
    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.skipped += 1;
        println!("{:7}{name:44} {}", "[SKIP]", why.as_ref());
    }
    fn finish(&self) -> ! {
        println!();
        println!(
            "{}/{} checks passed, {} skipped",
            self.passed,
            self.passed + self.failed,
            self.skipped
        );
        std::process::exit(i32::from(self.failed != 0));
    }
}

/// A rendered page, kept as rows so strips can be stacked without a second render.
struct Sheet {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
}

impl Sheet {
    fn rows(&self, top: u32, height: u32) -> &[u8] {
        let stride = self.width as usize * 4;
        let a = top as usize * stride;
        let b = a + height as usize * stride;
        &self.rgba[a..b.min(self.rgba.len())]
    }

    /// `under` stacked above `control`, returned with the control band's rect in points.
    fn stack(&self, under: (u32, u32), control: (u32, u32)) -> (Vec<u8>, u32, [f32; 4]) {
        let mut out =
            Vec::with_capacity((under.1 as usize + control.1 as usize) * self.width as usize * 4);
        out.extend_from_slice(self.rows(under.0, under.1));
        out.extend_from_slice(self.rows(control.0, control.1));
        let height = under.1 + control.1;
        let band = [
            0.0,
            under.1 as f32 / self.scale,
            self.width as f32 / self.scale,
            height as f32 / self.scale,
        ];
        (out, height, band)
    }

    /// The tallest run of rows whose every pixel is near-white.
    fn blank_band(&self) -> Option<(u32, u32)> {
        let stride = self.width as usize * 4;
        let blank: Vec<bool> = (0..self.height)
            .map(|y| {
                let row = &self.rgba[y as usize * stride..(y as usize + 1) * stride];
                row.chunks_exact(4)
                    .all(|p| p[0] > 245 && p[1] > 245 && p[2] > 245)
            })
            .collect();
        let (mut best, mut best_len) = (0u32, 0u32);
        let (mut start, mut len) = (0u32, 0u32);
        for (y, &b) in blank.iter().enumerate() {
            if b {
                if len == 0 {
                    start = y as u32;
                }
                len += 1;
                if len > best_len {
                    best_len = len;
                    best = start;
                }
            } else {
                len = 0;
            }
        }
        (best_len >= 24).then_some((best, best_len))
    }
}

pub fn main() {
    let mut args = std::env::args().skip(1);
    let mut file = PathBuf::new();
    let mut library = PathBuf::from("vendor/pdfium/lib");
    let mut scale = 2.0_f32;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            "--scale" => scale = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0),
            other => file = PathBuf::from(other),
        }
    }
    if file.as_os_str().is_empty() {
        eprintln!("[ERROR] usage: ocr-probe <file.pdf> [--lib DIR] [--scale N]");
        std::process::exit(2);
    }
    match run(&file, &library, scale) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[ERROR] {e}");
            std::process::exit(2);
        }
    }
}

fn run(file: &Path, library: &Path, scale: f32) -> Result<(), String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    let bindings = progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))));

    let document = OpenDocument::open(bindings, file, None)?;
    let page = document.page(0)?;
    let sheet = render(bindings, &page, scale)?;
    let embedded = text::extract(&page)?;

    println!("document   {}", file.display());
    println!(
        "page 0     {:.0}x{:.0} pt, rendered {}x{} px at scale {scale}",
        page.width_pt(),
        page.height_pt(),
        sheet.width,
        sheet.height
    );

    let engine = Vision;
    let id = engine.id();
    println!("engine     {id}");
    println!();

    let mut r = Report::default();
    let opts = Options::default();

    // ------------------------------------------------------------ it reads at all
    let whole = Pixels {
        rgba: &sheet.rgba,
        width: sheet.width,
        height: sheet.height,
        scale,
    };
    let read = engine.recognise(whole, &opts);
    let items = match read {
        Ok(items) => items,
        Err(e) => {
            r.check(false, "vision runs on a rendered page", format!("{e}"));
            r.finish();
        }
    };
    // ------------------------------------------------------- it read the right words
    let embedded_words: Vec<String> = String::from_utf16_lossy(
        &embedded
            .codes
            .iter()
            .map(|c| u16::try_from(*c).unwrap_or(b'?'.into()))
            .collect::<Vec<u16>>(),
    )
    .split_whitespace()
    .map(|w| w.chars().filter(char::is_ascii_alphanumeric).collect())
    .filter(|w: &String| w.len() >= 4)
    .collect();
    let read_blob: String = items
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let hit = embedded_words
        .iter()
        .filter(|w| read_blob.contains(w.as_str()))
        .count();

    // What counts as a correct result depends on whether the page has text at all. On a
    // page that has none, reading none is the right answer and reading something is the
    // engine inventing it -- so this is two checks wearing one name, not a check plus a
    // special case. A harness that only knew the first would have called `vector-heavy`,
    // which is a pure vector drawing, a failure.
    if embedded_words.is_empty() {
        r.check(
            items.is_empty(),
            "no text is invented on a page that has none",
            format!(
                "the document has no extractable words; vision read {} span(s){}",
                items.len(),
                items
                    .first()
                    .map(|i| format!(", first {:?}", i.text))
                    .unwrap_or_default()
            ),
        );
        r.finish();
    }

    r.check(
        !items.is_empty(),
        "vision runs on a rendered page",
        format!("{} span(s) read", items.len()),
    );
    if items.is_empty() {
        r.finish();
    }
    // Bound rather than written twice: the control chooser below reads the
    // document's own text, so whether that text says what the page draws decides
    // which way its check runs.
    let text_layer_agrees = hit * 2 >= embedded_words.len();
    r.check(
        text_layer_agrees,
        "what it read matches the embedded text",
        format!("{hit}/{} words of 4+ chars found", embedded_words.len()),
    );

    // --------------------------------------------------------------- the flip
    // The discriminator. Not "did it read the words" but "did it put them where the
    // document puts them". A y-flip passes every other check on this page.
    match top_and_bottom(&embedded, &items) {
        None => r.skip(
            "vertical order agrees with the document",
            "no two unambiguous words far enough apart in y on this page",
        ),
        Some((top_word, bottom_word, gap_pt)) => {
            let at = |w: &str| {
                items
                    .iter()
                    .find(|i| i.text.contains(w))
                    .map(|i| (i.rect[1] + i.rect[3]) / 2.0)
            };
            // Both are unique in the document and in what was read, so `find` cannot land
            // on a different occurrence than the one measured -- which is how this check
            // first "passed" on a two-column page by 1 pt out of 842.
            match (at(&top_word), at(&bottom_word)) {
                (Some(t), Some(b)) => r.check(
                    t < b && (b - t) > gap_pt / 2.0,
                    "vertical order agrees with the document",
                    format!(
                        "{top_word:?} at y={t:.0}, {bottom_word:?} at y={b:.0}; \
                         read gap {:.0} pt against {gap_pt:.0} pt in the document",
                        b - t
                    ),
                ),
                _ => r.skip(
                    "vertical order agrees with the document",
                    format!("vision did not read both {top_word:?} and {bottom_word:?}"),
                ),
            }
        }
    }

    // ------------------------------------------------------------------ the gate
    let Some((blank_top, blank_h)) = sheet.blank_band() else {
        r.skip(
            "a blank strip adjudicates Illegible",
            "no blank band on this page",
        );
        r.skip(
            "a text strip adjudicates Legible",
            "no blank band on this page",
        );
        r.skip(
            "an unread control adjudicates NotVerified",
            "no blank band on this page",
        );
        r.finish();
    };
    // A strip of real text to act as the control band, and the token is what is in it.
    let (ctrl_top, ctrl_h, token) = match control_strip(&sheet, &items) {
        Some(v) => v,
        None => {
            r.skip(
                "a blank strip adjudicates Illegible",
                "no single-span text strip found",
            );
            r.skip(
                "a text strip adjudicates Legible",
                "no single-span text strip found",
            );
            r.skip(
                "an unread control adjudicates NotVerified",
                "no single-span text strip found",
            );
            r.finish();
        }
    };
    println!();
    println!(
        "gate       blank rows {blank_top}..{}, control rows {ctrl_top}..{} carrying {token:?}",
        blank_top + blank_h,
        ctrl_top + ctrl_h
    );

    let (probe, h, band) = sheet.stack((blank_top, blank_h), (ctrl_top, ctrl_h));
    let px = Pixels {
        rgba: &probe,
        width: sheet.width,
        height: h,
        scale,
    };
    let control = Control::no_easier_than(&[[0.0, 0.0, 10.0, 8.0]], token.clone(), band)
        .map_err(|e| format!("control: {e}"))?;
    let verdict = adjudicate(&id, &control, &engine.recognise(px, &opts));
    r.check(
        verdict.certifies(),
        "a blank strip adjudicates Illegible",
        match &verdict {
            Legibility::Illegible { .. } => "nothing outside the control band".into(),
            Legibility::Legible { found } => format!(
                "{} survivor(s), first {:?}",
                found.len(),
                found.first().map(|f| f.text.as_str()).unwrap_or_default()
            ),
            Legibility::NotVerified { why } => {
                let seen = engine
                    .recognise(px, &opts)
                    .map(|v| {
                        v.iter()
                            .map(|i| format!("{:?}@y{:.0}", i.text, i.rect[1]))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|e| format!("<{e}>"));
                format!("{why} || band={band:?} read: [{seen}]")
            }
        },
    );

    // The same shape with text under it must not certify. Without this, a gate that always
    // says Illegible passes the check above.
    let (probe2, h2, band2) = sheet.stack((ctrl_top, ctrl_h), (ctrl_top, ctrl_h));
    let px2 = Pixels {
        rgba: &probe2,
        width: sheet.width,
        height: h2,
        scale,
    };
    let control2 = Control::no_easier_than(&[[0.0, 0.0, 10.0, 8.0]], token.clone(), band2)
        .map_err(|e| format!("control: {e}"))?;
    let verdict2 = adjudicate(&id, &control2, &engine.recognise(px2, &opts));
    r.check(
        matches!(verdict2, Legibility::Legible { .. }),
        "a text strip adjudicates Legible",
        match &verdict2 {
            Legibility::Legible { found } => format!("{} survivor(s) reported", found.len()),
            other => format!("{other:?}"),
        },
    );

    // And a control the engine cannot read must refuse rather than certify.
    let control3 =
        Control::no_easier_than(&[[0.0, 0.0, 10.0, 8.0]], "ZZQXJ7WKV4", [0.0, 0.0, 1.0, 1.0])
            .map_err(|e| format!("control: {e}"))?;
    let verdict3 = adjudicate(&id, &control3, &engine.recognise(px, &opts));
    r.check(
        matches!(verdict3, Legibility::NotVerified { .. }),
        "an unread control adjudicates NotVerified",
        match &verdict3 {
            Legibility::NotVerified { .. } => "refused, as it must".into(),
            other => format!("{other:?}"),
        },
    );

    // ------------------------------------------- the chooser, against a real engine
    // The three checks above take their control out of the ENGINE's own output,
    // which is the engine agreeing with itself: of course Vision reads back a
    // strip Vision has just read. `ocr::control_from_page` chooses from what the
    // *document* says instead, and this is the only place that claim meets an
    // engine at all.
    match tpdf_lib::objects::read(&page) {
        Err(e) => r.skip("the chosen control is read back", format!("objects: {e}")),
        Ok(objects) => {
            let height_pt = page.height_pt();
            let mut words: Vec<ControlWord> = Vec::new();
            let mut ordinal = 0usize;
            for object in &objects.all {
                if object.kind != "text" {
                    continue;
                }
                let drawn = objects.text.get(ordinal).cloned().unwrap_or_default();
                ordinal += 1;
                // PDFium reports `left, bottom, right, top` with y up; `ocr.rs`
                // is `left, top, right, bottom` with y down. Flipped here rather
                // than tolerated there, for the reason `ControlWord::rect` says.
                let [l, b, right, t] = object.bounds;
                if !(l.is_finite() && b.is_finite() && right.is_finite() && t.is_finite()) {
                    continue;
                }
                words.push(ControlWord {
                    rect: [l, height_pt - t, right, height_pt - b],
                    text: drawn,
                });
            }
            // A region over the topmost word, which is a redaction a reader
            // could plausibly have drawn and is deterministic on any page.
            let region = words
                .iter()
                .min_by(|a, b| {
                    a.rect[1]
                        .partial_cmp(&b.rect[1])
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.rect[0].total_cmp(&b.rect[0]))
                })
                .map(|w| w.rect);
            match region.map(|region| (region, control_from_page(&words, &[region]))) {
                None => r.skip(
                    "the chosen control is read back",
                    "no text objects on this page",
                ),
                Some((_, Err(why))) => r.skip("the chosen control is read back", why.to_string()),
                Some((region, Ok(chosen))) => {
                    let rows = |top: f32, bottom: f32| {
                        let a = (top * scale).floor().max(0.0) as u32;
                        let b = (bottom * scale).ceil().min(sheet.height as f32) as u32;
                        (a, b.saturating_sub(a))
                    };
                    let (crop_top, crop_h) = rows(chosen.crop[1], chosen.crop[3]);
                    // The band is a strip of full-width rows, so a region on the
                    // same line would put a real survivor inside the control. The
                    // chooser promises the chosen *word* is outside every region
                    // and cannot promise that about its neighbours.
                    let clash = region[1] < chosen.crop[3] && chosen.crop[1] < region[3];
                    if clash || crop_h < 2 {
                        r.skip(
                            "the chosen control is read back",
                            format!(
                                "the control sits on the redacted line ({} row(s), clash {clash})",
                                crop_h
                            ),
                        );
                    } else {
                        let (probe, h, band) =
                            sheet.stack((blank_top, blank_h), (crop_top, crop_h));
                        let px = Pixels {
                            rgba: &probe,
                            width: sheet.width,
                            height: h,
                            scale,
                        };
                        let control = chosen.placed(band);
                        let verdict = adjudicate(&id, &control, &engine.recognise(px, &opts));
                        // Two checks wearing one name, and the second is the one
                        // worth having. A control chosen from the document's own
                        // text is only evidence when that text says what the page
                        // draws; `encodings.pdf` has no usable `/ToUnicode`, so
                        // PDFium returns plausible garbage and the token is a
                        // string nobody can read off the page. The gate must
                        // refuse there, not certify.
                        r.check(
                            verdict.certifies() == text_layer_agrees,
                            "the chosen control is read back",
                            match (&verdict, text_layer_agrees) {
                                (Legibility::Illegible { .. }, _) => format!(
                                    "{:?} at {:.1} pt, chosen from the document and read back",
                                    control.token, control.size_pt
                                ),
                                (other, true) => format!("{:?} -> {other:?}", control.token),
                                (_, false) => format!(
                                    "refused, as it must: the text layer does not say what the \
                                     page draws, so {:?} is not on it",
                                    control.token
                                ),
                            },
                        );
                    }
                }
            }
        }
    }

    r.finish();
}

/// The topmost and bottom-most words that are far enough apart in y for an ordering
/// comparison to mean anything, **and unambiguous enough for the comparison to be about
/// them**.
///
/// The second condition was added after this check passed on `columns.pdf` with a 1 pt
/// margin on an 842 pt page. Matching a recognised span by substring finds the *first*
/// occurrence, which on a page where the word repeats is a different instance than the one
/// whose position was measured -- so the check was comparing two unrelated positions and
/// happening to get the sign right. A word used here must occur exactly once in the
/// document and be contained in exactly one recognised span. `docs/TRAPS.md` has the same
/// lesson from the other direction: a dense page of uniform lines cannot detect a y-flip.
fn top_and_bottom(
    page: &text::PageText,
    items: &[tpdf_lib::ocr::RecognisedItem],
) -> Option<(String, String, f32)> {
    let mut words: Vec<(f32, String)> = Vec::new();
    let mut current = String::new();
    let mut top = f32::NAN;
    for (i, code) in page.codes.iter().enumerate() {
        let ch = char::from_u32(*code).unwrap_or(' ');
        let b = &page.boxes[i * 4..i * 4 + 4];
        if ch.is_whitespace() {
            if current.chars().filter(char::is_ascii_alphanumeric).count() >= 4 {
                words.push((top, current.clone()));
            }
            current.clear();
            top = f32::NAN;
        } else {
            if current.is_empty() {
                top = b[1];
            }
            current.push(ch);
        }
    }
    if current.chars().filter(char::is_ascii_alphanumeric).count() >= 4 {
        words.push((top, current));
    }
    words.retain(|(y, _)| y.is_finite());
    let counted: Vec<(f32, String, usize)> = words
        .iter()
        .map(|(y, t)| (*y, t.clone(), words.iter().filter(|(_, o)| o == t).count()))
        .collect();
    words = counted
        .into_iter()
        .filter(|(_, t, n)| *n == 1 && items.iter().filter(|i| i.text.contains(t)).count() == 1)
        .map(|(y, t, _)| (y, t))
        .collect();
    words.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = words.first()?;
    let last = words.last()?;
    // Require real vertical separation: on a page whose words all sit on one line, an
    // ordering check is noise rather than evidence, and reporting it as a pass would be
    // the "dense page of uniform lines" trap.
    let gap = last.0 - first.0;
    (gap > 20.0).then(|| (first.1.clone(), last.1.clone(), gap))
}

/// Rows covering one recognised span, and the text in it, chosen so the strip holds exactly
/// that span and nothing else.
fn control_strip(
    sheet: &Sheet,
    items: &[tpdf_lib::ocr::RecognisedItem],
) -> Option<(u32, u32, String)> {
    let mut sorted: Vec<_> = items.iter().collect();
    sorted.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    for (n, it) in sorted.iter().enumerate() {
        let raw_top = (it.rect[1] * sheet.scale).floor().max(0.0) as u32;
        let raw_bot = (it.rect[3] * sheet.scale).ceil().min(sheet.height as f32) as u32;
        if raw_bot <= raw_top || raw_bot - raw_top < 8 {
            continue;
        }
        // Pad, because a strip cropped flush to a span's own box clips ascenders and
        // descenders and the engine then misreads its own text: on `outline-simple` a line
        // reading "Donn..." came back as "L UNVG" once isolated, and the control failed for
        // a reason that had nothing to do with the gate. A recogniser needs the whitespace
        // around a line as much as the line.
        // Most padding that still isolates this span. A fixed pad fixed `outline-simple`
        // and cost `rotated` its gate checks, because there the neighbouring lines are
        // close enough that padding pulled one in and no strip qualified at all.
        let Some((top_px, bot_px)) = [(raw_bot - raw_top) / 2, 12, 8, 4, 2, 0]
            .into_iter()
            .map(|pad| {
                (
                    raw_top.saturating_sub(pad),
                    (raw_bot + pad).min(sheet.height),
                )
            })
            .find(|(t, b)| {
                !sorted.iter().enumerate().any(|(m, other)| {
                    m != n
                        && (other.rect[3] * sheet.scale) > *t as f32
                        && (other.rect[1] * sheet.scale) < *b as f32
                })
            })
        else {
            continue;
        };
        // Isolation is guaranteed by the padding search above: no other span overlaps
        // these rows, so the control band cannot carry a survivor and make the blank-strip
        // check fail for the wrong reason.
        let token: String = it.text.split_whitespace().next()?.into();
        if token.chars().filter(char::is_ascii_alphanumeric).count() >= 3 {
            return Some((top_px, bot_px - top_px, token));
        }
    }
    None
}

fn render(
    bindings: progressive::Bindings,
    page: &RawPage<'_>,
    scale: f32,
) -> Result<Sheet, String> {
    let width = (page.width_pt() * scale).ceil() as u32;
    let height = (page.height_pt() * scale).ceil() as u32;
    let spec = TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width: u16::try_from(width).map_err(|_| "page too wide for one tile".to_string())?,
        height: u16::try_from(height).map_err(|_| "page too tall for one tile".to_string())?,
    };
    let (rgba, _) = progressive::render_tile(
        bindings,
        page,
        spec,
        Some(Duration::from_millis(50)),
        &CancelToken::default(),
    )?;
    Ok(Sheet {
        rgba,
        width,
        height,
        scale,
    })
}
