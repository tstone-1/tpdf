use super::*;
use lopdf::Dictionary;

// Spelling and adjacency match the producer, independently of parser constants.
const MAP: &str = "/CIDInit/ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName/Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<01> <0041>\n<02> <0042>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
fn stream(text: &str) -> Stream {
    Stream::new(Dictionary::new(), text.as_bytes().to_vec())
}

#[test]
fn textedit_mapping_reads_complete_bijective_ascii_data() {
    for input in [
        MAP.to_string(),
        MAP.replace("<01>", "% comment\n<01>"),
        MAP.replace("2 beginbfchar", "1 beginbfchar")
            .replace("<02>", "endbfchar\n1 beginbfchar\n<02>"),
    ] {
        let codes = parse(&stream(&input)).unwrap();
        assert_eq!(codes[1], Some(b'A'));
        assert_eq!(codes[2], Some(b'B'));
        assert_eq!(codes.iter().flatten().count(), 2);
        assert_eq!(codes[0], None);
    }
}

#[test]
fn textedit_mapping_refuses_ambiguous_partial_and_extended_data() {
    for (from, to) in [
        ("2 beginbfchar", "1 beginbfchar"),
        ("2 beginbfchar", "101 beginbfchar"),
        ("2 beginbfchar", "0 beginbfchar"),
        ("2 beginbfchar", "-1 beginbfchar"),
        ("<02>", "<01>"),
        ("<0042>", "<0041>"),
        ("<01>", "<0001>"),
        ("<0041>", "<00410042>"),
        ("<0041>", "<00e4>"),
        ("<0041>", "<001f>"),
        ("<0041>", "<007f>"),
        ("<0041>", "<d800>"),
        ("<FF>", "<FE>"),
        ("/CMapType 2", "/CMapType 1"),
        ("beginbfchar", "beginbfrange"),
        ("/Supplement 0", "/Supplement 1"),
        ("endbfchar", "endbfchar /Other usecmap"),
        ("begincmap", "begincmap /WMode 1 def"),
    ] {
        assert!(
            parse(&stream(&MAP.replace(from, to))).is_err(),
            "accepted {from} -> {to}"
        );
    }
    for text in [
        "".into(),
        format!("{MAP} 1"),
        format!("{MAP} pop"),
        MAP[..MAP.len() - 4].to_string(),
        MAP.replace("<01> <0041>\n<02> <0042>", ""),
    ] {
        assert!(parse(&stream(&text)).is_err());
    }
    let mut inherited = stream(MAP);
    inherited.dict.set("UseCMap", "Identity-H");
    assert!(parse(&inherited).unwrap_err().contains("inherited"));
}

#[test]
fn textedit_mapping_bounds_plain_and_compressed_streams() {
    let mut good = stream(MAP);
    good.compress().unwrap();
    assert_eq!(parse(&good).unwrap()[1], Some(b'A'));
    let mut large = stream(&format!("{MAP}{}", " ".repeat(MAX_MAP)));
    assert!(parse(&large).is_err());
    large.compress().unwrap();
    assert!(parse(&large).is_err());
}

fn single_body(body: &str) -> String {
    MAP.replace("2 beginbfchar\n<01> <0041>\n<02> <0042>\nendbfchar", body)
}

#[test]
fn textedit_single_ranges_expand_exactly_and_mix_with_bfchar() {
    // Every possible source byte is exercised, including a range ending at FF.
    // Targets stay ASCII even when the font's source codes are not.
    for first in 0_u16..=255 {
        let length = 95.min(256 - first);
        let last = first + length - 1;
        let source = single_body(&format!(
            "1 beginbfrange <{first:02x}> <{last:02x}> <0020> endbfrange"
        ));
        let codes = parse(&stream(&source)).unwrap();
        for (code, &value) in codes.iter().enumerate() {
            let expected = if (first..=last).contains(&(code as u16)) {
                Some((32 + code as u16 - first) as u8)
            } else {
                None
            };
            assert_eq!(value, expected, "first={first}, code={code}");
        }
    }
    let source = single_body(
        "1 beginbfchar <ff> <007e> endbfchar
         2 beginbfrange <00> <01> <0020> <a0> <a1> <0041> endbfrange
         1 beginbfchar <02> <0043> endbfchar",
    );
    for compressed in [false, true] {
        let mut input = stream(&source);
        if compressed {
            input.compress().unwrap();
        }
        let codes = parse(&input).unwrap();
        assert_eq!(codes.iter().flatten().count(), 6);
        for (code, ch) in [
            (0, b' '),
            (1, b'!'),
            (2, b'C'),
            (160, b'A'),
            (161, b'B'),
            (255, b'~'),
        ] {
            assert_eq!(codes[code], Some(ch));
        }
    }
}

