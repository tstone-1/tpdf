//! `Windows.Media.Ocr` as an [`crate::ocr::Recogniser`].
//!
//! **No new package.** Two features on the `windows` crate that the print path
//! already declares --- `Media_Ocr` and `Globalization` --- measured as 0 new
//! packages, 572 before and after. That is the whole of what
//! `docs/PLAN.md` §9.10 ranked this above Tesseract for, and the alternative was
//! roughly 30 MB of language data on an 8.0 MB installer plus a second C++ image
//! parser inside the trust boundary.
//!
//! ## Three things measured before this was written
//!
//! `examples/win_ocr_probe.rs` took all three on a stock `windows-2025` runner,
//! 2026-08-29, and `BUILD.md` carries the tables:
//!
//! * **A stock install carries `en-US`.** [`RecogniseError::Unavailable`] is a
//!   real state here and it is not the normal one, so this engine ships rather
//!   than working only on machines somebody set up.
//! * **It reads identically under the containment that ships** --- job object
//!   plus low integrity, `sandbox_win::Containment::default()`. macOS is the
//!   opposite: Vision is killed by SIGTRAP under `SANDBOX_PROFILE` and wants
//!   general `file-read`, which is why OCR is a separate process there under
//!   `OCR_SANDBOX_PROFILE`. This engine needs no profile of its own.
//! * **No correction was observed**, at 44 px and again at 16 px, which is
//!   `ocr_gate::MIN_CONTROL_PX`. See [`Self::recognise`] for what that does and
//!   does not settle about [`Options::language_correction`].
//!
//! ## The coordinate conversion, and why it is the *easy* one here
//!
//! `ocr_vision.rs` carries a long warning because Vision reports a box
//! **normalised 0..1 with the origin at the bottom-left, y up**, and everything
//! in this codebase is points with the origin top-left, y down --- so a flip is
//! required and `docs/TRAPS.md` has two entries about getting one wrong.
//!
//! `OcrWord::BoundingRect` is a `Rect` in **pixels of the source bitmap, origin
//! top-left, y down**. That is already this codebase's convention, so the
//! conversion is a division by `scale` and no flip at all. The asymmetry is
//! worth stating rather than leaving for someone to infer from the absence of a
//! flip: **the two engines disagree about the convention, and only one of them
//! needs correcting.** A reader who has just come from `ocr_vision.rs` and adds
//! a flip here for symmetry breaks it.
//!
//! ## Why the pixel conversion has a test and the text does not
//!
//! [`Pixels`] is RGBA and `SoftwareBitmap` wants BGRA, so red and blue are
//! swapped on the way in. **A test that OCRs black text cannot detect a missing
//! swap**, because black is `(0, 0, 0)` and white is `(255, 255, 255)` and both
//! are unchanged by exchanging two of their channels --- which is to say the
//! obvious end-to-end test of this module is structurally blind to the one
//! transformation it performs. So [`rgba_to_bgra_opaque`] is a free function
//! tested directly, against a colour whose channels differ.

use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::ocr::{EngineId, Options, Pixels, RecogniseError, RecognisedItem, Recogniser};

/// The in-box Windows text recogniser.
pub struct WindowsOcr {
    engine: OcrEngine,
    /// The BCP-47 tag the engine actually recognises, which is not necessarily
    /// what [`Options::languages`] asked for --- see [`WindowsOcr::new`].
    language: String,
}

impl WindowsOcr {
    /// Creates one, or says why the platform cannot.
    ///
    /// **The fallback exists because two things can be true at once**: packs are
    /// installed and none of them matches the user's profile languages.
    /// `TryCreateFromUserProfileLanguages` answers `Err` for that, and treating
    /// it as "no OCR on this machine" would refuse a machine that has an engine.
    /// The tag that was used is kept and reported through [`Recogniser::id`], so
    /// every reading downstream is attributable to a language rather than to
    /// "Windows".
    ///
    /// # Errors
    ///
    /// [`RecogniseError::Unavailable`] when no recogniser language is installed,
    /// or when neither route produces an engine.
    pub fn new() -> Result<Self, RecogniseError> {
        let installed = OcrEngine::AvailableRecognizerLanguages()
            .map_err(|e| RecogniseError::Unavailable(format!("listing languages: {e}")))?;
        let tags: Vec<String> = installed
            .into_iter()
            .filter_map(|lang| lang.LanguageTag().ok().map(|t| t.to_string()))
            .collect();
        if tags.is_empty() {
            return Err(RecogniseError::Unavailable(
                "no OCR recogniser language pack is installed on this machine".into(),
            ));
        }

        let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
            Ok(engine) => engine,
            Err(profile) => {
                let tag = &tags[0];
                Language::CreateLanguage(&HSTRING::from(tag.as_str()))
                    .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
                    .map_err(|e| {
                        RecogniseError::Unavailable(format!(
                            "no engine from the profile ({profile}) and none from {tag} ({e})"
                        ))
                    })?
            }
        };

