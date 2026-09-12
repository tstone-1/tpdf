//! Complete, bounded content decoding for the editor. General PDF recovery is
//! unsuitable here: accepting a decoded prefix can discard the rest on save.

use lopdf::{Object, Stream};

// Bounds scanning as well as allocation, including ASCII85 whitespace. This
// leaves room for the encoding overhead around a MAX_CONTENT-sized stream.
const MAX_ENCODED: usize = 2 * super::MAX_CONTENT;

pub(super) fn decode(stream: &Stream, limit: usize) -> Result<Vec<u8>, String> {
    if stream.content.len() > MAX_ENCODED {
        return Err("encoded page content exceeds its limit".into());
    }
    if stream.dict.has(b"F") || stream.dict.has(b"DecodeParms") {
        return Err("external or parameterised content streams are not editable yet".into());
    }
    let filters = match stream.dict.get(b"Filter") {
        Err(_) => &[][..],
        Ok(Object::Array(values)) if !values.is_empty() && values.len() <= 2 => values,
        Ok(value @ Object::Name(_)) => std::slice::from_ref(value),
        _ => return Err("unsupported content stream filter".into()),
    };
    match filters {
        [] if stream.content.len() <= limit => Ok(stream.content.clone()),
        [] => Err("page content exceeds its limit".into()),
        [Object::Name(name)] if name == b"FlateDecode" => flate(&stream.content, limit),
        [Object::Name(name)] if name == b"ASCII85Decode" => ascii85(&stream.content, limit),
        [Object::Name(first), Object::Name(second)]
            if first == b"ASCII85Decode" && second == b"FlateDecode" =>
        {
            // The intermediate compressed bytes have their own bound; the
            // remaining page budget is enforced on the final Flate output.
            flate(&ascii85(&stream.content, super::MAX_CONTENT)?, limit)
        }
        _ => Err("unsupported content stream filter".into()),
    }
}

