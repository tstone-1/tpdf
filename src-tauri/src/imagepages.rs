//! A PDF made from pictures: one page per PNG or JPEG file.
//!
//! Pure, and run in a worker: an image file is input like any other. A JPEG's
//! bytes go into the document as they are, under `DCTDecode`, after its header
//! has been read and the whole of it decoded once, so that a file whose
//! picture data runs out is refused. A
//! PNG is decoded to eight-bit samples and compressed again, with its
//! transparency as a soft mask.
//!
//! **A page is the picture's own size.** The file's stated resolution decides
//! how many points a pixel is, and a file that states none is taken at
//! [`DEFAULT_DPI`]. A caller that asks for a paper size gets that page with
//! the picture scaled down to fit and centred, never scaled up.
//!
//! **A JPEG is turned the way its EXIF orientation says.** A phone stores a
//! portrait photograph lying on its side with a tag saying so; a page built
//! without reading the tag shows it sideways. The turn is in the page's
//! content matrix, so the picture's bytes are still untouched.

use std::io::{Cursor, Write as _};

use lopdf::{dictionary, Document, Object, Stream};
use serde::{Deserialize, Serialize};
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

/// The most pictures one document is made from.
pub const MAX_IMAGES: usize = 500;
/// The most pixels one picture may have.
pub const MAX_PIXELS: u64 = 40_000_000;
/// The longest side of a picture, in pixels.
pub const MAX_SIDE: u32 = 30_000;
/// The resolution of a picture that states none: one pixel is one point.
pub const DEFAULT_DPI: f64 = 72.0;
/// The longest side of a page, in points. The format's own limit.
pub const MAX_PAGE_PT: f64 = 14_400.0;
/// The shortest side of a page, in points.
const MIN_PAGE_PT: f64 = 3.0;
/// A stated resolution outside this range is a mistake in the file.
const PLAUSIBLE_DPI: std::ops::RangeInclusive<f64> = 30.0..=2400.0;

const PNG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// The page each picture is put on.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum Paper {
    /// The picture's own size.
    #[default]
    Own,
    /// A4, turned to suit each picture.
    A4,
    /// US Letter, turned to suit each picture.
    Letter,
}

/// How the pages are made.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Options {
    /// The page.
    pub paper: Paper,
    /// A resolution used for every picture in place of the one its file states.
    pub dpi: Option<u32>,
}

impl Options {
    /// Refuses a resolution no picture is printed at.
    ///
    /// # Errors
    ///
    /// The sentence for the reader.
    pub fn checked(self) -> Result<Self, String> {
        match self.dpi {
            Some(dpi) if !PLAUSIBLE_DPI.contains(&f64::from(dpi)) => Err(format!(
                "a resolution of {dpi} DPI is outside {:.0} to {:.0}",
                PLAUSIBLE_DPI.start(),
                PLAUSIBLE_DPI.end()
            )),
            _ => Ok(self),
        }
    }
}

/// One picture, ready to be an image object.
#[derive(Debug)]
struct Picture {
    width: u32,
    height: u32,
    /// `DeviceGray` or `DeviceRGB`.
    gray: bool,
    /// `DCTDecode` or `FlateDecode`.
    jpeg: bool,
    data: Vec<u8>,
    /// Eight-bit alpha, deflated, when any pixel is not opaque.
    mask: Option<Vec<u8>>,
    /// EXIF orientation, 1 to 8.
    orientation: u8,
    /// Horizontal and vertical resolution the file states.
    dpi: Option<(f64, f64)>,
}

