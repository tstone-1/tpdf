//! macOS Vision as an [`crate::ocr::Recogniser`].
//!
//! One crate was added for this --- `objc2-vision`, `Zlib OR Apache-2.0 OR MIT`, read out of
//! `cargo metadata` rather than assumed from the rest of the `objc2` family. It is the only
//! new package in the tree: `objc2-core-graphics`, which builds the `CGImage` handed to
//! Vision, was already there transitively, so declaring it directly added nothing.
//!
//! ## This must not run in the app process
//!
//! `ocr.rs` records the ladder and `examples/ocr_sandbox_probe.rs` re-measures it on demand:
//! under the parser worker's profile Vision is **killed by SIGTRAP**, and it needs general
//! `file-read` to run at all. The second half is why it cannot share the parser's boundary;
//! the first half is why it should not share *any* process whose loss matters.
//!
//! [`crate::ocr_worker`] is the process it runs in, built 2026-08-27. What it does **not**
//! do is keep this framework out of the app's address space: `objc2-vision` links Vision the
//! ordinary way, so every binary that links this module maps it at launch, called or not.
//! Linking is not calling --- see `docs/TRAPS.md`, and note that `backend-probe`'s style of
//! evidence about `libpdfium` does not transfer here, because that one is `dlopen`ed.
//!
//! ## The coordinate conversion is the part that will be wrong
//!
//! Vision reports `boundingBox` **normalized to 0..1 with the origin at the bottom-left**,
//! y increasing upwards. Every other geometry in this codebase --- [`crate::text::PageText`],
//! [`crate::ocr::RecognisedItem`] --- is PDF points with the origin at the top-left and y
//! increasing downwards. `docs/TRAPS.md` already carries two entries about exactly this class
//! of mistake, including one where a y-flip could not be detected because the fixture was a
//! dense page of uniform lines.
//!
//! So [`normalised_to_points`] is a free function with its own tests, and the flip is
//! asserted against a box that is deliberately **not** vertically centred --- a centred box
//! survives the flip unchanged and tests nothing.

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{sel, AnyThread};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSArray, NSDictionary, NSRange, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
};

use crate::ocr::{EngineId, Options, Pixels, RecogniseError, RecognisedItem, Recogniser};

/// Vision's text recogniser.
#[derive(Debug, Default, Clone, Copy)]
pub struct Vision;

/// Converts one Vision bounding box into this codebase's rectangle convention.
///
/// Input is normalized `0..1`, origin bottom-left, y up --- `(x, y, w, h)` as Vision reports
/// it. Output is `left, top, right, bottom` in PDF points, origin top-left, y down.
///
/// `scale` is pixels per point, so the pixel dimensions are divided back out and a caller
/// gets points whatever resolution the page was rendered at.
#[must_use]
pub fn normalised_to_points(
    bbox: (f64, f64, f64, f64),
    width_px: u32,
    height_px: u32,
    scale: f32,
) -> [f32; 4] {
    let (x, y, w, h) = bbox;
    let wpt = f64::from(width_px) / f64::from(scale);
    let hpt = f64::from(height_px) / f64::from(scale);

    let left = x * wpt;
    let right = (x + w) * wpt;
    // The flip. Vision's `y` is the box's *bottom* measured up from the page's bottom, so
    // the top edge is `1 - (y + h)` measured down from the page's top.
    let top = (1.0 - (y + h)) * hpt;
    let bottom = (1.0 - y) * hpt;

    [left as f32, top as f32, right as f32, bottom as f32]
}

impl Vision {
    /// Wraps a borrowed RGBA buffer in a `CGImage` without copying it.
    ///
    /// The provider borrows the caller's bytes, so the image must not outlive them. It does
    /// not: it is created and consumed inside [`Recogniser::recognise`], and nothing derived
    /// from it escapes. The release callback is therefore `None` --- there is nothing to free,
    /// and handing Core Graphics a deallocator for a borrowed slice would be a double free.
    fn image(pixels: Pixels<'_>) -> Result<CFRetained<CGImage>, RecogniseError> {
        let space = CGColorSpace::new_device_rgb()
            .ok_or_else(|| RecogniseError::Rejected("no device RGB colour space".into()))?;

        // SAFETY: the slice is valid for this call, `size` is its true length, and the
        // callback is null because the data is borrowed rather than owned.
        let provider = unsafe {
            CGDataProvider::with_data(
                std::ptr::null_mut(),
                pixels.rgba.as_ptr().cast(),
                pixels.rgba.len(),
                None,
            )
        }
        .ok_or_else(|| RecogniseError::Rejected("could not wrap the pixel buffer".into()))?;

        // `NoneSkipLast` rather than `PremultipliedLast`: a page render is opaque, and
        // declaring premultiplication we did not perform would darken every pixel that has
        // an alpha other than 255 -- which is the kind of wrong that still OCRs *almost*
        // correctly and so would not be noticed.
        let info = CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0);

