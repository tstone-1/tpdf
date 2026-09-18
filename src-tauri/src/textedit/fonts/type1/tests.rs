// A synthetic Type 1 font assembled here: each glyph is a charstring built
// from the operators below, encrypted as Adobe Type 1 Font Format 7 requires,
// and wrapped in the cleartext and private dictionaries pdfTeX writes.
use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Stream};

#[derive(Clone, Copy)]
enum Op {
    N(i32),
    C(u8),
    E(u8),
}
use Op::{C, E, N};

const HSBW: Op = C(13);
const RMOVETO: Op = C(21);
const RLINETO: Op = C(5);
const CLOSEPATH: Op = C(9);
const ENDCHAR: Op = C(14);
const CALLSUBR: Op = C(10);
const RETURN: Op = C(11);

fn charstring(ops: &[Op]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for op in ops {
        match *op {
            N(v @ -107..=107) => bytes.push((v + 139) as u8),
            N(v @ 108..=1131) => {
                let v = v - 108;
                bytes.extend([(v >> 8) as u8 + 247, (v & 0xff) as u8]);
            }
            N(v @ -1131..=-108) => {
                let v = -v - 108;
                bytes.extend([(v >> 8) as u8 + 251, (v & 0xff) as u8]);
            }
            N(v) => {
                bytes.push(255);
                bytes.extend(v.to_be_bytes());
            }
            C(c) => bytes.push(c),
            E(c) => bytes.extend([12, c]),
        }
    }
    bytes
}

// A filled box: left side bearing, advance, width and height.
fn rectangle(left: i32, advance: i32, width: i32, height: i32) -> Vec<Op> {
    vec![
        N(left),
        N(advance),
        HSBW,
        N(0),
        N(0),
        RMOVETO,
        N(width),
        N(0),
        RLINETO,
        N(0),
        N(height),
        RLINETO,
        N(-width),
        N(0),
        RLINETO,
        CLOSEPATH,
        ENDCHAR,
    ]
}

struct Font {
    glyphs: Vec<(String, Vec<Op>)>,
    subrs: Vec<Vec<Op>>,
    encoding: String,
    len_iv: i32,
    header: String,
    matrix: String,
    extra: String,
    // How RD, ND and NP are spelled after each binary entry.
    spelling: [&'static str; 3],
}

impl Font {
    fn new() -> Self {
        let mut glyphs = vec![(".notdef".to_string(), vec![N(0), N(250), HSBW, ENDCHAR])];
        for (index, letter) in ('A'..='Z').enumerate() {
            glyphs.push((
                letter.to_string(),
                rectangle(20, 600 + index as i32, 540, 700),
            ));
        }
        glyphs.push(("space".into(), vec![N(0), N(250), HSBW, ENDCHAR]));
        glyphs.push(("fi".into(), rectangle(20, 560, 520, 700)));
        glyphs.push(("f_i".into(), rectangle(20, 560, 520, 700)));
        glyphs.push(("summation".into(), rectangle(10, 900, 880, 1400)));
        Self {
            glyphs,
            subrs: standard_subrs(),
            encoding: "/Encoding StandardEncoding def".into(),
            len_iv: 4,
            header: "%!PS-AdobeFont-1.0: TPDFSyntheticOne 001".into(),
            matrix: "[0.001 0 0 0.001 0 0]".into(),
            extra: String::new(),
            spelling: ["RD", "ND", "NP"],
        }
    }

    fn private(&self) -> Vec<u8> {
        let encrypt = |ops: &[Op]| -> Vec<u8> {
            let mut plain = if self.len_iv < 0 {
                Vec::new()
            } else {
                vec![7; self.len_iv as usize]
            };
            plain.extend(charstring(ops));
            if self.len_iv < 0 {
                plain
            } else {
                program::encrypt(&plain, 4330)
            }
        };
        let mut out = b"dup /Private 8 dict dup begin\n/RD{string currentfile exch readstring pop}executeonly def\n/ND{noaccess def}executeonly def\n/NP{noaccess put}executeonly def\n".to_vec();
        out.extend(format!("/lenIV {} def\n", self.len_iv).as_bytes());
        out.extend(format!("/Subrs {} array\n", self.subrs.len()).as_bytes());
        for (index, subr) in self.subrs.iter().enumerate() {
            let bytes = encrypt(subr);
            out.extend(format!("dup {index} {} {} ", bytes.len(), self.spelling[0]).as_bytes());
            out.extend(bytes);
            out.extend(format!(" {}\n", self.spelling[2]).as_bytes());
        }
        out.extend(b"ND\n2 index /CharStrings ");
        out.extend(format!("{} dict dup begin\n", self.glyphs.len()).as_bytes());
        for (name, ops) in &self.glyphs {
            let bytes = encrypt(ops);
            out.extend(format!("/{name} {} {} ", bytes.len(), self.spelling[0]).as_bytes());
            out.extend(bytes);
            out.extend(format!(" {}\n", self.spelling[1]).as_bytes());
        }
        out.extend(b"end\nend\nreadonly put\nnoaccess put\ndup/FontName get exch definefont pop\nmark currentfile closefile\n");
        out
    }

    // The decoded FontFile bytes and their Length1/Length2.
    fn program(&self) -> (Vec<u8>, usize, usize) {
        self.program_with(|private| private)
    }

