//! Complete, bounded content decoding for the editor. General PDF recovery is
//! unsuitable here: accepting a decoded prefix can discard the rest on save.

use crate::encoding::complete::{ascii85, flate};
use lopdf::{Dictionary, Document, Object, Stream};

// Bounds scanning as well as allocation, including ASCII85 whitespace. This
// leaves room for the encoding overhead around a MAX_CONTENT-sized stream.
const MAX_ENCODED: usize = 2 * super::MAX_CONTENT;

pub(crate) fn decode(stream: &Stream, limit: usize) -> Result<Vec<u8>, String> {
    if stream.dict.has(b"DecodeParms") {
        return Err(said!(
            "external or parameterised content streams are not editable yet"
        ));
    }
    decode_unpredicted(stream, limit)
}

/// Whether a Flate dictionary explicitly leaves the decoded samples unchanged.
pub(super) fn no_prediction(params: &Dictionary) -> bool {
    params.iter().all(|(key, value)| match key.as_slice() {
        b"Predictor" => value.as_i64().ok() == Some(1),
        b"Colors" | b"Columns" | b"BitsPerComponent" => value.as_i64().is_ok(),
        _ => false,
    })
}

/// Complete decoding for read-only text mapping, including no-op filter parameters.
pub(crate) fn mapping_stream(
    doc: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if let Ok(params) = stream.dict.get(b"DecodeParms") {
        use crate::encoding::resolve;
        let plain = |filter: &Object, param: &Object| match resolve(doc, param) {
            Object::Null => true,
            Object::Dictionary(dict) if filter.as_name().ok() == Some(b"FlateDecode") => {
                no_prediction(dict)
            }
            Object::Dictionary(dict) => dict.is_empty(),
            _ => false,
        };
        let valid = match (stream.dict.get(b"Filter"), resolve(doc, params)) {
            (_, Object::Null) => true,
            (Ok(Object::Array(filters)), Object::Array(params)) => {
                filters.len() <= 2
                    && filters.len() == params.len()
                    && filters
                        .iter()
                        .zip(params)
                        .all(|(filter, param)| plain(filter, param))
            }
            (Ok(filter @ Object::Name(_)), param) => plain(filter, param),
            _ => false,
        };
        if !valid {
            return Err(said!("unsupported content stream filter"));
        }
    }
    decode_unpredicted(stream, limit)
}

// For a caller that has established that this stream's decode parameters select
// no prediction, which leaves the filter's own output as the data. Every other
// caller goes through `decode` and gets no say in the matter.
pub(super) fn decode_unpredicted(stream: &Stream, limit: usize) -> Result<Vec<u8>, String> {
    if stream.content.len() > MAX_ENCODED {
        return Err(said!("encoded page content exceeds its limit"));
    }
    if stream.dict.has(b"F") {
        return Err(said!(
            "external or parameterised content streams are not editable yet"
        ));
    }
    let filters = match stream.dict.get(b"Filter") {
        Err(_) => &[][..],
        Ok(Object::Array(values)) if !values.is_empty() && values.len() <= 2 => values,
        Ok(value @ Object::Name(_)) => std::slice::from_ref(value),
        _ => return Err(said!("unsupported content stream filter")),
    };
    match filters {
        [] if stream.content.len() <= limit => Ok(stream.content.clone()),
        [] => Err(said!("page content exceeds its limit")),
        [Object::Name(name)] if name == b"FlateDecode" => flate(&stream.content, limit),
        [Object::Name(name)] if name == b"ASCII85Decode" => ascii85(&stream.content, limit),
        [Object::Name(first), Object::Name(second)]
            if first == b"ASCII85Decode" && second == b"FlateDecode" =>
        {
            // The intermediate compressed bytes have their own bound; the
            // remaining page budget is enforced on the final Flate output.
            flate(&ascii85(&stream.content, super::MAX_CONTENT)?, limit)
        }
        _ => Err(said!("unsupported content stream filter")),
    }
}

// The Flate stage in front of a DCT image (`[/FlateDecode /DCTDecode]`, which
// Acrobat writes when it recompresses a scan). Its output is a JPEG, bounded
// like encoded content; the JPEG checks then apply to it unchanged.
pub(super) fn inflate(stream: &Stream) -> Result<Vec<u8>, String> {
    if stream.content.len() > MAX_ENCODED || stream.dict.has(b"F") {
        return Err(said!("encoded page content exceeds its limit"));
    }
    flate(&stream.content, MAX_ENCODED)
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
