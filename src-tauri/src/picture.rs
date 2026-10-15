//! A picture placed on a page, sent from the webview as bytes.
//!
//! A picture is up to 16 MiB of RGBA ([`signature::PICTURE_PIXELS`]), and a
//! command's arguments travel as JSON, where each byte becomes up to four
//! characters and a number in the webview's memory. So `annot_picture` takes
//! a raw body instead, laid out as:
//!
//! ```text
//! u32 little-endian   length of the JSON that follows, at most HEADER_MAX
//! JSON                { "doc", "sweep", "width", "height", "mark": NewMark }
//! the rest            width * height * 4 bytes of straight-alpha RGBA
//! ```
//!
//! Nothing here decodes a picture file. The webview decoded it, as it decodes
//! a signature image; what arrives is pixels, checked for their count here and
//! for their limits by the model, as every other mark is.

use std::sync::Arc;

use serde::Deserialize;

use crate::docmodel::MarkKind;
use crate::edits::NewMark;
use crate::signature;

/// Longest JSON part of an upload. A mark's note is bounded far below this by
/// `edits::too_long`, so a longer header is not a mark.
pub const HEADER_MAX: usize = 64 * 1024;

/// What the JSON part says.
#[derive(Deserialize)]
struct Header {
    doc: u32,
    #[serde(default)]
    sweep: u64,
    width: u32,
    height: u32,
    mark: NewMark,
}

/// A parsed upload: the document, the gesture and the mark with its pixels.
#[derive(Debug)]
pub struct Upload {
    pub doc: u32,
    pub sweep: u64,
    pub mark: NewMark,
}

/// Reads an upload. Refuses a body whose parts do not add up, a mark that is
/// not a picture, and a JSON mark that brings pixels of its own.
pub fn parse(body: &[u8]) -> Result<Upload, String> {
    let bad = |why: &str| format!("not a picture upload: {why}");
    let (length, rest) = body
        .split_first_chunk::<4>()
        .ok_or_else(|| bad("too short"))?;
    let length = u32::from_le_bytes(*length) as usize;
    if length > HEADER_MAX || length > rest.len() {
        return Err(bad("the header length is out of range"));
    }
    let (json, rgba) = rest.split_at(length);
    let header: Header = serde_json::from_slice(json).map_err(|e| bad(&e.to_string()))?;
    if header.mark.kind != MarkKind::Picture {
        return Err(bad("the mark is not a picture"));
    }
    if header.mark.image.is_some() {
        return Err(bad("the pixels came twice"));
    }
    let expected = (header.width as usize)
        .checked_mul(header.height as usize)
        .and_then(|n| n.checked_mul(4));
    if expected != Some(rgba.len()) {
        return Err(bad("the pixel count does not match the size"));
    }
    let mut mark = header.mark;
    mark.image = Some(Arc::new(signature::Image {
        width: header.width,
        height: header.height,
        rgba: rgba.to_vec(),
    }));
    Ok(Upload {
        doc: header.doc,
        sweep: header.sweep,
        mark,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str, rgba: &[u8]) -> Vec<u8> {
        let mut out = (json.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(json.as_bytes());
        out.extend_from_slice(rgba);
        out
    }

    fn header(kind: &str, width: u32, height: u32) -> String {
        format!(
            r#"{{"doc":3,"sweep":9,"width":{width},"height":{height},
               "mark":{{"kind":"{kind}","page":1,"quads":[10,20,110,70],"strokes":[],
                        "color":[0,0,0],"author":"","note":"","width":1}}}}"#
        )
    }

    #[test]
    fn an_upload_carries_the_pixels_into_the_mark() {
        let rgba = [1, 2, 3, 255, 4, 5, 6, 0];
        let upload = parse(&body(&header("picture", 2, 1), &rgba)).unwrap();
        assert_eq!((upload.doc, upload.sweep), (3, 9));
        let image = upload.mark.image.unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, rgba);
    }

    #[test]
    fn an_upload_whose_parts_do_not_add_up_is_refused() {
        let rgba = [1, 2, 3, 255, 4, 5, 6, 0];
        // One byte short, one byte over, and a size whose product overflows.
        assert!(parse(&body(&header("picture", 2, 1), &rgba[..7])).is_err());
        assert!(parse(&body(
            &header("picture", 2, 1),
            &[rgba.as_slice(), &[0]].concat()
        ))
        .is_err());
        assert!(parse(&body(&header("picture", u32::MAX, u32::MAX), &rgba)).is_err());
        // A header length past the body, and a body too short to hold one.
        let mut long = body(&header("picture", 2, 1), &rgba);
        let past = long.len() as u32;
        long[..4].copy_from_slice(&past.to_le_bytes());
        assert!(parse(&long).is_err());
        assert!(parse(&[1, 0]).is_err());
    }

    #[test]
    fn only_a_picture_comes_this_way_and_its_pixels_only_once() {
        let rgba = [1, 2, 3, 255];
        assert!(parse(&body(&header("picture", 1, 1), &rgba)).is_ok());
        assert!(parse(&body(&header("signature", 1, 1), &rgba)).is_err());
        let twice = header("picture", 1, 1).replace(
            r#""width":1}"#,
            r#""width":1,"image":{"width":1,"height":1,"rgba":[1,2,3,255]}}"#,
        );
        assert!(twice.contains("\"image\""));
        assert!(parse(&body(&twice, &rgba)).is_err());
    }

    #[test]
    fn a_placed_picture_is_viewed_by_its_size_and_fetched_by_its_id() {
        let edits = crate::edits::Edits::default();
        edits.open(3, 1, None);
        let page = edits.state(3).unwrap().pages[0].id;
        let rgba = [1, 2, 3, 255, 4, 5, 6, 0];
        let json = header("picture", 2, 1).replace(r#""page":1"#, &format!(r#""page":{page}"#));
        let upload = parse(&body(&json, &rgba)).unwrap();
        let made = crate::save::pdf_date(std::time::SystemTime::UNIX_EPOCH);
        let state = edits
            .annotate_in(upload.doc, upload.mark, made, upload.sweep)
            .unwrap();
        let view = &state.marks[0];
        assert_eq!(view.kind, MarkKind::Picture);
        // The size in the view, the pixels out of it.
        assert_eq!(
            view.picture,
            Some(crate::edits::PictureSize {
                width: 2,
                height: 1
            })
        );
        assert!(view.image.is_none());
        assert_eq!(edits.picture_pixels(3, view.id).unwrap().rgba, rgba);
        // Only a picture is fetched this way, and only in its own document: a
        // signature holds pixels too, and is not one.
        let signature: crate::edits::NewMark = serde_json::from_str(&format!(
            r#"{{"kind":"signature","page":{page},"quads":[10,20,110,70],"strokes":[],
                "color":[0,0,0],"author":"","note":"","width":1,
                "image":{{"width":1,"height":1,"rgba":[1,2,3,255]}}}}"#
        ))
        .unwrap();
        let made = crate::save::pdf_date(std::time::SystemTime::UNIX_EPOCH);
        let state = edits.annotate_in(3, signature, made, 0).unwrap();
        let signed = state
            .marks
            .iter()
            .find(|m| m.kind == MarkKind::Signature)
            .unwrap();
        assert!(edits.picture_pixels(3, signed.id).is_err());
        assert!(edits.picture_pixels(3, signed.id + 1).is_err());
        assert!(edits.picture_pixels(4, view.id).is_err());
    }
}