    fn program_with(&self, edit: impl Fn(Vec<u8>) -> Vec<u8>) -> (Vec<u8>, usize, usize) {
        let clear = format!(
            "{}\n10 dict begin\n/FontType 1 def\n/FontMatrix {} readonly def\n/FontName /TPDFSyntheticOne def\n/PaintType 0 def\n/FontInfo 2 dict dup begin\n/Notice (Synthetic (test) font) readonly def\n{}end readonly def\n{}\ncurrentdict end\ncurrentfile eexec\n",
            self.header, self.matrix, self.extra, self.encoding
        );
        let mut private = vec![1, 2, 3, 4];
        private.extend(edit(self.private()));
        let encrypted = program::encrypt(&private, 55665);
        let mut bytes = clear.as_bytes().to_vec();
        bytes.extend(&encrypted);
        (bytes, clear.len(), encrypted.len())
    }
}

// Adobe Type 1 Font Format 8.3 and 8.1: the four conventional Subrs every
// font carries for flex and hint replacement.
fn standard_subrs() -> Vec<Vec<Op>> {
    vec![
        vec![N(3), N(0), E(16), E(17), E(17), E(33), RETURN],
        vec![N(0), N(1), E(16), RETURN],
        vec![N(0), N(2), E(16), RETURN],
        vec![RETURN],
    ]
}

// Replaces the first occurrence of `from`; the private text is binary.
fn swap(bytes: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
    let at = bytes.windows(from.len()).position(|w| w == from).unwrap();
    [&bytes[..at], to, &bytes[at + from.len()..]].concat()
}

fn parse(font: &Font) -> Result<program::Program, String> {
    let (bytes, length1, length2) = font.program();
    program::parse(&bytes, length1, length2, 0)
}

fn glyph(font: &Font, name: &str) -> (f64, Option<[f64; 4]>) {
    let program = parse(font).unwrap();
    let glyph = &program.glyphs[name.as_bytes()];
    (glyph.width, glyph.bounds)
}

#[test]
fn textedit_type1_program_reads_widths_and_outline_hulls() {
    let font = Font::new();
    let program = parse(&font).unwrap();
    assert_eq!(program.glyphs.len(), font.glyphs.len());
    assert!(program.rights.is_none());
    assert!(matches!(program.encoding, program::Builtin::Standard));
    assert_eq!(glyph(&font, "A"), (600., Some([20., 0., 560., 700.])));
    assert_eq!(glyph(&font, "space"), (250., None));
    // The contour's start counts once something is drawn from it; a final
    // move that draws nothing does not.
    let mut font = Font::new();
    font.glyphs.push((
        "start".into(),
        vec![
            N(0),
            N(500),
            HSBW,
            N(0),
            N(-50),
            RMOVETO,
            N(100),
            N(50),
            RLINETO,
            N(-100),
            N(50),
            RLINETO,
            CLOSEPATH,
            N(900),
            N(900),
            RMOVETO,
            ENDCHAR,
        ],
    ));
    assert_eq!(glyph(&font, "start"), (500., Some([0., -50., 100., 50.])));
    // Every charstring encoding of a number, lenIV 0 and unencrypted
    // charstrings, and RD/ND/NP spelled -| |- |.
    let mut font = Font::new();
    font.glyphs.push((
        "numbers".into(),
        vec![
            N(-5),
            N(1131),
            HSBW,
            N(-1131),
            N(-107),
            RMOVETO,
            N(40000),
            N(-40000),
            RLINETO,
            ENDCHAR,
        ],
    ));
    for len_iv in [0, 4, -1] {
        font.len_iv = len_iv;
        assert_eq!(
            glyph(&font, "numbers"),
            (1131., Some([-1136., -40107., 38864., -107.])),
            "lenIV {len_iv}"
        );
    }
    for spelling in [["-|", "|-", "|"], ["RD", "noaccess def", "noaccess put"]] {
        let mut font = Font::new();
        font.spelling = spelling;
        assert_eq!(glyph(&font, "A").0, 600., "{spelling:?}");
    }
    for spelling in [
        ["RX", "ND", "NP"],
        ["RD", "def", "NP"],
        ["RD", "ND", "noaccess def"],
    ] {
        let mut font = Font::new();
        font.spelling = spelling;
        assert!(parse(&font).is_err(), "{spelling:?}");
    }
}

#[test]
fn textedit_type1_interpreter_follows_curves_subrs_flex_and_hints() {
    let mut font = Font::new();
    font.glyphs.push((
        "curves".into(),
        vec![
            N(10),
            N(500),
            HSBW,
            N(100),
            N(0),
            RMOVETO,
            // hvcurveto, vhcurveto, rrcurveto, hlineto, vlineto, vmoveto, hmoveto
            N(100),
            N(50),
            N(50),
            N(100),
            C(31),
            N(100),
            N(-50),
            N(50),
            N(-100),
            C(30),
            N(10),
            N(10),
            N(10),
            N(10),
            N(10),
            N(10),
            C(8),
            N(-20),
            C(6),
            N(-30),
            C(7),
            N(5),
            C(4),
            N(5),
            C(22),
            N(1),
            N(1),
            C(5),
            CLOSEPATH,
            ENDCHAR,
        ],
    ));
    let (width, bounds) = glyph(&font, "curves");
    assert_eq!(width, 500.);
    // Traced by hand: the hull's far corner is the vhcurveto's first control
    // point (260, 250) and the rrcurveto's end (140, 330).
    assert_eq!(bounds, Some([110., 0., 260., 330.]));
    // A subroutine, div, sbw, hints, dotsection and hint replacement.
    font.subrs.push(vec![N(0), N(100), RLINETO, RETURN]);
    font.subrs.push(vec![N(0), N(10), C(1), RETURN]);
    font.glyphs.push((
        "calls".into(),
        vec![
            N(0),
            N(0),
            N(400),
            N(0),
            E(7),
            N(0),
            N(10),
            C(1),
            N(0),
            N(10),
            C(3),
            N(0),
            N(1),
            N(2),
            N(3),
            N(4),
            N(5),
            E(1),
            N(0),
            N(1),
            N(2),
            N(3),
            N(4),
            N(5),
            E(2),
            E(0),
            N(5),
            N(1),
            N(3),
            E(16),
            E(17),
            CALLSUBR,
            N(300),
            N(2),
            E(12),
            N(0),
            RMOVETO,
            N(4),
            CALLSUBR,
            ENDCHAR,
        ],
    ));
    assert_eq!(glyph(&font, "calls"), (400., Some([150., 0., 150., 100.])));
    // Flex: seven points collected by the standard Subrs 1 and 2, drawn by 0.
    let mut flex = vec![N(0), N(600), HSBW, N(100), N(0), RMOVETO, N(1), CALLSUBR];
    for (dx, dy) in [
        (150, 0),
        (0, 20),
        (50, 0),
        (50, 0),
        (50, 0),
        (0, -20),
        (50, 0),
    ] {
        flex.extend([N(dx), N(dy), RMOVETO, N(2), CALLSUBR]);
    }
    // Subr 0 ends the flex at its last point, (450, 0), which then draws on.
    flex.extend([
        N(50),
        N(450),
        N(0),
        N(0),
        CALLSUBR,
        N(0),
        N(10),
        RLINETO,
        ENDCHAR,
    ]);
    font.glyphs.push(("flex".into(), flex));
    assert_eq!(glyph(&font, "flex"), (600., Some([250., 0., 450., 20.])));
    // A curve's hull is its control points' too: this one bulges to 500 and
    // ends where it began.
    font.glyphs.push((
        "bulge".into(),
        vec![
            N(0),
            N(300),
            HSBW,
            N(0),
            N(0),
            RMOVETO,
            N(0),
            N(500),
            N(100),
            N(0),
            N(0),
            N(-500),
            C(8),
            CLOSEPATH,
            ENDCHAR,
        ],
    ));
    assert_eq!(glyph(&font, "bulge"), (300., Some([0., 0., 100., 500.])));
}

#[test]
fn textedit_type1_glyphs_it_cannot_follow_are_not_offered() {
    for (name, ops) in [
        // seac, an unknown othersubr, stack overflow, recursion, no endchar,
        // drawing before hsbw, a second hsbw, a vertical advance, div by zero,
        // flex left open, a pop with nothing returned, an unknown operator.
        (
            "seac",
            vec![N(0), N(500), HSBW, N(0), N(0), N(0), N(65), N(194), E(6)],
        ),
        (
            "othersubr",
            vec![N(0), N(500), HSBW, N(0), N(12), E(16), ENDCHAR],
        ),
        ("stack", {
            let mut ops = vec![N(0), N(500), HSBW];
            ops.extend((0..25).map(N));
            ops.push(ENDCHAR);
            ops
        }),
        (
            "recursion",
            vec![N(0), N(500), HSBW, N(4), CALLSUBR, ENDCHAR],
        ),
        ("unended", vec![N(0), N(500), HSBW]),
        (
            "unset",
            vec![N(0), N(0), RLINETO, N(0), N(500), HSBW, ENDCHAR],
        ),
        (
            "twice",
            vec![N(0), N(500), HSBW, N(0), N(500), HSBW, ENDCHAR],
        ),
        ("vertical", vec![N(0), N(0), N(500), N(10), E(7), ENDCHAR]),
        ("zero", vec![N(0), N(1), N(0), E(12), HSBW, ENDCHAR]),
        ("open", vec![N(0), N(500), HSBW, N(1), CALLSUBR, ENDCHAR]),
        (
            "pop",
            vec![N(0), N(500), HSBW, E(17), N(5), RMOVETO, ENDCHAR],
        ),
        ("unknown", vec![N(0), N(500), HSBW, C(15), ENDCHAR]),
    ] {
        let mut font = Font::new();
        font.subrs.push(vec![N(4), CALLSUBR, RETURN]);
        font.glyphs.push((name.into(), ops));
        let program = parse(&font).unwrap();
        assert!(!program.glyphs.contains_key(name.as_bytes()), "{name}");
        assert!(program.glyphs.contains_key(b"A".as_slice()), "{name}");
    }
}

// Each limit with the largest accepted case beside the smallest refused one.
#[test]
fn textedit_type1_interpreter_limits_have_boundary_controls() {
    let offered =
        |font: &Font, name: &str| parse(font).unwrap().glyphs.contains_key(name.as_bytes());
    // Twenty-four arguments, reduced by div to hsbw's two; twenty-five overflow.
    for (count, accepted) in [(24, true), (25, false)] {
        let mut font = Font::new();
        let mut ops = vec![N(1); count];
        ops.extend(vec![E(12); count - 2]);
        ops.extend([HSBW, ENDCHAR]);
        font.glyphs.push(("stack".into(), ops));
        assert_eq!(offered(&font, "stack"), accepted, "{count}");
    }
    // Subroutines nest ten deep.
    for (depth, accepted) in [(10, true), (11, false)] {
        let mut font = Font::new();
        for level in 0..depth {
            font.subrs.push(if level + 1 < depth {
                vec![N(5 + level), CALLSUBR, RETURN]
            } else {
                vec![RETURN]
            });
        }
        font.glyphs.push((
            "deep".into(),
            vec![N(0), N(500), HSBW, N(4), CALLSUBR, ENDCHAR],
        ));
        assert_eq!(offered(&font, "deep"), accepted, "{depth}");
    }
    // Nine levels calling the next twice finish; three times each exceeds
    // the operation budget without ever nesting deeper.
    for (fan, accepted) in [(2, true), (3, false)] {
        let mut font = Font::new();
        for level in 0..10 {
            let mut ops = Vec::new();
            if level < 9 {
                for _ in 0..fan {
                    ops.extend([N(5 + level), CALLSUBR]);
                }
            }
            ops.push(RETURN);
            font.subrs.push(ops);
        }
        font.glyphs.push((
            "fan".into(),
            vec![N(0), N(500), HSBW, N(4), CALLSUBR, ENDCHAR],
        ));
        assert_eq!(offered(&font, "fan"), accepted, "{fan}");
    }
    // A flex is exactly seven points.
    for (points, accepted) in [(7, true), (6, false), (8, false)] {
        let mut font = Font::new();
        let mut flex = vec![N(0), N(600), HSBW, N(100), N(0), RMOVETO, N(1), CALLSUBR];
        for _ in 0..points {
            flex.extend([N(10), N(0), RMOVETO, N(2), CALLSUBR]);
        }
        flex.extend([N(50), N(100 + 10 * points), N(0), N(0), CALLSUBR, ENDCHAR]);
        font.glyphs.push(("flex".into(), flex));
        assert_eq!(offered(&font, "flex"), accepted, "{points}");
    }
}

#[test]
fn textedit_type1_program_refuses_what_it_cannot_read_literally() {
    let refused = |font: &Font| assert!(parse(font).is_err());
    for header in ["%!PS-AdobeFont-2.0: X", "%!FontType3-1.0: X", "Font"] {
        let mut font = Font::new();
        font.header = header.into();
        refused(&font);
    }
    for matrix in [
        "[0.002 0 0 0.002 0 0]",
        "[0.001 0 0.0001 0.001 0 0]",
        "[0.001 0 0 0.001 0]",
    ] {
        let mut font = Font::new();
        font.matrix = matrix.into();
        refused(&font);
    }
    for (from, to) in [
        ("/PaintType 0 def", "/PaintType 2 def"),
        ("/FontType 1 def", "/FontType 3 def"),
        ("/FontType 1 def", "/FontType 1 def\n/FontType 1 def"),
    ] {
        let (bytes, length1, length2) = Font::new().program();
        let clear = String::from_utf8(bytes[..length1].to_vec())
            .unwrap()
            .replace(from, to);
        let mut changed = clear.as_bytes().to_vec();
        changed.extend(&bytes[length1..]);
        assert!(
            program::parse(&changed, clear.len(), length2, 0).is_err(),
            "{to}"
        );
    }
    let mut font = Font::new();
    font.extra = "/FSType 4.5 def\n".into();
    refused(&font);
    for encoding in [
        "/Encoding 255 array def",
        "/Encoding 256 array dup 300 /A put readonly def",
        "/Encoding 256 array dup 65 (A) put readonly def",
        "/Encoding MacRomanEncoding def",
        "",
    ] {
        let mut font = Font::new();
        font.encoding = encoding.into();
        refused(&font);
    }
    for len_iv in [5, 16] {
        let mut font = Font::new();
        font.len_iv = len_iv;
        refused(&font);
    }
    let mut font = Font::new();
    font.glyphs.remove(0);
    refused(&font);
    let mut font = Font::new();
    font.glyphs.push(("A".into(), rectangle(0, 500, 400, 400)));
    refused(&font);
    let count = Font::new().glyphs.len();
    for edit in [
        (b"dup 1 ".to_vec(), b"dup 0 ".to_vec()),
        (b"dup 1 ".to_vec(), b"dup 9 ".to_vec()),
        (b" RD ".to_vec(), b" RD\n".to_vec()),
        (b"/CharStrings".to_vec(), b"/CharStringz".to_vec()),
        (
            format!("{count} dict dup begin").into_bytes(),
            format!("{} dict dup begin", count - 1).into_bytes(),
        ),
    ] {
        let (bytes, length1, length2) =
            Font::new().program_with(|private| swap(private, &edit.0, &edit.1));
        assert!(
            program::parse(&bytes, length1, length2, 0).is_err(),
            "{}",
            String::from_utf8_lossy(&edit.1)
        );
    }
    // A hexadecimal private part, a truncated one, and trailing content.
    let (bytes, length1, length2) = Font::new().program();
    let mut hex = bytes[..length1].to_vec();
    hex.extend(b"d9d6");
    hex.extend(&bytes[length1 + 4..]);
    assert_eq!(
        program::parse(&hex, length1, length2, 0).err().unwrap(),
        "hexadecimal Type 1 private data is not editable yet"
    );
    assert!(program::parse(&bytes, length1, length2 + 1, 0).is_err());
    assert!(program::parse(&bytes[..bytes.len() - 400], length1, length2 - 400, 0).is_err());
    let mut trailer = bytes.clone();
    trailer.extend(b"\n0000000000\ncleartomark\n");
    assert!(program::parse(&trailer, length1, length2, 0).is_ok());
    trailer.extend(b"showpage\n");
    assert!(program::parse(&trailer, length1, length2, 0).is_err());
    // A custom built-in encoding is read literally, with the .notdef loop.
    let mut font = Font::new();
    font.encoding = "/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 65 /B put\ndup 66 /A put\nreadonly def".into();
    let program::Builtin::Custom(names) = parse(&font).unwrap().encoding else {
        panic!("custom encoding");
    };
    assert_eq!(names[65].as_deref(), Some(b"B".as_slice()));
    assert_eq!(names[66].as_deref(), Some(b"A".as_slice()));
    assert_eq!(names.iter().filter(|name| name.is_some()).count(), 2);
    // FSType is read and restricts editing like an OS/2 fsType.
    let mut font = Font::new();
    font.extra = "/FSType 8 def\n".into();
    assert_eq!(parse(&font).unwrap().rights, Some(8));
}

// A page whose F1 is the synthetic font with this encoding and ToUnicode.
fn page(font: &Font, encoding: Object, unicode: Option<&str>, content: &str) -> Document {
    let mut doc = textedit::tests::fixture();
    let (bytes, length1, length2) = font.program();
    let program = doc.add_object(Stream::new(
        dictionary! { "Length1" => length1 as i64, "Length2" => length2 as i64, "Length3" => 0 },
        bytes,
    ));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "TPDFSyntheticOne", "Flags" => 4,
        "FontBBox" => vec![0.into(), 0.into(), 900.into(), 1400.into()],
        "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200, "CapHeight" => 700,
        "StemV" => 80, "FontFile" => program,
    });
    let widths = (0..256)
        .map(|code| {
            let name = match code {
                32 => "space".to_string(),
                65..=90 => char::from(code as u8).to_string(),
                _ => String::new(),
            };
            let width = font.glyphs.iter().find(|(glyph, _)| *glyph == name).map_or(
                0,
                |(_, ops)| match ops.get(1) {
                    Some(N(width)) => *width,
                    _ => 0,
                },
            );
            Object::Integer(i64::from(width))
        })
        .collect::<Vec<_>>();
    let mut dict = dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "TPDFSyntheticOne",
        "FirstChar" => 0, "LastChar" => 255, "Widths" => widths,
        "FontDescriptor" => descriptor, "Encoding" => encoding,
    };
    if let Some(body) = unicode {
        let map = format!("%!PS-Adobe-3.0 Resource-CMap\n%%EndComments\n/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo\n<< /Registry (TeX)\n/Ordering (TPDF-synthetic)\n/Supplement 0\n>> def\n/CMapName /TeX-TPDF-synthetic-0 def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n{body}\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n%%EndResource\n%%EOF\n");
        dict.set(
            "ToUnicode",
            doc.add_object(Stream::new(Dictionary::new(), map.into_bytes())),
        );
    }
    let font_id = doc.add_object(dict);
    let root = doc
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    );
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    doc
}

