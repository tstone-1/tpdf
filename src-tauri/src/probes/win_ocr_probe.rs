//! The body of `examples/win_ocr_probe.rs`. See that file for what this answers.

use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HBITMAP, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT,
};

/// The bitmap the control is drawn into. Bigger than a gate band needs, so that a
/// failure to read is about the engine rather than about a control too small for
/// it --- `docs/TRAPS.md` has an entry per direction of that mistake.
const PROBE_W: i32 = 640;
const PROBE_H: i32 = 160;
/// Character height in pixels. At 96 dpi this is a little over 30 pt, which is
/// far above anything the gate has ever had to render, on purpose: this probe is
/// asking whether the engine works at all.
const GLYPH_PX: i32 = 44;

/// A word, and a string no dictionary holds.
///
/// The pair is the point. If both come back verbatim the engine is not
/// second-guessing what it read; if the first survives and the second arrives as
/// something else, `Windows.Media.Ocr` cannot honour
/// `ocr::Options::language_correction`, and a verdict from it means something
/// different from a verdict from Vision.
const REAL_WORD: &str = "REDACTED";
const NON_WORD: &str = "qwrtzp";

/// One reading, printed whatever it says.
fn say(label: &str, value: &str) {
    println!("  {label:<28} {value}");
}

pub fn main() {
    println!("[win-ocr-probe] Windows.Media.Ocr, on this machine");

    let langs = match OcrEngine::AvailableRecognizerLanguages() {
        Ok(list) => list,
        Err(e) => {
            // Not exit 1. Failing to *ask* is a different fact from an empty
            // answer, and folding the two together is what makes an absent
            // capability indistinguishable from a broken probe.
            eprintln!("[FAIL] AvailableRecognizerLanguages: {e}");
            std::process::exit(2);
        }
    };

    let mut tags: Vec<String> = Vec::new();
    for lang in &langs {
        let tag = lang
            .LanguageTag()
            .map(|t| t.to_string())
            .unwrap_or_default();
        let name = lang
            .DisplayName()
            .map(|t| t.to_string())
            .unwrap_or_default();
        say("recogniser language", &format!("{tag}  ({name})"));
        tags.push(tag);
    }
    say("languages installed", &tags.len().to_string());

    // THE GATING LINE. Greppable on purpose: this is the reading the ranking in
    // `docs/PLAN.md` §9.10 turns on, and it should not have to be inferred from
    // the absence of rows above.
    println!(
        "[verdict] language packs on a stock install: {}",
        if tags.is_empty() { "NONE" } else { "present" }
    );
    if tags.is_empty() {
        println!("[verdict] the in-box engine cannot ship as-is; nothing below could run");
        return;
    }

    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(e) => {
            say("from user profile", "an engine");
            e
        }
        Err(e) => {
            // A real and interesting state: packs are installed but none matches
            // the profile's languages. Falling back names which one was used, so
            // the reading below is attributable.
            say("from user profile", &format!("no engine ({e})"));
            let first = &tags[0];
            match Language::CreateLanguage(&HSTRING::from(first.as_str()))
                .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
            {
                Ok(e) => {
                    say("fell back to", first);
                    e
                }
                Err(e) => {
                    eprintln!("[FAIL] no engine from any listed language: {e}");
                    std::process::exit(2);
                }
            }
        }
    };

    match engine.RecognizerLanguage().and_then(|l| l.LanguageTag()) {
        Ok(tag) => say("engine language", &tag.to_string()),
        Err(e) => say("engine language", &format!("unreadable ({e})")),
    }
    match OcrEngine::MaxImageDimension() {
        // A real bound on `ocr::Pixels`: the gate composites a probe image and
        // hands it over whole, so a limit below a page at render scale is a
        // constraint on the caller, not a detail of the binding.
        Ok(max) => say("max image dimension", &max.to_string()),
        Err(e) => say("max image dimension", &format!("unreadable ({e})")),
    }

    for text in [REAL_WORD, NON_WORD] {
        match read_back(&engine, text) {
            Ok(got) => {
                let verbatim = got.split_whitespace().any(|w| w == text);
                say(
                    &format!("read back {text:?}"),
                    &format!("{got:?}  {}", if verbatim { "VERBATIM" } else { "DIFFERS" }),
                );
            }
            Err(why) => say(&format!("read back {text:?}"), &format!("failed: {why}")),
        }
    }

    println!(
        "[verdict] a non-word reading that DIFFERS means Options::language_correction \
         cannot be honoured here"
    );
}

