//! A signature image read from a file, for a caller with no webview.
//!
//! The signature chooser decodes an imported image with the webview's own
//! decoder (`src/lib/signature.ts`, `signaturedialog.ts`). The command-line
//! tool has no webview, so `sign --image` decodes here instead, inside a
//! worker (`Request::SignatureImage`): an image file is input like any other.
//!
//! What is shared with the chooser is everything but the decoder. The limits
//! below are `signature.ts`'s own, the header is read by the same grammar
//! before anything is decoded, transparent margins are trimmed, and the result
//! is scaled to the same 512 by 256 pixels and held to [`Image::valid`].
//! Two things differ: a JPEG's EXIF orientation is not applied, and the
//! scaling is an area average where the chooser uses the canvas's.

use crate::signature::Image;
use std::io::Cursor;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

/// The largest file read, `MAX_IMPORT_BYTES` in `signature.ts`.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// The most pixels decoded, `MAX_IMPORT_PIXELS` in `signature.ts`.
pub const MAX_PIXELS: u64 = 8 * 1024 * 1024;
/// The longest side decoded.
pub const MAX_SIDE: u32 = 8192;
/// The size the chooser hands on: `accept` in `signaturedialog.ts`.
const WIDTH: u32 = 512;
const HEIGHT: u32 = 256;

/// The chooser's sentence for an image it will not take.
pub const INVALID: &str =
    "it is not a valid, still PNG or JPEG image smaller than 10 MB and 8 megapixels";

/// The sentence for an image with nothing in it to draw.
pub const CLEAR: &str = "it is transparent everywhere, so nothing would be drawn";

const PNG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn bounded(width: u32, height: u32) -> Result<(u32, u32), String> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(INVALID.into());
    }
    Ok((width, height))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

/// The image's size, read from its header before anything is decoded.
/// `signatureDimensions` in `signature.ts`, rule for rule.
///
/// # Errors
///
/// [`INVALID`]: too large, not a PNG or JPEG, animated, or not framed whole.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    let invalid = || INVALID.to_string();
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    if bytes.len() >= 33 && bytes.starts_with(&PNG) {
        if be32(bytes, 8) != Some(13) || bytes.get(12..16) != Some(b"IHDR") {
            return Err(invalid());
        }
        let size = bounded(
            be32(bytes, 16).ok_or_else(invalid)?,
            be32(bytes, 20).ok_or_else(invalid)?,
        )?;
        let (mut data, mut end, mut offset) = (false, false, 8);
        while offset < bytes.len() {
            let length = be32(bytes, offset).ok_or_else(invalid)? as usize;
            let kind = bytes.get(offset + 4..offset + 8).ok_or_else(invalid)?;
            if offset + 12 > bytes.len()
                || length > bytes.len() - offset - 12
                || kind == b"acTL"
                || (offset != 8 && kind == b"IHDR")
            {
                return Err(invalid());
            }
            data |= kind == b"IDAT";
            offset += length + 12;
            if kind == b"IEND" {
                end = length == 0 && offset == bytes.len();
                break;
            }
        }
        return if data && end {
            Ok(size)
        } else {
            Err(invalid())
        };
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(invalid());
    }
    let (mut size, mut scan, mut offset) = (None, false, 2);
    while offset < bytes.len() {
        if scan {
            while offset < bytes.len() && bytes[offset] != 0xff {
                offset += 1;
            }
        }
        if bytes.get(offset) != Some(&0xff) {
            return Err(invalid());
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset).ok_or_else(invalid)?;
        offset += 1;
        if scan && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            return match size {
                Some(size) if scan && offset == bytes.len() => Ok(size),
                _ => Err(invalid()),
            };
        }
        let length = be16(bytes, offset).ok_or_else(invalid)? as usize;
        if length < 2 || offset + length > bytes.len() {
            return Err(invalid());
        }
        if marker == 0xda {
            if size.is_none() {
                return Err(invalid());
            }
            scan = true;
        }
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            if !matches!(marker, 0xc0..=0xc2)
                || size.is_some()
                || length < 8
                || bytes[offset + 2] != 8
            {
                return Err(invalid());
            }
            size = Some(bounded(
                be16(bytes, offset + 5).ok_or_else(invalid)?,
                be16(bytes, offset + 3).ok_or_else(invalid)?,
            )?);
        }
        // DNL changes a frame's height; it is outside the supported grammar.
        if marker == 0xdc {
            return Err(invalid());
        }
        offset += length;
    }
    Err(invalid())
}