fn texts(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .map(|run| run.text.clone())
        .collect()
}

fn replace(doc: &mut Document, original: &str, replacement: &str) -> Result<(), String> {
    let scan = textedit::scan(doc, 0)?;
    let run = scan.runs.iter().find(|run| run.text == original).unwrap();
    textedit::write(
        doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: run.operator,
            original: original.into(),
            replacement: replacement.into(),
        }],
    )
}

fn differences(entries: &[(i64, &str)]) -> Object {
    let mut array = Vec::new();
    for (code, name) in entries {
        array.push(Object::Integer(*code));
        array.push(Object::Name(name.as_bytes().to_vec()));
    }
    dictionary! { "Type" => "Encoding", "Differences" => array }.into()
}

#[test]
fn textedit_type1_fonts_edit_through_their_glyph_names() {
    // Built-in StandardEncoding, no ToUnicode: ASCII names select glyphs.
    let mut doc = page(
        &Font::new(),
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 12 Tf 40 180 Td (ABC DEF) Tj ET BT /F1 12 Tf 40 140 Td (XYZ) Tj ET",
    );
    assert_eq!(texts(&doc), ["ABC DEF", "XYZ"]);
    let before = doc.objects.clone();
    replace(&mut doc, "ABC DEF", "AB DE").unwrap();
    assert_eq!(texts(&doc), ["AB DE", "XYZ"]);
    // The font dictionary, its descriptor and its program are untouched.
    let page_id = crate::pagetree::ordered_pages(&doc)[0];
    for (id, object) in before {
        if id != page_id
            && object
                .as_stream()
                .map_or(true, |stream| stream.dict.has(b"Length1"))
        {
            assert_eq!(doc.objects[&id], object, "{id:?}");
        }
    }
    // A glyph the subset does not carry, and an unencodable character.
    assert!(replace(&mut doc, "AB DE", "AB \u{e9}").is_err());
    // No Encoding at all uses the program's built-in one.
    let mut font = Font::new();
    font.encoding = "/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 1 /A put\ndup 2 /B put\nreadonly def".into();
    let mut doc = page(
        &font,
        Object::Null,
        None,
        "BT /F1 12 Tf 40 180 Td <0102> Tj ET",
    );
    doc.get_dictionary_mut(
        doc.objects
            .iter()
            .find(|(_, object)| {
                object.as_dict().is_ok_and(|font| {
                    font.get(b"BaseFont").is_ok() && font.has(b"Encoding") && font.has(b"Widths")
                })
            })
            .map(|(id, _)| *id)
            .unwrap(),
    )
    .unwrap()
    .remove(b"Encoding");
    // Widths for codes 1 and 2.
    let font_id = doc
        .objects
        .iter()
        .find(|(_, object)| object.as_dict().is_ok_and(|font| font.has(b"Widths")))
        .map(|(id, _)| *id)
        .unwrap();
    let mut widths = vec![Object::Integer(0); 256];
    widths[1] = 600.into();
    widths[2] = 601.into();
    doc.get_dictionary_mut(font_id)
        .unwrap()
        .set("Widths", widths);
    assert_eq!(texts(&doc), ["AB"]);
    // B is one unit wider than A, so two of them overrun the source by one
    // unit. Swapping them does not: the ink ends where it did, and this
    // assertion used to pass only through a rounding error in the ink check.
    assert!(replace(&mut doc, "AB", "BB").is_err());
    replace(&mut doc, "AB", "B").unwrap();
    assert_eq!(texts(&doc), ["B"]);
}

