// CID-keyed CFF programs built in the test: CIDs that are not glyph indices,
// widths stated relative to nominalWidthX, and a second glyph for one letter.
use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

// Text on the fixture page, each character's CID. A small capital S (200)
// shares the text "S" with the capital (101).
const CIDS: [(char, u16); 13] = [
    ('S', 101),
    ('Y', 102),
    ('N', 103),
    ('T', 104),
    ('H', 105),
    ('E', 106),
    ('I', 107),
    ('C', 108),
    ('F', 109),
    ('R', 110),
    ('O', 111),
    ('D', 112),
    (' ', 113),
];
const SMALL_S: u16 = 200;
const NOMINAL: i32 = 100;

// A Type 2 operand.
fn n(value: i32) -> Vec<u8> {
    match value {
        -107..=107 => vec![(value + 139) as u8],
        _ => {
            let mut bytes = vec![28];
            bytes.extend((value as i16).to_be_bytes());
            bytes
        }
    }
}

// A 400 by 700 box, or nothing, with its width before the first operator.
fn glyph(width: i32, ink: bool) -> Vec<u8> {
    let mut bytes = n(width - NOMINAL);
    if ink {
        bytes.extend([n(0), n(0), vec![21]].concat());
        bytes.extend([n(0), n(700), n(400), n(0), n(0), n(-700), vec![5]].concat());
    }
    bytes.push(14);
    bytes
}

// A DICT integer, always five bytes so offsets can be laid out in two passes.
fn int(value: i32) -> Vec<u8> {
    let mut bytes = vec![29];
    bytes.extend(value.to_be_bytes());
    bytes
}

fn index(items: &[&[u8]]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(4);
    let mut offset = 1_u32;
    out.extend(offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend(offset.to_be_bytes());
    }
    for item in items {
        out.extend(*item);
    }
    out
}

#[derive(Clone)]
struct Spec {
    // (CID, charstring) in glyph order; glyph 0 is `.notdef`.
    glyphs: Vec<(u16, Vec<u8>)>,
    top: Vec<u8>,
    fd: Vec<u8>,
    private: Vec<u8>,
    select: Option<Vec<u8>>,
    registry: &'static [u8],
    // A third string, SID 393, for a Top DICT entry to name.
    string: Option<&'static [u8]>,
}

impl Spec {
    fn new() -> Self {
        // .notdef at the PDF width, so only its CID keeps it read-only.
        let mut glyphs = vec![(0, glyph(600, true))];
        for (ch, cid) in CIDS {
            glyphs.push((cid, glyph(600, ch != ' ')));
        }
        glyphs.push((SMALL_S, glyph(600, true)));
        Self {
            glyphs,
            top: Vec::new(),
            fd: Vec::new(),
            private: [int(500), vec![20], int(NOMINAL), vec![21]].concat(),
            select: None,
            registry: b"Adobe",
            string: None,
        }
    }

    fn build(&self) -> Vec<u8> {
        let assemble = |at: [usize; 5]| {
            let mut top = [int(391), int(392), int(0), vec![12, 30]].concat();
            top.extend([int(at[0] as i32), vec![15], int(at[2] as i32), vec![17]].concat());
            top.extend(
                [
                    int(at[3] as i32),
                    vec![12, 36],
                    int(at[1] as i32),
                    vec![12, 37],
                ]
                .concat(),
            );
            top.extend(&self.top);
            let mut out = vec![1, 0, 4, 4];
            out.extend(index(&[b"TPDFCID"]));
            out.extend(index(&[&top]));
            let mut strings = vec![self.registry, b"Identity".as_slice()];
            strings.extend(self.string);
            out.extend(index(&strings));
            out.extend(index(&[]));
            let mut pos = [0; 5];
            pos[0] = out.len();
            out.push(0);
            for (cid, _) in &self.glyphs[1..] {
                out.extend(cid.to_be_bytes());
            }
            pos[1] = out.len();
            out.extend(self.select.clone().unwrap_or_else(|| {
                let mut select = vec![3, 0, 1, 0, 0, 0];
                select.extend((self.glyphs.len() as u16).to_be_bytes());
                select
            }));
            pos[2] = out.len();
            let charstrings: Vec<&[u8]> = self.glyphs.iter().map(|(_, c)| c.as_slice()).collect();
            out.extend(index(&charstrings));
            pos[3] = out.len();
            let fd = [
                int(self.private.len() as i32),
                int(at[4] as i32),
                vec![18],
                self.fd.clone(),
            ]
            .concat();
            out.extend(index(&[&fd]));
            pos[4] = out.len();
            out.extend(&self.private);
            (out, pos)
        };
        let (_, pos) = assemble([0; 5]);
        let (out, again) = assemble(pos);
        assert_eq!(pos, again);
        out
    }
}

