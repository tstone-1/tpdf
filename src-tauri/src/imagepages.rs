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
/// Accepted manual DPI and plausible JPEG metadata. Metered PNG uses its own units.
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
            Some(dpi) if !PLAUSIBLE_DPI.contains(&f64::from(dpi)) => Err(said!(
                "a resolution of {dpi} DPI is outside {least:.0} to {most:.0}",
                least = PLAUSIBLE_DPI.start(),
                most = PLAUSIBLE_DPI.end()
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
    profile: Option<Vec<u8>>,
    intent: Option<png::SrgbRenderingIntent>,
    calibration: Option<Object>,
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
        return Err(said!("it has no pixels"));
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(said!(
            "it is {width} x {height} pixels, and tpdf takes pictures up to {megapixels} megapixels and \
             {MAX_SIDE} pixels a side",
            megapixels = MAX_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

/// A resolution worth using, or none.
fn plausible(x: f64, y: f64) -> Option<(f64, f64)> {
    (PLAUSIBLE_DPI.contains(&x) && PLAUSIBLE_DPI.contains(&y)).then_some((x, y))
}

/// A unitless density specifies shape only. Set the wider pixel dimension to
/// one point, as for a picture with no physical resolution.
fn relative_density(x: f64, y: f64) -> Option<(f64, f64)> {
    let least = x.min(y);
    (least > 0.0 && x != y).then(|| (DEFAULT_DPI * x / least, DEFAULT_DPI * y / least))
}

#[derive(Default)]
struct Exif {
    orientation: Option<u8>,
    dpi: Option<(f64, f64)>,
}

/// The orientation and physical resolution in an EXIF segment's payload, after `Exif\0\0`.
fn exif_properties(tiff: &[u8]) -> Option<Exif> {
    let little = match tiff.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let pair: [u8; 2] = tiff.get(at..)?.get(..2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    };
    let u32_at = |at: usize| {
        let four: [u8; 4] = tiff.get(at..)?.get(..4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(four)
        } else {
            u32::from_be_bytes(four)
        })
    };
    let directory = u32_at(4)? as usize;
    let entries = usize::from(u16_at(directory)?);
    let rational = |offset: u32| {
        let pos = usize::try_from(offset).ok()?;
        let numerator = u32_at(pos)?;
        let denominator = u32_at(pos.checked_add(4)?)?;
        (denominator != 0).then(|| f64::from(numerator) / f64::from(denominator))
    };
    let mut facts = Exif::default();
    let (mut x, mut y) = (None, None);
    // TIFF's default ResolutionUnit is inches.
    let mut unit = 2;
    // A truncated final entry must not discard facts from the complete ones.
    let available = (tiff.len() - directory - 2) / 12;
    for entry in 0..entries.min(512).min(available) {
        let at = directory + 2 + entry * 12;
        if u32_at(at + 4)? != 1 {
            continue;
        }
        match (u16_at(at)?, u16_at(at + 2)?) {
            (0x0112, 3) => {
                facts.orientation = u8::try_from(u16_at(at + 8)?)
                    .ok()
                    .filter(|v| (1..=8).contains(v))
            }
            (0x011a, 5) => x = rational(u32_at(at + 8)?),
            (0x011b, 5) => y = rational(u32_at(at + 8)?),
            (0x0128, 3) => unit = u16_at(at + 8)?,
            _ => {}
        }
    }
    if let (Some(x), Some(y)) = (x, y) {
        facts.dpi = match unit {
            2 => plausible(x, y),
            3 => plausible(x * 2.54, y * 2.54),
            _ => None,
        };
    }
    Some(facts)
}

/// What a JPEG's header says, read without decoding anything.
pub(crate) struct JpegFacts<'a> {
    // APP2 segment numbers run from 1 to 255. Keep slices until the raster is validated.
    profile_parts: Vec<&'a [u8]>,
    width: u32,
    height: u32,
    pub(crate) components: u8,
    pub(crate) orientation: u8,
    pub(crate) dpi: Option<(f64, f64)>,
}

pub(crate) fn jpeg_facts(bytes: &[u8]) -> Result<JpegFacts<'_>, String> {
    let broken = || said!("it is not a complete JPEG image");
    let mut facts = JpegFacts {
        profile_parts: Vec::new(),
        width: 0,
        height: 0,
        components: 0,
        orientation: 1,
        dpi: None,
    };
    let mut exif_dpi = None;
    let mut relative = None;
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
                    0 => {
                        relative = relative_density(x, y);
                        None
                    }
                    1 => plausible(x, y),
                    2 => plausible(x * 2.54, y * 2.54),
                    _ => None,
                };
            }
            0xe1 if payload.starts_with(b"Exif\0\0") => {
                if let Some(found) = exif_properties(&payload[6..]) {
                    if let Some(orientation) = found.orientation {
                        facts.orientation = orientation;
                    }
                    exif_dpi = found.dpi.or(exif_dpi);
                }
            }
            0xe2 if payload.starts_with(b"ICC_PROFILE\0") => {
                let [sequence, count, data @ ..] = &payload[12..] else {
                    return Err(said!("the JPEG colour profile is incomplete"));
                };
                if *sequence == 0 || sequence > count || data.is_empty() {
                    return Err(said!("the JPEG colour profile is incomplete"));
                }
                let parts = &mut facts.profile_parts;
                if parts.is_empty() {
                    parts.resize(usize::from(*count), &[]);
                }
                if parts.len() != usize::from(*count)
                    || !parts[usize::from(*sequence) - 1].is_empty()
                {
                    return Err(said!("the JPEG colour profile is incomplete"));
                }
                parts[usize::from(*sequence) - 1] = data;
            }
            // The frame: baseline, extended or progressive, eight bits a sample.
            0xc0..=0xc2 => {
                if payload.len() < 6 || payload[0] != 8 {
                    return Err(said!("it is a JPEG with more than eight bits a sample"));
                }
                facts.height = u32::from(u16::from_be_bytes([payload[1], payload[2]]));
                facts.width = u32::from(u16::from_be_bytes([payload[3], payload[4]]));
                facts.components = payload[5];
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(said!(
                    "it is a lossless or arithmetic-coded JPEG, which PDF readers do \
                            not display"
                ));
            }
            0xda if facts.components != 0 => {
                facts.dpi = facts.dpi.or(exif_dpi).or(relative);
                return Ok(facts);
            }
            0xda | 0xd9 => return Err(broken()),
            _ => {}
        }
        offset += length;
    }
}