#[test]
fn textedit_type1_to_unicode_narrows_but_never_contradicts_names() {
    let content = "BT /F1 12 Tf 40 180 Td <0102> Tj ET BT /F1 12 Tf 40 140 Td <03> Tj ET";
    let encoding = || differences(&[(1, "A"), (2, "fi"), (3, "summation")]);
    let fix_widths = |doc: &mut Document| {
        let font_id = doc
            .objects
            .iter()
            .find(|(_, object)| object.as_dict().is_ok_and(|font| font.has(b"Widths")))
            .map(|(id, _)| *id)
            .unwrap();
        let mut widths = vec![Object::Integer(0); 256];
        widths[1] = 600.into();
        widths[2] = 560.into();
        widths[3] = 900.into();
        doc.get_dictionary_mut(font_id)
            .unwrap()
            .set("Widths", widths);
    };
    // Agreeing ligature and a symbol outside the repertoire: the symbol's run
    // stays read-only while the other is edited.
    let map = "3 beginbfchar\n<01> <0041>\n<02> <00660069>\n<03> <2211>\nendbfchar";
    let mut doc = page(&Font::new(), encoding(), Some(map), content);
    fix_widths(&mut doc);
    assert_eq!(texts(&doc), ["Afi"]);
    replace(&mut doc, "Afi", "fiA").unwrap();
    assert_eq!(texts(&doc), ["fiA"]);
    // A code the map sends elsewhere is not offered, and text using it is
    // kept read-only rather than rewritten with a guess.
    let map = "3 beginbfchar\n<01> <0042>\n<02> <00660069>\n<03> <2211>\nendbfchar";
    let mut doc = page(&Font::new(), encoding(), Some(map), content);
    fix_widths(&mut doc);
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "page contains only read-only text"
    );
    // Without a map the ligature has no evidence of its text.
    let mut doc = page(&Font::new(), encoding(), None, content);
    fix_widths(&mut doc);
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "page contains only read-only text"
    );
    // A glyph whose PDF width disagrees with its program...
    let map = "2 beginbfchar\n<01> <0041>\n<02> <00660069>\nendbfchar";
    let mut doc = page(&Font::new(), encoding(), Some(map), content);
    fix_widths(&mut doc);
    let font_id = doc
        .objects
        .iter()
        .find(|(_, object)| object.as_dict().is_ok_and(|font| font.has(b"Widths")))
        .map(|(id, _)| *id)
        .unwrap();
    let mut widths = doc
        .get_dictionary(font_id)
        .unwrap()
        .get(b"Widths")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    widths[1] = 650.into();
    doc.get_dictionary_mut(font_id)
        .unwrap()
        .set("Widths", widths);
    // ...is not offered: the A it draws is kept read-only at the PDF width,
    // and nothing else on the page is editable here.
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "page contains only read-only text"
    );
}

