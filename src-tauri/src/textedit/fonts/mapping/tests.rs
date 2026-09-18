use super::*;
use lopdf::Dictionary;

// Spelling and adjacency match the producer, independently of parser constants.
const MAP: &str = "/CIDInit/ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName/Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<01> <0041>\n<02> <0042>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
fn stream(text: &str) -> Stream {
    Stream::new(Dictionary::new(), text.as_bytes().to_vec())
}

#[test]
fn textedit_mapping_dictionary_capacity_is_bounded_but_does_not_change_codes() {
    let expected = parse(&stream(MAP)).unwrap();
    for capacity in [1, 18, 256] {
        assert_eq!(
            parse(&stream(
                &MAP.replace("12 dict", &format!("{capacity} dict"))
            ))
            .unwrap(),
            expected
        );
    }
    for capacity in ["0", "-1", "257", "1.5", "(18)"] {
        assert!(parse(&stream(
            &MAP.replace("12 dict", &format!("{capacity} dict"))
        ))
        .is_err());
    }
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
fn unicode_cid_preserves_scalars_and_refuses_ambiguous_or_overflowing_maps() {
    let map = |body: &str| {
        wide_map().replace(
            "2 beginbfchar\n<0101> <0041>\n<0102> <0042>\nendbfchar",
            body,
        )
    };
    let valid = unicode_cid(&stream(&map(
        "2 beginbfchar <0101> <4e00> <0102> <d840dc00> endbfchar",
    )))
    .unwrap();
    assert_eq!(valid[&0x101], "\u{4e00}");
    assert_eq!(valid[&0x102], "\u{20000}");
    // A glyph may stand for up to three letters (Calibri's ft and Th).
    let letters = unicode_cid(&stream(&map(
        "2 beginbfchar <0101> <00660074> <0102> <005400680065> endbfchar",
    )))
    .unwrap();
    assert_eq!(letters[&0x101], "ft");
    assert_eq!(letters[&0x102], "The");
    // Two codes may share a text (a small capital and its capital); both read
    // as it, and `unicode::Metrics` writes neither unless a run chooses.
    let shared = unicode_cid(&stream(&map(
        "2 beginbfchar <0101> <4e00> <0102> <4e00> endbfchar",
    )))
    .unwrap();
    assert_eq!(
        (shared[&0x101].as_str(), shared[&0x102].as_str()),
        ("\u{4e00}", "\u{4e00}")
    );
    for invalid in [
        "2 beginbfchar <0101> <4e00> <0101> <4e01> endbfchar",
        "1 beginbfchar <0101> <d840> endbfchar",
        "1 beginbfchar <0101> <000a> endbfchar",
        "1 beginbfchar <0101> <00410031> endbfchar",
        "1 beginbfchar <0101> <00410020> endbfchar",
        "1 beginbfchar <0101> <0041004200430044> endbfchar",
        "1 beginbfrange <0101> <0102> <ffff> endbfrange",
        "1 beginbfrange <0000> <1000> <4e00> endbfrange",
    ] {
        assert!(unicode_cid(&stream(&map(invalid))).is_err(), "{invalid}");
    }
}

// ConTeXt names a composite font's ToUnicode after the font, like pdfTeX:
// the labels change no entry, so the Unicode path reads the same two codes.
#[test]
fn unicode_cid_accepts_the_cmap_labels_context_writes() {
    let labelled = |info: &str, name: &str| {
        wide_map()
            .replace(
                "<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >>",
                info,
            )
            .replace("/CMapName/Adobe-Identity-UCS def", name)
    };
    let map = unicode_cid(&stream(&labelled(
        "<< /Registry (TeX) /Ordering (ARAAYN-DejaVuSerif-Bold) /Supplement 0 >>",
        "/CMapName /TeX-Identity-ARAAYN-DejaVuSerif-Bold def",
    )))
    .unwrap();
    assert_eq!((map[&0x101].as_str(), map[&0x102].as_str()), ("A", "B"));
    for (info, name) in [
        (
            "<< /Registry (TeX) /Ordering (X) /Supplement 0 /Extra 1 >>",
            "/CMapName /X def",
        ),
        (
            "<< /Registry (TeX) /Ordering (X) /Supplement 0 >>",
            "/CMapName (X) def",
        ),
        ("<< /Registry (TeX) /Ordering (X) >>", "/CMapName /X def"),
    ] {
        assert!(
            unicode_cid(&stream(&labelled(info, name))).is_err(),
            "{info} {name}"
        );
    }
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
        ("/CMapType 2", "/CMapType 3"),
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

#[test]
fn textedit_cff_unicode_mapping_stays_bounded_and_font_specific() {
    for (target, slot) in [
        (0xa0, 0xa0),
        (0xa3, 0xa3),
        (0x2018, 0x91),
        (0x2019, 0x92),
        (0x2212, 0x80),
    ] {
        for code in [0, 26, 128, 255] {
            let body = format!("1 beginbfchar <{code:02x}> <{target:04x}> endbfchar");
            let map = stream(&single_body(&body));
            assert_eq!(parse_cff(&map).unwrap()[code], Some(slot));
            assert!(parse(&map).is_err());
            if target > 255 {
                let cid = format!("1 beginbfchar <{code:04x}> <{target:04x}> endbfchar");
                assert!(parse_cid(&stream(
                    &single_body(&cid).replace("<00> <FF>", "<0000> <FFFF>")
                ))
                .is_err());
            }
        }
    }
    let pair = parse_cff(&stream(&single_body(
        "1 beginbfrange <fe> <ff> <2018> endbfrange",
    )))
    .unwrap();
    assert_eq!(&pair[254..], &[Some(0x91), Some(0x92)]);
    for body in [
        "1 beginbfrange <fd> <ff> <2018> endbfrange",
        "1 beginbfrange <ff> <fe> <2018> endbfrange",
        "1 beginbfrange <00> <ff> <ff80> endbfrange",
        "2 beginbfchar <1a> <2212> <ff> <2212> endbfchar",
        "1 beginbfchar <1a> <0080> endbfchar",
        "1 beginbfchar <1a> <0091> endbfchar",
        "1 beginbfchar <1a> <0092> endbfchar",
        "1 beginbfchar <1a> <00730074> endbfchar",
        "1 beginbfchar <1a> <fb01> endbfchar",
        "1 beginbfchar <001a> <2212> endbfchar",
    ] {
        assert!(parse_cff(&stream(&single_body(body))).is_err(), "{body}");
    }
}

#[test]
fn textedit_cff_padded_header_keeps_single_byte_sources_and_exact_wrapper() {
    let body = MAP.replace("<00> <FF>", "<0000> <FFFF>");
    let good = stream(&body);
    assert_eq!(parse_cff(&good).unwrap()[1], Some(b'A'));
    assert_eq!(parse_cff(&good).unwrap()[2], Some(b'B'));
    assert!(parse(&good).is_err());
    assert!(parse_cid(&good).is_err()); // Identity-H still requires two-byte sources.
    for (from, to) in [
        ("<0000>", "<0001>"),
        ("<FFFF>", "<00FF>"),
        ("<0000>", "<000000>"),
        ("<01>", "<0001>"),
        ("<02>", "<0102>"),
        ("2 beginbfchar", "1 beginbfchar"),
        ("/CMapType 2", "/CMapType 1"),
        ("endbfchar", "endbfchar pop"),
    ] {
        assert!(
            parse_cff(&stream(&body.replace(from, to))).is_err(),
            "{from} -> {to}"
        );
    }
    let mut inherited = good;
    inherited.dict.set("UseCMap", "Identity-H");
    assert!(parse_cff(&inherited).is_err());
}

#[test]
fn textedit_cid_ligature_mapping_accepts_only_unique_exact_sequences() {
    let body = |entries: &str| {
        wide_map().replace(
            "2 beginbfchar\n<0101> <0041>\n<0102> <0042>\nendbfchar",
            entries,
        )
    };
    for (target, slot) in [
        ("006600660069", 1),
        ("00660066", 2),
        ("00660069", 3),
        ("0066006c", 4),
    ] {
        let map = body(&format!(
            "2 beginbfchar <0101> <{target}> <0102> <0066> endbfchar"
        ));
        let parsed = parse_cid(&stream(&map)).unwrap();
        assert_eq!(parsed[&257], slot);
        assert_eq!(parsed[&258], b'f');
        for invalid in [
            format!("2 beginbfchar <0101> <{target}> <0101> <{target}> endbfchar"),
            format!("2 beginbfchar <0101> <0066> <0101> <{target}> endbfchar"),
            format!("2 beginbfchar <0101> <{target}> <0102> <{target}> endbfchar"),
            format!("1 beginbfrange <0101> <0102> <{target}> endbfrange"),
        ] {
            assert!(parse_cid(&stream(&body(&invalid))).is_err());
        }
    }
    for target in [
        "", "006600", "00730074", "00410042", "fb01", "fb03", "0001", "009f", "d800dc00",
    ] {
        let map = body(&format!("1 beginbfchar <0101> <{target}> endbfchar"));
        assert!(parse_cid(&stream(&map)).is_err(), "{target}");
    }
}

// pdfTeX names the CMap and its character collection after the TeX encoding,
// maps the whole encoding and sends two codes to one hyphen.
const TEX: &str = "%!PS-Adobe-3.0 Resource-CMap\n%%DocumentNeededResources: ProcSet (CIDInit)\n%%EndComments\n/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo\n<< /Registry (TeX)\n/Ordering (LinLibertineT-tlf-t1)\n/Supplement 0\n>> def\n/CMapName /TeX-LinLibertineT-tlf-t1-0 def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfrange\n<41> <43> <0041>\n<C0> <C1> <00C0>\nendbfrange\n8 beginbfchar\n<1C> <00660066006C>\n<1B> <00660069>\n<20> <2423>\n<2D> <002D>\n<7F> <002D>\n<80> <0102>\n<81> <2212>\n<82> <00AD>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n%%EndResource\n%%EOF\n";

#[test]
fn textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code() {
    let codes = parse_names(&stream(TEX)).unwrap();
    assert_eq!(
        &codes[0x41..=0x43],
        &[Some(Some(b'A')), Some(Some(b'B')), Some(Some(b'C'))]
    );
    assert_eq!(&codes[0xC0..=0xC1], &[Some(Some(0xC0)), Some(Some(0xC1))]);
    // Ligatures by their sequence, both hyphens, and the minus slot.
    let ffl = super::super::ligatures::GLYPHS
        .iter()
        .find(|g| g.1 == "ffl")
        .unwrap()
        .2;
    let fi = super::super::ligatures::GLYPHS
        .iter()
        .find(|g| g.1 == "fi")
        .unwrap()
        .2;
    assert_eq!(codes[0x1C], Some(Some(ffl)));
    assert_eq!(codes[0x1B], Some(Some(fi)));
    assert_eq!(
        (codes[0x2D], codes[0x7F]),
        (Some(Some(b'-')), Some(Some(b'-')))
    );
    assert_eq!(codes[0x81], Some(Some(0x80)));
    // Mapped but outside the repertoire: the visible space and A-breve.
    assert_eq!((codes[0x20], codes[0x80]), (Some(None), Some(None)));
    // The soft hyphen's meaning is ambiguous, so it is never offered.
    assert_eq!(codes[0x82], Some(None));
    assert_eq!(codes[0x44], None);
    // The strict single-byte parser refuses the same map outright.
    assert!(parse(&stream(TEX)).is_err());
    for broken in [
        // A second mapping for one code, a range past U+FFFF, a control target.
        TEX.replace("<7F> <002D>", "<41> <002D>"),
        TEX.replace("<C0> <C1> <00C0>", "<C0> <C1> <FFFF>"),
        TEX.replace("<80> <0102>", "<80> <0009>")
            .replace("<81> <2212>", "<81> <0008>"),
        // Labels of another shape: an extra key, a string name, no Supplement.
        TEX.replace("/Supplement 0", "/Supplement 0 /Extra 1"),
        TEX.replace(
            "/CMapName /TeX-LinLibertineT-tlf-t1-0 def",
            "/CMapName (TeX) def",
        ),
        TEX.replace("/Supplement 0\n", ""),
        // A different operation in the wrapper.
        TEX.replace("/CMapType 2 def", "/CMapType 3 def"),
        TEX.replace("<00> <FF>", "<00> <7F>"),
    ] {
        let result = parse_names(&stream(&broken));
        // A control target maps to a code that is simply not offered.
        if broken.contains("<0009>") {
            let codes = result.unwrap();
            assert_eq!((codes[0x80], codes[0x81]), (Some(None), Some(None)));
            continue;
        }
        assert!(result.is_err(), "{broken}");
    }
}

// Typst's ToUnicode, verbatim but for its entries: DSC comments, a system info
// built as a PostScript dictionary, and a version and writing mode.
const TYPST: &str = "%!PS-Adobe-3.0 Resource-CMap
%%DocumentNeededResources: procset CIDInit
%%IncludeResource: procset CIDInit
%%BeginResource: CMap Custom
%%Title: (Custom Adobe Identity 0)
%%Version: 1
%%EndComments
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo 3 dict dup begin
    /Registry (Adobe) def
    /Ordering (Identity) def
    /Supplement 0 def
end def
/CMapName /Custom def
/CMapVersion 1 def
/CMapType 0 def
/WMode 0 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0008> <0020>
<0001> <0045>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
%%EndResource
%%EOF";

#[test]
fn unicode_cid_accepts_the_cmap_typst_writes() {
    let map = unicode_cid(&stream(TYPST)).unwrap();
    assert_eq!((map[&8].as_str(), map[&1].as_str()), (" ", "E"));
}

// The labels between begincmap and the code space: any order, each once, and
// the system info either literal or built as a dictionary. Nothing else.
#[test]
fn unicode_cid_labels_are_a_closed_grammar() {
    let start = TYPST.find("/CIDSystemInfo 3").unwrap();
    let end = TYPST.find("1 begincodespacerange").unwrap();
    let header = |labels: &str| TYPST.replace(&TYPST[start..end], &format!("{labels}\n"));
    let info = "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def";
    let dict = "/CIDSystemInfo 3 dict dup begin /Registry (A) def /Ordering (B) def \
                /Supplement 0 def end def";
    for labels in [
        String::new(),
        format!("/CMapName /X def /CMapType 2 def {info}"),
        format!("{info} /CMapName /X def /CMapType 2 def"),
        format!("{dict} /CMapName /X def /CMapVersion 1.5 def /WMode 0 def"),
        dict.replace(
            "/Registry (A) def /Ordering (B) def",
            "/Ordering (B) def /Registry (A) def",
        ),
    ] {
        assert!(unicode_cid(&stream(&header(&labels))).is_ok(), "{labels}");
    }
    for labels in [
        "/WMode 1 def".to_string(),
        "/CMapType 3 def".into(),
        "/CMapName (X) def".into(),
        "/CMapName /X def /CMapName /Y def".into(),
        format!("{info} {dict}"),
        "/Other 1 def".into(),
        "/CMapName /X".into(),
        "/CMapName /X def pop".into(),
        dict.replace(" dup", ""),
        dict.replace("dup begin", "begin dup"),
        dict.replace(" /Supplement 0 def", ""),
        dict.replace("/Supplement 0 def", "/Supplement 0 def /Extra 1 def"),
        dict.replace("/Ordering (B)", "/Registry (B)"),
        dict.replace("end def", "end"),
        dict.replace("3 dict", "9 dict"),
        format!("{info} /CMapName /X def usecmap"),
    ] {
        assert!(unicode_cid(&stream(&header(&labels))).is_err(), "{labels}");
    }
}
