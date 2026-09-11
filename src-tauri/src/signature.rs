//! A bounded raster produced by the webview, with no file parser in the coordinator.

use serde::{Deserialize, Serialize};

/// Maximum normalized signature dimensions; imported images are scaled before IPC.
pub const MAX_WIDTH: u32 = 512;
/// Maximum normalized signature height.
pub const MAX_HEIGHT: u32 = 512;
/// Maximum raster bytes retained in one document's signature journal.
pub const DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// Straight-alpha RGBA pixels, top row first. Cloned through Arc in the journal.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel, in red, green, blue, alpha order.
    pub rgba: Vec<u8>,
}

impl Image {
    /// Refuses malformed, oversized and invisible rasters before journalling.
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.width <= MAX_WIDTH
            && self.height > 0
            && self.height <= MAX_HEIGHT
            && self.rgba.len() == self.width as usize * self.height as usize * 4
            && self.rgba.len() <= 512 * 256 * 4
            && self.rgba.chunks_exact(4).any(|p| p[3] != 0)
    }

    /// Builds an RGB image with a separate soft mask inside the writing worker.
    pub fn xobject(&self, doc: &mut lopdf::Document) -> lopdf::ObjectId {
        use lopdf::{dictionary, Stream};
        let mut rgb = Vec::with_capacity(self.rgba.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(self.rgba.len() / 4);
        for pixel in self.rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
            alpha.push(pixel[3]);
        }
        let base = dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => self.width as i64, "Height" => self.height as i64,
            "BitsPerComponent" => 8,
        };
        let mut mask = base.clone();
        mask.set("ColorSpace", "DeviceGray");
        let mut mask = Stream::new(mask, alpha);
        let _ = mask.compress();
        let mask = doc.add_object(mask);
        let mut image = base;
        image.set("ColorSpace", "DeviceRGB");
        image.set("SMask", mask);
        let mut image = Stream::new(image, rgb);
        let _ = image.compress();
        doc.add_object(image)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::docmodel::{Command, Doc, Mark, MarkKind, Quad};
    use std::sync::Arc;

    #[test]
    fn signature_rasters_reject_bad_lengths_limits_and_invisible_pixels() {
        let mut image = Image {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 0],
        };
        assert!(image.valid());
        image.rgba.pop();
        assert!(!image.valid());
        image.rgba.push(0);
        image.width = 513;
        assert!(!image.valid());
        image.width = 2;
        image.height = 0;
        assert!(!image.valid());
        image.height = 1;
        image.rgba[3] = 0;
        assert!(!image.valid());
        assert!(!Image {
            width: 512,
            height: 512,
            rgba: vec![255; 512 * 512 * 4]
        }
        .valid());
    }

    #[test]
    fn signatures_move_resize_undo_and_redo_without_copying_pixels() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let image = Arc::new(Image {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 0],
        });
        let mark = Mark {
            kind: MarkKind::Signature,
            page,
            quads: vec![Quad {
                left: 10.0,
                top: 20.0,
                right: 110.0,
                bottom: 70.0,
            }],
            strokes: vec![],
            stamp: None,
            image: Some(image.clone()),
            reply_to: None,
            color: [0.0; 3],
            width: 1.0,
            author: String::new(),
            made: String::new(),
        };
        let mut invalid = mark.clone();
        invalid.image = None;
        assert!(doc.annotate(invalid, String::new()).is_err());
        let id = doc.annotate(mark, String::new()).unwrap();
        doc.resize_signature(id, 200.0).unwrap();
        assert_eq!(doc.quads_of(id)[0].right, 210.0);
        assert_eq!(doc.quads_of(id)[0].bottom, 120.0);
        doc.displace(id, 3.0, 7.0).unwrap();
        assert_eq!(doc.quads_of(id)[0].left, 13.0);
        assert!(doc.undo());
        assert_eq!(doc.quads_of(id)[0].left, 10.0);
        assert!(doc.undo());
        assert_eq!(doc.quads_of(id)[0].right, 110.0);
        assert!(doc.redo());
        assert_eq!(doc.quads_of(id)[0].right, 210.0);
        assert!(Arc::ptr_eq(
            doc.mark(id).unwrap().image.as_ref().unwrap(),
            &image
        ));
        assert!(doc.resize_signature(id, f32::NAN).is_err());
        assert!(doc.resize_signature(id, -20.0).is_err());
        doc.apply(Command::Unannotate { mark: id }).unwrap();
        assert!(doc.resize_signature(id, 100.0).is_err());
        assert!(doc.undo());
        assert_eq!(doc.quads_of(id)[0].right, 210.0);
    }

    #[test]
    fn signatures_bound_the_whole_retained_journal() {
        let mut doc = Doc::open(1);
        let image = Arc::new(Image {
            width: 512,
            height: 256,
            rgba: vec![255; 512 * 256 * 4],
        });
        let mark = Mark {
            kind: MarkKind::Signature,
            page: doc.working().order()[0],
            quads: vec![Quad {
                left: 10.0,
                top: 20.0,
                right: 110.0,
                bottom: 70.0,
            }],
            strokes: vec![],
            stamp: None,
            image: Some(image),
            reply_to: None,
            color: [0.0; 3],
            width: 1.0,
            author: String::new(),
            made: String::new(),
        };
        for _ in 0..8 {
            doc.annotate(mark.clone(), String::new()).unwrap();
        }
        assert!(doc.annotate(mark.clone(), String::new()).is_err());
        assert!(doc.undo());
        // Undo retains pixels for redo, so it must not bypass the allocation bound.
        assert!(doc.annotate(mark, String::new()).is_err());
        assert!(doc.redo());
        assert_eq!(doc.working().all_marks().len(), 8);
    }
}