#[test]
fn textedit_type1_rights_restrict_editing() {
    for (rights, allowed) in [
        ("0", true),
        ("8", true),
        ("256", true),
        ("2", false),
        ("4", false),
    ] {
        let mut font = Font::new();
        font.extra = format!("/FSType {rights} def\n");
        let doc = page(
            &font,
            Object::Name(b"StandardEncoding".to_vec()),
            None,
            "BT /F1 12 Tf 40 180 Td (ABC) Tj ET",
        );
        assert_eq!(textedit::scan(&doc, 0).is_ok(), allowed, "{rights}");
    }
}

// pdfTeX: no space glyph is offered, words are separated by TJ displacements,
// and lines open with a displacement. Reading shows the spaces; writing turns
// them back into displacements of the run's own mean gap, and keeps the
// opening displacement.
#[test]
fn textedit_type1_word_gaps_read_and_write_as_spaces() {
    let mut font = Font::new();
    font.glyphs.retain(|(name, _)| name != "space");
    let content = "BT /F1 10 Tf 40 180 Td [-500 (ABC) -333 (DEF) -50 (G) -300 (HI)] TJ (J) Tj ET BT /F1 10 Tf 40 140 Td (XYZ) Tj ET";
    let mut doc = page(
        &font,
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        content,
    );
    assert_eq!(texts(&doc), ["ABC DEFG HI", "J", "XYZ"]);
    let before = textedit::scan(&doc, 0).unwrap();
    let origin = before.runs[0].matrix[4];
    assert!((origin - 45.).abs() < 1e-9, "{origin}");
    replace(&mut doc, "ABC DEFG HI", "AB DE HI").unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(
        after
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["AB DE HI", "J", "XYZ"]
    );
    assert_eq!(after.runs[0].matrix, before.runs[0].matrix);
    // The following show stays exactly where it was.
    assert_eq!(after.runs[1].matrix, before.runs[1].matrix);
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = lopdf::content::Content::decode_strict(&doc.get_page_content(id)).unwrap();
    let show = content
        .operations
        .iter()
        .find(|op| op.operator == "TJ")
        .unwrap();
    let items = show.operands[0].as_array().unwrap();
    // The opening displacement and the unchanged " HI" keep their source
    // items, gap included; the new word boundary gets the source's mean gap,
    // (333 + 300) / 2.
    assert_eq!(
        items,
        &vec![
            Object::Integer(-500),
            Object::string_literal("AB"),
            Object::Real(-316.5),
            Object::string_literal("DE"),
            Object::Integer(-300),
            Object::string_literal("HI"),
            // What keeps the continued "J" where it was.
            Object::Integer(-1879),
            Object::Real(-0.5),
        ]
    );
    let mut alone = page(
        &font,
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 10 Tf 40 180 Td [-500 (ABC) -333 (DEF)] TJ ET",
    );
    replace(&mut alone, "ABC DEF", "AB DEF").unwrap();
    let id = crate::pagetree::ordered_pages(&alone)[0];
    let content = lopdf::content::Content::decode_strict(&alone.get_page_content(id)).unwrap();
    let show = content
        .operations
        .iter()
        .find(|op| op.operator == "TJ")
        .unwrap();
    assert_eq!(
        show.operands[0].as_array().unwrap()[0],
        Object::Integer(-500)
    );
    assert_eq!(texts(&alone), ["AB DEF"]);
    // Spaces with no word to separate have nothing to show.
    for replacement in [" AB", "AB ", "A  B"] {
        assert_eq!(
            replace(&mut doc, "AB DE HI", replacement).unwrap_err(),
            super::super::GAP_SPACES
        );
    }
    // A font that can write a space keeps a large displacement as a gap.
    let doc = page(
        &Font::new(),
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 10 Tf 40 180 Td [(ABC) -333 (DEF)] TJ ET",
    );
    assert_eq!(texts(&doc), ["ABCDEF"]);
}