        // SAFETY: dimensions and stride describe the buffer above, which `is_consistent`
        // has already been checked to match.
        let image = unsafe {
            CGImage::new(
                pixels.width as usize,
                pixels.height as usize,
                8,
                32,
                pixels.width as usize * 4,
                Some(&space),
                info,
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .ok_or_else(|| RecogniseError::Rejected("CGImageCreate returned null".into()))?;

        Ok(image)
    }
}

/// The side of the blank square [`Vision::warm`] shows the engine.
///
/// Measured rather than chosen for looks: a 64 x 64 white image, which contains no text,
/// still makes Vision compile and load all three of the model bundles a page with text
/// needs (`com.apple.e5rt.e5bundlecache/<build>/...`, three `.bundle` directories either
/// way), so the warm-up needs no text renderer and no embedded bitmap.
pub const WARM_SIDE: u32 = 64;

impl Vision {
    /// Loads the engine's models into this process, on an image that is not input.
    ///
    /// **Called by the OCR worker before its sandbox comes down, and that order is the
    /// point.** On macOS 27 (26A428) Vision compiles its text models on first use in a
    /// process and writes them to `~/Library/Caches/<name>/com.apple.e5rt.e5bundlecache`,
    /// where `<name>` is the bundle identifier inside an app bundle and the executable's
    /// name outside one. [`crate::ocr::OCR_SANDBOX_PROFILE`] denies every
    /// write, so a worker that met Vision for the first time *inside* the profile had its
    /// cache write refused (`deny(1) file-write-create ~/Library/Caches/<name>`, the only
    /// denial the kernel logged) and every recognition came back `__objc2.missingError` ---
    /// so every redaction was *not verified*. With the models loaded here, the sandboxed
    /// process never asks to write: measured on a cold cache, a page and a crop of a
    /// different shape both read after the profile, with no denial logged.
    ///
    /// The image is a constant blank square, so nothing a document supplied is processed
    /// outside the boundary. The first call on a machine pays the compile (23.4 s on an M5
    /// under 26A428, cold cache); every later worker finds the cache and pays ~0.1 s.
    ///
    /// **Both ways the worker is asked, because they are not one request to Vision.** The
    /// redaction gate asks with language detection on ([`Recogniser::recognise_any_script`])
    /// and the text layer asks without it, and whatever either needs that the other does
    /// not has to be in the process before the profile as well.
    ///
    /// **That covers every script but two, measured 2026-10-05 on 26A434 with a cold cache
    /// and the profile on.** After this warm-up a line of Chinese, Japanese, Korean or Thai
    /// is read when the worker is asked for any script. Arabic and Devanagari are not:
    /// each has a model of its own that Vision compiles the first time it *detects* that
    /// script, a blank image detects none, and inside the profile the compile's cache
    /// write is refused --- the request fails with `CRImageReaderError error 1`, which the
    /// gate reports as *not verified*. A request naming `ar-SA` or `hi-IN` on this same
    /// blank image does compile them (measured: 14.0 s and 12.6 s on a cold cache, against
    /// 23.4 s for everything above), and an Arabic page was then read inside the profile;
    /// Devanagari was not measured again afterwards. It is not done here because it more
    /// than doubles the wait before a machine's first redaction, all of it inside
    /// [`crate::ocr_worker::FIRST_REPLY_DEADLINE`], for two scripts whose failure is
    /// already the safe one.
    ///
    /// # Errors
    ///
    /// Whatever the engine reports. The caller does not treat that as fatal: a failed
    /// warm-up leaves a worker whose recognitions fail on their own, each with its own
    /// reason, and the gate above turns any of those into *not verified*.
    pub fn warm(&self) -> Result<(), RecogniseError> {
        let side = WARM_SIDE;
        let rgba = vec![0xff_u8; (side * side * 4) as usize];
        let blank = Pixels {
            rgba: &rgba,
            width: side,
            height: side,
            scale: 1.0,
        };
        self.recognise(blank, &Options::default())?;
        self.recognise_any_script(blank, &Options::default())
            .map(|_| ())
    }
}

impl Vision {
    /// The languages a text layer can be asked to expect, as Vision spells them.
    ///
    /// Asked of a request built the way [`Vision::read`] builds the text
    /// layer's --- accurate level, no detection --- because the list depends on
    /// the request: the fast level offers fewer.
    ///
    /// **This one call may run in the app process**, which the module docs
    /// forbid for recognition. It is given no image and no document, so nothing
    /// a file supplied is processed; it reads a list the system holds. Measured
    /// 2026-10-05 on 26A434: 33 tags in 15 ms, in a test process that had not
    /// used Vision before. One of them is `vi-VT`, which is Vision's spelling
    /// and is passed back to it as given.
    ///
    /// The selector arrived with macOS 12 and this binary's deployment target
    /// is older, so it is sent only where the request answers to it, as
    /// `setAutomaticallyDetectsLanguage:` is. An older system offers no list,
    /// and the window then offers the engine's own choice alone.
    ///
    /// # Errors
    ///
    /// Whatever Vision reports for the question.
    pub fn languages() -> Result<Vec<String>, RecogniseError> {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        if !request.respondsToSelector(sel!(supportedRecognitionLanguagesAndReturnError:)) {
            return Ok(Vec::new());
        }
        // SAFETY: the request is a live object that answers to the selector, and
        // the method takes nothing but the error slot the binding supplies.
        let listed = unsafe { request.supportedRecognitionLanguagesAndReturnError() }
            .map_err(|e| RecogniseError::Unavailable(format!("listing languages: {e}")))?;
        Ok(listed.iter().map(|tag| tag.to_string()).collect())
    }
}

impl Recogniser for Vision {
    fn id(&self) -> EngineId {
        EngineId {
            name: "vision",
            // Vision exposes no version of its own. The OS build is the closest honest
            // answer, and it is what actually changes the results underneath us.
            build: std::process::Command::new("sw_vers")
                .arg("-buildVersion")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map_or_else(|| "unknown".into(), |s| s.trim().to_string()),
        }
    }

