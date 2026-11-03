//! *Copy page for PowerPoint*: one page as a Windows enhanced metafile whose
//! shapes PowerPoint can ungroup and whose words stay words.
//!
//! PDFium can draw a page into a metafile itself (`FPDF_RenderPage` on a
//! metafile DC), and on its own that is useless for this: it draws every glyph
//! as a path, so the EMF holds no text record and PowerPoint ungroups it into
//! hundreds of letter outlines (`docs/TRAPS.md`, *PDFium's EMF output draws
//! every glyph as an outline*). So the page's text objects are taken off a
//! private copy of the page first, PDFium draws what is left, and each text is
//! written back with `ExtTextOutW` in its own font, size, colour and angle.
//!
//! Only text PDFium can read back as Unicode is moved (`movable`). Anything
//! else --- a symbol font with no character map, a run of `U+FFFD` --- stays on
//! the page and is drawn as PDFium draws it, as outlines: the copy may then
//! hold a shape where a word would be editable, but never a wrong word.
//! Invisible text (an OCR layer over a scan) is taken off and not written, so
//! the picture shows what the page shows.
//!
//! The copy is a second PDFium document over the same bytes
//! (`DocumentGraph::reopen`), because removing an object changes the page for
//! everyone holding it and the viewer's pages are cached
//! (*Two handles to one cached page are aliases*). Removed objects are left to
//! that document to free, never destroyed (*Destroying an object removed from
//! a page segfaults*).
//!
//! Text inside a Form XObject is not moved: it is drawn as outlines.

/// One text object as it will be written back.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Words {
    /// UTF-16, as `ExtTextOutW` takes it.
    pub text: Vec<u16>,
    /// The baseline origin in the page's own space, points.
    pub at: (f64, f64),
    /// The font size after the object's matrix, points.
    pub size: f64,
    /// Counter-clockwise, degrees, in the page's own space.
    pub angle: f64,
    /// `0x00BBGGRR`, as GDI takes a colour.
    pub colour: u32,
    pub face: String,
    pub weight: i32,
    pub italic: bool,
}

/// Whether a text object's characters can be written back as they read.
///
/// Empty text, the replacement character, a control character and an unpaired
/// surrogate all mean PDFium did not know what the glyphs are, and writing
/// what it guessed would put a wrong word on the slide.
#[cfg(any(windows, test))]
pub(crate) fn movable(text: &[u16]) -> bool {
    let mut any = false;
    for unit in char::decode_utf16(text.iter().copied()) {
        match unit {
            Err(_) => return false,
            Ok('\u{FFFD}') => return false,
            Ok(c) if c.is_control() => return false,
            Ok(c) => any |= !c.is_whitespace(),
        }
    }
    any
}

/// The face, weight and slant GDI is asked for.
///
/// `family` is PDFium's family name and `base` the `/BaseFont`, which for a
/// subset is `ABCDEF+Verdana-Bold`. PDFium's own weight is believed only in
/// GDI's range: it answered 4028 for a Verdana subset, which is no weight.
/// Otherwise the style words in the base name decide, then the descriptor's
/// `ForceBold` and `Italic` flags.
#[cfg(any(windows, test))]
pub(crate) fn style(family: &str, base: &str, flags: i32, weight: i32) -> (String, i32, bool) {
    const ITALIC: i32 = 1 << 6;
    const FORCE_BOLD: i32 = 1 << 18;
    let unsubset = match base.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => base,
    };
    let (stem, styles) = match unsubset.split_once(['-', ',']) {
        Some((stem, styles)) => (stem, styles.to_ascii_lowercase()),
        None => (unsubset, String::new()),
    };
    let named = if family.trim().is_empty() {
        stem
    } else {
        family.trim()
    };
    let face = match named
        .strip_suffix("PSMT")
        .or_else(|| named.strip_suffix("MT"))
    {
        Some(short) if !short.is_empty() => short,
        _ => named,
    };
    let face = if face.is_empty() { "Arial" } else { face };
    let weight = if (100..=900).contains(&weight) {
        weight
    } else if styles.contains("black") || styles.contains("heavy") {
        900
    } else if styles.contains("extrabold") || styles.contains("ultrabold") {
        800
    } else if styles.contains("semibold") || styles.contains("demibold") {
        600
    } else if styles.contains("bold") || flags & FORCE_BOLD != 0 {
        700
    } else if styles.contains("medium") {
        500
    } else if styles.contains("light") {
        300
    } else {
        400
    };
    let italic = flags & ITALIC != 0 || styles.contains("italic") || styles.contains("oblique");
    // GDI takes a face name of at most 31 characters.
    let face: String = face.chars().take(31).collect();
    (face, weight, italic)
}

