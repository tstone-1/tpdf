//! Writing a smaller copy of a document.
//!
//! Two things, and the second only when it is asked for by name.
//!
//! **Without loss.** Every stream stored without a filter is deflated, every
//! stream that is only deflated is deflated again as hard as the encoder can
//! and kept when that is smaller, and the copy is written with object streams
//! and a cross-reference stream (`save::serialise_packed`). Nothing a reader sees changes. On 59 documents of
//! 242 MB this saves 2.0%, 2.4% for the median file and up to 47% for one that
//! was stored uncompressed: pictures are 81% of the bytes, and this does not
//! touch them.
//!
//! **Pictures, with loss.** [`Pictures`] is a resolution and a JPEG quality,
//! chosen freely or by a [`Preset`]. A picture drawn at more than that resolution is scaled down to it,
//! and a picture that compresses badly without loss --- a photograph or a
//! scan --- is stored as JPEG. What a picture is drawn at is read from the
//! pages: [`drawn`] walks each page's content, and the content of the reusable
//! blocks it draws, for the matrix in effect at every `Do`.
//!
//! **What is left exactly as it is**, because changing it could change what
//! it shows by more than resolution:
//!
//! - a picture no page draws directly or through a block (one in a pattern or
//!   an annotation's appearance), since nothing says how large it is shown;
//! - a stencil mask, a picture of fewer or more than 8 bits a sample, and one
//!   with a `/Decode` array;
//! - a colour space that is not grey or RGB by one of their plain names or an
//!   ICC profile of one or three components: indexed, CMYK, Lab, separations;
//! - a picture with a `/Mask`, where one changed sample changes what is
//!   transparent, and a soft mask with a `/Matte`;
//! - a filter that is neither `FlateDecode` nor `DCTDecode`, or more than one:
//!   JBIG2 and CCITT are already small, and JPEG 2000 is not decoded here;
//! - a JPEG that needs no scaling, since encoding it again loses quality for
//!   little;
//! - a picture whose new form is not at least a tenth smaller than the old.
//!
//! A soft mask is scaled with the picture it belongs to and is never stored as
//! JPEG: its edges are what make a cut-out look right.
//!
//! Everything here decodes attacker-chosen bytes and runs in a worker. A
//! picture is bounded by [`MAX_PIXELS`] before anything is allocated for it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write as _};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

use crate::pathcut::{concat, Matrix, IDENTITY};
use crate::redact::MAX_CONTENT_BYTES;

/// The most pixels one picture may hold and still be decoded.
pub const MAX_PIXELS: u64 = 40_000_000;

/// How deep blocks drawn inside blocks are followed.
const MAX_NESTING: usize = 8;

/// The most `Do` operations followed in one document, so a file of blocks that
/// each draw ten more cannot hold a worker.
const MAX_DRAWS: usize = 200_000;

/// A picture is scaled only when it is drawn at more than this many times the
/// preset's resolution. Scaling by a few percent blurs and saves nothing.
const SLACK: f64 = 1.2;

/// A picture stored without loss is called photo-like, and stored as JPEG,
/// when deflating its pixels leaves more than this share of them.
const PHOTO_SHARE: f64 = 0.25;

/// How much pictures are shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// For reading on a screen: 110 pixels an inch.
    Screen,
    /// For a screen and an office printer: 150 pixels an inch.
    Balanced,
    /// For printing: 300 pixels an inch.
    Print,
}

impl Preset {
    /// The resolution pictures are scaled down to, in pixels an inch.
    #[must_use]
    pub fn dpi(self) -> f64 {
        match self {
            Preset::Screen => 110.0,
            Preset::Balanced => 150.0,
            Preset::Print => 300.0,
        }
    }

    /// The JPEG quality, 1 to 100.
    #[must_use]
    pub fn quality(self) -> u8 {
        match self {
            Preset::Screen => 60,
            Preset::Balanced => 75,
            Preset::Print => 85,
        }
    }

    /// The preset a word names, as the command line and the window spell it.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        match word {
            "screen" => Some(Preset::Screen),
            "balanced" => Some(Preset::Balanced),
            "print" => Some(Preset::Print),
            _ => None,
        }
    }
}

/// The lowest and the highest resolution pictures may be scaled down to.
pub const DPI_RANGE: std::ops::RangeInclusive<u32> = 20..=1200;

