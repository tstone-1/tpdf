//! Bounded filters that require a complete encoded stream.

pub(crate) fn flate(input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut decoder = flate2::Decompress::new(true);
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let consumed = decoder.total_in() as usize;
        let produced = decoder.total_out() as usize;
        let status = decoder
            .decompress(
                &input[consumed..],
                &mut buffer,
                flate2::FlushDecompress::None,
            )
            .map_err(|e| said!("invalid Flate content: {e}"))?;
        let count = decoder.total_out() as usize - produced;
        if count > limit - output.len() {
            return Err(said!("incomplete or oversized Flate content"));
        }
        output.extend_from_slice(&buffer[..count]);
        if status == flate2::Status::StreamEnd && decoder.total_in() == input.len() as u64 {
            return Ok(output);
        }
        if status == flate2::Status::StreamEnd
            || (count == 0 && consumed == decoder.total_in() as usize)
        {
            return Err(said!("incomplete or oversized Flate content"));
        }
    }
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

fn append(output: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<(), String> {
    if bytes.len() > limit.saturating_sub(output.len()) {
        return Err(said!("ASCII85 output exceeds its limit"));
    }
    output.extend_from_slice(bytes);
    Ok(())
}

// ISO 32000, 7.4.3, including the corrected EOD/error rules:
// https://pdf-issues.pdfa.org/32000-2-2020/clause07.html#743-ascii85decode-filter
// u64 holds five base-85 digits without arithmetic overflow; conversion to u32
// rejects impossible groups before their bytes can reach the content parser.
pub(crate) fn ascii85(input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut value = 0_u64;
    let mut count = 0;
    for (index, &byte) in input.iter().enumerate() {
        match byte {
            byte if whitespace(byte) => {}
            b'~' => {
                if input.get(index + 1) != Some(&b'>')
                    || !input[index + 2..].iter().all(|&b| whitespace(b))
                    || count == 1
                {
                    return Err(said!("invalid ASCII85 end marker or partial group"));
                }
                if count > 1 {
                    for _ in count..5 {
                        value = value * 85 + 84;
                    }
                    let value =
                        u32::try_from(value).map_err(|_| said!("ASCII85 group out of range"))?;
                    append(&mut output, &value.to_be_bytes()[..count - 1], limit)?;
                }
                return Ok(output);
            }
            b'z' if count == 0 => append(&mut output, &[0; 4], limit)?,
            b'!'..=b'u' => {
                value = value * 85 + u64::from(byte - b'!');
                count += 1;
                if count == 5 {
                    let group =
                        u32::try_from(value).map_err(|_| said!("ASCII85 group out of range"))?;
                    append(&mut output, &group.to_be_bytes(), limit)?;
                    value = 0;
                    count = 0;
                }
            }
            _ => return Err(said!("invalid ASCII85 character or zero group")),
        }
    }
    Err(said!("ASCII85 content is missing its end marker"))
}