fn cid(ch: char) -> u16 {
    CIDS.iter().find(|(c, _)| *c == ch).unwrap().1
}

fn hex(codes: &[u16]) -> String {
    codes.iter().map(|code| format!("{code:04X}")).collect()
}

fn encode(text: &str) -> Vec<u16> {
    text.chars().map(cid).collect()
}

fn to_unicode() -> String {
    let mut entries: Vec<String> = CIDS
        .iter()
        .map(|(ch, cid)| format!("<{cid:04X}> <{:04X}>", u32::from(*ch)))
        .collect();
    entries.push(format!("<{SMALL_S:04X}> <0053>"));
    format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /X def \
         /CMapType 2 def /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> \
         def 1 begincodespacerange <0000> <FFFF> endcodespacerange {} beginbfchar {} endbfchar \
         endcmap CMapName currentdict /CMap defineresource pop end end",
        entries.len(),
        entries.join(" ")
    )
}

// The two runs, each a list of CIDs; ids are [font, child, descriptor, program].
fn document(spec: &Spec, first: &[u16], second: &[u16]) -> (Document, [ObjectId; 4]) {
    let mut doc = textedit::tests::fixture();
    let program = doc.add_object(Stream::new(
        dictionary! { "Subtype" => "CIDFontType0C" },
        spec.build(),
    ));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "TPDFCID", "Flags" => 4,
        "FontBBox" => vec![0.into(), 0.into(), 400.into(), 700.into()],
        "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200,
        "CapHeight" => 700, "StemV" => 100, "FontFile3" => program
    });
    let child = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType0", "BaseFont" => "TPDFCID",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
        "FontDescriptor" => descriptor, "DW" => 600
    });
    let mapping = doc.add_object(Stream::new(Dictionary::new(), to_unicode().into_bytes()));
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "TPDFCID-Identity-H",
        "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(child)],
        "ToUnicode" => mapping
    });
    let content = format!(
        "BT /F1 12 Tf 40 180 Td <{}> Tj ET\nBT /F1 12 Tf 40 140 Td <{}> Tj ET",
        hex(first),
        hex(second)
    );
    let contents = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    for page in crate::pagetree::ordered_pages(&doc) {
        let page = doc.get_dictionary_mut(page).unwrap();
        page.set("Contents", contents);
        page.set(
            "Resources",
            dictionary! { "Font" => dictionary! { "F1" => font } },
        );
    }
    (doc, [font, child, descriptor, program])
}

fn standard(spec: &Spec) -> (Document, [ObjectId; 4]) {
    document(
        spec,
        &encode("SYNTHETIC FIRST"),
        &encode("SYNTHETIC SECOND"),
    )
}

fn texts(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .map(|run| run.text)
        .collect()
}

fn change(doc: &Document, index: usize, replacement: &str) -> Change {
    let runs = textedit::scan(doc, 0).unwrap();
    Change {
        layout: None,
        page: 0,
        operator: runs.runs[index].operator,
        revision: runs.revision,
        original: runs.runs[index].text.clone(),
        replacement: replacement.into(),
    }
}