/// How pictures are shrunk: a [`Preset`], or the three numbers behind one
/// chosen freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pictures {
    /// The resolution a picture drawn finer than this is scaled down to, in
    /// pixels an inch.
    pub dpi: u32,
    /// The JPEG quality, 1 to 100.
    pub quality: u8,
    /// Whether a picture stored without loss may be stored as JPEG when it is
    /// photo-like. Without it such a picture is scaled and stays lossless; a
    /// picture that is a JPEG already stays one either way.
    pub jpeg: bool,
}

impl From<Preset> for Pictures {
    fn from(preset: Preset) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self {
            dpi: preset.dpi() as u32,
            quality: preset.quality(),
            jpeg: true,
        }
    }
}

impl Pictures {
    /// These settings, when a writer can act on them.
    ///
    /// # Errors
    ///
    /// The sentence for the reader: the resolution or the quality is outside
    /// what is offered.
    pub fn checked(self) -> Result<Self, String> {
        if !DPI_RANGE.contains(&self.dpi) {
            return Err(format!(
                "the resolution is {} pixels an inch; it may be {} to {}",
                self.dpi,
                DPI_RANGE.start(),
                DPI_RANGE.end()
            ));
        }
        if !(1..=100).contains(&self.quality) {
            return Err(format!(
                "the JPEG quality is {}; it may be 1 to 100",
                self.quality
            ));
        }
        Ok(self)
    }

    /// The preset these are, when they are one.
    #[must_use]
    pub fn preset(self) -> Option<Preset> {
        [Preset::Screen, Preset::Balanced, Preset::Print]
            .into_iter()
            .find(|preset| Self::from(*preset) == self)
    }
}

/// What a rewrite does about the copy's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Compress {
    /// Nothing: the copy is written as every other copy is.
    #[default]
    No,
    /// Smaller without changing anything a reader sees.
    Lossless,
    /// That, and pictures shrunk.
    Pictures(Pictures),
}

/// What [`apply`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Done {
    /// Pictures a page draws, counted once each.
    pub pictures: usize,
    /// Of those, the ones stored smaller.
    pub pictures_changed: usize,
    /// Streams that were stored without a filter and are now deflated.
    pub streams_deflated: usize,
}

/// The largest size, in points, any page draws a picture at.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Shown {
    width: f64,
    height: f64,
    /// The first page that draws it, counted from 0.
    page: u32,
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn matrix_of(operands: &[Object]) -> Option<Matrix> {
    let mut matrix = [0.0; 6];
    if operands.len() != 6 {
        return None;
    }
    for (slot, operand) in matrix.iter_mut().zip(operands) {
        *slot = number(operand)?;
    }
    Some(matrix)
}

fn resolved<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

/// The XObject a resource dictionary names `name`, as the id it is stored
/// under. One written inline has no id and is not followed.
fn x_object(doc: &Document, resources: &Dictionary, name: &[u8]) -> Option<ObjectId> {
    let all = resolved(doc, resources.get(b"XObject").ok()?)?
        .as_dict()
        .ok()?;
    all.get(name).ok()?.as_reference().ok()
}

struct Walk<'a> {
    doc: &'a Document,
    shown: BTreeMap<ObjectId, Shown>,
    draws: usize,
    /// The page being walked, counted from 0.
    page: u32,
}