#[test]
fn textedit_single_ranges_reject_bad_endpoints_counts_and_overlap() {
    for body in [
        "1 beginbfrange <02> <01> <0041> endbfrange",
        "1 beginbfrange <00> <ff> <0020> endbfrange",
        "1 beginbfrange <01> <02> <007e> endbfrange",
        "1 beginbfrange <01> <02> <001f> endbfrange",
        "1 beginbfrange <01> <01> <0100> endbfrange",
        "1 beginbfrange <01> <02> <d800> endbfrange",
        "1 beginbfrange <01> <02> <00ff> endbfrange",
        "1 beginbfrange <0001> <02> <0041> endbfrange",
        "1 beginbfrange <01> <0002> <0041> endbfrange",
        "1 beginbfrange <01> <02> <41> endbfrange",
        "1 beginbfrange <01> <02> <00410042> endbfrange",
        "1 beginbfrange <01> <02> [<0041> <0042>] endbfrange",
        "1 beginbfrange 1 <02> <0041> endbfrange",
        "1 beginbfrange <01> <02> /A endbfrange",
        "1 beginbfrange <01> <02> <0041> endbfchar",
        "1 beginbfchar <01> <0041> endbfrange",
        "0 beginbfrange <01> <02> <0041> endbfrange",
        "101 beginbfrange <01> <02> <0041> endbfrange",
        "2 beginbfrange <01> <02> <0041> endbfrange",
        "1 beginbfrange <01> <02> <0041> <03> <04> <0043> endbfrange",
        "1 beginbfrange <01> <02> endbfrange",
        "1 beginbfrange <01> <02> <0041> 1 endbfrange",
        "1 beginbfrange <01> <02> <0041> endbfrange 1 beginbfchar <02> <0043> endbfchar",
        "1 beginbfchar <02> <0043> endbfchar 1 beginbfrange <01> <02> <0041> endbfrange",
        "1 beginbfrange <01> <02> <0041> endbfrange 1 beginbfchar <03> <0042> endbfchar",
        "1 beginbfchar <03> <0042> endbfchar 1 beginbfrange <01> <02> <0041> endbfrange",
        "2 beginbfrange <01> <02> <0041> <02> <03> <0043> endbfrange",
        "2 beginbfrange <01> <02> <0041> <03> <04> <0042> endbfrange",
    ] {
        assert!(parse(&stream(&single_body(body))).is_err(), "{body}");
    }
}

fn wide_map() -> String {
    MAP.replace("<00> <FF>", "<0000> <FFFF>")
        .replace("<01>", "<0101>")
        .replace("<02>", "<0102>")
}

#[test]
fn textedit_cid_mapping_accepts_bfchar_ranges_and_high_codes() {
    for map in [
        wide_map(),
        wide_map().replace(
            "2 beginbfchar\n<0101> <0041>\n<0102> <0042>\nendbfchar",
            "1 beginbfrange\n<0101> <0102> <0041>\nendbfrange",
        ),
    ] {
        let parsed = parse_cid(&stream(&map)).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[&257], b'A');
        assert_eq!(parsed[&258], b'B');
        assert!(parse(&stream(&map)).is_err());
        let high = map.replace("0101", "fffe").replace("0102", "ffff");
        assert_eq!(parse_cid(&stream(&high)).unwrap()[&65535], b'B');
    }
}

#[test]
fn textedit_cid_mapping_latin1_boundaries_and_ranges() {
    let map = |body: &str| {
        wide_map().replace(
            "2 beginbfchar\n<0101> <0041>\n<0102> <0042>\nendbfchar",
            body,
        )
    };
    // Both ends of both printable intervals, and the control gap between them.
    // Large UTF-16 values must be rejected before casting down to a byte.
    for target in [
        0_u16, 31, 32, 126, 127, 128, 159, 160, 196, 223, 228, 255, 256, 0xd800, 0xffff,
    ] {
        let source = map(&format!("1 beginbfchar <0101> <{target:04x}> endbfchar"));
        let result = parse_cid(&stream(&source));
        let accepted = matches!(target, 32..=126 | 160..=255);
        assert_eq!(result.is_ok(), accepted, "{target:04x}");
        if accepted {
            assert_eq!(result.unwrap()[&257], target as u8);
        }
    }
    let source = map("2 beginbfrange <0100> <015e> <0020> <0200> <025f> <00a0> endbfrange");
    let result = parse_cid(&stream(&source)).unwrap();
    assert_eq!(result.len(), 191);
    assert_eq!(
        result.values().copied().collect::<Vec<_>>(),
        (32..=126).chain(160..=255).collect::<Vec<u8>>()
    );
    for body in [
        "1 beginbfrange <0100> <0101> <00ff> endbfrange",
        "1 beginbfrange <0100> <0180> <007e> endbfrange",
        "1 beginbfrange <0100> <0101> <009f> endbfrange",
        "2 beginbfchar <0100> <00e4> <0101> <00e4> endbfchar",
        "2 beginbfrange <0100> <0101> <00e4> <0200> <0201> <00e5> endbfrange",
    ] {
        assert!(parse_cid(&stream(&map(body))).is_err(), "{body}");
    }
}