// The codes each show writes, read back from the saved content.
fn shown(doc: &Document) -> Vec<Vec<u16>> {
    let page = crate::pagetree::ordered_pages(doc)[0];
    let content = lopdf::content::Content::decode(&doc.get_page_content(page)).unwrap();
    content
        .operations
        .iter()
        .filter(|op| op.operator == "Tj" || op.operator == "TJ")
        .map(|op| {
            let bytes: Vec<u8> = match &op.operands[0] {
                Object::String(bytes, _) => bytes.clone(),
                Object::Array(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().ok())
                    .flatten()
                    .copied()
                    .collect(),
                _ => panic!("unexpected show operand"),
            };
            bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect()
        })
        .collect()
}

#[test]
fn textedit_cid_keyed_cff_maps_codes_through_its_charset() {
    let (mut doc, ids) = standard(&Spec::new());
    assert_eq!(texts(&doc), ["SYNTHETIC FIRST", "SYNTHETIC SECOND"]);
    let before: Vec<_> = ids.iter().map(|id| doc.objects[id].clone()).collect();
    let edit = change(&doc, 0, "SYNTHETIC FIRS");
    textedit::write(&mut doc, &[edit]).unwrap();
    assert_eq!(texts(&doc), ["SYNTHETIC FIRS", "SYNTHETIC SECOND"]);
    assert_eq!(shown(&doc)[0], encode("SYNTHETIC FIRS"));
    for (id, object) in ids.iter().zip(before) {
        assert_eq!(doc.objects[id], object, "font objects are kept");
    }
}

// PDF widths position glyphs; the program's own widths must agree for a
// glyph to be written. One that does not reads as read-only text.
#[test]
fn textedit_cid_widths_follow_nominal_width_and_disagreement_is_read_only() {
    let (mut doc, [_, child, _, _]) = standard(&Spec::new());
    doc.get_dictionary_mut(child).unwrap().set(
        "W",
        vec![
            Object::Integer(i64::from(cid('F'))),
            vec![Object::Real(600.5)].into(),
        ],
    );
    assert_eq!(texts(&doc), ["SYNTHETIC FIRST", "SYNTHETIC SECOND"]);
    doc.get_dictionary_mut(child).unwrap().set(
        "W",
        vec![
            Object::Integer(i64::from(cid('F'))),
            vec![Object::Integer(700)].into(),
        ],
    );
    assert_eq!(texts(&doc), ["SYNTHETIC SECOND"]);
    // A width of the private dict's defaultWidthX: no stated width at all.
    let mut spec = Spec::new();
    let at = spec
        .glyphs
        .iter()
        .position(|(c, _)| *c == cid('F'))
        .unwrap();
    spec.glyphs[at].1 = [
        n(0),
        n(0),
        vec![21],
        n(0),
        n(700),
        n(400),
        n(0),
        n(0),
        n(-700),
        vec![5, 14],
    ]
    .concat();
    let (doc, _) = standard(&spec);
    assert_eq!(texts(&doc), ["SYNTHETIC SECOND"]);
    let (mut doc, [_, child, _, _]) = standard(&spec);
    doc.get_dictionary_mut(child).unwrap().set(
        "W",
        vec![
            Object::Integer(i64::from(cid('F'))),
            vec![Object::Integer(500)].into(),
        ],
    );
    assert_eq!(texts(&doc), ["SYNTHETIC FIRST", "SYNTHETIC SECOND"]);
}

