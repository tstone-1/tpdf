//! Validate the JPEG behind an unchanged DCT image, inside the document worker.
//! No decoded pixels or rewritten image bytes enter the saved document.

use std::io::Cursor;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

// A decoder's strict mode still recovers an empty entropy scan. Check marker
// framing separately, without attempting to implement Huffman decoding here.
// This is bounded preservation, not a claim that every JPEG sample is valid.
pub(super) fn framing(input: &[u8]) -> Result<(), String> {
    let invalid = || "incomplete or unsupported JPEG framing".to_string();
    if input.len() > 2 * super::super::MAX_CONTENT || !input.starts_with(&[0xff, 0xd8]) {
        return Err(invalid());
    }
    let mut position = 2;
    let mut scans = 0;
    let mut frame = false;
    while position < input.len() {
        if input[position] != 0xff {
            return Err(invalid());
        }
        while input.get(position) == Some(&0xff) {
            position += 1;
        }
        let marker = *input.get(position).ok_or_else(invalid)?;
        position += 1;
        // NUL padding after EOI (Acrobat's scan recompression writes a few
        // bytes of it) is ignored by every decoder and cannot hold an image.
        if marker == 0xd9 {
            return if scans > 0 && input[position..].iter().all(|&byte| byte == 0) {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        match marker {
            0xc0..=0xc2 if !frame => frame = true,
            0xc4 | 0xdb | 0xdd | 0xe0..=0xef | 0xfe => {}
            0xda if frame => {}
            _ => return Err(invalid()),
        }
        let length = input.get(position..position + 2).ok_or_else(invalid)?;
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        if length < 2 || length > input.len() - position {
            return Err(invalid());
        }
        position += length;
        if marker == 0xda {
            scans += 1;
            if scans > 64 {
                return Err("too many JPEG scans".into());
            }
            let mut samples = false;
            while position < input.len() {
                if input[position] != 0xff {
                    samples = true;
                    position += 1;
                    continue;
                }
                let start = position;
                while input.get(position) == Some(&0xff) {
                    position += 1;
                }
                let next = *input.get(position).ok_or_else(invalid)?;
                match next {
                    0x00 if position == start + 1 => samples = true,
                    0xd0..=0xd7 => {}
                    _ => {
                        position = start;
                        break;
                    }
                }
                position += 1;
            }
            if !samples {
                return Err(invalid());
            }
        }
    }
    Err(invalid())
}

pub(super) fn check(
    input: &[u8],
    width: usize,
    height: usize,
    components: usize,
) -> Result<(), String> {
    let invalid = || "incomplete or unsupported JPEG image".to_string();
    framing(input)?;
    // Framing proved everything after EOI is NUL padding; EOI ends in 0xd9,
    // so this is exactly the JPEG the decoder must consume in full.
    let input = &input[..input
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |at| at + 1)];
    let space = match components {
        1 => ColorSpace::Luma,
        3 => ColorSpace::RGB,
        _ => return Err(invalid()),
    };
    // The caller checks the shared pixel budget before reaching the decoder.
    // JPEG dimensions must agree before any pixel/coefficient buffer allocation.
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(width)
        .set_max_height(height)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(space);
    let mut source = Cursor::new(input);
    let mut decoder = JpegDecoder::new_with_options(&mut source, options);
    let bytes = width * height * components;
    decoder.decode_headers().map_err(|_| invalid())?;
    let info = decoder.info().ok_or_else(invalid)?;
    if usize::from(info.width) != width
        || usize::from(info.height) != height
        || usize::from(info.components) != components
    {
        return Err("JPEG dimensions or components disagree with the image dictionary".into());
    }
    if decoder.output_buffer_size() != Some(bytes) {
        return Err(invalid());
    }
    let mut pixels = vec![0; bytes];
    decoder.decode_into(&mut pixels).map_err(|_| invalid())?;
    drop(decoder);
    // The decoder can stop after one image in a concatenated stream. Reject
    // that case and missing EOI explicitly.
    if source.position() != input.len() as u64 {
        return Err(invalid());
    }
    Ok(())
}
