use super::*;
use crate::textedit::{self, Change};
use lopdf::{content::Content, dictionary, Stream};

fn page(content: &[u8]) -> (Document, lopdf::ObjectId) {
    let mut doc = textedit::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    (doc, page)
}

fn spaces(doc: &mut Document, page: lopdf::ObjectId, spaces: Dictionary) {
    let mut resources = crate::textedit::resources(doc, page).unwrap().clone();
    resources.set("ColorSpace", spaces);
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
}

// Only an ICC envelope for unit checks; the real Quartz profile is the renderer
// integration control. No claim that a header alone defines a colour transform.
fn profile(n: i64) -> Stream {
    let mut bytes = vec![0; 132];
    bytes[..4].copy_from_slice(&132_u32.to_be_bytes());
    bytes[16..20].copy_from_slice(match n {
        1 => b"GRAY",
        3 => b"RGB ",
        _ => b"CMYK",
    });
    bytes[36..40].copy_from_slice(b"acsp");
    Stream::new(dictionary! { "N" => n }, bytes)
}

fn with_profile(profile: Stream) -> (Document, lopdf::ObjectId) {
    let (mut doc, page) = page(b"/C cs 0 sc BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let id = doc.add_object(profile);
    spaces(
        &mut doc,
        page,
        dictionary! { "C" => vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(id)] },
    );
    (doc, page)
}

#[test]
fn textedit_colours_restore_state_and_preserve_operators() {
    let (mut doc, page_id) = page(b"/DeviceRGB cs q /DeviceGray cs 0 sc BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q 1 0 0 scn BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
    let before = textedit::scan(&doc, 0).unwrap();
    let ops = Content::decode_strict(&doc.get_page_content(page_id))
        .unwrap()
        .operations;
    let update = Change {
        layout: None,
        page: 0,
        revision: before.revision,
        operator: before.runs[0].operator,
        original: "FIRST".into(),
        replacement: "IN".into(),
    };
    textedit::write(&mut doc, &[update]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "IN");
    assert_eq!(after.runs[1], before.runs[1]);
    let saved = Content::decode_strict(&doc.get_page_content(page_id))
        .unwrap()
        .operations;
    assert_eq!(saved.len(), ops.len());
    for (index, (saved, original)) in saved.iter().zip(&ops).enumerate() {
        assert_eq!(saved.operator, original.operator);
        if index != before.runs[0].operator as usize {
            assert_eq!(saved.operands, original.operands);
        }
    }
    for prefix in [
        "0 g",
        "1 0 0 rg",
        "0 0 0 1 k",
        "/DeviceCMYK cs 0 0 0 1 sc",
        "/DeviceRGB cs 0 g 1 sc",
        "/DeviceGray cs 1 0 0 rg 0 1 0 sc",
    ] {
        let (doc, _) = page(format!("{prefix} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET").as_bytes());
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "TEXT");
    }
}

#[test]
fn textedit_colours_refuse_bad_components_and_unsupported_spaces() {
    for prefix in [
        "cs",
        "0 cs",
        "/Pattern cs",
        "/Missing cs",
        "1 0 sc",
        "0 0 rg",
        "0 0 0 1 scn",
        "-0.1 g",
        "1.1 g",
        "(0) g",
        "true g",
        "[0] g",
        "0 0 0 /Pattern scn",
        "/DeviceRGB cs q /DeviceGray cs Q 0 sc",
        "0 g 0 0 SC",
        "0 g 0 cs",
    ] {
        let (doc, _) = page(format!("{prefix} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET").as_bytes());
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {prefix}");
    }
    for key in ["DefaultGray", "DefaultRGB", "DefaultCMYK"] {
        let (mut doc, id) = page(b"BT /F1 12 Tf 40 180 Td (TEXT) Tj ET");
        let mut defaults = Dictionary::new();
        defaults.set(key, "DeviceGray");
        spaces(&mut doc, id, defaults);
        assert!(textedit::scan(&doc, 0).is_err());
    }
}