// The layout editor wraps and writes in the original TeX font: its spaces are
// gaps there too, so a replacement with spaces does not fall back to Noto Sans.
#[test]
fn textedit_type1_layout_keeps_the_original_font_and_writes_gaps() {
    let mut font = Font::new();
    font.glyphs.retain(|(name, _)| name != "space");
    let content =
        "BT /F1 10 Tf 40 180 Td [(ABC) -333 (DEF)] TJ ET BT /F1 10 Tf 40 60 Td (XYZ) Tj ET";
    let layout = |doc: &Document, replacement: &str, width: f64, wrap: bool| {
        let scan = textedit::scan(doc, 0).unwrap();
        Change {
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: scan.runs[0].text.clone(),
            replacement: replacement.into(),
            layout: Some(textedit::Layout {
                width,
                height: if wrap { 40. } else { 14. },
                size: 10.,
                wrap,
                font: textedit::EditFont::Auto,
            }),
        }
    };
    for (width, wrap, expected) in [
        (200., false, vec!["ABC DEF GHI", "XYZ"]),
        (45., true, vec!["ABC DEF", "GHI", "XYZ"]),
    ] {
        let mut doc = page(
            &font,
            Object::Name(b"StandardEncoding".to_vec()),
            None,
            content,
        );
        let change = layout(&doc, "ABC DEF GHI", width, wrap);
        assert_eq!(
            textedit::preview_layout(&doc, &change).unwrap().font,
            "TPDFSyntheticOne"
        );
        textedit::write(&mut doc, &[change]).unwrap();
        // The layout writer's empty cursor-restoration show is a run of its own.
        let written: Vec<String> = texts(&doc)
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();
        assert_eq!(written, expected, "{width}");
    }
    // A doubled space has nothing to show in the original font, so Auto takes
    // the bundled font, exactly as for a character the subset lacks.
    let doc = page(
        &font,
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        content,
    );
    let change = layout(&doc, "ABC  DEF", 200., false);
    assert_eq!(
        textedit::preview_layout(&doc, &change).unwrap().font,
        "Noto Sans"
    );
}

