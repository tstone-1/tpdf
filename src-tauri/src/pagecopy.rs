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
//! Invisible text (an OCR layer over a scan, or text at alpha 0) is taken off
//! and not written, so the picture shows what the page shows. A word that
//! anything drawn after it covers, or that a clip cuts, is left out
//! altogether: written back it would land above its cover, and drawn as
//! outlines in place it is no safer, because *Ungroup* drops the clip and makes
//! the cover a shape that can be moved off it. A translucent word stays on the
//! page as outlines. The window is told how many texts were left out.
//!
//! The copy is a second PDFium document over the same bytes
//! (`DocumentGraph::reopen`), because removing an object changes the page for
//! everyone holding it and the viewer's pages are cached
//! (*Two handles to one cached page are aliases*). Deactivated objects stay owned by the private page.
//!
//! A page containing a Form XObject uses a single bitmap for its remaining artwork.

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
    if text.len() > 8192 {
        return false; // ExtTextOutW's maximum number of UTF-16 units.
    }
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

/// A rectangle in page space, points: left, bottom, right, top.
#[cfg(any(windows, test))]
pub(crate) type Bounds = [f64; 4];

/// Whether `cover`, drawn after a text object with bounds `text`, can hide
/// any of its glyphs.
///
/// It can when the two overlap across at least a quarter of the text's
/// height. A thinner overlap is an underline, a strike-through or a table
/// rule, which crosses glyphs without hiding them, and treating those as
/// covers would turn every underlined word into outlines. A box over even one
/// letter of a number covers its full height and is caught.
#[cfg(any(windows, test))]
pub(crate) fn hides(text: Bounds, cover: Bounds) -> bool {
    let [l, b, r, t] = text;
    let wide = r.min(cover[2]) - l.max(cover[0]);
    let high = t.min(cover[3]) - b.max(cover[1]);
    wide > 0.0 && high > 0.0 && high >= 0.25 * (t - b)
}

/// An editable word must fit in the visible page and every clipping rectangle.
#[cfg(any(windows, test))]
fn contains(area: Bounds, text: Bounds) -> bool {
    text[0] >= area[0] && text[1] >= area[1] && text[2] <= area[2] && text[3] <= area[3]
}

