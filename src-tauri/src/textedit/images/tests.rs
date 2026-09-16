use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Stream};

fn image() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 3,
            "Height" => 2, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB",
            "Interpolate" => true, "Intent" => "Perceptual",
        },
        vec![127; 18],
    )
}

fn fixture(image: Stream, body: &str) -> Document {
    let mut doc = textedit::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let image = doc.add_object(image);
    let mut resources = textedit::resources(&doc, id).unwrap().clone();
    resources.set("XObject", dictionary! {"Im" => image});
    let content = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    let page = doc.get_dictionary_mut(id).unwrap();
    page.set("Resources", resources);
    page.set("Contents", content);
    doc
}

const BODY: &str = "q 30 0 0 20 20 20 cm /Im Do Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET q /Im Do Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";

fn jpeg(data: &[u8], space: &str) -> Stream {
    let mut stream = image();
    stream.content = data.to_vec();
    stream.dict.set("Width", 8);
    stream.dict.set("Height", 8);
    stream.dict.set("ColorSpace", space);
    stream.dict.set("Filter", "DCTDecode");
    stream.dict.set("Name", "AuthoredImage");
    stream
}

const RGB: &[u8] = include_bytes!("synthetic-rgb.jpg");
const GRAY: &[u8] = include_bytes!("synthetic-gray.jpg");
const PROGRESSIVE: &[u8] = include_bytes!("synthetic-progressive.jpg");

#[test]
fn textedit_jpeg_preserves_compressed_bytes_resources_and_following_text() {
    for (data, space) in [
        (RGB, "DeviceRGB"),
        (GRAY, "DeviceGray"),
        (PROGRESSIVE, "DeviceRGB"),
    ] {
        for array in [false, true] {
            let mut stream = jpeg(data, space);
            if array {
                stream
                    .dict
                    .set("Filter", vec![Object::Name(b"DCTDecode".to_vec())]);
            }
            let mut doc = fixture(stream, BODY);
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let before = textedit::scan(&doc, 0).unwrap();
            let objects = doc.objects.clone();
            let content = textedit::page_content(&doc, id).unwrap();
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, "IN");
            assert_eq!(after.runs[1], before.runs[1]);
            let saved = crate::encoding::resolve(
                &doc,
                doc.get_dictionary(id).unwrap().get(b"Contents").unwrap(),
            )
            .as_stream()
            .unwrap();
            assert_eq!(
                saved.content,
                String::from_utf8(content)
                    .unwrap()
                    .replace("(FIRST)", "(IN)")
                    .as_bytes()
            );
            for (key, object) in objects {
                if key != id {
                    assert_eq!(doc.objects[&key], object);
                }
            }
        }
    }
}