fn png(bytes: &[u8], (width, height): (u32, u32)) -> Result<Vec<u8>, String> {
    let invalid = |_| INVALID.to_string();
    let pixels = width as usize * height as usize;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    // The header bounded the pixels; this bounds what the decoder may hold.
    decoder.set_limits(png::Limits {
        bytes: pixels * 4 + (1 << 20),
    });
    let mut reader = decoder.read_info().map_err(invalid)?;
    if reader.info().width != width || reader.info().height != height {
        return Err(INVALID.into());
    }
    let size = reader.output_buffer_size().ok_or(INVALID)?;
    if size > pixels * 4 {
        return Err(INVALID.into());
    }
    let mut decoded = vec![0; size];
    let frame = reader.next_frame(&mut decoded).map_err(invalid)?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(INVALID.into());
    }
    match frame.color_type {
        png::ColorType::Rgba if size == pixels * 4 => Ok(decoded),
        png::ColorType::GrayscaleAlpha if size == pixels * 2 => Ok(decoded
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect()),
        _ => Err(INVALID.into()),
    }
}

fn jpeg(bytes: &[u8], (width, height): (u32, u32)) -> Result<Vec<u8>, String> {
    let invalid = |_| INVALID.to_string();
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(width as usize)
        .set_max_height(height as usize)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut source = Cursor::new(bytes);
    let mut decoder = JpegDecoder::new_with_options(&mut source, options);
    decoder.decode_headers().map_err(invalid)?;
    let info = decoder.info().ok_or(INVALID)?;
    let pixels = width as usize * height as usize;
    if u32::from(info.width) != width
        || u32::from(info.height) != height
        || decoder.output_buffer_size() != Some(pixels * 3)
    {
        return Err(INVALID.into());
    }
    let mut rgb = vec![0; pixels * 3];
    decoder.decode_into(&mut rgb).map_err(invalid)?;
    Ok(rgb
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect())
}

// `trimSignature` in `signature.ts`: the box of the pixels that are not
// fully transparent, or nothing when there are none.
fn trimmed(width: u32, height: u32, rgba: &[u8]) -> Option<[u32; 4]> {
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if rgba[(y as usize * width as usize + x as usize) * 4 + 3] > 0 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (right > left).then_some([left, top, right, bottom])
}

// The trimmed box scaled down to fit 512 by 256, never up, each new pixel the
// average of the source pixels it covers, weighted by their alpha so that a
// transparent pixel's colour does not tint the edge.
fn scaled(width: u32, rgba: &[u8], [left, top, right, bottom]: [u32; 4]) -> Image {
    let (w, h) = (right - left, bottom - top);
    let scale = (f64::from(WIDTH) / f64::from(w))
        .min(f64::from(HEIGHT) / f64::from(h))
        .min(1.);
    let out_w = ((f64::from(w) * scale).round() as u32).max(1);
    let out_h = ((f64::from(h) * scale).round() as u32).max(1);
    let mut out = Vec::with_capacity(out_w as usize * out_h as usize * 4);
    let span = |i: u32, from: u32, to: u32| {
        let start = u64::from(i) * u64::from(from) / u64::from(to);
        let end = (u64::from(i + 1) * u64::from(from)).div_ceil(u64::from(to));
        (start as u32, (end as u32).max(start as u32 + 1).min(from))
    };
    for y in 0..out_h {
        let (y0, y1) = span(y, h, out_h);
        for x in 0..out_w {
            let (x0, x1) = span(x, w, out_w);
            let (mut sum, mut alpha) = ([0_u64; 3], 0_u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = ((top + sy) as usize * width as usize + (left + sx) as usize) * 4;
                    let a = u64::from(rgba[at + 3]);
                    for (total, value) in sum.iter_mut().zip(&rgba[at..at + 3]) {
                        *total += u64::from(*value) * a;
                    }
                    alpha += a;
                }
            }
            let count = u64::from(y1 - y0) * u64::from(x1 - x0);
            for total in sum {
                out.push((total + alpha / 2).checked_div(alpha).unwrap_or(0) as u8);
            }
            out.push(((alpha + count / 2) / count) as u8);
        }
    }
    Image {
        width: out_w,
        height: out_h,
        rgba: out,
    }
}

/// The signature image in a PNG or JPEG file's bytes.
///
/// # Errors
///
/// [`INVALID`] for what [`dimensions`] refuses and for a file the decoder
/// does not read whole, or a sentence saying that the image shows nothing.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let size = dimensions(bytes)?;
    let rgba = if bytes.starts_with(&PNG) {
        png(bytes, size)?
    } else {
        jpeg(bytes, size)?
    };
    // An image with nothing visible keeps its whole box, and is refused with
    // one that scaling averaged away: `valid` wants a pixel that shows.
    let visible = trimmed(size.0, size.1, &rgba).unwrap_or([0, 0, size.0, size.1]);
    let image = scaled(size.0, &rgba, visible);
    if !image.valid() {
        return Err(CLEAR.into());
    }
    Ok(image)
}

#[cfg(test)]
mod tests;