impl JpegFacts<'_> {
    pub(crate) fn profile(&self) -> Result<Option<Vec<u8>>, String> {
        if self.profile_parts.iter().any(|part| part.is_empty()) {
            return Err(said!("the JPEG colour profile is incomplete"));
        }
        Ok((!self.profile_parts.is_empty()).then(|| self.profile_parts.concat()))
    }
}

fn jpeg(bytes: &[u8]) -> Result<Picture, String> {
    let facts = jpeg_facts(bytes)?;
    bounded(facts.width, facts.height)?;
    let gray = match facts.components {
        1 => true,
        3 => false,
        4 => {
            return Err(said!(
                "it is a CMYK JPEG, which tpdf does not place yet; save it as RGB"
            ))
        }
        other => return Err(said!("it is a JPEG with {other} colour components")),
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
    let undecodable = |e: zune_jpeg::errors::DecodeErrors| said!("it does not decode: {e}");
    decoder.decode_headers().map_err(undecodable)?;
    if decoder.output_buffer_size() != Some(wanted) {
        return Err(said!("its header and its picture disagree about its size"));
    }
    let mut decoded = vec![0; wanted];
    decoder.decode_into(&mut decoded).map_err(undecodable)?;
    drop(decoded);
    let profile = facts.profile()?;
    validate_profile(profile.as_deref(), gray, "JPEG")?;
    Ok(Picture {
        width: facts.width,
        height: facts.height,
        gray,
        jpeg: true,
        calibration: None,
        profile,
        intent: None,
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
        .map_err(|e| said!("its pixels could not be compressed: {e}"))
}

fn validate_profile(profile: Option<&[u8]>, gray: bool, format: &str) -> Result<(), String> {
    if profile.is_some_and(|bytes| {
        !crate::encoding::icc_header_matches(bytes, if gray { b"GRAY" } else { b"RGB " })
    }) {
        return Err(said!(
            "the {format} colour profile does not match its samples"
        ));
    }
    Ok(())
}

/// PNG white point, decoding exponent, and linear XYZ columns (RGB only).
pub(crate) struct PngCalibration {
    pub(crate) white: [f64; 3],
    pub(crate) gamma: f32,
    pub(crate) primaries: Option<[[f64; 3]; 3]>,
}

pub(crate) fn png_calibration(
    info: &png::Info<'_>,
    gray: bool,
) -> Result<Option<PngCalibration>, String> {
    if info.icc_profile.is_some()
        || info.srgb.is_some()
        || (info.gama_chunk.is_none() && info.chrm_chunk.is_none())
    {
        return Ok(None);
    }
    let invalid = || said!("the PNG colour calibration is invalid");
    let gamma = info.gama_chunk.map_or(2.2, |g| g.into_value().recip());
    if !gamma.is_finite() {
        return Err(invalid());
    }
    let colours = info.chrm_chunk.unwrap_or_else(|| {
        png::SourceChromaticities::new((0.3127, 0.3290), (0.64, 0.33), (0.30, 0.60), (0.15, 0.06))
    });
    let xyz = |(x, y): (png::ScaledFloat, png::ScaledFloat)| {
        let (x, y) = (f64::from(x.into_scaled()), f64::from(y.into_scaled()));
        if y == 0.0 || x + y > 100_000.0 {
            return Err(invalid());
        }
        Ok([x / y, 1.0, (100_000.0 - x - y) / y])
    };
    let white = xyz(colours.white)?;
    if white[0] <= 0.0 || white[2] <= 0.0 {
        return Err(invalid());
    }
    let primaries = if gray {
        None
    } else {
        let r = xyz(colours.red)?;
        let g = xyz(colours.green)?;
        let b = xyz(colours.blue)?;
        // Solve [R G B] * scales = white. PDF stores the three scaled columns.
        let det = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            a[0] * (b[1] * c[2] - b[2] * c[1]) - b[0] * (a[1] * c[2] - a[2] * c[1])
                + c[0] * (a[1] * b[2] - a[2] * b[1])
        };
        let divisor = det(r, g, b);
        if divisor.abs() < f64::EPSILON {
            return Err(invalid());
        }
        let scaled = |primary: [f64; 3], numerator: f64| {
            let scale = numerator / divisor;
            if scale <= 0.0 {
                return Err(invalid());
            }
            Ok(primary.map(|v| v * scale))
        };
        Some([
            scaled(r, det(white, g, b))?,
            scaled(g, det(r, white, b))?,
            scaled(b, det(r, g, white))?,
        ])
    };
    Ok(Some(PngCalibration {
        gamma,
        white,
        primaries,
    }))
}

impl PngCalibration {
    fn pdf_space(&self) -> Object {
        let array = |v: &[f64]| Object::Array(v.iter().map(|x| Object::Real(*x as f32)).collect());
        let mut params = dictionary! { "WhitePoint" => array(&self.white) };
        let name = if let Some(primaries) = self.primaries {
            params.set("Matrix", array(primaries.as_flattened()));
            params.set("Gamma", vec![Object::Real(self.gamma); 3]);
            "CalRGB"
        } else {
            params.set("Gamma", self.gamma);
            "CalGray"
        };
        Object::Array(vec![name.into(), params.into()])
    }
}

pub(crate) fn png_properties(info: &png::Info<'_>) -> (u8, Option<(f64, f64)>) {
    let (dpi, relative) = info.pixel_dims.map_or((None, None), |dims| {
        let (x, y) = (f64::from(dims.xppu), f64::from(dims.yppu));
        match dims.unit {
            png::Unit::Meter => {
                // pHYs rounds to whole pixels per metre. Recover an integer DPI
                // when it produces exactly the stored value (e.g. 72 -> 2835).
                let dpi = |per_metre: f64| {
                    let measured = per_metre * 0.0254;
                    let whole = measured.round();
                    if (whole / 0.0254).round() == per_metre {
                        whole
                    } else {
                        measured
                    }
                };
                let density = (dpi(x), dpi(y));
                // Fitted renders can be outside the manual DPI range. Physical
                // PNG units are explicit, and placement bounds the page size.
                ((x > 0.0 && y > 0.0).then_some(density), None)
            }
            png::Unit::Unspecified => (None, relative_density(x, y)),
        }
    });
    let exif = info
        .exif_metadata
        .as_deref()
        .and_then(exif_properties)
        .unwrap_or_default();
    let orientation = exif.orientation.unwrap_or(1);
    let dpi = dpi.or(exif.dpi).or(relative);
    (orientation, dpi)
}

fn srgb_profile(gray: bool) -> Vec<u8> {
    let srgb = moxcms::ColorProfile::new_srgb();
    let profile = if gray {
        // Grayscale PNG samples use the same piecewise sRGB transfer curve.
        let mut mono = moxcms::ColorProfile::new_gray_with_gamma(1.0);
        mono.gray_trc = srgb.red_trc;
        mono.description = srgb.description;
        mono
    } else {
        srgb
    };
    profile
        .encode()
        .expect("the built-in sRGB profile can be encoded")
}

fn png(bytes: &[u8]) -> Result<Picture, String> {
    let undecodable = |e: png::DecodingError| said!("it does not decode: {e}");
    let be32 = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    // The size from the header, before the decoder is given any room.
    let (width, height) = match (bytes.get(12..16), be32(16), be32(20)) {
        (Some(b"IHDR"), Some(width), Some(height)) => (width, height),
        _ => return Err(said!("it is not a complete PNG image")),
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
        return Err(said!("its header and its picture disagree about its size"));
    }
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= pixels * 4)
        .ok_or_else(|| said!("it is larger than its header says"))?;
    let mut decoded = vec![0; size];
    let frame = reader.next_frame(&mut decoded).map_err(undecodable)?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(said!("it could not be read as eight-bit samples"));
    }
    let (channels, gray, alpha) = match frame.color_type {
        png::ColorType::Grayscale => (1, true, false),
        png::ColorType::GrayscaleAlpha => (2, true, true),
        png::ColorType::Rgb => (3, false, false),
        png::ColorType::Rgba => (4, false, true),
        png::ColorType::Indexed => return Err(said!("its palette could not be expanded")),
    };
    if size != pixels * channels {
        return Err(said!("it is not the size its header says"));
    }
    // An eXIf chunk can follow the image data.
    reader.finish().map_err(undecodable)?;
    let (orientation, dpi) = png_properties(reader.info());
    let embedded = reader.info().icc_profile.as_deref();
    validate_profile(embedded, gray, "PNG")?;
    let intent = reader.info().srgb.filter(|_| embedded.is_none());
    let profile = embedded
        .map(<[u8]>::to_vec)
        .or_else(|| intent.map(|_| srgb_profile(gray)));
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
        calibration: png_calibration(reader.info(), gray)?.map(|cal| cal.pdf_space()),
        profile,
        intent,
        data: deflated(&data)?,
        mask: mask.as_deref().map(deflated).transpose()?,
        orientation,
        dpi,
    })
}