/// GDI's escapement for a text angle on a page shown turned `turns` quarter
/// turns clockwise: tenths of a degree, counter-clockwise, in `0..3600`.
#[cfg(any(windows, test))]
pub(crate) fn escapement(angle: f64, turns: u8) -> i32 {
    let degrees = angle - 90.0 * f64::from(turns % 4);
    ((degrees * 10.0).round() as i32).rem_euclid(3600)
}

#[cfg(windows)]
pub(crate) use platform::emf;

/// Puts metafile bytes on the clipboard, owned by the window `owner`.
///
/// An owner and not `NULL`: with no window, `EmptyClipboard` leaves the
/// clipboard ownerless and `SetClipboardData` then fails. Another program can
/// hold the clipboard for a moment, so opening it is tried a few times.
#[cfg(windows)]
pub fn clipboard(owner: isize, emf: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{DeleteEnhMetaFile, SetEnhMetaFileBits};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    /// `CF_ENHMETAFILE`.
    const ENHMETAFILE: u32 = 14;
    // SAFETY: the clipboard is opened and closed on this thread; the
    // metafile handle is the system's once `SetClipboardData` accepts it and
    // ours to delete otherwise.
    unsafe {
        let picture = SetEnhMetaFileBits(emf.len() as u32, emf.as_ptr());
        if picture.is_null() {
            return Err(said!("Windows could not read the picture."));
        }
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(owner as _) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            DeleteEnhMetaFile(picture);
            return Err(said!("another program is holding the clipboard."));
        }
        let set = EmptyClipboard() != 0 && !SetClipboardData(ENHMETAFILE, picture).is_null();
        CloseClipboard();
        if set {
            Ok(())
        } else {
            DeleteEnhMetaFile(picture);
            Err(said!("the clipboard refused it."))
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::{c_char, c_int};

    use pdfium_render::prelude::*;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        CloseEnhMetaFile, CreateEnhMetaFileW, CreateFontW, DeleteEnhMetaFile, DeleteObject,
        ExtTextOutW, GetDeviceCaps, GetEnhMetaFileBits, SelectObject, SetBkMode, SetTextAlign,
        SetTextColor, DEFAULT_CHARSET, HDC, LOGPIXELSX, TA_BASELINE, TA_LEFT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    use super::{escapement, movable, style, Words};
    use crate::document::OpenDocument;
    use crate::progressive::{Bindings, RawPage};

    const PAGEOBJ_TEXT: c_int = 1;
    const RENDER_INVISIBLE: i32 = 3;
    const RENDER_STROKE: i32 = 1;
    const RENDER_STROKE_CLIP: i32 = 5;
    /// Clip only: draws nothing, like invisible text.
    const RENDER_CLIP: i32 = 7;
    /// `FPDF_PRINTMODE_EMF`: draw paths, not bitmaps or PostScript.
    const PRINTMODE_EMF: c_int = 0;
    /// `FPDF_ANNOT | FPDF_PRINTING`: annotations with their print appearance.
    const FLAGS: c_int = 0x01 | 0x800;
    /// A page larger than this many bytes of EMF is refused rather than cut.
    pub(crate) const LIMIT: usize = crate::worker::TILE_CAPACITY;

    /// `FPDF_RenderPage` and `FPDF_SetPrintMode`, which `pdfium-render` binds
    /// only with its Win32 feature, looked up in the library this process
    /// already mapped.
    type RenderPage =
        unsafe extern "C" fn(HDC, FPDF_PAGE, c_int, c_int, c_int, c_int, c_int, c_int);
    type SetPrintMode = unsafe extern "C" fn(c_int) -> c_int;

    /// The address of `name` in the mapped engine.
    fn symbol(name: &std::ffi::CStr) -> Result<unsafe extern "system" fn() -> isize, String> {
        let module: Vec<u16> = "pdfium.dll\0".encode_utf16().collect();
        // SAFETY: NUL-terminated names; the module stays mapped for the
        // process's life, so the address outlives every call.
        unsafe {
            let handle = GetModuleHandleW(module.as_ptr());
            if handle.is_null() {
                return Err(said!("the PDF engine is not loaded in this process"));
            }
            GetProcAddress(handle, name.as_ptr().cast())
                .ok_or_else(|| said!("the PDF engine cannot draw a metafile"))
        }
    }

    /// The page as EMF bytes, text written as text. See the module note.
    pub(crate) fn emf(
        document: &OpenDocument,
        page: u32,
        crop: Option<[f32; 4]>,
        turns: u8,
    ) -> Result<Vec<u8>, String> {
        // SAFETY: both are the engine's exported functions of these types
        // (`fpdfview.h`).
        let (render, print_mode) = unsafe {
            (
                std::mem::transmute::<unsafe extern "system" fn() -> isize, RenderPage>(symbol(
                    c"FPDF_RenderPage",
                )?),
                std::mem::transmute::<unsafe extern "system" fn() -> isize, SetPrintMode>(symbol(
                    c"FPDF_SetPrintMode",
                )?),
            )
        };
        let bindings = document.pdfium().bindings();
        let copy = document.graph().reopen(bindings)?;
        let pages = copy.page_count() as usize;
        let raw = copy.page_cropped(page, crop, &|index| document.graph().sheet(index, pages))?;
        let words = take_words(bindings, &raw);
        let turns = turns % 4;
        let (mut width, mut height) = (f64::from(raw.width_pt()), f64::from(raw.height_pt()));
        if turns % 2 == 1 {
            std::mem::swap(&mut width, &mut height);
        }
        // SAFETY: every handle below is created and released in this block,
        // and `raw` outlives the render that reads it.
        unsafe {
            // The frame is in 0.01 mm; drawing is in the reference DC's pixels.
            let frame = RECT {
                left: 0,
                top: 0,
                right: (width / 72.0 * 2540.0).round() as i32,
                bottom: (height / 72.0 * 2540.0).round() as i32,
            };
            let dc = CreateEnhMetaFileW(
                std::ptr::null_mut(),
                std::ptr::null(),
                &frame,
                std::ptr::null(),
            );
            if dc.is_null() {
                return Err(said!("Windows would not start a picture"));
            }
            let dpi = f64::from(GetDeviceCaps(dc, LOGPIXELSX as _).max(1));
            let px = (width / 72.0 * dpi).round().max(1.0) as c_int;
            let py = (height / 72.0 * dpi).round().max(1.0) as c_int;
            if print_mode(PRINTMODE_EMF) == 0 {
                CloseEnhMetaFile(dc);
                return Err(said!("the PDF engine refused to draw a metafile"));
            }
            render(dc, raw.handle(), 0, 0, px, py, c_int::from(turns), FLAGS);
            // The words turn with the page's own `/Rotate` as well as the
            // reader's; the drawing and `FPDF_PageToDevice` apply both.
            let device = Device {
                size: (px, py),
                dpi,
                turns,
                shown: (raw.quarter_turns() + turns) % 4,
            };
            write_words(bindings, &raw, dc, &words, &device);
            let picture = CloseEnhMetaFile(dc);
            if picture.is_null() {
                return Err(said!("Windows would not finish the picture"));
            }
            let length = GetEnhMetaFileBits(picture, 0, std::ptr::null_mut()) as usize;
            if length == 0 || length > LIMIT {
                DeleteEnhMetaFile(picture);
                return Err(if length == 0 {
                    said!("Windows would not read the picture back")
                } else {
                    said!("the page is too detailed to copy as a picture")
                });
            }
            let mut bytes = vec![0u8; length];
            GetEnhMetaFileBits(picture, length as u32, bytes.as_mut_ptr());
            DeleteEnhMetaFile(picture);
            Ok(bytes)
        }
    }

    /// Reads every movable top-level text object and takes it off the page.
    fn take_words(bindings: Bindings, page: &RawPage<'_>) -> Vec<Words> {
        let handle = page.handle();
        let mut found = Vec::new();
        let mut taken = Vec::new();
        // SAFETY: the text page is closed before any object is removed, and
        // every object handle is valid while the page is.
        unsafe {
            let text_page = bindings.FPDFText_LoadPage(handle);
            if text_page.is_null() {
                return found;
            }
            for index in 0..bindings.FPDFPage_CountObjects(handle).max(0) {
                let object = bindings.FPDFPage_GetObject(handle, index);
                if object.is_null() || bindings.FPDFPageObj_GetType(object) != PAGEOBJ_TEXT {
                    continue;
                }
                let mode = bindings.FPDFTextObj_GetTextRenderMode(object);
                if mode == RENDER_INVISIBLE || mode == RENDER_CLIP {
                    taken.push(object);
                    continue;
                }
                if let Some(words) = read(bindings, object, text_page, mode) {
                    found.push(words);
                    taken.push(object);
                }
            }
            bindings.FPDFText_ClosePage(text_page);
            for object in taken {
                // Not destroyed: see the module note.
                bindings.FPDFPage_RemoveObject(handle, object);
            }
        }
        found
    }

    unsafe fn read(
        bindings: Bindings,
        object: FPDF_PAGEOBJECT,
        text_page: FPDF_TEXTPAGE,
        mode: i32,
    ) -> Option<Words> {
        let wanted = bindings.FPDFTextObj_GetText(object, text_page, std::ptr::null_mut(), 0);
        if !(4..=1 << 20).contains(&wanted) {
            return None;
        }
        let mut text = vec![0u16; wanted as usize / 2];
        let got = bindings.FPDFTextObj_GetText(object, text_page, text.as_mut_ptr(), wanted);
        text.truncate((got as usize / 2).saturating_sub(1));
        if !movable(&text) {
            return None;
        }
        let mut size = 0f32;
        if bindings.FPDFTextObj_GetFontSize(object, &mut size) == 0
            || !size.is_finite()
            || size <= 0.0
        {
            return None;
        }
        let mut m = FS_MATRIX {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        if bindings.FPDFPageObj_GetMatrix(object, &mut m) == 0 {
            return None;
        }
        let [a, b, c, d, e, f] = [m.a, m.b, m.c, m.d, m.e, m.f].map(f64::from);
        if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) {
            return None;
        }
        let scale = c.hypot(d);
        let (mut r, mut g, mut bl, mut alpha) = (0u32, 0u32, 0u32, 0u32);
        let stroked = mode == RENDER_STROKE || mode == RENDER_STROKE_CLIP;
        let coloured = if stroked {
            bindings.FPDFPageObj_GetStrokeColor(object, &mut r, &mut g, &mut bl, &mut alpha)
        } else {
            bindings.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut bl, &mut alpha)
        };
        if coloured == 0 {
            (r, g, bl) = (0, 0, 0);
        }
        let font = bindings.FPDFTextObj_GetFont(object);
        let (family, base, flags, weight) = if font.is_null() {
            (String::new(), String::new(), 0, 0)
        } else {
            (
                name(|buf, len| bindings.FPDFFont_GetFamilyName(font, buf, len)),
                name(|buf, len| bindings.FPDFFont_GetBaseFontName(font, buf, len)),
                bindings.FPDFFont_GetFlags(font),
                bindings.FPDFFont_GetWeight(font),
            )
        };
        let (face, weight, italic) = style(&family, &base, flags, weight);
        Some(Words {
            text,
            at: (e, f),
            size: f64::from(size) * scale,
            angle: b.atan2(a).to_degrees(),
            colour: (r & 0xFF) | (g & 0xFF) << 8 | (bl & 0xFF) << 16,
            face,
            weight,
            italic,
        })
    }

    /// A UTF-8 name PDFium writes into a buffer it was asked the length of.
    unsafe fn name(get: impl Fn(*mut c_char, usize) -> usize) -> String {
        let length = get(std::ptr::null_mut(), 0);
        if length == 0 || length > 4096 {
            return String::new();
        }
        let mut buffer = vec![0u8; length];
        let written = get(buffer.as_mut_ptr().cast(), length).min(length);
        buffer.truncate(written.saturating_sub(1));
        String::from_utf8_lossy(&buffer).into_owned()
    }

    /// Where the page was drawn: its size in the metafile's pixels, their
    /// density, the reader's turn PDFium was given, and the turn the page is
    /// shown at, its own `/Rotate` included.
    #[derive(Clone, Copy)]
    struct Device {
        size: (c_int, c_int),
        dpi: f64,
        turns: u8,
        shown: u8,
    }

    unsafe fn write_words(
        bindings: Bindings,
        page: &RawPage<'_>,
        dc: HDC,
        words: &[Words],
        device: &Device,
    ) {
        let Device {
            size: (px, py),
            dpi,
            turns,
            shown,
        } = *device;
        SetBkMode(dc, TRANSPARENT as _);
        SetTextAlign(dc, TA_BASELINE | TA_LEFT);
        for word in words {
            let (mut x, mut y) = (0, 0);
            if bindings.FPDF_PageToDevice(
                page.handle(),
                0,
                0,
                px,
                py,
                c_int::from(turns),
                word.at.0,
                word.at.1,
                &mut x,
                &mut y,
            ) == 0
            {
                continue;
            }
            let mut face: Vec<u16> = word.face.encode_utf16().collect();
            face.push(0);
            let tilt = escapement(word.angle, shown);
            let font = CreateFontW(
                -((word.size * dpi / 72.0).round().max(1.0) as i32),
                0,
                tilt,
                tilt,
                word.weight,
                u32::from(word.italic),
                0,
                0,
                u32::from(DEFAULT_CHARSET),
                0,
                0,
                0,
                0,
                face.as_ptr(),
            );
            if font.is_null() {
                continue;
            }
            let before = SelectObject(dc, font);
            SetTextColor(dc, word.colour);
            ExtTextOutW(
                dc,
                x,
                y,
                0,
                std::ptr::null(),
                word.text.as_ptr(),
                word.text.len() as u32,
                std::ptr::null(),
            );
            SelectObject(dc, before);
            DeleteObject(font);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn pagecopy_moves_only_text_that_reads_as_written() {
        assert!(movable(&utf16("Fertigungsunterstützung")));
        assert!(movable(&utf16(" 1),2)")));
        assert!(!movable(&utf16("")));
        assert!(!movable(&utf16("   ")));
        assert!(!movable(&utf16("ab\u{FFFD}")));
        assert!(!movable(&utf16("a\u{2}b")));
        assert!(!movable(&[0x61, 0xD800]));
    }

    #[test]
    fn pagecopy_style_reads_the_subset_name_when_pdfium_has_no_weight() {
        assert_eq!(
            style("Verdana", "ABCDEF+Verdana", 32, 4028),
            ("Verdana".into(), 400, false)
        );
        assert_eq!(
            style("Verdana", "ABCDEF+Verdana-Bold", 32, 4028),
            ("Verdana".into(), 700, false)
        );
        assert_eq!(
            style("", "ABCDEF+Arial-BoldItalicMT", 0, 0),
            ("Arial".into(), 700, true)
        );
        assert_eq!(
            style("", "TimesNewRomanPSMT", 0, 0),
            ("TimesNewRoman".into(), 400, false)
        );
        assert_eq!(
            style("Calibri", "Calibri,Light", 0, -1),
            ("Calibri".into(), 300, false)
        );
        assert_eq!(
            style("Segoe UI", "SegoeUI-Semibold", 0, 0),
            ("Segoe UI".into(), 600, false)
        );
        assert_eq!(style("Verdana", "Verdana", 1 << 18, 0).1, 700);
        assert!(style("Verdana", "Verdana", 1 << 6, 0).2);
        assert_eq!(style("Verdana", "Verdana", 0, 300).1, 300);
        assert_eq!(style("", "", 0, 0).0, "Arial");
        // A six-letter prefix that is not a subset tag is part of the name.
        assert_eq!(style("", "abcdef+Font", 0, 0).0, "abcdef+Font");
    }

    #[test]
    fn pagecopy_escapement_follows_the_view_turn() {
        assert_eq!(escapement(0.0, 0), 0);
        assert_eq!(escapement(90.0, 0), 900);
        assert_eq!(escapement(0.0, 1), 2700);
        assert_eq!(escapement(-45.0, 0), 3150);
        assert_eq!(escapement(90.0, 5), 0);
    }
}