fn flate(input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut decoder = flate2::Decompress::new(true);
    let mut output = vec![0; limit + 1];
    let status = decoder
        .decompress(input, &mut output, flate2::FlushDecompress::Finish)
        .map_err(|e| format!("invalid Flate content: {e}"))?;
    if status != flate2::Status::StreamEnd
        || decoder.total_in() != input.len() as u64
        || decoder.total_out() > limit as u64
    {
        return Err("incomplete or oversized Flate content".into());
    }
    output.truncate(decoder.total_out() as usize);
    Ok(output)
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

fn append(output: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<(), String> {
    if bytes.len() > limit.saturating_sub(output.len()) {
        return Err("ASCII85 output exceeds its limit".into());
    }
    output.extend_from_slice(bytes);
    Ok(())
}

// ISO 32000, 7.4.3, including the corrected EOD/error rules:
// https://pdf-issues.pdfa.org/32000-2-2020/clause07.html#743-ascii85decode-filter
// u64 holds five base-85 digits without arithmetic overflow; conversion to u32
// rejects impossible groups before their bytes can reach the content parser.
fn ascii85(input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
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
                    return Err("invalid ASCII85 end marker or partial group".into());
                }
                if count > 1 {
                    for _ in count..5 {
                        value = value * 85 + 84;
                    }
                    let value = u32::try_from(value).map_err(|_| "ASCII85 group out of range")?;
                    append(&mut output, &value.to_be_bytes()[..count - 1], limit)?;
                }
                return Ok(output);
            }
            b'z' if count == 0 => append(&mut output, &[0; 4], limit)?,
            b'!'..=b'u' => {
                value = value * 85 + u64::from(byte - b'!');
                count += 1;
                if count == 5 {
                    let group = u32::try_from(value).map_err(|_| "ASCII85 group out of range")?;
                    append(&mut output, &group.to_be_bytes(), limit)?;
                    value = 0;
                    count = 0;
                }
            }
            _ => return Err("invalid ASCII85 character or zero group".into()),
        }
    }
    Err("ASCII85 content is missing its end marker".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn wrapped(input: &[u8]) -> Stream {
        Stream::new(
            dictionary! { "Filter" => vec![Object::Name(b"ASCII85Decode".to_vec()), Object::Name(b"FlateDecode".to_vec())] },
            input.to_vec(),
        )
    }

    #[test]
    fn textedit_ascii85_flate_pipeline_requires_both_complete_stages() {
        // Independently produced with Python zlib.compress + base64.a85encode.
        let raw = b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET";
        let good = br#"Garg^;:'MC<%p.,#Y@rK0i0R20Mi$H;:#T/oF(2.%InO@E:XJF%ae5]a\m'e!<Dp#$T\~>"#;
        assert_eq!(decode(&wrapped(good), raw.len()).unwrap(), raw);
        assert!(decode(&wrapped(good), raw.len() - 1).is_err());
        assert!(decode(&wrapped(&good[..good.len() - 2]), raw.len())
            .unwrap_err()
            .contains("end marker"));
        for incomplete in [
            br#"Garg^;:'MC<%p.,#Y@rK0i0R20Mi$H;:#T/oF(2.%InO@E:XJF%ae5]a\m'e!<Dp#$N~>"#.as_slice(),
            br#"Garg^;:'MC<%p.,#Y@rK0i0R20Mi$H;:#T/oF(2.%InO@E:XJF%ae5]a\m'e!<Dp#$T`$FDJK~>"#,
        ] {
            assert!(decode(&wrapped(incomplete), 100)
                .unwrap_err()
                .contains("Flate"));
        }
        let mut reversed = wrapped(good);
        reversed.dict.set(
            "Filter",
            vec![
                Object::Name(b"FlateDecode".to_vec()),
                Object::Name(b"ASCII85Decode".to_vec()),
            ],
        );
        assert!(decode(&reversed, 100).unwrap_err().contains("unsupported"));
    }

    #[test]
    fn textedit_ascii85_flate_bounds_intermediate_and_final_output_separately() {
        let bomb = wrapped(br#"Gb"0;!=]#/!5bBYn"8#L%O_;W!8uZ5!$2+^~>"#);
        assert_eq!(decode(&bomb, 4096).unwrap(), vec![b' '; 4096]);
        assert!(decode(&bomb, 64).unwrap_err().contains("oversized Flate"));
        let mut zeros = vec![b'z'; super::super::MAX_CONTENT / 4 + 1];
        zeros.extend_from_slice(b"~>");
        assert!(decode(&wrapped(&zeros), 64)
            .unwrap_err()
            .contains("ASCII85 output"));
    }

    #[test]
    fn textedit_ascii85_matches_independently_encoded_vectors() {
        // Python base64.a85encode, with the PDF EOD marker appended.
        for (encoded, decoded) in [
            (b"~>".as_slice(), b"".as_slice()),
            (b"!!~>", b"\0"),
            (b"!!!~>", b"\0\0"),
            (b"!!!!~>", b"\0\0\0"),
            (b"z~>", b"\0\0\0\0"),
            (b"rr~>", b"\xff"),
            (b"s8N~>", b"\xff\xff"),
            (b"s8W*~>", b"\xff\xff\xff"),
            (b"s8W-!~>", b"\xff\xff\xff\xff"),
            (b"BOu!rDZ~>", b"hello"),
            (b"L/669[9<6.~>", b"\x86\x4f\xd2\x6f\xb5\x59\xf7\x5b"),
            (b"\0 \tB\nO\ru\x0c!rDZ~>\0\n", b"hello"),
        ] {
            assert_eq!(ascii85(encoded, decoded.len()).unwrap(), decoded);
            if !decoded.is_empty() {
                assert!(ascii85(encoded, decoded.len() - 1).is_err());
            }
        }
    }

    #[test]
    fn textedit_ascii85_refuses_incomplete_or_invalid_input() {
        for input in [
            b"".as_slice(),
            b"BOu!rDZ",
            b"BOu!rDZ~",
            b"BOu!rDZ~ >",
            b"BOu!rDZ~>junk",
            b"!~>",
            b"!z~>",
            b"!!z~>",
            b"!!!z~>",
            b"!!!!z~>",
            b"uuuuu~>",
            b"s8W-\"~>",
            b"s8W-~>",
            b"<~BOu!rDZ~>",
            b"v~>",
            b"\x0b~>",
        ] {
            assert!(ascii85(input, 100).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn textedit_ascii85_bounds_expansion_and_encoded_work() {
        assert_eq!(ascii85(b"zz~>", 8).unwrap(), [0; 8]);
        assert!(ascii85(b"zz~>", 7).is_err());
        let stream = Stream::new(
            dictionary! { "Filter" => "ASCII85Decode" },
            vec![b' '; MAX_ENCODED + 1],
        );
        assert!(decode(&stream, super::super::MAX_CONTENT)
            .unwrap_err()
            .contains("encoded page"));
    }
}
