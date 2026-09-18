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

fn mask() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 3,
            "Height" => 2, "BitsPerComponent" => 8, "ColorSpace" => "DeviceGray",
        },
        vec![64; 6],
    )
}

fn fixture(image: Stream, body: &str) -> Document {
    masked(image, None, body)
}

// The image XObject this page paints, for tests that reach past the resources.
fn painted(doc: &Document) -> lopdf::ObjectId {
    let page = crate::pagetree::ordered_pages(doc)[0];
    textedit::resources(doc, page)
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Im")
        .unwrap()
        .as_reference()
        .unwrap()
}

// A soft mask is a stream, so it is written as its own object and the image
// names it. Nothing else about the page changes.
fn masked(mut image: Stream, mask: Option<Stream>, body: &str) -> Document {
    let mut doc = textedit::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    if let Some(mask) = mask {
        let mask = doc.add_object(mask);
        image.dict.set("SMask", mask);
    }
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

#[test]
fn textedit_indexed_images_preserve_palette_and_refuse_invalid_samples() {
    for bad in 0..4 {
        let mut indexed = image();
        indexed.content = vec![0, 1, 0, 1, 0, 1];
        if bad == 1 {
            indexed.content[0] = 2;
        }
        let palette = if bad == 2 {
            vec![0; 5]
        } else {
            vec![0, 0, 0, 255, 255, 255]
        };
        indexed.dict.set(
            "ColorSpace",
            vec![
                Object::Name(b"Indexed".to_vec()),
                Object::Name(b"DeviceRGB".to_vec()),
                Object::Integer(if bad == 3 { 256 } else { 1 }),
                Object::string_literal(palette),
            ],
        );
        let mut doc = fixture(indexed, BODY);
        let objects = doc.objects.clone();
        let before = textedit::scan(&doc, 0);
        if bad != 0 {
            assert!(before.is_err(), "case {bad}");
            continue;
        }
        let before = before.unwrap();
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
        let page = crate::pagetree::ordered_pages(&doc)[0];
        for (id, value) in objects {
            if id != page {
                assert_eq!(doc.objects[&id], value);
            }
        }
    }
}

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
    assert_eq!(check(&doc, resources, b"Im", 192).unwrap().bytes, 192);
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
    // NUL padding after EOI is accepted; any other trailing byte is not.
    for (tail, accepted) in [
        (&[0][..], true),
        (&[0, 0, 0], true),
        (&[0, 1], false),
        (&[0xff], false),
        (&[0xff, 0xd8], false),
    ] {
        let mut data = RGB.to_vec();
        data.extend_from_slice(tail);
        assert_eq!(framing(&data).is_ok(), accepted, "{tail:?}");
    }
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
    // A gray image that fills the page budget exactly.
    stream.dict.set("Width", 8192);
    stream
        .dict
        .set("Height", (textedit::MAX_IMAGES / 8192) as i64);
    stream.dict.set("ColorSpace", "DeviceGray");
    stream.content = vec![0; textedit::MAX_IMAGES];
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

const EDIT: &str = "FIRST";

fn change(scan: &textedit::PageRuns) -> Change {
    Change {
        layout: None,
        page: 0,
        revision: scan.revision.clone(),
        operator: scan.runs[0].operator,
        original: EDIT.into(),
        replacement: "IN".into(),
    }
}

fn indexed() -> Stream {
    let mut stream = image();
    stream.content = vec![0, 1, 0, 1, 0, 1];
    stream.dict.set(
        "ColorSpace",
        vec![
            Object::Name(b"Indexed".to_vec()),
            Object::Name(b"DeviceRGB".to_vec()),
            Object::Integer(1),
            Object::string_literal(vec![0, 0, 0, 255, 255, 255]),
        ],
    );
    stream
}

#[test]
fn textedit_soft_masked_images_keep_both_streams_and_charge_the_page_budget() {
    for compressed in [false, true] {
        let mut alpha = mask();
        if compressed {
            alpha.compress().unwrap();
        }
        let mut doc = masked(image(), Some(alpha), BODY);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let before = textedit::scan(&doc, 0).unwrap();
        let objects = doc.objects.clone();
        let bytes = textedit::page_content(&doc, id).unwrap();
        textedit::write(&mut doc, &[change(&before)]).unwrap();
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
            String::from_utf8(bytes)
                .unwrap()
                .replace("(FIRST)", "(IN)")
                .as_bytes()
        );
        // The image and its mask keep every byte; only the page content moves.
        for (key, object) in objects {
            if key != id {
                assert_eq!(doc.objects[&key], object);
            }
        }
    }
    // A palette may carry alpha too, which is what an ordinary export of a logo
    // with a transparent background looks like.
    assert!(textedit::scan(&masked(indexed(), Some(mask()), BODY), 0).is_ok());
    // The mask's own samples are charged: this base fills the budget exactly,
    // so one more pixel of alpha is one byte too many.
    let mut full = image();
    // A gray image that fills the page budget exactly.
    full.dict.set("Width", 8192);
    full.dict
        .set("Height", (textedit::MAX_IMAGES / 8192) as i64);
    full.dict.set("ColorSpace", "DeviceGray");
    full.content = vec![0; textedit::MAX_IMAGES];
    full.compress().unwrap();
    let mut pixel = mask();
    pixel.dict.set("Width", 1);
    pixel.dict.set("Height", 1);
    pixel.content = vec![64];
    assert!(textedit::scan(&masked(full.clone(), None, BODY), 0).is_ok());
    assert!(textedit::scan(&masked(full, Some(pixel), BODY), 0).is_err());
}