impl Walk<'_> {
    /// Follows one content stream. `resources` is searched nearest first.
    fn content(
        &mut self,
        data: &[u8],
        resources: &[&Dictionary],
        base: Option<Matrix>,
        depth: usize,
    ) {
        let Ok(content) = Content::decode(data) else {
            return;
        };
        let mut ctm = base;
        let mut stack: Vec<Option<Matrix>> = Vec::new();
        for operation in &content.operations {
            match operation.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => {
                    if let Some(saved) = stack.pop() {
                        ctm = saved;
                    }
                }
                "cm" => {
                    ctm = match (matrix_of(&operation.operands), ctm) {
                        (Some(matrix), Some(ctm)) => Some(concat(matrix, ctm)),
                        _ => None,
                    };
                }
                "Do" => {
                    self.draws += 1;
                    if self.draws > MAX_DRAWS {
                        return;
                    }
                    let Some(name) = operation.operands.first().and_then(|n| n.as_name().ok())
                    else {
                        continue;
                    };
                    let Some(id) = resources
                        .iter()
                        .find_map(|resources| x_object(self.doc, resources, name))
                    else {
                        continue;
                    };
                    self.draw(id, ctm, resources, depth);
                }
                _ => {}
            }
        }
    }

    fn draw(&mut self, id: ObjectId, ctm: Option<Matrix>, resources: &[&Dictionary], depth: usize) {
        let Ok(stream) = self.doc.get_object(id).and_then(Object::as_stream) else {
            return;
        };
        match stream.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Image") => {
                let page = self.page;
                let entry = self.shown.entry(id).or_insert(Shown {
                    page,
                    ..Shown::default()
                });
                match ctm {
                    // The unit square under the matrix: its two sides.
                    Some([a, b, c, d, ..]) => {
                        entry.width = entry.width.max(a.hypot(b));
                        entry.height = entry.height.max(c.hypot(d));
                    }
                    // Drawn under a matrix that would not read: its size is
                    // not known, so it is treated as drawn very large and left.
                    None => {
                        entry.width = f64::INFINITY;
                        entry.height = f64::INFINITY;
                    }
                }
            }
            Ok(b"Form") if depth < MAX_NESTING => {
                let inner = stream
                    .dict
                    .get(b"Matrix")
                    .ok()
                    .and_then(|matrix| resolved(self.doc, matrix))
                    .and_then(|matrix| matrix.as_array().ok())
                    .map_or(Some(IDENTITY), |matrix| matrix_of(matrix));
                // An unknown matrix is followed all the same, so the block's
                // pictures are found and left: each records an unknown size.
                let base = match (inner, ctm) {
                    (Some(inner), Some(ctm)) => Some(concat(inner, ctm)),
                    _ => None,
                };
                let own = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|own| resolved(self.doc, own))
                    .and_then(|own| own.as_dict().ok());
                let Ok(data) = stream.decompressed_content_with_limit(MAX_CONTENT_BYTES) else {
                    return;
                };
                let mut nearest: Vec<&Dictionary> = own.into_iter().collect();
                nearest.extend(resources);
                self.content(&data, &nearest, base, depth + 1);
            }
            _ => {}
        }
    }
}

/// The largest size each picture is drawn at, by the pages and the blocks
/// they draw.
fn drawn(doc: &Document) -> BTreeMap<ObjectId, Shown> {
    let mut walk = Walk {
        doc,
        shown: BTreeMap::new(),
        draws: 0,
        page: 0,
    };
    for (index, page) in doc.get_pages().into_values().enumerate() {
        walk.page = u32::try_from(index).unwrap_or(u32::MAX);
        let Ok(data) = doc.get_page_content_with_limit(page, MAX_CONTENT_BYTES) else {
            continue;
        };
        let Ok((inline, inherited)) = doc.get_page_resources(page) else {
            continue;
        };
        let mut resources: Vec<&Dictionary> = inline.into_iter().collect();
        resources.extend(
            inherited
                .into_iter()
                .filter_map(|id| doc.get_dictionary(id).ok()),
        );
        walk.content(&data, &resources, Some(IDENTITY), 0);
    }
    walk.shown
}

/// How a picture's samples are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stored {
    Raw,
    Flate,
    Jpeg,
}

/// A picture this can shrink: its size, its samples and how they were stored.
struct Picture {
    width: usize,
    height: usize,
    /// 1 for grey, 3 for RGB.
    channels: usize,
    stored: Stored,
    /// The id of its soft mask, when it has one this can scale with it.
    mask: Option<ObjectId>,
}