// xdvipdfmx maps CID 0 to U+FFFF and may show it; its ToUnicode ranges also
// name CIDs the subset left out.
#[test]
fn textedit_cid_notdef_is_read_only_and_absent_cids_are_not_offered() {
    let mut first = encode("SYNTHETIC FIRST");
    first.insert(3, 0);
    let (mut doc, [font, _, _, _]) = document(&Spec::new(), &first, &encode("SYNTHETIC SECOND"));
    let mapping = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"ToUnicode")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = String::from_utf8(stream.content.clone())
        .unwrap()
        .replace(
            "14 beginbfchar",
            "16 beginbfchar <0000> <FFFF> <0FFF> <0058>",
        )
        .into_bytes();
    assert_eq!(texts(&doc), ["SYNTHETIC SECOND"]);
    let edit = change(&doc, 0, "SYNTHETIC X");
    assert_eq!(
        textedit::write(&mut doc, &[edit]).unwrap_err(),
        "the font has no validated glyph for this character"
    );
}

// Two glyphs read as "S". A replacement writes the one its run already
// shows; a run showing both, or neither, cannot choose.
#[test]
fn textedit_shared_texts_take_the_glyph_their_run_shows() {
    let small = |text: &str| -> Vec<u16> {
        text.chars()
            .map(|ch| if ch == 'S' { SMALL_S } else { cid(ch) })
            .collect()
    };
    let (mut doc, _) = document(
        &Spec::new(),
        &encode("SYNTHETIC FIRST"),
        &small("SYNTHETIC SECOND"),
    );
    assert_eq!(texts(&doc), ["SYNTHETIC FIRST", "SYNTHETIC SECOND"]);
    let edits = [change(&doc, 0, "SITS"), change(&doc, 1, "SONS")];
    textedit::write(&mut doc, &edits).unwrap();
    assert_eq!(shown(&doc), [encode("SITS"), small("SONS")]);

    let mut mixed = encode("SYNTHETIC SECOND");
    mixed[10] = SMALL_S;
    let (mut doc, _) = document(&Spec::new(), &encode("TIE"), &mixed);
    for index in [0, 1] {
        let edit = change(&doc, index, "SIT");
        assert_eq!(
            textedit::write(&mut doc, &[edit]).unwrap_err(),
            "the font has several glyphs for this character and cannot choose one"
        );
    }
}

#[test]
fn textedit_cid_programs_refuse_what_they_cannot_state() {
    let refused = |spec: &Spec| parse(&spec.build()).map(|_| ()).unwrap_err();
    assert!(parse(&Spec::new().build()).is_ok());
    let mut cases: Vec<(&str, Spec)> = Vec::new();
    let mut spec = Spec::new();
    spec.registry = b"TeX";
    cases.push(("registry", spec));
    for (label, top) in [
        (
            "matrix",
            [int(2), int(0), int(0), int(2), int(0), int(0), vec![12, 7]].concat(),
        ),
        ("encoding", [int(0), vec![16]].concat()),
        ("paint", [int(2), vec![12, 5]].concat()),
        ("missing string", [int(393), vec![12, 21]].concat()),
        ("count", [int(150), vec![12, 34]].concat()),
        ("synthetic", [int(1), vec![12, 20]].concat()),
    ] {
        let mut spec = Spec::new();
        spec.top = top;
        cases.push((label, spec));
    }
    let mut spec = Spec::new();
    spec.fd = [int(1), int(0), int(0), int(1), int(0), int(0), vec![12, 7]].concat();
    assert!(parse(&spec.build()).is_ok(), "identity FD matrix");
    spec.fd = [int(2), int(0), int(0), int(2), int(0), int(0), vec![12, 7]].concat();
    cases.push(("fd matrix", spec));
    let mut spec = Spec::new();
    spec.private.extend([int(1), vec![12, 30]].concat());
    cases.push(("private", spec));
    let mut spec = Spec::new();
    spec.select = Some(vec![3, 0, 1, 0, 0, 1, 0, 15]);
    cases.push(("select dict", spec));
    let mut spec = Spec::new();
    spec.select = Some(vec![3, 0, 1, 0, 1, 0, 0, 15]);
    cases.push(("select start", spec));
    let mut spec = Spec::new();
    spec.select = Some(vec![3, 0, 1, 0, 0, 0, 0, 14]);
    cases.push(("select short", spec));
    let mut spec = Spec::new();
    spec.select = Some(vec![1]);
    cases.push(("select format", spec));
    let mut spec = Spec::new();
    spec.glyphs[2].0 = spec.glyphs[1].0;
    cases.push(("duplicate cid", spec));
    let mut spec = Spec::new();
    spec.glyphs[1].0 = 0;
    cases.push(("a second CID 0", spec));
    for (label, spec) in cases {
        assert_eq!(refused(&spec), INVALID, "{label}");
    }
    let mut spec = Spec::new();
    spec.select = Some(vec![0; 16]);
    assert!(parse(&spec.build()).is_ok(), "format 0");
    // A CID count covering every CID, and the default one.
    let mut spec = Spec::new();
    spec.top = [int(201), vec![12, 34]].concat();
    assert!(parse(&spec.build()).is_ok());
    let bytes = Spec::new().build();
    for len in 0..bytes.len() {
        assert!(parse(&bytes[..len]).is_err(), "prefix {len}");
    }
}

