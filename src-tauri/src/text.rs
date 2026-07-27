//! Characters, where they sit on the page, and what asking costs.
//!
//! Selection, search and the accessibility tree all need the same thing: the
//! page's characters with their positions. This is that layer, and it is
//! deliberately the *only* one --- three features reading three different
//! extractions would disagree with each other in ways no test would catch,
//! because each would be self-consistent.
//!
//! ## Codes, not a string
//!
//! [`PageText`] carries one Unicode scalar per PDFium character index, not a
//! string. `FPDFText_GetText` exists and would be shorter, but it extracts
//! UCS-2 and, in its own words, "ignores characters without UCS-2
//! representations" --- so the string it returns and the character indices the
//! boxes are keyed by can silently fall out of step, on exactly the documents
//! (CJK, symbol fonts, anything astral) where nobody would notice until a
//! selection highlighted the wrong glyphs.
//!
//! One code per index cannot desync, and the caller builds whatever string it
//! wants from the range it selected. It is the same lesson `AGENTS.md` records
//! for `set_text()`: work in the code space the document uses, not in a
//! re-encoding of it.
//!
//! ## Page space, and why the boxes are flipped here
//!
//! PDFium reports character boxes in page space --- y upwards, origin at the
//! bottom-left. Every consumer here works in device space --- y downwards,
//! origin top-left --- because that is what the tiles and the viewport are in.
//! The flip happens once, at the bottom of this file, against the same
//! `height_pt` that `Placement` uses to map a render onto a bitmap. Doing it
//! anywhere else means two conventions in the codebase and an inevitable
//! off-by-a-page-height.

use std::os::raw::{c_double, c_int};

use pdfium_render::prelude::*;

use crate::progressive::{Bindings, RawPage};

/// A loaded `FPDF_TEXTPAGE`, closed on drop.
///
/// Borrows its page for the same reason [`RawPage`] borrows its document:
/// PDFium does not tolerate a text page outliving the page it was loaded from,
/// and the lifetime is what makes that unrepresentable rather than merely
/// documented.
pub struct RawTextPage<'page> {
    bindings: Bindings,
    handle: FPDF_TEXTPAGE,
    _page: std::marker::PhantomData<&'page ()>,
}

impl<'page> RawTextPage<'page> {
    /// Loads the text of a page.
    pub fn load(page: &'page RawPage<'_>) -> Result<Self, String> {
        let bindings = page.bindings();
        // SAFETY: the handle is valid for the borrow of `page`.
        let handle = unsafe { bindings.FPDFText_LoadPage(page.handle()) };
        if handle.is_null() {
            return Err("could not load the page's text".to_string());
        }
        Ok(Self {
            bindings,
            handle,
            _page: std::marker::PhantomData,
        })
    }

    /// Characters on the page, including ones that draw nothing.
    pub fn count(&self) -> u32 {
        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        let count = unsafe { self.bindings.FPDFText_CountChars(self.handle) };
        count.max(0) as u32
    }

    /// The Unicode scalar at a character index.
    pub fn code(&self, index: u32) -> u32 {
        // SAFETY: as above; an out-of-range index returns 0 rather than faulting.
        unsafe {
            self.bindings
                .FPDFText_GetUnicode(self.handle, index as c_int)
        }
    }

    /// The tight box of a character, in page space: `[left, bottom, right, top]`.
    ///
    /// `None` when PDFium declines, which it does for characters that occupy no
    /// area --- a space at the end of a line is the common one.
    pub fn char_box(&self, index: u32) -> Option<[f64; 4]> {
        let (mut left, mut right, mut bottom, mut top) = (0f64, 0f64, 0f64, 0f64);
        // SAFETY: four writable doubles, and the index is bounds-checked by
        // PDFium, which returns false rather than writing on a bad one.
        let ok = unsafe {
            self.bindings.FPDFText_GetCharBox(
                self.handle,
                index as c_int,
                &mut left as *mut c_double,
                &mut right as *mut c_double,
                &mut bottom as *mut c_double,
                &mut top as *mut c_double,
            )
        };
        // Note the argument order: PDFium takes left, *right*, bottom, top --
        // not the left, bottom, right, top that every rect in this file uses.
        (ok != 0).then_some([left, bottom, right, top])
    }
}

impl Drop for RawTextPage<'_> {
    fn drop(&mut self) {
        // SAFETY: loaded by `load`, closed exactly once, and its page outlives
        // it by construction.
        unsafe { self.bindings.FPDFText_ClosePage(self.handle) };
    }
}

/// A page's characters and where they are, in device-space units of one point.
///
/// The arrays are flat and parallel rather than a `Vec` of structs: this
/// crosses to the webview as JSON, where a struct per character would repeat
/// four field names a few thousand times per page.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PageText {
    /// One Unicode scalar per character index. See the module docs.
    pub codes: Vec<u32>,
    /// Four values per character --- `left, top, right, bottom` --- with y
    /// increasing downwards and the origin at the page's top-left corner, in
    /// PDF points. A character PDFium gave no box for is four zeroes; `codes`
    /// still carries it, so the indices stay aligned.
    pub boxes: Vec<f32>,
    /// Page height in points, so a caller can scale without a second request.
    pub height_pt: f32,
    /// Page width in points.
    pub width_pt: f32,
    /// Time spent inside PDFium extracting this, in milliseconds.
    pub extract_ms: f64,
}

impl PageText {
    /// Characters in this page.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Whether the page has no extractable characters --- a scan, typically.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

/// Extracts a page's text and character geometry.
pub fn extract(page: &RawPage<'_>) -> Result<PageText, String> {
    let started = std::time::Instant::now();
    let height_pt = page.height_pt();
    let width_pt = page.width_pt();

    let text = RawTextPage::load(page)?;
    let count = text.count();

    let mut codes = Vec::with_capacity(count as usize);
    let mut boxes = Vec::with_capacity(count as usize * 4);

    for index in 0..count {
        codes.push(text.code(index));
        match text.char_box(index) {
            Some([left, bottom, right, top]) => {
                // The one flip. `top` is the larger y in page space and becomes
                // the smaller y here, which is why it is written first.
                boxes.push(left as f32);
                boxes.push(height_pt - top as f32);
                boxes.push(right as f32);
                boxes.push(height_pt - bottom as f32);
            }
            None => boxes.extend_from_slice(&[0.0; 4]),
        }
    }

    Ok(PageText {
        codes,
        boxes,
        height_pt,
        width_pt,
        extract_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}