/// How many components a colour space has, when it is one this handles.
fn channels(doc: &Document, space: &Object) -> Option<usize> {
    match resolved(doc, space)? {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"CalGray" => Some(1),
            b"DeviceRGB" | b"CalRGB" => Some(3),
            _ => None,
        },
        Object::Array(array) => {
            let family = array.first()?.as_name().ok()?;
            match family {
                b"CalGray" => Some(1),
                b"CalRGB" => Some(3),
                b"ICCBased" => {
                    let profile = resolved(doc, array.get(1)?)?.as_stream().ok()?;
                    match profile.dict.get(b"N").and_then(Object::as_i64) {
                        Ok(1) => Some(1),
                        Ok(3) => Some(3),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The one filter a stream names, or `Raw` for none; `None` for any other.
fn stored(doc: &Document, dict: &Dictionary) -> Option<Stored> {
    let Ok(filter) = dict.get(b"Filter") else {
        return Some(Stored::Raw);
    };
    let name = match resolved(doc, filter)? {
        Object::Name(name) => name.as_slice(),
        Object::Array(array) if array.len() == 1 => resolved(doc, &array[0])?.as_name().ok()?,
        Object::Array(array) if array.is_empty() => return Some(Stored::Raw),
        _ => return None,
    };
    match name {
        b"FlateDecode" => Some(Stored::Flate),
        b"DCTDecode" => Some(Stored::Jpeg),
        _ => None,
    }
}

/// Whether `lopdf` will undo the predictor a deflated stream was stored with.
///
/// It reads `/DecodeParms` only as a dictionary written in the stream's own
/// dictionary, and each number in it only as a number written there. An array
/// of one dictionary and a reference are both allowed by the format, and for
/// either it inflates the stream and hands back samples that are still
/// differences. For a TIFF predictor those are exactly as many bytes as the
/// picture has samples, so [`samples`] cannot tell by counting them, and a
/// picture scaled from them is a different picture stored as this one.
///
/// No parameters at all is no predictor, and is read.
fn parms_are_read(dict: &Dictionary) -> bool {
    /// The numbers a predictor is undone by.
    const NUMBERS: [&[u8]; 4] = [b"Predictor", b"Colors", b"Columns", b"BitsPerComponent"];
    match dict.get(b"DecodeParms") {
        Err(_) => true,
        Ok(Object::Dictionary(parms)) => NUMBERS
            .iter()
            .all(|key| matches!(parms.get(key), Err(_) | Ok(Object::Integer(_)))),
        Ok(_) => false,
    }
}

/// What a picture is, when it is one this can shrink.
///
/// `as_mask` reads a soft mask, which is always grey and may not carry a
/// `/Matte`: its picture's colours were blended against that already.
fn picture(doc: &Document, id: ObjectId, as_mask: bool) -> Option<Picture> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let dict = &stream.dict;
    if dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Image" {
        return None;
    }
    let flag = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|value| resolved(doc, value))
            .and_then(|value| value.as_bool().ok())
            .unwrap_or(false)
    };
    let whole = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|value| resolved(doc, value))
            .and_then(|value| value.as_i64().ok())
    };
    if flag(b"ImageMask") || whole(b"BitsPerComponent") != Some(8) {
        return None;
    }
    if dict.has(b"Decode") || dict.has(b"Mask") || dict.has(b"Matte") {
        return None;
    }
    let width = usize::try_from(whole(b"Width")?).ok().filter(|w| *w > 0)?;
    let height = usize::try_from(whole(b"Height")?).ok().filter(|h| *h > 0)?;
    if (width as u64).checked_mul(height as u64)? > MAX_PIXELS {
        return None;
    }
    let channels = channels(doc, dict.get(b"ColorSpace").ok()?)?;
    if as_mask && channels != 1 {
        return None;
    }
    let stored = stored(doc, dict)?;
    if stored == Stored::Flate && !parms_are_read(dict) {
        return None;
    }
    let mask = match dict.get(b"SMask") {
        Err(_) => None,
        Ok(mask) => {
            // A mask this cannot scale keeps its picture as it is too.
            let mask = mask.as_reference().ok()?;
            if as_mask {
                return None;
            }
            picture(doc, mask, true)?;
            Some(mask)
        }
    };
    Some(Picture {
        width,
        height,
        channels,
        stored,
        mask,
    })
}

/// A picture's samples, one byte each, rows top to bottom.
fn samples(doc: &Document, id: ObjectId, picture: &Picture) -> Option<Vec<u8>> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let wanted = picture.width * picture.height * picture.channels;
    match picture.stored {
        Stored::Raw | Stored::Flate => {
            let data = stream.decompressed_content_with_limit(wanted).ok()?;
            (data.len() == wanted).then_some(data)
        }
        Stored::Jpeg => {
            let colors = if picture.channels == 1 {
                ColorSpace::Luma
            } else {
                ColorSpace::RGB
            };
            let options = DecoderOptions::default()
                .set_strict_mode(true)
                .set_max_width(picture.width)
                .set_max_height(picture.height)
                .jpeg_set_max_scans(64)
                .jpeg_set_out_colorspace(colors);
            let mut source = Cursor::new(stream.content.as_slice());
            let mut decoder = JpegDecoder::new_with_options(&mut source, options);
            decoder.decode_headers().ok()?;
            let (width, height) = decoder.dimensions()?;
            if (width, height) != (picture.width, picture.height) {
                return None;
            }
            // A JPEG of four components under a three-component colour space
            // is not this picture, whatever the decoder would make of it.
            let coded = decoder.input_colorspace()?.num_components();
            if coded != picture.channels || decoder.output_buffer_size() != Some(wanted) {
                return None;
            }
            let mut decoded = vec![0; wanted];
            decoder.decode_into(&mut decoded).ok()?;
            Some(decoded)
        }
    }
}