fn font_id(doc: &Document) -> lopdf::ObjectId {
    doc.objects
        .iter()
        .find(|(_, object)| object.as_dict().is_ok_and(|font| font.has(b"Widths")))
        .map(|(id, _)| *id)
        .unwrap()
}

fn set_widths(doc: &mut Document, widths: &[(usize, i64)]) {
    let id = font_id(doc);
    let mut array = doc
        .get_dictionary(id)
        .unwrap()
        .get(b"Widths")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    for &(code, width) in widths {
        array[code] = width.into();
    }
    doc.get_dictionary_mut(id).unwrap().set("Widths", array);
}

#[test]
fn textedit_type1_encodings_flags_and_glyph_limits() {
    let text = "BT /F1 12 Tf 40 180 Td (ABC) Tj ET";
    let doc = page(
        &Font::new(),
        Object::Name(b"WinAnsiEncoding".to_vec()),
        None,
        text,
    );
    assert_eq!(texts(&doc), ["ABC"]);
    for encoding in [
        Object::Name(b"MacRomanEncoding".to_vec()),
        dictionary! { "BaseEncoding" => "MacRomanEncoding" }.into(),
        dictionary! { "Differences" => vec![65.into(), "A".into(), 65.into(), "B".into()] }.into(),
        dictionary! { "Type" => "Font" }.into(),
    ] {
        let doc = page(&Font::new(), encoding, None, text);
        assert_eq!(
            textedit::scan(&doc, 0).unwrap_err(),
            "unsupported or ambiguous Type 1 encoding"
        );
    }
    for (flags, accepted) in [(4, true), (32, true), (36, false), (0, false)] {
        let mut doc = page(
            &Font::new(),
            Object::Name(b"StandardEncoding".to_vec()),
            None,
            text,
        );
        let descriptor = doc
            .objects
            .iter()
            .find(|(_, object)| object.as_dict().is_ok_and(|d| d.has(b"FontFile")))
            .map(|(id, _)| *id)
            .unwrap();
        doc.get_dictionary_mut(descriptor)
            .unwrap()
            .set("Flags", flags);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{flags}");
    }
    // A glyph taller than the editable envelope keeps its text read-only.
    let mut font = Font::new();
    font.glyphs.retain(|(name, _)| name != "Z");
    font.glyphs
        .push(("Z".into(), rectangle(20, 625, 540, 1200)));
    let doc = page(
        &font,
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 12 Tf 40 180 Td (Z) Tj ET BT /F1 12 Tf 40 120 Td (Y) Tj ET",
    );
    assert_eq!(texts(&doc), ["Y"]);
    // Two different glyphs for one character make the choice arbitrary.
    let map = "2 beginbfchar\n<01> <00660069>\n<02> <00660069>\nendbfchar";
    let mut doc = page(
        &Font::new(),
        differences(&[(1, "fi"), (2, "f_i")]),
        Some(map),
        "BT /F1 12 Tf 40 180 Td <01> Tj ET",
    );
    set_widths(&mut doc, &[(1, 560), (2, 560)]);
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "ambiguous duplicate Type 1 glyph encoding"
    );
}