fn be16(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn bounded(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("it has no pixels".into());
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "it is {width} x {height} pixels, and tpdf takes pictures up to {} megapixels and \
             {MAX_SIDE} pixels a side",
            MAX_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

/// A resolution worth using, or none.
fn plausible(x: f64, y: f64) -> Option<(f64, f64)> {
    (PLAUSIBLE_DPI.contains(&x) && PLAUSIBLE_DPI.contains(&y)).then_some((x, y))
}

/// The orientation in an EXIF segment's payload, after `Exif\0\0`.
fn exif_orientation(tiff: &[u8]) -> Option<u8> {
    let little = match tiff.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let pair: [u8; 2] = tiff.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    };
    let u32_at = |at: usize| {
        let four: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(four)
        } else {
            u32::from_be_bytes(four)
        })
    };
    let directory = u32_at(4)? as usize;
    let entries = usize::from(u16_at(directory)?);
    // A directory of a few hundred entries is already not a camera's.
    for entry in 0..entries.min(512) {
        let at = directory + 2 + entry * 12;
        if u16_at(at)? == 0x0112 {
            let value = u16_at(at + 8)?;
            return u8::try_from(value).ok().filter(|v| (1..=8).contains(v));
        }
    }
    None
}

/// What a JPEG's header says, read without decoding anything.
struct JpegFacts {
    width: u32,
    height: u32,
    components: u8,
    orientation: u8,
    dpi: Option<(f64, f64)>,
}

fn jpeg_facts(bytes: &[u8]) -> Result<JpegFacts, String> {
    let broken = || "it is not a complete JPEG image".to_string();
    let mut facts = JpegFacts {
        width: 0,
        height: 0,
        components: 0,
        orientation: 1,
        dpi: None,
    };
    let mut offset = 2;
    loop {
        if bytes.get(offset) != Some(&0xff) {
            return Err(broken());
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset).ok_or_else(broken)?;
        offset += 1;
        let length = be16(bytes, offset).ok_or_else(broken)?;
        if length < 2 || offset + length > bytes.len() {
            return Err(broken());
        }
        let payload = &bytes[offset + 2..offset + length];
        match marker {
            0xe0 if payload.starts_with(b"JFIF\0") && payload.len() >= 12 => {
                let (x, y) = (
                    f64::from(u16::from_be_bytes([payload[8], payload[9]])),
                    f64::from(u16::from_be_bytes([payload[10], payload[11]])),
                );
                facts.dpi = match payload[7] {
                    1 => plausible(x, y),
                    2 => plausible(x * 2.54, y * 2.54),
                    _ => None,
                };
            }
            0xe1 if payload.starts_with(b"Exif\0\0") => {
                if let Some(found) = exif_orientation(&payload[6..]) {
                    facts.orientation = found;
                }
            }
            // The frame: baseline, extended or progressive, eight bits a sample.
            0xc0..=0xc2 => {
                if payload.len() < 6 || payload[0] != 8 {
                    return Err("it is a JPEG with more than eight bits a sample".into());
                }
                facts.height = u32::from(u16::from_be_bytes([payload[1], payload[2]]));
                facts.width = u32::from(u16::from_be_bytes([payload[3], payload[4]]));
                facts.components = payload[5];
                return Ok(facts);
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(
                    "it is a lossless or arithmetic-coded JPEG, which PDF readers do \
                            not display"
                        .into(),
                );
            }
            0xda | 0xd9 => return Err(broken()),
            _ => {}
        }
        offset += length;
    }
}