/// `data` scaled to `to` by averaging the area each new pixel covers.
fn scaled(data: &[u8], from: (usize, usize), to: (usize, usize), channels: usize) -> Vec<u8> {
    // One axis at a time. Each new sample is the mean of the old samples its
    // span covers, an old sample on a boundary counted by the part inside.
    fn axis(
        data: &[f32],
        lines: usize,
        from: usize,
        to: usize,
        channels: usize,
        stride: usize,
        step: usize,
    ) -> Vec<f32> {
        let ratio = from as f64 / to as f64;
        let mut out = vec![0.0f32; lines * to * channels];
        for at in 0..to {
            let (start, end) = (at as f64 * ratio, (at + 1) as f64 * ratio);
            let (first, last) = (start.floor() as usize, (end.ceil() as usize).min(from));
            for line in 0..lines {
                for channel in 0..channels {
                    let mut sum = 0.0f64;
                    for old in first..last {
                        let covered = (end.min((old + 1) as f64) - start.max(old as f64)).max(0.0);
                        sum += covered * f64::from(data[line * stride + old * step + channel]);
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        out[line * to * channels + at * channels + channel] =
                            (sum / (end - start)) as f32;
                    }
                }
            }
        }
        out
    }
    let source: Vec<f32> = data.iter().map(|sample| f32::from(*sample)).collect();
    // Across: every row from `from.0` to `to.0` samples.
    let across = axis(
        &source,
        from.1,
        from.0,
        to.0,
        channels,
        from.0 * channels,
        channels,
    );
    // Down: turn it, so a column is a line, scale, and turn it back.
    let mut turned = vec![0.0f32; across.len()];
    for y in 0..from.1 {
        for x in 0..to.0 {
            for c in 0..channels {
                turned[(x * from.1 + y) * channels + c] = across[(y * to.0 + x) * channels + c];
            }
        }
    }
    let down = axis(
        &turned,
        to.0,
        from.1,
        to.1,
        channels,
        from.1 * channels,
        channels,
    );
    let mut out = vec![0u8; to.0 * to.1 * channels];
    for y in 0..to.1 {
        for x in 0..to.0 {
            for c in 0..channels {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    out[(y * to.0 + x) * channels + c] = down[(x * to.1 + y) * channels + c]
                        .round()
                        .clamp(0.0, 255.0)
                        as u8;
                }
            }
        }
    }
    out
}

