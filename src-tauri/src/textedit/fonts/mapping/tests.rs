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