fn jpeg(bytes: &[u8]) -> Result<Picture, String> {
    let facts = jpeg_facts(bytes)?;
    bounded(facts.width, facts.height)?;
    let gray = match facts.components {
        1 => true,
        3 => false,
        4 => return Err("it is a CMYK JPEG, which tpdf does not place yet; save it as RGB".into()),
        other => return Err(format!("it is a JPEG with {other} colour components")),
    };
    // Decoded once and thrown away: the bytes that go into the document are
    // the file's. This refuses a file whose picture data runs out; the
    // decoder forgives a missing end marker and a few missing bytes.
    let colors = if gray {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(facts.width as usize)
        .set_max_height(facts.height as usize)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(colors);
    let mut source = Cursor::new(bytes);
    let mut decoder = JpegDecoder::new_with_options(&mut source, options);
    let pixels = facts.width as usize * facts.height as usize;
    let wanted = pixels * if gray { 1 } else { 3 };
    let undecodable = |e: zune_jpeg::errors::DecodeErrors| format!("it does not decode: {e}");
    decoder.decode_headers().map_err(undecodable)?;
    if decoder.output_buffer_size() != Some(wanted) {
        return Err("its header and its picture disagree about its size".into());
    }
    let mut decoded = vec![0; wanted];
    decoder.decode_into(&mut decoded).map_err(undecodable)?;
    drop(decoded);
    Ok(Picture {
        width: facts.width,
        height: facts.height,
        gray,
        jpeg: true,
        data: bytes.to_vec(),
        mask: None,
        orientation: facts.orientation,
        dpi: facts.dpi,
    })
}

fn deflated(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(raw)
        .and_then(|()| encoder.finish())
        .map_err(|e| format!("its pixels could not be compressed: {e}"))
}

fn png(bytes: &[u8]) -> Result<Picture, String> {
    let undecodable = |e: png::DecodingError| format!("it does not decode: {e}");
    let be32 = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    // The size from the header, before the decoder is given any room.
    let (width, height) = match (bytes.get(12..16), be32(16), be32(20)) {
        (Some(b"IHDR"), Some(width), Some(height)) => (width, height),
        _ => return Err("it is not a complete PNG image".into()),
    };
    bounded(width, height)?;
    let pixels = width as usize * height as usize;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: pixels * 4 + (1 << 20),
    });
    let mut reader = decoder.read_info().map_err(undecodable)?;
    if reader.info().width != width || reader.info().height != height {
        return Err("its header and its picture disagree about its size".into());
    }
    let dpi = reader.info().pixel_dims.and_then(|dims| match dims.unit {
        png::Unit::Meter => plausible(f64::from(dims.xppu) * 0.0254, f64::from(dims.yppu) * 0.0254),
        png::Unit::Unspecified => None,
    });
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= pixels * 4)
        .ok_or("it is larger than its header says")?;
    let mut decoded = vec![0; size];
    let frame = reader.next_frame(&mut decoded).map_err(undecodable)?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err("it could not be read as eight-bit samples".into());
    }
    let (channels, gray, alpha) = match frame.color_type {
        png::ColorType::Grayscale => (1, true, false),
        png::ColorType::GrayscaleAlpha => (2, true, true),
        png::ColorType::Rgb => (3, false, false),
        png::ColorType::Rgba => (4, false, true),
        png::ColorType::Indexed => return Err("its palette could not be expanded".into()),
    };
    if size != pixels * channels {
        return Err("it is not the size its header says".into());
    }
    let (data, mask) = if alpha {
        let colour = channels - 1;
        let mut samples = Vec::with_capacity(pixels * colour);
        let mut opacity = Vec::with_capacity(pixels);
        for pixel in decoded.chunks_exact(channels) {
            samples.extend_from_slice(&pixel[..colour]);
            opacity.push(pixel[colour]);
        }
        // An alpha channel that hides nothing is left out.
        let shows_through = opacity.iter().any(|a| *a != 255);
        (samples, shows_through.then_some(opacity))
    } else {
        (decoded, None)
    };
    Ok(Picture {
        width,
        height,
        gray,
        jpeg: false,
        data: deflated(&data)?,
        mask: mask.as_deref().map(deflated).transpose()?,
        orientation: 1,
        dpi,
    })
}