/// The length a side of `pixels` becomes when it is shown over `points` and
/// may be no finer than `dpi`.
fn side(pixels: usize, points: f64, dpi: f64) -> usize {
    if points <= 0.0 || !points.is_finite() {
        return pixels;
    }
    let at = pixels as f64 * 72.0 / points;
    if at <= dpi * SLACK {
        return pixels;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let wanted = (points / 72.0 * dpi).ceil() as usize;
    wanted.clamp(1, pixels)
}

/// Rows filtered as PNG's *Up*, each behind its filter byte, and deflated.
///
/// *Up* stores each sample as its difference from the one above, which is
/// what makes flat areas and vertical edges deflate well.
fn deflated_rows(data: &[u8], row: usize) -> Option<Vec<u8>> {
    let mut filtered = Vec::with_capacity(data.len() + data.len() / row.max(1));
    let mut above: Option<&[u8]> = None;
    for line in data.chunks(row) {
        filtered.push(2);
        match above {
            Some(above) => filtered.extend(
                line.iter()
                    .zip(above)
                    .map(|(sample, above)| sample.wrapping_sub(*above)),
            ),
            None => filtered.extend_from_slice(line),
        }
        above = Some(line);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&filtered).ok()?;
    encoder.finish().ok()
}

fn jpeg(data: &[u8], width: usize, height: usize, channels: usize, quality: u8) -> Option<Vec<u8>> {
    let color = if channels == 1 {
        image::ExtendedColorType::L8
    } else {
        image::ExtendedColorType::Rgb8
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            data,
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
            color,
        )
        .ok()?;
    Some(out)
}

/// One picture stored smaller, or `false` when it is left as it is.
fn shrink(
    doc: &mut Document,
    id: ObjectId,
    picture: &Picture,
    shown: Shown,
    how: Pictures,
    is_mask: bool,
) -> bool {
    let dpi = f64::from(how.dpi);
    let to = (
        side(picture.width, shown.width, dpi),
        side(picture.height, shown.height, dpi),
    );
    let from = (picture.width, picture.height);
    let scaling = to != from;
    if !scaling && picture.stored == Stored::Jpeg {
        return false;
    }
    let Some(original) = samples(doc, id, picture) else {
        return false;
    };
    let pixels = if scaling {
        scaled(&original, from, to, picture.channels)
    } else {
        original
    };
    let Some(lossless) = deflated_rows(&pixels, to.0 * picture.channels) else {
        return false;
    };
    #[allow(clippy::cast_precision_loss)]
    let photo = picture.stored == Stored::Jpeg
        || (how.jpeg && lossless.len() as f64 > pixels.len() as f64 * PHOTO_SHARE);
    let lossy = (photo && !is_mask)
        .then(|| jpeg(&pixels, to.0, to.1, picture.channels, how.quality))
        .flatten()
        .filter(|lossy| lossy.len() < lossless.len());

    let Ok(stream) = doc.get_object_mut(id).and_then(Object::as_stream_mut) else {
        return false;
    };
    let new_len = lossy.as_ref().map_or(lossless.len(), Vec::len);
    if new_len * 10 > stream.content.len() * 9 {
        return false;
    }
    stream.dict.set("Width", to.0 as i64);
    stream.dict.set("Height", to.1 as i64);
    match lossy {
        Some(lossy) => {
            stream.dict.set("Filter", "DCTDecode");
            stream.dict.remove(b"DecodeParms");
            stream.set_content(lossy);
        }
        None => {
            let mut parms = Dictionary::new();
            parms.set("Predictor", 15);
            parms.set("Colors", picture.channels as i64);
            parms.set("BitsPerComponent", 8);
            parms.set("Columns", to.0 as i64);
            stream.dict.set("Filter", "FlateDecode");
            stream.dict.set("DecodeParms", parms);
            stream.set_content(lossless);
        }
    }
    true
}

/// Shrinks the pictures the pages draw. Returns how many there are and how
/// many were stored smaller.
fn pictures(doc: &mut Document, how: Pictures) -> (usize, usize) {
    let shown = drawn(doc);
    let mut changed = 0usize;
    // A mask is scaled once, for the largest any of its pictures is shown.
    let mut masks: BTreeMap<ObjectId, Shown> = BTreeMap::new();
    let mut done: BTreeSet<ObjectId> = BTreeSet::new();
    for (id, shown) in &shown {
        let Some(found) = picture(doc, *id, false) else {
            continue;
        };
        if let Some(mask) = found.mask {
            let entry = masks.entry(mask).or_default();
            entry.width = entry.width.max(shown.width);
            entry.height = entry.height.max(shown.height);
        }
        if shrink(doc, *id, &found, *shown, how, false) {
            changed += 1;
        }
        done.insert(*id);
    }
    for (id, shown) in masks {
        // One a page also draws as a picture was handled above.
        if done.contains(&id) {
            continue;
        }
        if let Some(found) = picture(doc, id, true) {
            shrink(doc, id, &found, shown, how, true);
        }
    }
    (shown.len(), changed)
}

/// Deflates every stream stored without a filter. Returns how many.
fn deflate_streams(doc: &mut Document) -> usize {
    let mut deflated = 0usize;
    for object in doc.objects.values_mut() {
        let Object::Stream(stream) = object else {
            continue;
        };
        // An XMP packet is left readable: tools that never parse the file
        // look for it in the bytes, and PDF/A asks for that.
        let metadata = stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Metadata");
        if metadata || !stream.allows_compression {
            continue;
        }
        if stream.dict.has(b"Filter") {
            redeflate(stream);
        } else if stream.compress().is_ok() && stream.dict.has(b"Filter") {
            deflated += 1;
        }
    }
    deflated
}

/// Deflates again, as hard as the encoder can, a stream whose one filter is
/// `FlateDecode`, and keeps the result when it is smaller.
///
/// The bytes under the filter are not looked at, so a predictor in
/// `/DecodeParms` stays true of them.
fn redeflate(stream: &mut lopdf::Stream) {
    use std::io::Read as _;
    let one = match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => name == b"FlateDecode",
        Ok(Object::Array(array)) => {
            array.len() == 1 && array[0].as_name().ok() == Some(b"FlateDecode")
        }
        _ => false,
    };
    if !one {
        return;
    }
    let mut inflated = Vec::new();
    let limit = MAX_CONTENT_BYTES as u64 + 1;
    let read = flate2::read::ZlibDecoder::new(stream.content.as_slice())
        .take(limit)
        .read_to_end(&mut inflated);
    if read.is_err() || inflated.len() as u64 >= limit {
        return;
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    if encoder.write_all(&inflated).is_err() {
        return;
    }
    if let Ok(packed) = encoder.finish() {
        if packed.len() < stream.content.len() {
            stream.set_content(packed);
        }
    }
}

/// Makes `doc` smaller as `compress` says, before it is encrypted and written.
#[must_use]
pub fn apply(doc: &mut Document, compress: Compress) -> Done {
    let mut done = Done::default();
    match compress {
        Compress::No => return done,
        Compress::Lossless => {}
        Compress::Pictures(how) => {
            (done.pictures, done.pictures_changed) = pictures(doc, how);
        }
    }
    done.streams_deflated = deflate_streams(doc);
    done
}

/// The longest side of a [`Sample`], in pixels.
pub const SAMPLE_SIDE: u32 = 320;

/// How far a [`Sample`] is enlarged against the page at its full size.
pub const SAMPLE_ZOOM: f32 = 2.0;

/// One part of one page before and after, drawn the same size, so the two can
/// be laid side by side and compared.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Sample {
    /// The width of both, in pixels.
    pub width: u32,
    /// The height of both.
    pub height: u32,
    /// The part as the document draws it: RGB, rows top to bottom.
    pub before: Vec<u8>,
    /// The same part as the smaller copy draws it.
    pub after: Vec<u8>,
    /// The page it is from, counted from 1.
    pub page: u32,
    /// How far both are enlarged, in percent of the page's full size.
    pub zoom_percent: u32,
    /// The resolution that page shows its most reduced picture at before, in
    /// pixels an inch.
    pub dpi_before: u32,
    /// And after.
    pub dpi_after: u32,
}