#[test]
fn textedit_cid_mapping_rejects_ambiguity_expansion_and_wrong_width() {
    for (from, to) in [
        ("<0102>", "<0101>"),
        ("<0042>", "<0041>"),
        ("<0101>", "<01>"),
        ("<0041>", "<41>"),
        ("<0041>", "<00410042>"),
        ("<0041>", "<d800>"),
        ("2 beginbfchar", "1 beginbfchar"),
        ("2 beginbfchar", "101 beginbfchar"),
        ("<FFFF>", "<FFFE>"),
        ("/CMapType 2", "/CMapType 1"),
        ("endbfchar", "endbfchar /Other usecmap"),
    ] {
        assert!(
            parse_cid(&stream(&wide_map().replace(from, to))).is_err(),
            "{from} -> {to}"
        );
    }
    for range in [
        "<0001> <ffff> <0041>",
        "<0102> <0101> <0041>",
        "<0101> <0102> <007e>",
        "<0101> <0102> [<0041> <0042>]",
        "<0101> <0102> <001f>",
        "<0101> <0102> <00410042>",
    ] {
        let map = wide_map().replace(
            "2 beginbfchar\n<0101> <0041>\n<0102> <0042>\nendbfchar",
            &format!("1 beginbfrange\n{range}\nendbfrange"),
        );
        assert!(parse_cid(&stream(&map)).is_err(), "{range}");
    }
    let mut inherited = stream(&wide_map());
    inherited.dict.set("UseCMap", "Identity-H");
    assert!(parse_cid(&inherited).is_err());
    let mut large = stream(&(wide_map() + &" ".repeat(MAX_MAP)));
    large.compress().unwrap();
    assert!(parse_cid(&large).is_err());
}

#[test]
fn textedit_mapped_dash_uses_unicode_targets_not_low_bytes() {
    for code in 0..=255 {
        for body in [
            format!("1 beginbfchar <{code:02x}> <2013> endbfchar"),
            format!("1 beginbfrange <{code:02x}> <{code:02x}> <2013> endbfrange"),
        ] {
            let map = parse(&stream(&single_body(&body))).unwrap();
            assert_eq!(map[code], Some(0x96));
            assert_eq!(map.iter().flatten().count(), 1);
        }
    }
    for body in [
        "1 beginbfrange <01> <02> <2013> endbfrange",
        "1 beginbfrange <01> <02> <2012> endbfrange",
        "1 beginbfrange <01> <02> <ffff> endbfrange",
        "2 beginbfchar <01> <2013> <02> <2013> endbfchar",
        "2 beginbfchar <01> <2013> <02> <0096> endbfchar",
        "1 beginbfchar <01> <0113> endbfchar",
        "1 beginbfchar <01> <20130041> endbfchar",
    ] {
        assert!(parse(&stream(&single_body(body))).is_err(), "{body}");
    }
    let cid = |body: &str| stream(&single_body(body).replace("<00> <FF>", "<0000> <FFFF>"));
    for body in [
        "1 beginbfchar <ffff> <2013> endbfchar",
        "1 beginbfrange <ffff> <ffff> <2013> endbfrange",
    ] {
        assert_eq!(parse_cid(&cid(body)).unwrap().get(&65535), Some(&0x96));
    }
    for body in [
        "1 beginbfchar <0001> <0096> endbfchar",
        "1 beginbfrange <0001> <0002> <2013> endbfrange",
        "1 beginbfrange <0001> <ffff> <0020> endbfrange",
        "1 beginbfrange <0001> <0002> <ffff> endbfrange",
        "2 beginbfchar <0001> <2013> <0002> <2013> endbfchar",
    ] {
        assert!(parse_cid(&cid(body)).is_err(), "{body}");
    }
}