fn picture(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.starts_with(&PNG) {
        png(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg(bytes)
    } else {
        Err(said!("it is not a PNG or JPEG image"))
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
            // A picture more than 4,800 times as long as it is wide cannot
            // keep its shape inside both bounds: its page keeps them instead,
            // and the picture is fitted onto it below.
            let side = |length: f64| (length * scale).clamp(MIN_PAGE_PT, MAX_PAGE_PT);
            (side(w), side(h))
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

pub(crate) fn check_count(count: usize) -> Result<(), String> {
    if count == 0 {
        return Err(said!("choose at least one picture"));
    }
    if count > MAX_IMAGES {
        return Err(said!(
            "{chosen} pictures were chosen, and tpdf makes a document from at most {MAX_IMAGES} at once",
            chosen = count
        ));
    }
    Ok(())
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
    check_count(images.len())?;
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::with_capacity(images.len());
    for (bytes, label) in images {
        let found = picture(bytes).map_err(|why| said!("{label} cannot be used: {why}"))?;
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
        let device = if found.gray {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        let color_space = if let Some(profile) = found.profile {
            let id = doc.add_object(Stream::new(
                dictionary! { "N" => if found.gray { 1 } else { 3 }, "Alternate" => device },
                profile,
            ));
            Object::Array(vec!["ICCBased".into(), id.into()])
        } else {
            found.calibration.unwrap_or_else(|| Object::from(device))
        };
        let mut image = dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => i64::from(found.width), "Height" => i64::from(found.height),
            "ColorSpace" => color_space,
            "BitsPerComponent" => 8,
            "Filter" => if found.jpeg { "DCTDecode" } else { "FlateDecode" },
        };
        if let Some(mask) = mask {
            image.set("SMask", mask);
        }
        if let Some(intent) = found.intent {
            let name = match intent {
                png::SrgbRenderingIntent::Perceptual => "Perceptual",
                png::SrgbRenderingIntent::RelativeColorimetric => "RelativeColorimetric",
                png::SrgbRenderingIntent::Saturation => "Saturation",
                png::SrgbRenderingIntent::AbsoluteColorimetric => "AbsoluteColorimetric",
            };
            image.set("Intent", name);
        }
        // `Stream::new` would mark it for compression; these bytes are final.
        let mut stream = Stream::new(image, found.data);
        stream.allows_compression = false;
        let image = doc.add_object(stream);
        let [a, b, c, d, e, f] = at.matrix;
        let content = format!("q {a} {b} {c} {d} {e} {f} cm /Im0 Do Q");
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