#[test]
fn textedit_soft_masks_refuse_colour_palettes_and_masks_of_their_own() {
    for (key, value) in [
        ("ColorSpace", Object::Name(b"DeviceRGB".to_vec())),
        ("ColorSpace", Object::Name(b"DeviceCMYK".to_vec())),
        (
            "ColorSpace",
            indexed().dict.get(b"ColorSpace").unwrap().clone(),
        ),
        ("Subtype", Object::Name(b"Form".to_vec())),
        ("BitsPerComponent", 1.into()),
        ("Width", 0.into()),
        ("Decode", vec![1.into(), 0.into()].into()),
        ("Mask", Object::Null),
        ("Unknown", Object::Null),
    ] {
        let mut alpha = mask();
        alpha.dict.set(key, value);
        assert!(
            textedit::scan(&masked(image(), Some(alpha), BODY), 0).is_err(),
            "{key}"
        );
    }
    // Control: the same mask without any of those entries is accepted, so each
    // refusal above is that entry rather than the shape of the fixture.
    assert!(textedit::scan(&masked(image(), Some(mask()), BODY), 0).is_ok());
    // Alpha for alpha has no meaning; the chain is refused, never followed.
    let mut doc = masked(image(), Some(mask()), BODY);
    let alpha = doc
        .get_object(painted(&doc))
        .unwrap()
        .as_stream()
        .unwrap()
        .dict
        .get(b"SMask")
        .unwrap()
        .as_reference()
        .unwrap();
    let inner = doc.add_object(mask());
    doc.get_object_mut(alpha)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("SMask", inner);
    assert!(textedit::scan(&doc, 0).is_err());
    // A mask entry naming something that is not a stream is refused, not ignored.
    for value in [
        Object::Integer(3),
        Object::Name(b"Im".to_vec()),
        Object::Reference((9999, 0)),
    ] {
        let mut doc = fixture(image(), BODY);
        let id = painted(&doc);
        doc.get_object_mut(id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("SMask", value);
        assert!(textedit::scan(&doc, 0).is_err());
    }
}

#[test]
fn textedit_images_accept_only_the_default_sample_mapping() {
    let identity = |count: usize| {
        Object::Array(
            (0..count)
                .flat_map(|_| [Object::Real(0.), Object::Real(1.)])
                .collect(),
        )
    };
    for (space, components) in [("DeviceGray", 1), ("DeviceRGB", 3), ("DeviceCMYK", 4)] {
        let mut stream = image();
        stream.dict.set("ColorSpace", space);
        stream.content = vec![127; 3 * 2 * components];
        for (decode, accepted) in [
            (identity(components), true),
            (identity(components + 1), false),
            (Object::Array(vec![1.into(), 0.into()]), false),
            (Object::Array(vec![0.into(), 255.into()]), false),
            (Object::Array(Vec::new()), false),
            (
                Object::Array(vec![Object::Name(b"Zero".to_vec()), 1.into()]),
                false,
            ),
            (Object::Null, false),
        ] {
            stream.dict.set("Decode", decode);
            assert_eq!(
                textedit::scan(&fixture(stream.clone(), BODY), 0).is_ok(),
                accepted,
                "{space}"
            );
        }
    }
    // ISO 32000-1 Table 89: an indexed image maps its whole sample range, which
    // is the bit depth rather than the palette's highest index.
    for (decode, accepted) in [
        (vec![0.into(), 255.into()], true),
        (vec![Object::Real(0.), Object::Real(255.)], true),
        (vec![0.into(), 1.into()], false),
        (vec![0.into(), 254.into()], false),
        (vec![0.into(), 255.into(), 0.into(), 255.into()], false),
    ] {
        let mut stream = indexed();
        stream.dict.set("Decode", Object::Array(decode));
        assert_eq!(
            textedit::scan(&fixture(stream, BODY), 0).is_ok(),
            accepted,
            "indexed"
        );
    }
    // The array may be written indirectly, as several ordinary producers do.
    let mut doc = fixture(image(), BODY);
    let default = doc.add_object(Object::Array(
        (0..3)
            .flat_map(|_| [Object::Real(0.), Object::Real(1.)])
            .collect(),
    ));
    let id = painted(&doc);
    doc.get_object_mut(id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Decode", default);
    assert!(textedit::scan(&doc, 0).is_ok());
}

#[test]
fn textedit_images_accept_decode_parameters_that_select_no_prediction() {
    let mut compressed = image();
    compressed.compress().unwrap();
    compressed.dict.remove(b"DecodeParms");
    for (parms, accepted) in [
        (Object::Dictionary(Dictionary::new()), true),
        (dictionary! {"Predictor" => 1}.into(), true),
        (
            dictionary! {
                "Predictor" => 1, "Colors" => 3, "Columns" => 3, "BitsPerComponent" => 8,
            }
            .into(),
            true,
        ),
        (dictionary! {"Colors" => 3, "Columns" => 3}.into(), true),
        (
            Object::Array(vec![dictionary! {"Columns" => 3}.into()]),
            true,
        ),
        (dictionary! {"Predictor" => 2}.into(), false),
        (dictionary! {"Predictor" => 12}.into(), false),
        (dictionary! {"Predictor" => 15}.into(), false),
        (dictionary! {"Predictor" => Object::Real(1.)}.into(), false),
        (dictionary! {"EarlyChange" => 0}.into(), false),
        (dictionary! {"Unknown" => 0}.into(), false),
        (
            dictionary! {"Columns" => Object::Name(b"Three".to_vec())}.into(),
            false,
        ),
        (
            Object::Array(vec![
                Object::Dictionary(Dictionary::new()),
                Object::Dictionary(Dictionary::new()),
            ]),
            false,
        ),
        (Object::Array(Vec::new()), false),
        (Object::Null, false),
        (Object::Integer(1), false),
    ] {
        let mut stream = compressed.clone();
        stream.dict.set("DecodeParms", parms.clone());
        assert_eq!(
            textedit::scan(&fixture(stream, BODY), 0).is_ok(),
            accepted,
            "image {parms:?}"
        );
        // A soft mask carries the same parameters and is held to the same rule.
        let mut alpha = mask();
        alpha.compress().unwrap();
        alpha.dict.set("DecodeParms", parms.clone());
        assert_eq!(
            textedit::scan(&masked(image(), Some(alpha), BODY), 0).is_ok(),
            accepted,
            "mask {parms:?}"
        );
    }
    // Control: a page content stream still refuses decode parameters outright,
    // because nothing has established that its filter output is the content.
    let mut doc = fixture(image(), BODY);
    assert!(textedit::scan(&doc, 0).is_ok());
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc
        .get_dictionary(id)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_object_mut(content)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("DecodeParms", dictionary! {"Predictor" => 1});
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_image_metadata_packets_are_kept_and_must_declare_their_type() {
    const PACKET: &[u8] = b"<?xpacket begin=\"\"?><x:xmpmeta/><?xpacket end=\"r\"?>";
    for (declared, accepted) in [
        (Some("Metadata"), true),
        (Some("XObject"), false),
        (None, false),
    ] {
        let mut doc = fixture(image(), BODY);
        let mut packet = Stream::new(dictionary! {"Subtype" => "XML"}, PACKET.to_vec());
        if let Some(name) = declared {
            packet.dict.set("Type", name);
        }
        let packet = doc.add_object(packet);
        let id = painted(&doc);
        doc.get_object_mut(id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Metadata", packet);
        let scan = textedit::scan(&doc, 0);
        assert_eq!(scan.is_ok(), accepted, "{declared:?}");
        let Ok(scan) = scan else {
            continue;
        };
        // The packet describes the image and is preserved with it.
        let before = doc.objects.clone();
        textedit::write(&mut doc, &[change(&scan)]).unwrap();
        assert_eq!(doc.objects[&packet], before[&packet]);
        assert_eq!(doc.objects[&id], before[&id]);
    }
    for value in [
        Object::Null,
        Object::Integer(3),
        Object::string_literal(PACKET),
    ] {
        let mut doc = fixture(image(), BODY);
        let id = painted(&doc);
        doc.get_object_mut(id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Metadata", value);
        assert!(textedit::scan(&doc, 0).is_err());
    }
}

// A screenshot pasted into a paper: 2264 x 1440 RGB with a soft mask, 13 MB of
// samples, the size that refused the arXiv sample's second page.
#[test]
fn textedit_a_screenshot_with_its_mask_fits_the_page_budget() {
    let mut picture = image();
    picture.dict.set("Width", 2264);
    picture.dict.set("Height", 1440);
    picture.content = vec![127; 2264 * 1440 * 3];
    picture.compress().unwrap();
    let mut alpha = mask();
    alpha.dict.set("Width", 2264);
    alpha.dict.set("Height", 1440);
    alpha.content = vec![255; 2264 * 1440];
    alpha.compress().unwrap();
    assert_eq!(
        textedit::scan(&masked(picture, Some(alpha), BODY), 0)
            .unwrap()
            .runs[0]
            .text,
        "FIRST"
    );
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

// Acrobat recompresses scanned JPEGs as `[/FlateDecode /DCTDecode]`. The
// inflated JPEG meets the same checks as a plain one; the stream is kept.
#[test]
fn textedit_flate_wrapped_jpeg_is_checked_as_a_jpeg_and_kept() {
    let chain = |first: &str, second: &str| {
        Object::Array(vec![
            Object::Name(first.as_bytes().to_vec()),
            Object::Name(second.as_bytes().to_vec()),
        ])
    };
    let mut stream = jpeg(&deflate(RGB), "DeviceRGB");
    stream.dict.set("Filter", chain("FlateDecode", "DCTDecode"));
    let mut doc = fixture(stream.clone(), BODY);
    let before = textedit::scan(&doc, 0).unwrap();
    let image = painted(&doc);
    textedit::write(&mut doc, &[change(&before)]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    assert_eq!(doc.objects[&image], Object::Stream(stream.clone()));
    let mut truncated = stream.clone();
    truncated.content.truncate(truncated.content.len() - 1);
    let mut reversed = stream.clone();
    reversed
        .dict
        .set("Filter", chain("DCTDecode", "FlateDecode"));
    let mut broken = jpeg(&deflate(&RGB[..RGB.len() - 2]), "DeviceRGB");
    broken.dict.set("Filter", chain("FlateDecode", "DCTDecode"));
    let mut sized = stream.clone();
    sized.dict.set("Width", 9);
    for invalid in [truncated, reversed, broken, sized] {
        assert!(textedit::scan(&fixture(invalid, BODY), 0).is_err());
    }
    // The decoder also stops at EOI, so padded JPEGs pass the whole check.
    let mut padded = [RGB, &[0, 0, 0]].concat();
    let mut stream = jpeg(&deflate(&padded), "DeviceRGB");
    stream.dict.set("Filter", chain("FlateDecode", "DCTDecode"));
    assert!(textedit::scan(&fixture(stream, BODY), 0).is_ok());
    padded.push(1);
    assert!(textedit::scan(&fixture(jpeg(&padded, "DeviceRGB"), BODY), 0).is_err());
}

// Group 4 rows of a 16-pixel stencil: a diagonal band, so every row differs
// from the one above it and the decoder cannot coast on vertical modes alone.
fn group4(rows: usize) -> Vec<u8> {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for row in 0..rows {
        let pels = (0..16).map(|x| {
            if (x + row) % 5 < 2 {
                fax::Color::Black
            } else {
                fax::Color::White
            }
        });
        encoder.encode_line(pels, 16).unwrap();
    }
    encoder.finish().unwrap().finish()
}

fn stencil(content: Vec<u8>) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 16, "Height" => 4,
            "ImageMask" => true, "BitsPerComponent" => 1, "Filter" => "CCITTFaxDecode",
            "DecodeParms" => dictionary! { "K" => -1, "Columns" => 16, "Rows" => 4, "BlackIs1" => true },
        },
        content,
    )
}

// Scanned pages and Acrobat's OCR output keep text as CCITT stencil masks.
// They paint the current fill colour, are kept byte for byte, and must
// decode to exactly Height complete rows.
#[test]
fn textedit_ccitt_stencil_masks_are_decoded_whole_and_kept() {
    let mask = stencil(group4(4));
    let mut doc = fixture(mask.clone(), BODY);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let resources = textedit::resources(&doc, page).unwrap();
    assert_eq!(
        check(&doc, resources, b"Im", 1000).unwrap(),
        Image {
            bytes: 8,
            stencil: true
        }
    );
    assert!(check(&doc, resources, b"Im", 7)
        .unwrap_err()
        .contains("budget"));
    let before = textedit::scan(&doc, 0).unwrap();
    let image = painted(&doc);
    textedit::write(&mut doc, &[change(&before)]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    assert_eq!(doc.objects[&image], Object::Stream(mask.clone()));

    let accepted: Vec<fn(&mut Stream)> = vec![
        |s| s.dict.set("Decode", vec![1.into(), 0.into()]),
        |s| s.dict.remove(b"BitsPerComponent").map(drop).unwrap(),
        |s| {
            s.dict
                .set("Filter", vec![Object::Name(b"CCITTFaxDecode".to_vec())])
        },
        |s| {
            s.dict
                .set("DecodeParms", dictionary! { "K" => -1, "Columns" => 16 })
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                vec![
                    dictionary! { "K" => -1, "Columns" => 16, "Rows" => 0, "EndOfBlock" => false }
                        .into(),
                ],
            )
        },
        |s| {
            // Unfiltered, two bytes per row.
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
            s.content = vec![0; 8];
        },
        |s| {
            // A 12-pixel row still takes two bytes; rows are byte-aligned.
            s.dict.set("Width", 12);
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
            s.content = vec![0; 8];
        },
    ];
    for (index, edit) in accepted.into_iter().enumerate() {
        let mut mask = stencil(group4(4));
        edit(&mut mask);
        assert!(
            textedit::scan(&fixture(mask, BODY), 0).is_ok(),
            "accepted {index}"
        );
    }
    let refused: Vec<fn(&mut Stream)> = vec![
        |s| s.content = group4(3),
        |s| s.content.truncate(s.content.len() / 2),
        |s| s.content.clear(),
        |s| s.dict.set("Height", 5),
        |s| s.dict.set("Width", 15),
        |s| s.dict.set("Width", 0),
        |s| s.dict.set("BitsPerComponent", 8),
        |s| s.dict.set("ColorSpace", "DeviceGray"),
        |s| s.dict.set("SMask", Object::Null),
        |s| s.dict.set("Mask", Object::Null),
        |s| s.dict.set("Decode", vec![0.into(), 0.5.into()]),
        |s| s.dict.set("Filter", "DCTDecode"),
        |s| {
            s.dict
                .set("DecodeParms", dictionary! { "K" => 0, "Columns" => 16 })
        },
        |s| s.dict.set("DecodeParms", dictionary! { "Columns" => 16 }),
        |s| s.dict.set("DecodeParms", dictionary! { "K" => -1 }),
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => 16, "Rows" => 3 },
            )
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => 16, "EncodedByteAlign" => true },
            )
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => 16, "EndOfLine" => true },
            )
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => 16, "DamagedRowsBeforeError" => 1 },
            )
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => 16, "Unknown" => 1 },
            )
        },
        |s| {
            s.dict.set(
                "DecodeParms",
                dictionary! { "K" => -1, "Columns" => Object::Real(16.5) },
            )
        },
        |s| {
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
            s.content = vec![0; 7];
        },
    ];
    for (index, edit) in refused.into_iter().enumerate() {
        let mut mask = stencil(group4(4));
        edit(&mut mask);
        assert!(
            textedit::scan(&fixture(mask, BODY), 0).is_err(),
            "refused {index}"
        );
    }
}

// A stencil paints the fill colour current at each Do; an ordinary image
// paints its own samples, whatever that colour is.
#[test]
fn textedit_stencil_masks_paint_only_a_ready_fill_colour() {
    let body = BODY.replacen("q 30", "q /Pattern cs 30", 1);
    assert!(textedit::scan(&fixture(image(), &body), 0).is_ok());
    assert!(textedit::scan(&fixture(stencil(group4(4)), &body), 0)
        .unwrap_err()
        .contains("unselected"));
    // Checked at each use, not only the first: the second Do is refused.
    let later = BODY.replacen("ET q /Im Do Q", "ET q /Pattern cs /Im Do Q", 1);
    assert!(textedit::scan(&fixture(stencil(group4(4)), &later), 0).is_err());
    assert!(textedit::scan(&fixture(image(), &later), 0).is_ok());
}