        // Asked of the engine rather than assumed from the list: the fallback
        // above and the profile route can land on different languages, and the
        // one that answers is the one this is about.
        let language = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string())
            .unwrap_or_else(|_| tags[0].clone());

        Ok(Self { engine, language })
    }

    /// The largest edge this engine will accept, in pixels.
    ///
    /// A real bound on [`Pixels`] rather than a curiosity: `ocr_gate` composites
    /// a region with a control band appended and hands the whole thing over, so a
    /// page rendered at a high scale can exceed it. Measured at **10000** on
    /// `windows-2025`; asked here rather than pinned, because it is the
    /// platform's number and not ours.
    fn max_edge() -> Option<u32> {
        OcrEngine::MaxImageDimension().ok()
    }
}

/// Swaps red and blue, and makes every pixel opaque.
///
/// [`Pixels`] is RGBA --- `docs/TRAPS.md` records PDFium's buffer being RGBA when
/// a reader expected BGRA --- and `SoftwareBitmap` is asked for `Bgra8`, so the
/// two outer channels exchange.
///
/// **Opaque is not tidiness.** `CreateCopyFromBuffer` takes no alpha mode and
/// `SoftwareBitmap::BitmapAlphaMode` is read-only, so a `Bgra8` bitmap is treated
/// as premultiplied: any pixel whose alpha is below its colour would be read as a
/// different colour, and a fully transparent one as blank. Renders here are
/// opaque, so on real input this changes nothing --- which is exactly why it is
/// done in a function with its own test rather than trusted to stay true.
///
/// Returns `None` when the buffer is not a whole number of pixels.
#[must_use]
pub fn rgba_to_bgra_opaque(rgba: &[u8]) -> Option<Vec<u8>> {
    if rgba.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], 0xFF]);
    }
    Some(out)
}

/// Converts one `OcrWord` rectangle into this codebase's convention.
///
/// Input is `(x, y, width, height)` in **pixels of the source bitmap, origin
/// top-left, y down**. Output is `left, top, right, bottom` in PDF points, same
/// origin and direction --- so this divides by `scale` and does **not** flip.
/// `ocr_vision.rs` flips because Vision's convention is the other one; adding a
/// flip here for symmetry with that module is the mistake this doc comment
/// exists to prevent.
#[must_use]
pub fn pixels_to_points(rect: (f32, f32, f32, f32), scale: f32) -> [f32; 4] {
    let (x, y, w, h) = rect;
    let left = x / scale;
    let top = y / scale;
    [left, top, left + w / scale, top + h / scale]
}

impl Recogniser for WindowsOcr {
    fn id(&self) -> EngineId {
        EngineId {
            name: "windows-ocr",
            // Windows exposes no version for the OCR engine itself, so the build
            // string carries the thing that actually decides what it reads. An
            // `EngineId` exists to let a downstream verdict be invalidated when
            // the engine changes, and a change of recogniser language is such a
            // change.
            build: format!("recogniser language {}", self.language),
        }
    }

    fn recognise(
        &self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<Vec<RecognisedItem>, RecogniseError> {
        if !pixels.is_consistent() {
            return Err(RecogniseError::MalformedInput(format!(
                "{}x{} at 4 bytes per pixel is {} bytes, and the buffer holds {}",
                pixels.width,
                pixels.height,
                (pixels.width as usize).saturating_mul(pixels.height as usize) * 4,
                pixels.rgba.len()
            )));
        }
        if let Some(max) = Self::max_edge() {
            if pixels.width > max || pixels.height > max {
                return Err(RecogniseError::Rejected(format!(
                    "{}x{} exceeds this engine's largest edge of {max} px",
                    pixels.width, pixels.height
                )));
            }
        }

        // **`Options` is honoured in one of its three fields, and saying which is
        // the point of this comment.**
        //
        // `languages` is decided at construction: an `OcrEngine` is created for a
        // language and cannot be re-pointed, so a per-call list would be a
        // parameter that silently did nothing. `id()` reports what was chosen.
        //
        // `deadline_ms` is not passed to anything, because `RecognizeAsync` takes
        // no timeout --- the same hole `ocr_worker::REPLY_DEADLINE` was written to
        // cover for Vision, and it is covered in the same place: the only process
        // that survives a wedged engine is the parent.
        //
        // `language_correction` has no switch at all. Vision honours it through
        // `setUsesLanguageCorrection`; this engine has an internal language model
        // and no way to turn it off. Measured on 2026-08-29: a non-word read back
        // verbatim at 44 px and at `ocr_gate::MIN_CONTROL_PX`, so no correction
        // was observed. That is support and not proof --- at both sizes the engine
        // read clean text exactly, so it was never near its limit, and a corrector
        // only shows where a recogniser is struggling.
        let _ = options;

        let bgra = rgba_to_bgra_opaque(pixels.rgba).ok_or_else(|| {
            RecogniseError::MalformedInput(format!(
                "the buffer is {} bytes, which is not a whole number of pixels",
                pixels.rgba.len()
            ))
        })?;

        // `DataWriter` with no stream behind it: `DetachBuffer` is the shortest
        // route from a slice to an `IBuffer`, and it sidesteps the trap about a
        // `DataWriter` closing the stream it was created over.
        let writer =
            DataWriter::new().map_err(|e| RecogniseError::Rejected(format!("DataWriter: {e}")))?;
        writer
            .WriteBytes(&bgra)
            .map_err(|e| RecogniseError::Rejected(format!("WriteBytes: {e}")))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| RecogniseError::Rejected(format!("DetachBuffer: {e}")))?;

        let width = i32::try_from(pixels.width).map_err(|_| {
            RecogniseError::Rejected(format!("width {} is too large", pixels.width))
        })?;
        let height = i32::try_from(pixels.height).map_err(|_| {
            RecogniseError::Rejected(format!("height {} is too large", pixels.height))
        })?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
                .map_err(|e| RecogniseError::Rejected(format!("CreateCopyFromBuffer: {e}")))?;

        let result = self
            .engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| RecogniseError::Crashed(format!("RecognizeAsync: {e}")))?
            .get()
            .map_err(|e| RecogniseError::Crashed(format!("awaiting RecognizeAsync: {e}")))?;

