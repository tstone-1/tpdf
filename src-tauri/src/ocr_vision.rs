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
use objc2::AnyThread;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
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
        handler
            .performRequests_error(&requests)
            .map_err(|e| RecogniseError::Rejected(format!("Vision refused the image: {e}")))?;

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
            let b = unsafe { observation.boundingBox() };
            items.push(RecognisedItem {
                text,
                rect: normalised_to_points(
                    (b.origin.x, b.origin.y, b.size.width, b.size.height),
                    pixels.width,
                    pixels.height,
                    pixels.scale,
                ),
                confidence: Some(best.confidence()),
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