/// A positive GDI length. Rust float casts otherwise saturate oversized
/// frames, pixel dimensions and font heights to LONG_MAX.
#[cfg(any(windows, test))]
fn gdi_length(value: f64) -> Result<i32, String> {
    let rounded = value.round();
    if !(value > 0.0 && rounded <= f64::from(i32::MAX)) {
        return Err(said!(
            "the picture dimensions cannot be represented in a Windows metafile"
        ));
    }
    Ok(rounded.max(1.0) as i32)
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

    use super::{contains, escapement, gdi_length, hides, movable, style, Bounds, Words};
    use crate::document::OpenDocument;
    use crate::progressive::{Bindings, RawPage};

    const PAGEOBJ_TEXT: c_int = 1;
    const RENDER_INVISIBLE: i32 = 3;
    const RENDER_STROKE: i32 = 1;
    const RENDER_FILL_STROKE: i32 = 2;
    const RENDER_STROKE_CLIP: i32 = 5;
    const RENDER_FILL_STROKE_CLIP: i32 = 6;
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
    type RenderPage = unsafe extern "system" fn(
        HDC,
        FPDF_PAGE,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
    ) -> c_int;
    type SetPrintMode = unsafe extern "system" fn(c_int) -> c_int;

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
    ) -> Result<crate::render::CopiedPage, String> {
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
        let (words, left_out, flatten) = take_words(bindings, &raw)?;
        let turns = turns % 4;
        let unit = f64::from(document.page_units(&[page])?[0]);
        let (mut width, mut height) = (
            f64::from(raw.width_pt()) * unit,
            f64::from(raw.height_pt()) * unit,
        );
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
                right: gdi_length((width / 72.0 * 2540.0).round())?,
                bottom: gdi_length((height / 72.0 * 2540.0).round())?,
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
            let drawn = (|| {
                let dpi = f64::from(GetDeviceCaps(dc, LOGPIXELSX as _).max(1));
                let px = gdi_length(width / 72.0 * dpi)?;
                let py = gdi_length(height / 72.0 * dpi)?;
                if flatten {
                    draw_composite(bindings, raw.handle(), turns, dc, px, py)?;
                } else {
                    if print_mode(PRINTMODE_EMF) == 0 {
                        return Err(said!("the PDF engine refused to draw a metafile"));
                    }
                    if render(dc, raw.handle(), 0, 0, px, py, c_int::from(turns), FLAGS) == 0 {
                        return Err(said!("the PDF engine refused to draw a metafile"));
                    }
                }
                // The words turn with the page's own `/Rotate` as well as the
                // reader's; the drawing and `FPDF_PageToDevice` apply both.
                let device = Device {
                    size: (px, py),
                    pixels_per_unit: dpi * unit / 72.0,
                    turns,
                    shown: (raw.quarter_turns() + turns) % 4,
                };
                write_words(bindings, &raw, dc, &words, &device)
            })();
            let picture = CloseEnhMetaFile(dc);
            if let Err(why) = drawn {
                if !picture.is_null() {
                    DeleteEnhMetaFile(picture);
                }
                return Err(why);
            }
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
            let read = GetEnhMetaFileBits(picture, length as u32, bytes.as_mut_ptr());
            DeleteEnhMetaFile(picture);
            if read as usize != length {
                return Err(said!("Windows would not read the picture back"));
            }
            Ok((bytes, left_out))
        }
    }

    /// Record the visible composite as one bitmap. Ungrouping cannot separate its layers.
    unsafe fn draw_composite(
        bindings: Bindings,
        page: FPDF_PAGE,
        turns: u8,
        dc: HDC,
        px: i32,
        py: i32,
    ) -> Result<(), String> {
        use windows_sys::Win32::Graphics::Gdi::{
            StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
        };
        let (w, h) = crate::graphic::picture_size(px as u32, py as u32);
        let len = u64::from(w) * u64::from(h) * 4;
        if len >= LIMIT as u64 || w > (i32::MAX / 4) as u32 {
            return Err(said!("the page is too detailed to copy as a picture"));
        }
        let (w, h) = (w as i32, h as i32);
        let mut pixels = vec![255u8; len as usize];
        // PDFium BGRx and the top-down 32-bit Windows DIB use the same layout.
        let bitmap = bindings.FPDFBitmap_CreateEx(w, h, 3, pixels.as_mut_ptr().cast(), w * 4);
        if bitmap.is_null() {
            return Err(said!("the PDF engine refused to draw a metafile"));
        }
        bindings.FPDF_RenderPageBitmap(bitmap, page, 0, 0, w, h, i32::from(turns), FLAGS);
        bindings.FPDFBitmap_Destroy(bitmap);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };
        if StretchDIBits(
            dc,
            0,
            0,
            px,
            py,
            0,
            0,
            w,
            h,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        ) <= 0
        {
            return Err(said!("Windows could not draw the page picture"));
        }
        Ok(())
    }

    /// What one top-level object is to the copy.
    enum Seen {
        /// A visible text object: its words if they can be written back, and
        /// its bounds if PDFium reports them.
        Text {
            object: FPDF_PAGEOBJECT,
            words: Option<Words>,
            bounds: Option<Bounds>,
        },
        /// Anything else PDFium draws: it may hide text drawn before it.
        Cover(Bounds, Option<Bounds>),
        /// Taken off and not drawn: invisible text.
        Nothing,
    }

    /// Bounds that hide everything, for a cover whose extent is unknown.
    const EVERYWHERE: Bounds = [
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::INFINITY,
    ];

    /// Reads every movable top-level text object and takes it off the page.
    ///
    /// The words are written after PDFium has drawn everything else, so they
    /// land above it. A word is moved only where that changes nothing the
    /// page shows: nothing drawn after it covers it, no clip cuts it, and it
    /// is fully opaque. A word that is covered or clipped is left out; one
    /// that is only translucent, or that PDFium cannot read, stays on the
    /// page and PDFium draws it as outlines in its place. Writing it
    /// on top would put text the page hides --- under a white box, a
    /// redaction mark or outside a clip --- into the picture as readable
    /// words (`docs/TRAPS.md`, *Text written on top of a metafile shows what
    /// the page covered*).
    ///
    /// Also answers how many text objects a clip or a cover hid and were left
    /// out, so the window can say the copy lacks them. Invisible text, and
    /// text at alpha 0, is not counted: the page shows none of it, so the copy
    /// lacks nothing by leaving it out. A text hidden entirely is counted
    /// too: neither an upright clip rectangle nor the quarter-height cover
    /// rule tells "all hidden" from "partly", so the copy can lose letters the
    /// page showed, and the window says so.
    fn take_words(
        bindings: Bindings,
        page: &RawPage<'_>,
    ) -> Result<(Vec<Words>, u32, bool), String> {
        let handle = page.handle();
        let page_box = page.crop_pt().map(f64::from);
        let mut found = Vec::new();
        let mut taken = Vec::new();
        let mut left_out = 0u32;
        let mut flatten = false;
        // SAFETY: the text page is closed before any object is removed, every
        // object handle is valid while the page is, and every annotation
        // handle is closed in the loop that opened it.
        unsafe {
            let text_page = bindings.FPDFText_LoadPage(handle);
            if text_page.is_null() {
                return Err(said!("could not inspect the page text for copying"));
            }
            let mut seen = Vec::new();
            for index in 0..bindings.FPDFPage_CountObjects(handle).max(0) {
                let object = bindings.FPDFPage_GetObject(handle, index);
                if object.is_null() {
                    continue;
                }
                let bounds = bounds(bindings, object);
                let kind = bindings.FPDFPageObj_GetType(object);
                if kind != PAGEOBJ_TEXT {
                    flatten |= kind == FPDF_PAGEOBJ_FORM as i32;
                    let interior = if kind == FPDF_PAGEOBJ_PATH as i32 {
                        crate::graphic::hollow_rect(bindings, object)
                    } else {
                        None
                    };
                    seen.push(Seen::Cover(bounds.unwrap_or(EVERYWHERE), interior));
                    continue;
                }
                let mode = bindings.FPDFTextObj_GetTextRenderMode(object);
                // Text at alpha 0 draws nothing, as invisible text does; left
                // to PDFium it became a picture of nothing on the slide.
                if mode == RENDER_INVISIBLE || mode == RENDER_CLIP || unseen(bindings, object, mode)
                {
                    taken.push(object);
                    seen.push(Seen::Nothing);
                    continue;
                }
                // A word its clip cuts, or whose clip cannot be read as an
                // upright rectangle around it, is left out: PowerPoint's
                // *Ungroup* drops the metafile's clip, so even PDFium's
                // clipped outlines would show the whole word. The copy then
                // lacks what the clip let through, and never shows more than
                // the page does.
                let shows = bounds
                    .is_some_and(|at| contains(page_box, at) && clip_shows(bindings, object, at))
                    && !crate::graphic::may_be_layered(bindings, object);
                if !shows {
                    taken.push(object);
                    seen.push(Seen::Nothing);
                    left_out += 1;
                    continue;
                }
                let words = bounds.and_then(|_| read(bindings, object, text_page, mode));
                seen.push(Seen::Text {
                    object,
                    words,
                    bounds,
                });
            }
            bindings.FPDFText_ClosePage(text_page);
            // Annotations are drawn after all page content.
            let mut covers: Vec<_> = annotation_covers(bindings, handle)
                .into_iter()
                .map(|bounds| (bounds, None))
                .collect();
            let hides_text = |at, &(cover, interior): &(Bounds, Option<Bounds>)| {
                hides(at, cover) && !interior.is_some_and(|empty| contains(empty, at))
            };
            for item in seen.into_iter().rev() {
                match item {
                    Seen::Text {
                        object,
                        words: Some(words),
                        bounds: Some(at),
                    } if !covers.iter().any(|cover| hides_text(at, cover)) => {
                        found.push(words);
                        taken.push(object);
                    }
                    // Covered, readable or not: left out, and counted. Drawn
                    // as outlines in place it looked hidden on the slide, but
                    // *Ungroup* makes the cover a shape of its own, and moving
                    // it showed the word (measured 2026-11-07).
                    Seen::Text {
                        object,
                        bounds: Some(at),
                        ..
                    } if covers.iter().any(|cover| hides_text(at, cover)) => {
                        taken.push(object);
                        left_out += 1;
                    }
                    Seen::Text { bounds, .. } => covers.push((bounds.unwrap_or(EVERYWHERE), None)),
                    Seen::Cover(at, interior) => covers.push((at, interior)),
                    Seen::Nothing => {}
                }
            }
            // Walked from the last object back; written in page order.
            found.reverse();
            for object in taken {
                if bindings.FPDFPageObj_SetIsActive(object, 0) == 0 {
                    return Err(said!("could not isolate the selected artwork"));
                }
            }
        }
        Ok((found, left_out, flatten))
    }

    /// An object's bounds in page space, when PDFium reports finite ones.
    unsafe fn bounds(bindings: Bindings, object: FPDF_PAGEOBJECT) -> Option<Bounds> {
        let (mut l, mut b, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
        if bindings.FPDFPageObj_GetBounds(object, &mut l, &mut b, &mut r, &mut t) == 0 {
            return None;
        }
        let at = [l, b, r, t].map(f64::from);
        (at.iter().all(|v| v.is_finite()) && at[0] <= at[2] && at[1] <= at[3]).then_some(at)
    }

    /// Whether a text object draws nothing because every colour it is drawn
    /// with has alpha 0. A colour PDFium cannot report is not taken as unseen.
    unsafe fn unseen(bindings: Bindings, object: FPDF_PAGEOBJECT, mode: i32) -> bool {
        let (mut r, mut g, mut b, mut alpha) = (0u32, 0u32, 0u32, 0u32);
        let stroked = matches!(
            mode,
            RENDER_STROKE | RENDER_FILL_STROKE | RENDER_STROKE_CLIP | RENDER_FILL_STROKE_CLIP
        );
        let filled = !matches!(mode, RENDER_STROKE | RENDER_STROKE_CLIP);
        let fill_unseen = !filled
            || (bindings.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut b, &mut alpha) != 0
                && alpha == 0);
        let stroke_unseen = !stroked
            || (bindings.FPDFPageObj_GetStrokeColor(object, &mut r, &mut g, &mut b, &mut alpha)
                != 0
                && alpha == 0);
        fill_unseen && stroke_unseen
    }

    /// PDFium returns -1 for an object's empty clip. Each real path intersects
    /// the previous one and must itself be a rectangle containing the text.
    unsafe fn clip_shows(bindings: Bindings, object: FPDF_PAGEOBJECT, text: Bounds) -> bool {
        let clip = bindings.FPDFPageObj_GetClipPath(object);
        if clip.is_null() {
            return true;
        }
        let count = bindings.FPDFClipPath_CountPaths(clip);
        count == -1
            || (count >= 0
                && (0..count).all(|path| {
                    crate::graphic::rectangular_path(bindings, clip, path)
                        .is_ok_and(|rect| contains(rect, text))
                }))
    }

    /// The rectangles of the annotations PDFium draws over the page. A link
    /// and a popup draw nothing there, and a hidden annotation is not drawn.
    unsafe fn annotation_covers(bindings: Bindings, page: FPDF_PAGE) -> Vec<Bounds> {
        const LINK: i32 = 2;
        const POPUP: i32 = 16;
        const HIDDEN: c_int = 2;
        let mut covers = Vec::new();
        for index in 0..bindings.FPDFPage_GetAnnotCount(page).max(0) {
            let annot = bindings.FPDFPage_GetAnnot(page, index);
            if annot.is_null() {
                covers.push(EVERYWHERE);
                continue;
            }
            let subtype = bindings.FPDFAnnot_GetSubtype(annot);
            if subtype != LINK
                && subtype != POPUP
                && bindings.FPDFAnnot_GetFlags(annot) & HIDDEN == 0
            {
                let mut rect = FS_RECTF {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                };
                let at = if bindings.FPDFAnnot_GetRect(annot, &mut rect) == 0 {
                    EVERYWHERE
                } else {
                    let [l, b, r, t] =
                        [rect.left, rect.bottom, rect.right, rect.top].map(f64::from);
                    if [l, b, r, t].iter().all(|v| v.is_finite()) {
                        [l.min(r), b.min(t), l.max(r), b.max(t)]
                    } else {
                        EVERYWHERE
                    }
                };
                covers.push(at);
            }
            bindings.FPDFPage_CloseAnnot(annot);
        }
        covers
    }

    unsafe fn read(
        bindings: Bindings,
        object: FPDF_PAGEOBJECT,
        text_page: FPDF_TEXTPAGE,
        mode: i32,
    ) -> Option<Words> {
        // Accessible replacement words may describe a symbol rather than spell it.
        // Keep those glyphs in PDFium's drawing.
        let marks = bindings.FPDFPageObj_CountMarks(object);
        if !(0..=4096).contains(&marks)
            || (0..marks).any(|i| {
                let mark = bindings.FPDFPageObj_GetMark(object, i as _);
                mark.is_null()
                    || bindings.FPDFPageObjMark_GetParamValueType(mark, "ActualText") != 0
            })
        {
            return None;
        }
        let wanted = bindings.FPDFTextObj_GetText(object, text_page, std::ptr::null_mut(), 0);
        if !(4..=2 * (8192 + 1)).contains(&wanted) {
            return None;
        }
        let mut text = vec![0u16; wanted as usize / 2];
        let got = bindings.FPDFTextObj_GetText(object, text_page, text.as_mut_ptr(), wanted);
        if got != wanted {
            return None;
        }
        text.pop(); // terminating NUL
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
        // GDI can reproduce a uniform positive scale and rotation. Keep other
        // affine transforms in the original artwork at their painted dimensions.
        let scale = c.hypot(d);
        let tolerance = scale * 1e-6;
        if scale == 0. || (a - d).abs() > tolerance || (b + c).abs() > tolerance {
            return None;
        }
        let (mut r, mut g, mut bl, mut alpha) = (0u32, 0u32, 0u32, 0u32);
        let stroked = mode == RENDER_STROKE || mode == RENDER_STROKE_CLIP;
        let coloured = if stroked {
            bindings.FPDFPageObj_GetStrokeColor(object, &mut r, &mut g, &mut bl, &mut alpha)
        } else {
            bindings.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut bl, &mut alpha)
        };
        // A colour PDFium cannot report, or one that is not opaque, is drawn
        // by PDFium: GDI text has no alpha, so written back it would be
        // solid, and text at alpha 0 the page does not show would appear.
        if coloured == 0 || alpha != 255 {
            return None;
        }
        if mode == RENDER_FILL_STROKE || mode == RENDER_FILL_STROKE_CLIP {
            let (mut sr, mut sg, mut sb, mut sa) = (0u32, 0u32, 0u32, 0u32);
            if bindings.FPDFPageObj_GetStrokeColor(object, &mut sr, &mut sg, &mut sb, &mut sa) == 0
                || sa != 255
            {
                return None;
            }
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
        pixels_per_unit: f64,
        turns: u8,
        shown: u8,
    }

    unsafe fn write_words(
        bindings: Bindings,
        page: &RawPage<'_>,
        dc: HDC,
        words: &[Words],
        device: &Device,
    ) -> Result<(), String> {
        let Device {
            size: (px, py),
            pixels_per_unit,
            turns,
            shown,
        } = *device;
        if SetBkMode(dc, TRANSPARENT as _) == 0
            || SetTextAlign(dc, TA_BASELINE | TA_LEFT) == u32::MAX
        {
            return Err(said!("Windows could not draw the page text"));
        }
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
                return Err(said!("Windows could not draw the page text"));
            }
            let mut face: Vec<u16> = word.face.encode_utf16().collect();
            face.push(0);
            let tilt = escapement(word.angle, shown);
            let font = CreateFontW(
                -gdi_length(word.size * pixels_per_unit)?,
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
                return Err(said!("Windows could not draw the page text"));
            }
            let before = SelectObject(dc, font);
            if before.is_null() {
                DeleteObject(font);
                return Err(said!("Windows could not draw the page text"));
            }
            let colour = SetTextColor(dc, word.colour);
            let drawn = write_text(dc, x, y, &word.text);
            SelectObject(dc, before);
            DeleteObject(font);
            if colour == u32::MAX {
                return Err(said!("Windows could not draw the page text"));
            }
            drawn?;
        }
        Ok(())
    }

    unsafe fn write_text(dc: HDC, x: i32, y: i32, text: &[u16]) -> Result<(), String> {
        if ExtTextOutW(
            dc,
            x,
            y,
            0,
            std::ptr::null(),
            text.as_ptr(),
            text.len() as u32,
            std::ptr::null(),
        ) == 0
        {
            return Err(said!("Windows could not draw the page text"));
        }
        Ok(())
    }

    #[test]
    fn pagecopy_reports_a_failed_native_text_write() {
        // A null device context cannot accept a text record.
        let failed = unsafe { write_text(std::ptr::null_mut(), 0, 0, &[65]) };
        assert_eq!(failed.unwrap_err(), "Windows could not draw the page text");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn metafile_dimensions_refuse_saturation() {
        assert_eq!(gdi_length(f64::from(i32::MAX)), Ok(i32::MAX));
        assert_eq!(gdi_length(0.25), Ok(1));
        for dimension in [
            f64::NAN,
            f64::INFINITY,
            -1.0,
            0.0,
            f64::from(i32::MAX) + 1.0,
            1500.0 * 75_000.0 / 72.0 * 2540.0,
        ] {
            assert!(gdi_length(dimension).is_err(), "{dimension}");
        }
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
    fn pagecopy_a_cover_hides_text_only_across_a_quarter_of_its_height() {
        let text = [60.0, 646.0, 160.0, 664.0];
        // A box over the whole word, and over one letter of it.
        assert!(hides(text, [55.0, 640.0, 255.0, 670.0]));
        assert!(hides(text, [150.0, 640.0, 170.0, 670.0]));
        // The top half of the word.
        assert!(hides(text, [55.0, 655.0, 255.0, 670.0]));
        // An underline and a strike-through cross it without hiding it.
        assert!(!hides(text, [60.0, 645.0, 160.0, 647.0]));
        assert!(!hides(text, [60.0, 654.6, 160.0, 655.4]));
        // Beside it, above it, touching its edge.
        assert!(!hides(text, [170.0, 640.0, 200.0, 670.0]));
        assert!(!hides(text, [60.0, 670.0, 160.0, 690.0]));
        assert!(!hides(text, [160.0, 640.0, 200.0, 670.0]));
    }

    #[test]
    fn pagecopy_keeps_only_words_inside_the_visible_bounds() {
        let page = [10.0, 20.0, 150.0, 200.0];
        assert!(contains(page, page));
        assert!(contains(page, [12.0, 50.0, 84.0, 66.0]));
        assert!(!contains(page, [9.0, 50.0, 84.0, 66.0]));
        assert!(!contains(page, [12.0, 190.0, 84.0, 205.0]));
        assert!(movable(&vec![65; 8192]));
        assert!(!movable(&vec![65; 8193]));
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