        let lines = result
            .Lines()
            .map_err(|e| RecogniseError::Crashed(format!("reading lines: {e}")))?;

        let mut items = Vec::new();
        for line in &lines {
            let words = line
                .Words()
                .map_err(|e| RecogniseError::Crashed(format!("reading words: {e}")))?;
            for word in &words {
                let text = word
                    .Text()
                    .map_err(|e| RecogniseError::Crashed(format!("reading a word: {e}")))?
                    .to_string();
                let rect = word
                    .BoundingRect()
                    .map_err(|e| RecogniseError::Crashed(format!("reading a box: {e}")))?;
                items.push(RecognisedItem {
                    text,
                    rect: pixels_to_points((rect.X, rect.Y, rect.Width, rect.Height), pixels.scale),
                    // Not `Some(0.0)`, and not a made-up number. This engine
                    // reports no per-word confidence at all, and
                    // `RecognisedItem::confidence` is an `Option` for exactly
                    // this engine: treating absent as low would make every
                    // Windows result filterable away.
                    confidence: None,
                });
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The swap is real, and this is the test the end-to-end one cannot be.
    ///
    /// A colour whose three channels all differ, so exchanging two of them is
    /// visible. Black on white --- what every OCR fixture draws --- is symmetric
    /// under the swap and would pass with the conversion deleted.
    #[test]
    fn red_and_blue_are_exchanged_and_the_pixel_is_made_opaque() {
        let rgba = [10u8, 20, 30, 40];
        let bgra = rgba_to_bgra_opaque(&rgba).expect("one whole pixel");
        assert_eq!(bgra, vec![30, 20, 10, 0xFF], "B G R A, alpha forced opaque");
    }

    /// The control the entry above names: with the swap removed this input still
    /// passes, so it must not be the only test.
    #[test]
    fn a_grey_pixel_is_unchanged_by_the_swap_which_is_why_it_cannot_be_the_test() {
        let grey = [77u8, 77, 77, 0xFF];
        let bgra = rgba_to_bgra_opaque(&grey).expect("one whole pixel");
        assert_eq!(bgra, grey.to_vec(), "a grey pixel cannot see the swap");
    }

    #[test]
    fn a_buffer_that_is_not_whole_pixels_is_refused() {
        assert!(rgba_to_bgra_opaque(&[1, 2, 3]).is_none(), "three bytes");
        assert!(
            rgba_to_bgra_opaque(&[]).is_some(),
            "no pixels is still whole"
        );
    }

    /// Scale divides out, so the answer is in points whatever the render scale.
    #[test]
    fn pixels_divide_by_scale_into_points() {
        let got = pixels_to_points((20.0, 40.0, 10.0, 6.0), 2.0);
        assert_eq!(got, [10.0, 20.0, 15.0, 23.0]);
    }

    /// **The flip that must not be here.** A box in the top half of the image has
    /// to stay in the top half; a flip would send it to the bottom. Deliberately
    /// not vertically centred --- a centred box survives a flip unchanged and
    /// tests nothing, which is the control `ocr_vision.rs` states for its own.
    #[test]
    fn a_box_near_the_top_stays_near_the_top() {
        let [_, top, _, bottom] = pixels_to_points((0.0, 10.0, 50.0, 20.0), 1.0);
        assert!(
            top < 30.0,
            "top {top} should be near the top of a 100 pt page"
        );
        assert!(bottom > top, "bottom {bottom} is below top {top}");
    }

    #[test]
    fn top_is_always_above_bottom_and_left_left_of_right() {
        for (x, y, w, h) in [
            (0.0, 0.0, 1.0, 1.0),
            (5.0, 7.0, 0.5, 0.25),
            (99.0, 3.0, 1.0, 90.0),
        ] {
            let [left, top, right, bottom] = pixels_to_points((x, y, w, h), 1.5);
            assert!(left < right, "{left} < {right}");
            assert!(top < bottom, "{top} < {bottom}");
        }
    }
}