/// The white gap between the two halves of [`Sample::side_by_side`].
const GAP: usize = 8;

fn rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
        .collect()
}

impl Sample {
    /// The part before, as RGBA.
    #[must_use]
    pub fn before_rgba(&self) -> Vec<u8> {
        rgba(&self.before)
    }

    /// The part after, as RGBA.
    #[must_use]
    pub fn after_rgba(&self) -> Vec<u8> {
        rgba(&self.after)
    }

    /// Before on the left and after on the right with a white gap between, as
    /// a width, a height and RGBA.
    #[must_use]
    pub fn side_by_side(&self) -> (u32, u32, Vec<u8>) {
        let (width, height) = (self.width as usize, self.height as usize);
        let across = width * 2 + GAP;
        let mut out = vec![255u8; across * height * 4];
        for (half, pixels) in [(0, &self.before), (width + GAP, &self.after)] {
            for (row, line) in pixels
                .chunks_exact(width.max(1) * 3)
                .enumerate()
                .take(height)
            {
                let start = (row * across + half) * 4;
                out[start..start + width * 4].copy_from_slice(&rgba(line));
            }
        }
        (u32::try_from(across).unwrap_or(u32::MAX), self.height, out)
    }

    /// The part of two drawings of one page that differs most, cut from both.
    ///
    /// `before` and `after` are RGBA of `width` by `height`. The window is at
    /// most [`SAMPLE_SIDE`] a side and is tried every half window; the one
    /// with the largest summed difference wins, the first on a tie. `None`
    /// when the drawings are not that size or do not differ at all.
    #[must_use]
    pub fn of_page(
        before: &[u8],
        after: &[u8],
        width: u32,
        height: u32,
        focus: Focus,
        zoom_percent: u32,
    ) -> Option<Self> {
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 || before.len() != w * h * 4 || after.len() != before.len() {
            return None;
        }
        let window = (w.min(SAMPLE_SIDE as usize), h.min(SAMPLE_SIDE as usize));
        let places = |length: usize, window: usize| -> Vec<usize> {
            let room = length - window;
            let step = (window / 2).max(1);
            let mut at: Vec<usize> = (0..room).step_by(step).collect();
            at.push(room);
            at
        };
        // How much each row of a column band differs is summed once per row.
        let mut best: Option<(u64, usize, usize)> = None;
        for top in places(h, window.1) {
            for left in places(w, window.0) {
                let mut sum = 0u64;
                // Every other pixel is enough to rank windows.
                for y in (top..top + window.1).step_by(2) {
                    let row = (y * w + left) * 4;
                    for x in (0..window.0 * 4).step_by(8) {
                        for c in 0..3 {
                            let at = row + x + c;
                            sum += u64::from(before[at].abs_diff(after[at]));
                        }
                    }
                }
                if best.is_none_or(|(most, ..)| sum > most) {
                    best = Some((sum, left, top));
                }
            }
        }
        let (most, left, top) = best?;
        if most == 0 {
            return None;
        }
        let cut = |data: &[u8]| -> Vec<u8> {
            let mut out = Vec::with_capacity(window.0 * window.1 * 3);
            for y in top..top + window.1 {
                let start = (y * w + left) * 4;
                for pixel in data[start..start + window.0 * 4].chunks_exact(4) {
                    out.extend_from_slice(&pixel[..3]);
                }
            }
            out
        };
        Some(Self {
            width: u32::try_from(window.0).ok()?,
            height: u32::try_from(window.1).ok()?,
            before: cut(before),
            after: cut(after),
            page: focus.page + 1,
            zoom_percent,
            dpi_before: focus.dpi_before,
            dpi_after: focus.dpi_after,
        })
    }
}