#[test]
fn textedit_jpeg_refuses_bad_envelopes_and_incomplete_streams_atomically() {
    let mut invalid = Vec::new();
    for (key, value) in [
        ("Width", 7.into()),
        ("Width", 9.into()),
        ("Height", 7.into()),
        ("Height", 9.into()),
        ("ColorSpace", Object::Name(b"DeviceGray".to_vec())),
        ("Name", Object::string_literal("not a name")),
        ("Name", Object::Name(vec![])),
        ("Name", Object::Name(vec![b'X'; 128])),
        ("DecodeParms", dictionary! {"ColorTransform" => 0}.into()),
        (
            "Filter",
            vec![
                Object::Name(b"ASCII85Decode".to_vec()),
                Object::Name(b"DCTDecode".to_vec()),
            ]
            .into(),
        ),
        ("SMask", Object::Null),
        ("Decode", vec![1.into(), 0.into()].into()),
    ] {
        let mut stream = jpeg(RGB, "DeviceRGB");
        stream.dict.set(key, value);
        invalid.push(stream);
    }
    invalid.push(jpeg(include_bytes!("synthetic-cmyk.jpg"), "DeviceCMYK"));
    let mut invalid_table = RGB.to_vec();
    let table = RGB.windows(2).position(|w| w == [0xff, 0xdb]).unwrap();
    invalid_table[table + 4] = 0xff;
    invalid.push(jpeg(&invalid_table, "DeviceRGB"));
    let mut missing_table = RGB.to_vec();
    while let Some(table) = missing_table.windows(2).position(|w| w == [0xff, 0xc4]) {
        let length = usize::from(u16::from_be_bytes([
            missing_table[table + 2],
            missing_table[table + 3],
        ]));
        missing_table.drain(table..table + 2 + length);
    }
    invalid.push(jpeg(&missing_table, "DeviceRGB"));
    for data in [RGB, GRAY, PROGRESSIVE] {
        let space = if data == GRAY {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        for cut in [0, 1, 2, data.len() / 2, data.len() - 2, data.len() - 1] {
            invalid.push(jpeg(&data[..cut], space));
        }
        let mut joined = data.to_vec();
        joined.extend_from_slice(data);
        invalid.push(jpeg(&joined, space));
        let mut trailer = data.to_vec();
        trailer.extend_from_slice(b"extra\xff\xd9");
        invalid.push(jpeg(&trailer, space));
        let sos = data.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
        let start = sos + 2 + usize::from(u16::from_be_bytes([data[sos + 2], data[sos + 3]]));
        let mut no_samples = data[..start].to_vec();
        no_samples.extend_from_slice(&[0xff, 0xd9]);
        invalid.push(jpeg(&no_samples, space));
        let mut oversize = data.to_vec();
        oversize.resize(2 * textedit::MAX_CONTENT + 1, 0);
        oversize.extend_from_slice(&[0xff, 0xd9]);
        invalid.push(jpeg(&oversize, space));
    }
    for (index, stream) in invalid.into_iter().enumerate() {
        let mut doc = fixture(stream, BODY);
        let objects = doc.objects.clone();
        assert!(
            textedit::scan(&doc, 0).is_err(),
            "invalid JPEG case {index}"
        );
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: vec![],
                operator: 0,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
}

#[test]
fn textedit_jpeg_consumes_shared_image_budget_before_decoding() {
    let stream = jpeg(RGB, "DeviceRGB");
    let doc = fixture(stream, BODY);
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let resources = textedit::resources(&doc, id).unwrap();
    assert_eq!(check(&doc, resources, b"Im", 192).unwrap(), 192);
    assert!(check(&doc, resources, b"Im", 191)
        .unwrap_err()
        .contains("budget"));
    let mut bad = fixture(jpeg(b"not a JPEG", "DeviceRGB"), BODY);
    let resources = textedit::resources(&bad, id).unwrap();
    assert!(check(&bad, resources, b"Im", 191)
        .unwrap_err()
        .contains("budget"));
    // A bogus small dictionary must not let the JPEG's larger dimensions reach
    // allocation. The header-only path refuses a 65535-pixel width.
    let sof = RGB.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let mut bytes = RGB.to_vec();
    bytes[sof + 7..sof + 9].copy_from_slice(&u16::MAX.to_be_bytes());
    bad = fixture(jpeg(&bytes, "DeviceRGB"), BODY);
    assert!(textedit::scan(&bad, 0).is_err());
}

#[test]
fn textedit_jpeg_bounds_valid_encoded_metadata_and_refuses_lossless_headers() {
    for (count, accepted) in [(31, true), (32, false)] {
        let mut data = vec![0xff, 0xd8];
        for _ in 0..count {
            data.extend_from_slice(&[0xff, 0xef, 0xff, 0xff]);
            data.resize(data.len() + 65533, 0);
        }
        data.extend_from_slice(&RGB[2..]);
        assert_eq!(super::jpeg::check(&data, 8, 8, 3).is_ok(), accepted);
    }
    let mut data = RGB.to_vec();
    let sof = data.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    data[sof + 1] = 0xc3;
    assert!(super::jpeg::check(&data, 8, 8, 3)
        .unwrap_err()
        .contains("framing"));
}

#[test]
fn textedit_jpeg_framing_bounds_scans_and_requires_complete_marker_segments() {
    use super::jpeg::framing;
    assert!(framing(PROGRESSIVE).is_ok());
    for end in 0..PROGRESSIVE.len() {
        assert!(framing(&PROGRESSIVE[..end]).is_err(), "prefix {end}");
    }
    let sos = RGB.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    let mut many = RGB[..sos].to_vec();
    for _ in 0..64 {
        many.extend_from_slice(&RGB[sos..RGB.len() - 2]);
    }
    many.extend_from_slice(&[0xff, 0xd9]);
    assert!(framing(&many).is_ok()); // Framing only; repeated coefficients are not certified.
    many.splice(
        many.len() - 2..many.len() - 2,
        RGB[sos..RGB.len() - 2].iter().copied(),
    );
    assert!(framing(&many).unwrap_err().contains("too many"));
    for tail in [
        &[0xff, 0xd9][..],
        &[0xff, 0xef, 0, 1],
        &[0xff, 0xef, 0xff, 0xff, 1],
        &[0xff, 0xdc, 0, 2],
        &[0xff, 0xda, 0, 2, 1, 0xff, 0xd9],
    ] {
        let mut data = vec![0xff, 0xd8];
        data.extend_from_slice(tail);
        assert!(framing(&data).is_err());
    }
}

#[test]
fn textedit_images_preserve_pixels_resources_and_other_text() {
    for (space, components) in [("DeviceGray", 1), ("DeviceRGB", 3), ("DeviceCMYK", 4)] {
        for compressed in [false, true] {
            let mut image = image();
            image.dict.set("ColorSpace", space);
            image.content = vec![127; 3 * 2 * components];
            if compressed {
                image.compress().unwrap();
            }
            let mut doc = fixture(image, BODY);
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let before = textedit::scan(&doc, 0).unwrap();
            let objects = doc.objects.clone();
            let bytes = textedit::page_content(&doc, id).unwrap();
            let change = Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            textedit::write(&mut doc, &[change]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, "IN");
            assert_eq!(after.runs[1], before.runs[1]);
            assert_eq!(after.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
            let saved = crate::encoding::resolve(
                &doc,
                doc.get_dictionary(id).unwrap().get(b"Contents").unwrap(),
            )
            .as_stream()
            .unwrap();
            assert_eq!(
                saved.content,
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("(FIRST)", "(IN)")
                    .as_bytes()
            );
            for (key, object) in objects {
                if key != id {
                    assert_eq!(doc.objects[&key], object);
                }
            }
        }
    }
}

#[test]
fn textedit_images_refuse_masks_forms_and_malformed_samples_atomically() {
    let mut invalid = Vec::new();
    for (key, value) in [
        ("Subtype", Object::Name(b"Form".to_vec())),
        ("Type", Object::Name(b"Other".to_vec())),
        ("ImageMask", true.into()),
        ("SMask", Object::Null),
        ("Mask", Object::Null),
        ("Decode", vec![1.into(), 0.into()].into()),
        ("DecodeParms", Object::Null),
        ("F", Object::string_literal("outside.pdf")),
        ("OC", Object::Null),
        ("Alternates", Object::Null),
        ("OPI", Object::Null),
        ("Unknown", Object::Null),
        ("BitsPerComponent", 1.into()),
        ("Width", 0.into()),
        ("Height", (-1).into()),
        ("Width", 8193.into()),
        ("Height", 8193.into()),
        ("Width", 3.0.into()),
        ("Interpolate", 1.into()),
        ("Intent", Object::Name(b"Unknown".to_vec())),
        ("ColorSpace", Object::Name(b"Pattern".to_vec())),
        ("Filter", Object::Name(b"DCTDecode".to_vec())),
    ] {
        let mut stream = image();
        stream.dict.set(key, value);
        invalid.push(stream);
    }
    for key in [
        "Subtype",
        "Width",
        "Height",
        "BitsPerComponent",
        "ColorSpace",
    ] {
        let mut stream = image();
        stream.dict.remove(key.as_bytes());
        invalid.push(stream);
    }
    for length in [17, 19] {
        let mut stream = image();
        stream.content.resize(length, 0);
        invalid.push(stream);
    }
    for stream in invalid {
        let mut doc = fixture(stream, BODY);
        let before = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err());
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: vec![],
                operator: 0,
                original: "FIRST".into(),
                replacement: "IN".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, before);
    }
    for body in [
        "Do",
        "1 Do",
        "/Missing Do",
        "/Im /Im Do",
        "BT /F1 12 Tf 40 180 Td /Im Do (FIRST) Tj ET",
    ] {
        assert!(
            textedit::scan(&fixture(image(), body), 0).is_err(),
            "{body}"
        );
    }
}

#[test]
fn textedit_images_share_decode_budget_and_bound_resource_names() {
    for (width, accepted) in [(8192, true), (8193, false)] {
        let mut stream = image();
        stream.dict.set("Width", width);
        stream.dict.set("Height", 1);
        stream.content.resize(width as usize * 3, 0);
        assert_eq!(textedit::scan(&fixture(stream, BODY), 0).is_ok(), accepted);
    }
    let mut stream = image();
    stream.dict.set("Width", 1024);
    stream.dict.set("Height", 1024);
    stream.dict.set("ColorSpace", "DeviceGray");
    stream.content = vec![0; 1024 * 1024];
    stream.compress().unwrap();
    let doc = fixture(stream.clone(), BODY);
    assert!(textedit::scan(&doc, 0).is_ok()); // Repeated name consumes the budget once.
    for (count, accepted) in [(1, true), (2, false)] {
        let body = "/Im Do ".repeat(count) + "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET";
        let mut doc = fixture(
            stream.clone(),
            &body.replace("/Im Do /Im Do", "/Im Do /Second Do"),
        );
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let other = doc.add_object(image());
        doc.get_dictionary_mut(id)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"XObject")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Second", other);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted);
    }
    for (count, accepted) in [(32, true), (33, false)] {
        let body = (0..count).map(|i| format!("/I{i} Do ")).collect::<String>()
            + "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET";
        let mut doc = fixture(image(), &body);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let mut objects = Dictionary::new();
        for i in 0..count {
            objects.set(format!("I{i}"), doc.add_object(image()));
        }
        doc.get_dictionary_mut(id)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("XObject", objects);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted);
    }
}