// A glyph the editor cannot write still measures its run: its ink joins the
// font's vertical bounds, and word spacing applies to it at code 32.
#[test]
fn textedit_type1_opaque_glyphs_measure_read_only_text() {
    let mut font = Font::new();
    font.glyphs
        .push(("visiblespace".into(), rectangle(0, 250, 200, 50)));
    let content = "BT /F1 10 Tf 5 Tw 40 180 Td (A A) Tj (A) Tj ET";
    let mut doc = page(
        &font,
        differences(&[(32, "visiblespace"), (65, "A")]),
        None,
        content,
    );
    set_widths(&mut doc, &[(32, 250)]);
    let scan = textedit::scan(&doc, 0).unwrap();
    assert_eq!(
        scan.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["A"]
    );
    // 40 + (600 + 250 + 600) / 100 + 5 for word spacing on code 32.
    assert!(
        (scan.runs[0].matrix[4] - 59.5).abs() < 1e-9,
        "{:?}",
        scan.runs[0].matrix
    );
    let metrics = embedded(&doc, doc.get_dictionary(font_id(&doc)).unwrap()).unwrap();
    assert_eq!(metrics.vertical_bounds, Some([0., 700.]));
    let mut doc = page(
        &Font::new(),
        differences(&[(1, "A"), (3, "summation")]),
        None,
        "BT /F1 12 Tf 40 180 Td <01> Tj ET",
    );
    set_widths(&mut doc, &[(1, 600), (3, 900)]);
    let metrics = embedded(&doc, doc.get_dictionary(font_id(&doc)).unwrap()).unwrap();
    assert_eq!(metrics.vertical_bounds, Some([0., 1400.]));
    assert_eq!(texts(&doc), ["A"]);
    // TeX's largest parentheses hang 2.4 em below the baseline and are still
    // measured, so the run beside them stays editable.
    let mut font = Font::new();
    font.glyphs.push((
        "parenleftbigg".into(),
        vec![
            N(50),
            N(369),
            HSBW,
            N(0),
            N(-2400),
            RMOVETO,
            N(200),
            N(0),
            RLINETO,
            N(0),
            N(2456),
            RLINETO,
            N(-200),
            N(0),
            RLINETO,
            CLOSEPATH,
            ENDCHAR,
        ],
    ));
    let mut doc = page(
        &font,
        differences(&[(1, "A"), (4, "parenleftbigg")]),
        None,
        "BT /F1 12 Tf 40 180 Td <01> Tj ET BT /F1 12 Tf 40 120 Td <04> Tj ET",
    );
    set_widths(&mut doc, &[(1, 600), (4, 369)]);
    assert_eq!(texts(&doc), ["A"]);
}

// Words and gaps measure as the TJ they are written to: each word's own layout
// at its own offset, the gaps between them.
#[test]
fn textedit_type1_gapped_layout_matches_the_written_words() {
    let mut font = Font::new();
    font.glyphs.retain(|(name, _)| name != "space");
    let doc = page(
        &font,
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 10 Tf 40 180 Td (AB) Tj ET",
    );
    let metrics = embedded(&doc, doc.get_dictionary(font_id(&doc)).unwrap()).unwrap();
    assert!(!metrics.writes_space());
    let (first, [first_left, first_right]) = metrics.spaced_layout("AB", 10., 0.5, 0.).unwrap();
    let (second, [second_left, second_right]) = metrics.spaced_layout("CD", 10., 0.5, 0.).unwrap();
    let (advance, bounds) = metrics.gapped_layout("AB CD", 10., 0.5, 0., 250.).unwrap();
    assert!((advance - (first + 2.5 + second)).abs() < 1e-12);
    assert_eq!(
        bounds,
        [
            first_left.min(first + 2.5 + second_left),
            first_right.max(first + 2.5 + second_right)
        ]
    );
    let items = metrics.items("AB CD", 250.).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[1], Object::Real(-250.));
    // A font that writes a space measures and writes it as a glyph.
    let doc = page(
        &Font::new(),
        Object::Name(b"StandardEncoding".to_vec()),
        None,
        "BT /F1 10 Tf 40 180 Td (AB) Tj ET",
    );
    let metrics = embedded(&doc, doc.get_dictionary(font_id(&doc)).unwrap()).unwrap();
    assert_eq!(
        metrics.gapped_layout("AB CD", 10., 0., 0., 999.).unwrap(),
        metrics.spaced_layout("AB CD", 10., 0., 0.).unwrap()
    );
    assert_eq!(metrics.items("AB CD", 999.).unwrap().len(), 1);
}