    fn recognise(
        &self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<Vec<RecognisedItem>, RecogniseError> {
        Self::read(pixels, options, false)
    }

    fn recognise_any_script(
        &self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<Vec<RecognisedItem>, RecogniseError> {
        Self::read(pixels, options, true)
    }
}

impl Vision {
    /// One recognition, with or without Vision working out the language for itself.
    ///
    /// **`any_script` is `automaticallyDetectsLanguage`, and without it Vision is a Latin
    /// and Cyrillic reader.** This said "Latin, Cyrillic and Greek" until 2026-10-06, and
    /// Greek had not been measured: a page of four Greek sentences comes back as Latin
    /// and Cyrillic letters of the same shape, 4 of 187 characters right, with detection
    /// on, with it off and with any language named, because Vision offers no Greek
    /// (`docs/TRAPS.md` has the table). Armenian is misread the same way, and both are
    /// answered with a confidence of 0.3 to 0.5 where a page it reads gets 1.0.
    ///
    /// Measured 2026-10-05 on 26A434 in the gate's own probe
    /// image, 12 pt type at 2x, with the request built as it is here --- accurate level,
    /// correction off, no languages named: a line of Chinese, Japanese or Thai comes back
    /// as **no span at all**, and one of Korean, Arabic or Devanagari as nothing or as a
    /// few misread characters, depending on the word. With detection on, Chinese,
    /// Japanese, Korean and Thai are read, and so are Arabic and Devanagari where their
    /// models can be loaded ([`Vision::warm`] has where they cannot). Hebrew is read in
    /// neither mode, and neither is Georgian; both come back empty. Where the Arabic or
    /// Devanagari model cannot be loaded the request is an error and not an empty answer,
    /// and detection sends Persian, Bengali and Tamil to those two models as well, so
    /// they fail the same way. For a text layer that is recall lost; for the redaction gate an
    /// empty answer is the claim, which is why the gate asks this way and why it does not
    /// rest on the answer alone ([`crate::ocr::hold_to_scripts`]).
    ///
    /// The property arrived with macOS 13 and this binary's deployment target is older, so
    /// it is set only where the request answers to the selector. On an older system the
    /// request is the plain one, and the gate's script rule is what keeps a region in a
    /// script the engine cannot read from being certified.
    fn read(
        pixels: Pixels<'_>,
        options: &Options,
        any_script: bool,
    ) -> Result<Vec<RecognisedItem>, RecogniseError> {
        if !pixels.is_consistent() {
            return Err(RecogniseError::MalformedInput(format!(
                "{}x{} at scale {} does not describe {} bytes",
                pixels.width,
                pixels.height,
                pixels.scale,
                pixels.rgba.len()
            )));
        }

        let image = Self::image(pixels)?;

        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(options.language_correction);
        if any_script && request.respondsToSelector(sel!(setAutomaticallyDetectsLanguage:)) {
            request.setAutomaticallyDetectsLanguage(true);
        }
        if !options.languages.is_empty() {
            let langs: Vec<Retained<NSString>> = options
                .languages
                .iter()
                .map(|l| NSString::from_str(l))
                .collect();
            let refs: Vec<&NSString> = langs.iter().map(std::convert::AsRef::as_ref).collect();
            request.setRecognitionLanguages(&NSArray::from_slice(&refs));
        }

        let handler = unsafe {
            VNImageRequestHandler::initWithCGImage_options(
                VNImageRequestHandler::alloc(),
                &image,
                &NSDictionary::new(),
            )
        };

        let request_any: Retained<VNRequest> =
            Retained::into_super(Retained::into_super(request.clone()));
        let requests = NSArray::from_slice(&[request_any.as_ref()]);
        handler.performRequests_error(&requests).map_err(|e| {
            // Said apart because they have different causes: asked for any script,
            // Vision also fails on a script whose model it could not load.
            let asked = if any_script {
                "Vision refused the image when asked to read any script"
            } else {
                "Vision refused the image"
            };
            RecogniseError::Rejected(format!("{asked}: {e}"))
        })?;

        // `None` here is Vision reporting no results object at all, which is different from
        // an empty one. Both mean "nothing read", and neither may be turned into a claim
        // that nothing is there -- that decision belongs to `ocr::adjudicate`, which is why
        // this returns an empty vec rather than trying to be clever about it.
        let Some(results) = request.results() else {
            return Ok(Vec::new());
        };

        let mut items = Vec::with_capacity(results.len());
        for observation in &results {
            let candidates = observation.topCandidates(1);
            let Some(best) = candidates.iter().next() else {
                continue;
            };
            let text = best.string().to_string();
            if text.is_empty() {
                continue;
            }
            let confidence = Some(best.confidence());
            if options.words {
                // One item per word, each with the box Vision gives for that
                // range of the line. If it gives none for any of them, the line
                // goes out whole below: a coarser answer, and not a lost one.
                let mut words = Vec::new();
                for (start, length, word) in crate::ocr::word_ranges(&text) {
                    let range = NSRange::new(start, length);
                    let Ok(found) = (unsafe { best.boundingBoxForRange_error(range) }) else {
                        words.clear();
                        break;
                    };
                    let b = unsafe { found.boundingBox() };
                    words.push(RecognisedItem {
                        text: word.to_string(),
                        rect: normalised_to_points(
                            (b.origin.x, b.origin.y, b.size.width, b.size.height),
                            pixels.width,
                            pixels.height,
                            pixels.scale,
                        ),
                        confidence,
                    });
                }
                if !words.is_empty() {
                    items.append(&mut words);
                    continue;
                }
            }
            let b = unsafe { observation.boundingBox() };
            items.push(RecognisedItem {
                text,
                rect: normalised_to_points(
                    (b.origin.x, b.origin.y, b.size.width, b.size.height),
                    pixels.width,
                    pixels.height,
                    pixels.scale,
                ),
                confidence,
            });
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 200x100 px at scale 2 is a 100x50 pt page.
    const W: u32 = 200;
    const H: u32 = 100;
    const S: f32 = 2.0;

    #[test]
    fn a_box_in_the_top_left_maps_to_the_top_left() {
        // Vision: x=0, and y measured up from the bottom, so a box occupying the top tenth
        // sits at y = 0.9 with height 0.1.
        let r = normalised_to_points((0.0, 0.9, 0.5, 0.1), W, H, S);
        assert!((r[0] - 0.0).abs() < 0.01, "left: {r:?}");
        assert!((r[1] - 0.0).abs() < 0.01, "top: {r:?}");
        assert!((r[2] - 50.0).abs() < 0.01, "right: {r:?}");
        assert!((r[3] - 5.0).abs() < 0.01, "bottom: {r:?}");
    }

    #[test]
    fn a_box_in_the_bottom_right_maps_to_the_bottom_right() {
        let r = normalised_to_points((0.5, 0.0, 0.5, 0.1), W, H, S);
        assert!((r[0] - 50.0).abs() < 0.01, "left: {r:?}");
        assert!((r[1] - 45.0).abs() < 0.01, "top: {r:?}");
        assert!((r[2] - 100.0).abs() < 0.01, "right: {r:?}");
        assert!((r[3] - 50.0).abs() < 0.01, "bottom: {r:?}");
    }

    /// The list is the system's, so only what every supported system has is
    /// held: English, and tags of the shape the session file will keep.
    #[test]
    fn the_engine_lists_the_languages_it_can_be_asked_for() {
        let started = std::time::Instant::now();
        let listed = Vision::languages().expect("Vision lists its languages");
        println!(
            "{} tags in {:?}: {listed:?}",
            listed.len(),
            started.elapsed()
        );
        assert!(listed.iter().any(|tag| tag == "en-US"), "{listed:?}");
        for tag in &listed {
            assert!(crate::ocr_layer::is_language_tag(tag), "{tag}");
        }
    }

    #[test]
    fn the_vertical_flip_is_actually_applied() {
        // The discriminating case. A vertically centred box is unchanged by the flip, so a
        // test written with one passes whether or not the flip happens. This box is not
        // centred: near the top in Vision's frame, and it must come back near the top in
        // ours -- which is the *opposite* end of the number Vision handed over.
        let near_top_for_vision = normalised_to_points((0.0, 0.8, 1.0, 0.2), W, H, S);
        let near_bottom_for_vision = normalised_to_points((0.0, 0.0, 1.0, 0.2), W, H, S);
        assert!(
            near_top_for_vision[1] < near_bottom_for_vision[1],
            "a box Vision put near the top came back below one it put near the bottom: \
             {near_top_for_vision:?} vs {near_bottom_for_vision:?}"
        );
        assert!(
            near_top_for_vision[1] < 1.0,
            "the top box should be within a point of the page top, got {near_top_for_vision:?}"
        );
    }

    #[test]
    fn top_is_always_above_bottom() {
        // y-down means top < bottom numerically. An inverted subtraction would still place
        // boxes in the right order relative to each other while making every rect empty or
        // negative, which the ordering test above cannot see.
        for y in [0.0_f64, 0.3, 0.55, 0.9] {
            let r = normalised_to_points((0.1, y, 0.4, 0.1), W, H, S);
            assert!(
                r[1] < r[3],
                "top {} not above bottom {} at y={y}",
                r[1],
                r[3]
            );
            assert!(
                r[0] < r[2],
                "left {} not left of right {} at y={y}",
                r[0],
                r[2]
            );
        }
    }

    #[test]
    fn scale_divides_out_to_points() {
        // The same normalized box on the same pixels at twice the scale is half the size in
        // points. A conversion that forgot the scale would return pixels and still look
        // plausible on a scale-1 render.
        let at1 = normalised_to_points((0.0, 0.0, 1.0, 1.0), W, H, 1.0);
        let at2 = normalised_to_points((0.0, 0.0, 1.0, 1.0), W, H, 2.0);
        assert!((at1[2] - 200.0).abs() < 0.01, "{at1:?}");
        assert!((at2[2] - 100.0).abs() < 0.01, "{at2:?}");
        assert!((at1[3] - 100.0).abs() < 0.01, "{at1:?}");
        assert!((at2[3] - 50.0).abs() < 0.01, "{at2:?}");
    }
}