// A CID-keyed program whose rights forbid editing is refused, so its text
// stays read-only: only simple fonts are read for replacement in Noto.
#[test]
fn textedit_cid_cff_refuses_restricted_rights() {
    for (rights, allowed) in [
        (b"/FSType 8 def".as_slice(), true),
        (b"/FSType 0 def", true),
        (b"/FSType 4 def", false),
        (b"/FSType 2 def", false),
    ] {
        let mut spec = Spec::new();
        spec.string = Some(rights);
        spec.top = [int(393), vec![12, 21]].concat();
        assert_eq!(parse(&spec.build()).is_ok(), allowed, "{rights:?}");
    }
}

// The composite dictionary around the program: a CIDToGIDMap selects nothing
// in a CFF font and is refused, as is any other FontFile3 subtype.
#[test]
fn textedit_cid_font_dictionaries_refuse_other_carriers() {
    let (mut doc, [_, child, descriptor, program]) = standard(&Spec::new());
    doc.get_dictionary_mut(child)
        .unwrap()
        .set("CIDToGIDMap", "Identity");
    assert!(textedit::scan(&doc, 0).is_err());
    let (mut doc, _) = standard(&Spec::new());
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Subtype", "Type1C");
    assert!(textedit::scan(&doc, 0).is_err());
    let (mut doc, _) = standard(&Spec::new());
    doc.get_dictionary_mut(descriptor)
        .unwrap()
        .set("FontFile2", program);
    assert!(textedit::scan(&doc, 0).is_err());
    // ForceBold is a hint for the whole glyph set; contradictory flags are not.
    let (mut doc, _) = standard(&Spec::new());
    doc.get_dictionary_mut(descriptor)
        .unwrap()
        .set("Flags", 4 | 262144);
    assert_eq!(texts(&doc).len(), 2);
    doc.get_dictionary_mut(descriptor)
        .unwrap()
        .set("Flags", 4 | 32);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_cid_widths_come_before_the_first_clearing_operator() {
    let widths = (500., 100.);
    for (charstring, expected) in [
        (vec![139 + 50, 139, 139, 21, 14], Some(150.)),
        (vec![139, 139, 21, 14], Some(500.)),
        (vec![139 + 50, 139, 22, 14], Some(150.)),
        (vec![139, 22, 14], Some(500.)),
        (vec![139 + 50, 139, 4, 14], Some(150.)),
        (vec![139 + 50, 139, 139, 1, 14], Some(150.)),
        (vec![139, 139, 1, 14], Some(500.)),
        (vec![139 + 50, 139, 139, 18, 14], Some(150.)),
        (vec![139 + 50, 139, 139, 23, 14], Some(150.)),
        (vec![139 + 50, 139, 139, 3, 14], Some(150.)),
        (vec![139 + 50, 19, 0], Some(150.)),
        (vec![139, 139, 19, 0], Some(500.)),
        (vec![139 + 50, 20, 0], Some(150.)),
        (vec![139 + 50, 14], Some(150.)),
        (vec![14], Some(500.)),
        (vec![139 + 50, 139, 139, 139, 139, 14], Some(150.)),
        (vec![139, 139, 139, 139, 14], Some(500.)),
        (vec![28, 1, 0, 139, 139, 21], Some(356.)),
        (vec![255, 0, 50, 128, 0, 139, 139, 21], Some(150.5)),
        (vec![247, 0, 139, 139, 21], Some(208.)),
        (vec![251, 0, 139, 139, 21], Some(-8.)),
        // A subroutine call, an arithmetic operator or anything else first.
        (vec![139, 10], None),
        (vec![139, 29], None),
        (vec![139, 12, 3], None),
        (vec![139, 139, 139, 139, 21], None),
        (vec![139, 139, 139, 22], None),
        (vec![139, 139, 14], None),
        (vec![139, 139, 139, 14], None),
        (vec![], None),
        (vec![139, 139], None),
        (vec![28, 1], None),
        (vec![255, 0, 0], None),
        (vec![247], None),
        (vec![139; 49], None),
        ([vec![139; 49], vec![1]].concat(), None),
    ] {
        assert_eq!(width(&charstring, widths), expected, "{charstring:?}");
    }
    let mut full = vec![139; 47];
    full.push(1);
    assert_eq!(width(&full, widths), Some(100.));
}

// Typst sets a macron as a zero-width mark under `DW 0`. It is measured for
// read-only text, with its ink reserved; its program width is zero too, so
// only the rule that a zero width is never written keeps it out.
#[test]
fn textedit_zero_width_marks_are_read_only() {
    const MARK: u16 = 120;
    let mut spec = Spec::new();
    spec.glyphs.push((MARK, glyph(0, true)));
    let mut first = encode("SYNTHETIC FIRST");
    first.insert(9, MARK);
    let (mut doc, [font, child, _, _]) = document(&spec, &first, &encode("SYNTHETIC SECOND"));
    let mapping = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"ToUnicode")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = String::from_utf8(stream.content.clone())
        .unwrap()
        .replace("14 beginbfchar", "15 beginbfchar <0078> <0304>")
        .into_bytes();
    let widths = |doc: &mut Document, width: i64| {
        doc.get_dictionary_mut(child).unwrap().set(
            "W",
            vec![
                Object::Integer(i64::from(MARK)),
                vec![Object::Integer(width)].into(),
            ],
        );
    };
    widths(&mut doc, 0);
    assert_eq!(texts(&doc), ["SYNTHETIC SECOND"]);
    widths(&mut doc, -1);
    assert!(textedit::scan(&doc, 0).is_err());

    // Writable text still advances: character spacing of -3 at size 12 stops
    // a 250-unit I exactly, and that run is refused, not measured.
    let mut spec = Spec::new();
    let at = spec
        .glyphs
        .iter()
        .position(|(c, _)| *c == cid('I'))
        .unwrap();
    spec.glyphs[at].1 = glyph(250, true);
    for (spacing, stands_still) in [("-3", true), ("-2.9", false)] {
        let (mut doc, [_, child, _, _]) = standard(&spec);
        doc.get_dictionary_mut(child).unwrap().set(
            "W",
            vec![
                Object::Integer(i64::from(cid('I'))),
                vec![Object::Integer(250)].into(),
            ],
        );
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let content = doc.get_page_content(page);
        let content = [format!("{spacing} Tc ").into_bytes(), content].concat();
        let stream = doc.add_object(Stream::new(Dictionary::new(), content));
        for page in crate::pagetree::ordered_pages(&doc) {
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Contents", stream);
        }
        match textedit::scan(&doc, 0) {
            Err(error) => assert!(
                stands_still && error == "backtracking character spacing is not editable yet",
                "{spacing}: {error}"
            ),
            Ok(runs) => assert!(!stands_still && runs.runs.len() == 2, "{spacing}"),
        }
    }
}