fn picture(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.starts_with(&PNG) {
        png(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg(bytes)
    } else {
        Err("it is not a PNG or JPEG image".into())
    }
}

/// Where a picture goes: the page, and the matrix that places the image's
/// unit square on it.
#[derive(Debug, PartialEq)]
pub struct Placement {
    /// The page, in points.
    pub page: (f64, f64),
    /// `a b c d e f` for `cm`.
    pub matrix: [f64; 6],
}

/// The page and matrix for a picture of `width` by `height` stored pixels.
///
/// `orientation` is EXIF's: 5 to 8 show the picture with its sides exchanged.
#[must_use]
pub fn placement(
    width: u32,
    height: u32,
    orientation: u8,
    stated: Option<(f64, f64)>,
    options: Options,
) -> Placement {
    let (dpi_x, dpi_y) = match options.dpi {
        Some(dpi) => (f64::from(dpi), f64::from(dpi)),
        None => stated.unwrap_or((DEFAULT_DPI, DEFAULT_DPI)),
    };
    let stored = (
        f64::from(width) * 72.0 / dpi_x,
        f64::from(height) * 72.0 / dpi_y,
    );
    let turned = orientation >= 5;
    // The picture as it is shown, in points.
    let (mut w, mut h) = if turned { (stored.1, stored.0) } else { stored };
    let paper = match options.paper {
        Paper::Own => None,
        Paper::A4 => Some((595.0, 842.0)),
        Paper::Letter => Some((612.0, 792.0)),
    };
    let page = match paper {
        // Turned to the picture: a landscape picture gets a landscape page.
        Some((short, long)) if w > h => (long, short),
        Some(upright) => upright,
        None => {
            // Within what a page may be, keeping the picture's shape.
            let fit = (MAX_PAGE_PT / w.max(h)).min(1.0);
            let grow = (MIN_PAGE_PT / w.min(h)).max(1.0);
            let scale = if fit < 1.0 { fit } else { grow };
            (w * scale, h * scale)
        }
    };
    // Down to fit, never up.
    let scale = (page.0 / w).min(page.1 / h).min(1.0);
    w *= scale;
    h *= scale;
    let (x, y) = ((page.0 - w) / 2.0, (page.1 - h) / 2.0);
    // The unit square's corner (u, v) lands at (a·u + c·v + e, b·u + d·v + f);
    // the stored picture's top row is v = 1.
    let matrix = match orientation {
        2 => [-w, 0.0, 0.0, h, x + w, y],
        3 => [-w, 0.0, 0.0, -h, x + w, y + h],
        4 => [w, 0.0, 0.0, -h, x, y + h],
        5 => [0.0, -h, -w, 0.0, x + w, y + h],
        6 => [0.0, -h, w, 0.0, x, y + h],
        7 => [0.0, h, w, 0.0, x, y],
        8 => [0.0, h, -w, 0.0, x + w, y],
        _ => [w, 0.0, 0.0, h, x, y],
    };
    Placement { page, matrix }
}

fn number(value: f64) -> Object {
    // Two decimals of a point is finer than any reader places a page.
    Object::Real(((value * 100.0).round() / 100.0) as f32)
}

/// The document: one page for each of `images`, in order.
///
/// `images` pairs each file's bytes with the name to call it in a refusal.
///
/// # Errors
///
/// No picture, too many, or one that is not a PNG or JPEG that tpdf can
/// place, named by its label.
pub fn document(images: &[(&[u8], &str)], options: Options) -> Result<Document, String> {
    let options = options.checked()?;
    if images.is_empty() {
        return Err("choose at least one picture".into());
    }
    if images.len() > MAX_IMAGES {
        return Err(format!(
            "{} pictures were chosen, and tpdf makes a document from at most {MAX_IMAGES} at once",
            images.len()
        ));
    }
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::with_capacity(images.len());
    for (bytes, label) in images {
        let found = picture(bytes).map_err(|why| format!("{label} cannot be used: {why}"))?;
        let at = placement(
            found.width,
            found.height,
            found.orientation,
            found.dpi,
            options,
        );
        let mask = found.mask.map(|alpha| {
            doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => i64::from(found.width), "Height" => i64::from(found.height),
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                    "Filter" => "FlateDecode",
                },
                alpha,
            ))
        });
        let mut image = dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => i64::from(found.width), "Height" => i64::from(found.height),
            "ColorSpace" => if found.gray { "DeviceGray" } else { "DeviceRGB" },
            "BitsPerComponent" => 8,
            "Filter" => if found.jpeg { "DCTDecode" } else { "FlateDecode" },
        };
        if let Some(mask) = mask {
            image.set("SMask", mask);
        }
        // `Stream::new` would mark it for compression; these bytes are final.
        let mut stream = Stream::new(image, found.data);
        stream.allows_compression = false;
        let image = doc.add_object(stream);
        let [a, b, c, d, e, f] = at.matrix;
        let content = format!("q {a:.2} {b:.2} {c:.2} {d:.2} {e:.2} {f:.2} cm /Im0 Do Q");
        let content = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), number(at.page.0), number(at.page.1)],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            "Contents" => content,
        })));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    Ok(doc)
}

#[cfg(test)]
mod tests;