#[test]
fn textedit_icc_checks_header_range_and_preserves_profile_bytes() {
    for n in [1_i64, 3, 4] {
        let (doc, page) = with_profile(profile(n));
        let resources = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        assert_eq!(named(&doc, resources, b"C").unwrap(), n as usize);
    }
    let mut stream = profile(1);
    stream.dict.set("Alternate", "DeviceGray");
    stream.dict.set("Range", vec![0.into(), 1.into()]);
    let (mut doc, _) = with_profile(stream);
    let before = doc.objects.clone();
    let runs = textedit::scan(&doc, 0).unwrap();
    let update = Change {
        layout: None,
        page: 0,
        operator: runs.runs[0].operator,
        revision: runs.revision,
        original: "FIRST".into(),
        replacement: "IN".into(),
    };
    textedit::write(&mut doc, &[update]).unwrap();
    for (id, object) in before
        .iter()
        .filter(|(_, obj)| obj.as_stream().is_ok_and(|s| s.dict.has(b"N")))
    {
        assert_eq!(&doc.objects[id], object);
    }
    for defect in 0..8 {
        let mut stream = profile(1);
        match defect {
            0 => stream.dict.set("N", 2),
            1 => stream.dict.set("Alternate", "DeviceRGB"),
            2 => stream.dict.set("Range", vec![(-1).into(), 1.into()]),
            3 => stream.dict.set("Range", vec![0.into()]),
            4 => stream.content[36..40].copy_from_slice(b"xxxx"),
            5 => stream.content[16..20].copy_from_slice(b"RGB "),
            6 => stream.content[3] = 131,
            _ => stream.content.truncate(100),
        }
        let (doc, _) = with_profile(stream);
        assert!(textedit::scan(&doc, 0).is_err(), "accepted defect {defect}");
    }
}

#[test]
fn textedit_icc_decoding_and_colour_space_count_are_bounded() {
    let mut stream = profile(1);
    stream.compress().unwrap();
    assert!(stream.dict.has(b"Filter"));
    // Tiny headers might not benefit from compression; the repeated zeros do.
    assert!(textedit::scan(&with_profile(stream).0, 0).is_ok());
    let mut stream = profile(1);
    stream.content.resize(super::super::MAX_CONTENT + 1, 0);
    let len = stream.content.len() as u32;
    stream.content[..4].copy_from_slice(&len.to_be_bytes());
    for compressed in [false, true] {
        let mut stream = stream.clone();
        if compressed {
            stream.compress().unwrap();
        }
        assert!(textedit::scan(&with_profile(stream).0, 0).is_err());
    }
    for count in [32, 33] {
        let prefix = (0..count).map(|i| format!("/C{i} cs ")).collect::<String>();
        let (mut doc, id) =
            page(format!("{prefix} 0 sc BT /F1 12 Tf 40 180 Td (TEXT) Tj ET").as_bytes());
        let mut entries = Dictionary::new();
        for i in 0..count {
            entries.set(format!("C{i}"), "DeviceGray");
        }
        spaces(&mut doc, id, entries);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), count == 32);
    }
}

#[test]
fn textedit_stroke_colours_restore_independent_state_and_preserve_operators() {
    let body = b"/RGB CS /DeviceCMYK cs q /DeviceGray CS /DeviceRGB cs 0 SC 1 0 0 scn Q .1 .2 .3 SCN 0 0 0 1 sc 0 0 m 20 20 l S BT /F1 12 Tf 40 180 Td (FIRST) Tj ET";
    let (mut doc, id) = page(body);
    spaces(&mut doc, id, dictionary! { "RGB" => "DeviceRGB" });
    let before = textedit::scan(&doc, 0).unwrap();
    let original = Content::decode_strict(body).unwrap();
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
    let saved = Content::decode_strict(&doc.get_page_content(id)).unwrap();
    assert_eq!(saved.operations.len(), original.operations.len());
    for (index, (a, b)) in original
        .operations
        .iter()
        .zip(&saved.operations)
        .enumerate()
    {
        assert_eq!(a.operator, b.operator);
        if index != before.runs[0].operator as usize {
            assert_eq!(a.operands, b.operands);
        }
    }
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    for (prefix, accepted) in [
        ("0 SC", true),
        ("/DeviceRGB CS 0 G 1 SC", true),
        ("/DeviceGray CS 0 0 0 RG 1 0 0 SCN", true),
        ("/DeviceRGB CS 0 0 0 1 K 0 0 0 0 SC", true),
        ("/DeviceRGB CS q /DeviceGray CS Q 0 SC", false),
        ("/DeviceRGB CS /DeviceGray cs 0 SC", false),
        ("/DeviceRGB CS /DeviceGray cs 0 sc 1 0 0 SC", true),
        ("/Missing CS", false),
        ("/Pattern CS", true),
        ("1 CS", false),
        ("/DeviceRGB CS -1 0 0 SCN", false),
        ("/DeviceRGB CS 0 0 1.1 SC", false),
        ("/DeviceRGB CS 0 0 /Pattern SCN", false),
    ] {
        let (doc, _) = page(format!("{prefix} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{prefix}");
    }
}