/// Where to look for what a smaller copy changed: the page that draws the
/// picture which lost the most pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    /// The page, counted from 0.
    pub page: u32,
    /// The resolution that page shows the picture at before, in pixels an inch.
    pub dpi_before: u32,
    /// And after.
    pub dpi_after: u32,
}

/// What a smaller copy would come to, without writing one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Estimate {
    /// The copy's size in bytes. For an encrypted document it is the size
    /// before encryption, which pads each stream by a few bytes.
    pub bytes_after: u64,
    /// What would be done to the pictures and streams.
    pub done: Done,
    /// One part of one page before and after. `None` when no picture changes
    /// or the two drawings do not differ; filled by the caller that can draw
    /// a page (`render::run_shrink`), since this module cannot.
    pub sample: Option<Sample>,
}

/// The picture `after` lost the most pixels of, and the page that draws it.
fn focus(before: &Document, after: &Document, shown: &BTreeMap<ObjectId, Shown>) -> Option<Focus> {
    let size = |doc: &Document, id: ObjectId| -> Option<(i64, i64)> {
        let dict = &doc.get_object(id).ok()?.as_stream().ok()?.dict;
        Some((
            dict.get(b"Width").and_then(Object::as_i64).ok()?,
            dict.get(b"Height").and_then(Object::as_i64).ok()?,
        ))
    };
    fn content(doc: &Document, id: ObjectId) -> Option<&[u8]> {
        doc.get_object(id)
            .ok()
            .and_then(|object| object.as_stream().ok())
            .map(|stream| stream.content.as_slice())
    }
    let (id, shown, old, new) = shown
        .iter()
        .filter(|(id, _)| content(before, **id) != content(after, **id))
        .filter_map(|(id, shown)| Some((*id, *shown, size(before, *id)?, size(after, *id)?)))
        // The most pixels lost; among pictures only re-encoded, the largest.
        .max_by_key(|(_, _, old, new)| (old.0 * old.1 - new.0 * new.1, old.0 * old.1))?;
    let _ = id;
    let dpi = |pixels: i64| -> u32 {
        if shown.width <= 0.0 || !shown.width.is_finite() {
            return 0;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            (pixels as f64 * 72.0 / shown.width).round() as u32
        }
    };
    Some(Focus {
        page: shown.page,
        dpi_before: dpi(old.0),
        dpi_after: dpi(new.0),
    })
}

/// A smaller copy as [`estimate`] made it: what it comes to, its bytes, and
/// where to look for the difference.
pub struct Estimated {
    /// The size and the counts. Its `sample` is `None`.
    pub estimate: Estimate,
    /// The copy, serialised and not encrypted.
    pub bytes: Vec<u8>,
    /// The page to compare, when a picture changed.
    pub focus: Option<Focus>,
}

/// What a copy of `doc` made smaller as `compress` says would come to.
///
/// The work is done on a copy of the document in memory and serialised as the
/// writer serialises it; nothing is written.
///
/// # Errors
///
/// The copy would not serialise, in `save::serialise`'s words.
pub fn estimate(doc: &Document, compress: Compress) -> Result<Estimated, String> {
    let mut copy = doc.clone();
    let shown = match compress {
        Compress::Pictures(_) => drawn(doc),
        _ => BTreeMap::new(),
    };
    let done = apply(&mut copy, compress);
    let focus = focus(doc, &copy, &shown);
    let bytes = crate::save::serialise_packed(&mut copy, "the smaller copy")?;
    Ok(Estimated {
        estimate: Estimate {
            bytes_after: bytes.len() as u64,
            done,
            sample: None,
        },
        bytes,
        focus,
    })
}

#[cfg(test)]
mod tests;