#[test]
fn textedit_images_resolve_named_and_indirect_icc_spaces() {
    // An envelope control only: the independent integration fixture uses a
    // real profile. Header validation does not prove an ICC colour transform.
    let mut bytes = vec![0; 132];
    bytes[..4].copy_from_slice(&132_u32.to_be_bytes());
    bytes[16..20].copy_from_slice(b"RGB ");
    bytes[36..40].copy_from_slice(b"acsp");
    for (named, dct) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut doc = fixture(if dct { jpeg(RGB, "DeviceRGB") } else { image() }, BODY);
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let image_id = textedit::resources(&doc, page)
            .unwrap()
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im")
            .unwrap()
            .as_reference()
            .unwrap();
        let profile = doc.add_object(Stream::new(dictionary! {"N" => 3}, bytes.clone()));
        let space = doc.add_object(Object::Array(vec![
            Object::Name(b"ICCBased".to_vec()),
            profile.into(),
        ]));
        let value = if named {
            doc.get_dictionary_mut(page)
                .unwrap()
                .get_mut(b"Resources")
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("ColorSpace", dictionary! {"ImageSpace" => space});
            Object::Name(b"ImageSpace".to_vec())
        } else {
            Object::Reference(space)
        };
        doc.get_object_mut(image_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("ColorSpace", value);
        assert!(textedit::scan(&doc, 0).is_ok());
        doc.get_object_mut(profile)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content[36] = b'x';
        assert!(textedit::scan(&doc, 0).is_err());
    }
}