/// Draws `text` into a bitmap and asks the engine to read it.
fn read_back(engine: &OcrEngine, text: &str) -> Result<String, String> {
    let bgra = draw(text)?;

    // `DataWriter` with no stream behind it: `DetachBuffer` is the shortest route
    // from a Rust slice to an `IBuffer`, and it sidesteps the trap about a
    // `DataWriter` closing the stream it was created over.
    let writer = DataWriter::new().map_err(|e| format!("DataWriter: {e}"))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|e| format!("WriteBytes: {e}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|e| format!("DetachBuffer: {e}"))?;

    let bitmap =
        SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, PROBE_W, PROBE_H)
            .map_err(|e| format!("CreateCopyFromBuffer: {e}"))?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("RecognizeAsync: {e}"))?
        .get()
        .map_err(|e| format!("awaiting RecognizeAsync: {e}"))?;
    result
        .Text()
        .map(|t| t.to_string())
        .map_err(|e| format!("Text: {e}"))
}

/// Black `text` on white, centred, as top-down BGRA.
fn draw(text: &str) -> Result<Vec<u8>, String> {
    // SAFETY: every handle created below is deleted on the way out, and the bits
    // pointer is owned by the DIB section for as long as the bitmap lives --- the
    // copy out happens before it is deleted.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        if dc.is_invalid() {
            ReleaseDC(None, screen);
            return Err("CreateCompatibleDC".into());
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(0),
                biWidth: PROBE_W,
                // Negative: top-down, so the rows are in the order
                // `SoftwareBitmap` reads them and no flip is needed. A bottom-up
                // DIB here would give the engine an upside-down image, which
                // reads as an engine that cannot read rather than as a caller
                // that handed one over wrong.
                biHeight: -PROBE_H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        // Matched rather than `?`: an early return here would be inside the
        // `unsafe` block with two device contexts already open, and `?` runs no
        // cleanup. The probe is called twice, so a leak on the sad path is a leak
        // the second call inherits.
        let bitmap: HBITMAP =
            match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(h) => h,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    ReleaseDC(None, screen);
                    return Err(format!("CreateDIBSection: {e}"));
                }
            };

        let old_bitmap: HGDIOBJ = SelectObject(dc, bitmap.into());

        let len = (PROBE_W * PROBE_H * 4) as usize;
        // White, by writing it rather than by `PatBlt`: the bits are ours and
        // there is no brush to select.
        core::ptr::write_bytes(bits.cast::<u8>(), 0xFF, len);

        let font = CreateFontW(
            -GLYPH_PX,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            // The one parameter of the five that really is a bare `u32`; the
            // other four are newtypes and take their constants unwrapped.
            u32::from(DEFAULT_PITCH.0),
            // Ships on every Windows install, so the probe does not depend on a
            // font somebody added.
            &HSTRING::from("Segoe UI"),
        );
        let old_font: HGDIOBJ = SelectObject(dc, font.into());

        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x0000_0000));
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: PROBE_W,
            bottom: PROBE_H,
        };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
        // GDI batches, and the bits are read directly rather than through another
        // GDI call that would flush on our behalf.
        let _ = GdiFlush();

        let mut out = vec![0u8; len];
        core::ptr::copy_nonoverlapping(bits.cast::<u8>(), out.as_mut_ptr(), len);

        // Every pixel opaque, and this is not tidiness. GDI writes RGB into a
        // 32-bit DIB and leaves the alpha byte alone -- so a glyph drawn in black
        // arrives as 0x00000000, alpha included. `CreateCopyFromBuffer` takes no
        // alpha mode and `SoftwareBitmap::BitmapAlphaMode` is read-only, so the
        // buffer has to be right rather than the declaration: under
        // `Premultiplied`, which is what Bgra8 gets, every glyph pixel would be
        // fully transparent and the engine would be handed a blank image. It
        // would then report no text, honestly, and the reading would be a bug
        // wearing the shape of a finding -- `docs/TRAPS.md` on the reassuring
        // branch. At alpha 255 throughout, premultiplied and straight are the
        // same image, so no mode can be the wrong one.
        for pixel in out.chunks_exact_mut(4) {
            pixel[3] = 0xFF;
        }

        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        Ok(out)
    }
}
