//! The text a document's content streams show, decoded through each string's
//! own font, for the redaction scan.
//!
//! `verify::scan` reads a stream's bytes and its string operands as stored.
//! That finds a word drawn through a simple font in a standard encoding, and it
//! cannot find one drawn through a composite (Type0) font, which stores glyph
//! numbers: until 2026-11-20 a file holding any such font withheld *verified*
//! outright. Most composite fonts carry a `/ToUnicode` map, and PDFium extracts
//! the words a redaction removes through that very map, so the words can be
//! read back the same way. This walks every page, the forms it draws and its
//! annotations' appearances with their `/Resources`, follows `Tf` through `q`
//! and `Q`, and decodes each string through the font that shows it.
//!
//! **What it reads is the whole claim, so it says when it could not.**
//! [`Read::composite_read`] is true only when every string shown through a
//! composite font was shown through one that is `Identity-H` or `Identity-V`
//! with a `/ToUnicode` that parses and decodes it code by code, no string in
//! a file with a composite font is shown through a font its resources do not
//! hold, and the walk ran to its end inside its bounds. A composite font no
//! stream uses draws nothing, so it is not asked about. A stream that shows text and that the walk never reached is the
//! caller's half of the same rule, since only the caller has every stream
//! decoded: see `verify::scan`.
//!
//! A simple font's string is decoded through its `/ToUnicode` too when it has
//! one, which reads a font whose own encoding moves letters; a code that map
//! lacks is left out rather than withheld, because the stored bytes and their
//! WinAnsi reading are searched as well.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::encoding::MAX_DECODE;
use crate::verify::{hex_string, literal_string};

/// How deep forms may nest inside forms before the walk stops.
const MAX_DEPTH: usize = 32;

/// How many tokens the whole walk may read, across every stream.
const MAX_TOKENS: usize = 20_000_000;

/// How many codes one `/ToUnicode` map may define.
const MAX_CODES: usize = 1 << 17;

/// What the walk read.
#[derive(Debug, Default)]
pub(crate) struct Read {
    /// The decoded text each content stream showed, as UTF-8 joined with
    /// nothing between strings, keyed by the stream's object.
    pub text: BTreeMap<ObjectId, Vec<u8>>,
    /// Whether every string a composite font showed was read: see the module
    /// documentation for the conditions.
    pub composite_read: bool,
}

/// A code-to-text map from a `/ToUnicode` CMap.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ToUnicode {
    /// Bytes per code: 1 or 2.
    width: usize,
    map: HashMap<u32, String>,
}

/// A font as the walk needs it.
#[derive(Debug)]
enum Font {
    /// A composite font whose strings can be read, or `None` when they cannot.
    Composite(Option<ToUnicode>),
    /// A simple font, with its `/ToUnicode` when it has one that parses.
    Simple(Option<ToUnicode>),
}

/// Reads every page of `doc`. `composite` says whether the file holds a
/// composite font anywhere, which decides what a string shown through an
/// unknown font costs.
pub(crate) fn read(doc: &Document, composite: bool) -> Read {
    let mut walk = Walk {
        doc,
        fonts: HashMap::new(),
        out: Read::default(),
        tokens: 0,
        stopped: false,
        lost: false,
        visited: BTreeSet::new(),
        composite,
    };
    for (_, page) in doc.get_pages() {
        let Ok(dict) = doc.get_dictionary(page) else {
            walk.lost = true;
            continue;
        };
        let resources = inherited_resources(doc, page);
        let mut state = State::default();
        for content in doc.get_page_contents(page) {
            walk.stream(content, resources.as_ref(), &mut state, 0);
        }
        walk.annotations(dict, resources.as_ref());
    }
    walk.out.composite_read = !walk.stopped && !walk.lost;
    walk.out
}

/// The text state the walk tracks: the current font, saved by `q`.
#[derive(Default, Clone)]
struct State {
    font: Option<ObjectId>,
    /// A font named directly in a resources dictionary rather than by
    /// reference has no object of its own; it is decoded from this.
    direct: Option<std::rc::Rc<Font>>,
    /// The current font was named by `Tf` and not found.
    unknown: bool,
    saved: Vec<(Option<ObjectId>, Option<std::rc::Rc<Font>>, bool)>,
}

struct Walk<'a> {
    doc: &'a Document,
    fonts: HashMap<ObjectId, Font>,
    out: Read,
    tokens: usize,
    stopped: bool,
    /// Something the walk needed was missing or could not be read.
    lost: bool,
    visited: BTreeSet<ObjectId>,
    composite: bool,
}

/// One token of content-stream syntax, as far as text needs it.
enum Token {
    Name(Vec<u8>),
    Str(Vec<u8>),
    ArrayStart,
    ArrayEnd,
    Op(Vec<u8>),
    Other,
}

impl Walk<'_> {
    fn annotations(&mut self, page: &Dictionary, resources: Option<&Dictionary>) {
        let Some(Object::Array(annots)) = resolve(self.doc, page.get(b"Annots").ok()) else {
            return;
        };
        for annot in annots.clone() {
            let Some(Object::Dictionary(annot)) = resolve(self.doc, Some(&annot)) else {
                continue;
            };
            let Some(Object::Dictionary(ap)) = resolve(self.doc, annot.get(b"AP").ok()) else {
                continue;
            };
            for key in [b"N".as_slice(), b"D", b"R"] {
                let Ok(value) = ap.get(key) else {
                    continue;
                };
                let mut appearances = Vec::new();
                match value {
                    Object::Reference(id) => match self.doc.get_object(*id) {
                        Ok(Object::Stream(_)) => appearances.push(*id),
                        Ok(Object::Dictionary(states)) => appearances
                            .extend(states.iter().filter_map(|(_, one)| one.as_reference().ok())),
                        _ => {}
                    },
                    Object::Dictionary(states) => appearances
                        .extend(states.iter().filter_map(|(_, one)| one.as_reference().ok())),
                    _ => {}
                }
                for id in appearances {
                    self.form(id, resources, &State::default(), 0);
                }
            }
        }
    }

    /// Reads a form XObject, with its own resources or else `inherited`, and
    /// the font current where it is drawn, which a form inherits.
    fn form(&mut self, id: ObjectId, inherited: Option<&Dictionary>, at: &State, depth: usize) {
        if depth > MAX_DEPTH {
            self.stopped = true;
            return;
        }
        if !self.visited.insert(id) {
            return;
        }
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return;
        };
        let own = match resolve(self.doc, stream.dict.get(b"Resources").ok()) {
            Some(Object::Dictionary(own)) => Some(own.clone()),
            _ => None,
        };
        let resources = own.as_ref().or(inherited);
        let mut state = State {
            font: at.font,
            direct: at.direct.clone(),
            unknown: at.unknown,
            saved: Vec::new(),
        };
        self.stream(id, resources, &mut state, depth);
    }

    /// Reads one content stream under `resources`, carrying `state`.
    fn stream(
        &mut self,
        id: ObjectId,
        resources: Option<&Dictionary>,
        state: &mut State,
        depth: usize,
    ) {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            self.lost = true;
            return;
        };
        let content = match stream.decompressed_content_with_limit(MAX_DECODE) {
            Ok(content) => content,
            Err(_) if !stream.dict.has(b"Filter") => stream.content.clone(),
            // Unreadable streams are the scan's reason already.
            Err(_) => return,
        };
        let tokens = tokenize(&content);
        self.tokens = self.tokens.saturating_add(tokens.len());
        if self.tokens > MAX_TOKENS {
            self.stopped = true;
            return;
        }
        let mut text = Vec::new();
        let mut operands: Vec<Token> = Vec::new();
        let mut in_array: Option<Vec<Vec<u8>>> = None;
        for token in tokens {
            match token {
                Token::ArrayStart => in_array = Some(Vec::new()),
                Token::ArrayEnd => {
                    if let Some(strings) = in_array.take() {
                        operands.push(Token::Str(strings.concat()));
                    }
                }
                Token::Str(bytes) if in_array.is_some() => {
                    if let Some(strings) = in_array.as_mut() {
                        strings.push(bytes);
                    }
                }
                Token::Op(op) => {
                    in_array = None;
                    self.operator(&op, &operands, resources, state, &mut text, depth);
                    operands.clear();
                }
                other => {
                    if in_array.is_none() {
                        operands.push(other);
                    }
                }
            }
        }
        self.out.text.entry(id).or_default().extend(text);
    }

    fn operator(
        &mut self,
        op: &[u8],
        operands: &[Token],
        resources: Option<&Dictionary>,
        state: &mut State,
        text: &mut Vec<u8>,
        depth: usize,
    ) {
        match op {
            b"q" => state
                .saved
                .push((state.font, state.direct.clone(), state.unknown)),
            b"Q" => {
                if let Some((font, direct, unknown)) = state.saved.pop() {
                    state.font = font;
                    state.direct = direct;
                    state.unknown = unknown;
                }
            }
            b"Tf" => {
                let name = operands.iter().rev().find_map(|one| match one {
                    Token::Name(name) => Some(name.clone()),
                    _ => None,
                });
                self.select(name.as_deref(), resources, state);
            }
            b"Tj" | b"'" | b"\"" | b"TJ" => {
                if let Some(Token::Str(bytes)) = operands.last() {
                    self.show(bytes, state, text);
                }
            }
            b"Do" => {
                let name = operands.iter().rev().find_map(|one| match one {
                    Token::Name(name) => Some(name.clone()),
                    _ => None,
                });
                if let Some(id) = name.and_then(|name| xobject(self.doc, resources, &name)) {
                    if matches!(self.doc.get_object(id), Ok(Object::Stream(stream))
                        if stream.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"Form"))
                    {
                        self.form(id, resources, state, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    /// Makes the font `name` in `resources` current.
    fn select(&mut self, name: Option<&[u8]>, resources: Option<&Dictionary>, state: &mut State) {
        state.font = None;
        state.direct = None;
        state.unknown = true;
        let Some(name) = name else {
            return;
        };
        let Some(Object::Dictionary(fonts)) =
            resources.and_then(|r| resolve(self.doc, r.get(b"Font").ok()))
        else {
            return;
        };
        match fonts.get(name) {
            Ok(Object::Reference(id)) => {
                let id = *id;
                if !self.fonts.contains_key(&id) {
                    let Ok(Object::Dictionary(dict)) = self.doc.get_object(id) else {
                        return;
                    };
                    let font = font_of(self.doc, dict);
                    self.fonts.insert(id, font);
                }
                state.font = Some(id);
                state.unknown = false;
            }
            Ok(Object::Dictionary(dict)) => {
                state.direct = Some(std::rc::Rc::new(font_of(self.doc, dict)));
                state.unknown = false;
            }
            _ => {}
        }
    }

    /// Decodes one shown string through the current font into `text`.
    fn show(&mut self, bytes: &[u8], state: &State, text: &mut Vec<u8>) {
        let font = match (&state.direct, state.font) {
            (Some(direct), _) => Some(direct.as_ref()),
            (None, Some(id)) => self.fonts.get(&id),
            (None, None) => None,
        };
        match font {
            None => {
                // Shown through a font the resources do not hold, or before
                // any `Tf`: whether it was composite is unknown.
                if state.unknown || state.font.is_none() {
                    self.lost |= self.composite;
                }
            }
            Some(Font::Composite(None)) => self.lost = true,
            Some(Font::Composite(Some(map))) => {
                if map.decode(bytes, text).is_none() {
                    self.lost = true;
                }
            }
            Some(Font::Simple(Some(map))) => {
                let _ = map.decode_lossy(bytes, text);
            }
            Some(Font::Simple(None)) => {}
        }
    }
}

impl ToUnicode {
    /// Appends the text `bytes` stands for, or `None` when a code is not in
    /// the map or the string is not a whole number of codes.
    fn decode(&self, bytes: &[u8], out: &mut Vec<u8>) -> Option<()> {
        if bytes.len() % self.width != 0 {
            return None;
        }
        for code in bytes.chunks(self.width) {
            let code = code.iter().fold(0u32, |acc, b| acc << 8 | u32::from(*b));
            out.extend_from_slice(self.map.get(&code)?.as_bytes());
        }
        Some(())
    }

    /// [`ToUnicode::decode`], leaving out codes the map lacks.
    fn decode_lossy(&self, bytes: &[u8], out: &mut Vec<u8>) -> Option<()> {
        for code in bytes.chunks(self.width) {
            let code = code.iter().fold(0u32, |acc, b| acc << 8 | u32::from(*b));
            if let Some(text) = self.map.get(&code) {
                out.extend_from_slice(text.as_bytes());
            }
        }
        Some(())
    }
}

/// Whether `dict` is a font dictionary of subtype `/Type0`.
fn is_composite(doc: &Document, dict: &Dictionary) -> bool {
    let named = |key: &[u8], want: &[u8]| {
        resolve(doc, dict.get(key).ok())
            .and_then(|value| value.as_name().ok())
            .is_some_and(|name| name == want)
    };
    named(b"Subtype", b"Type0")
}

/// A composite font's map, when its strings can be read: `Identity-H` or
/// `Identity-V`, and a `/ToUnicode` of two-byte codes that parses.
fn composite_map(doc: &Document, dict: &Dictionary) -> Option<ToUnicode> {
    let encoding = resolve(doc, dict.get(b"Encoding").ok())?.as_name().ok()?;
    if encoding != b"Identity-H" && encoding != b"Identity-V" {
        return None;
    }
    let map = to_unicode(doc, dict)?;
    (map.width == 2 && !map.says_nothing()).then_some(map)
}

impl ToUnicode {
    /// Whether any code maps to a private-use or control character: a map
    /// that does not say what its glyphs are. A real document was measured
    /// with composite fonts whose maps sent visible Latin text to private-use
    /// code points (`docs/TRAPS.md`, the third state between text and no
    /// text); decoded through such a map a visible word is not found, so the
    /// font is refused as before rather than read.
    fn says_nothing(&self) -> bool {
        self.map.values().flat_map(|text| text.chars()).any(|ch| {
            ch.is_control() || matches!(u32::from(ch), 0xE000..=0xF8FF | 0xF_0000..=0x10_FFFF)
        })
    }
}

fn font_of(doc: &Document, dict: &Dictionary) -> Font {
    if is_composite(doc, dict) {
        Font::Composite(composite_map(doc, dict))
    } else {
        Font::Simple(to_unicode(doc, dict).filter(|map| map.width == 1))
    }
}

/// The font's `/ToUnicode` map, decoded and parsed.
fn to_unicode(doc: &Document, dict: &Dictionary) -> Option<ToUnicode> {
    let Some(Object::Stream(stream)) = resolve(doc, dict.get(b"ToUnicode").ok()) else {
        return None;
    };
    let bytes = stream.decompressed_content_with_limit(MAX_DECODE).ok()?;
    parse_cmap(&bytes)
}

/// Parses a `/ToUnicode` CMap: its code space, `bfchar` and `bfrange`.
///
/// `None` for anything it does not read whole: a `usecmap`, code spaces of
/// mixed or unusual widths, a destination that is not UTF-16BE, or more codes
/// than [`MAX_CODES`]. A map read in part would decode some strings and miss
/// others, which is the shape of answer this module exists to refuse.
pub(crate) fn parse_cmap(bytes: &[u8]) -> Option<ToUnicode> {
    let tokens = tokenize(bytes);
    let mut width: Option<usize> = None;
    let mut map = HashMap::new();
    let mut at = 0;
    while at < tokens.len() {
        let Token::Op(op) = &tokens[at] else {
            at += 1;
            continue;
        };
        let (end, group): (&[u8], usize) = match op.as_slice() {
            b"usecmap" => return None,
            b"begincodespacerange" => (b"endcodespacerange", 2),
            b"beginbfchar" => (b"endbfchar", 2),
            b"beginbfrange" => (b"endbfrange", 3),
            _ => {
                at += 1;
                continue;
            }
        };
        let kind = op.clone();
        let close = tokens[at + 1..]
            .iter()
            .position(|one| matches!(one, Token::Op(name) if name.as_slice() == end))?
            + at
            + 1;
        let body = &tokens[at + 1..close];
        let items = entries(body)?;
        if items.len() % group != 0 {
            return None;
        }
        for item in items.chunks(group) {
            match kind.as_slice() {
                b"begincodespacerange" => {
                    let (Entry::Str(low), Entry::Str(high)) = (&item[0], &item[1]) else {
                        return None;
                    };
                    if low.len() != high.len() || !matches!(low.len(), 1 | 2) {
                        return None;
                    }
                    if width.is_some_and(|w| w != low.len()) {
                        return None;
                    }
                    width = Some(low.len());
                }
                b"beginbfchar" => {
                    let (Entry::Str(code), Entry::Str(dest)) = (&item[0], &item[1]) else {
                        return None;
                    };
                    map.insert(number(code)?, utf16(dest)?);
                }
                _ => {
                    let (Entry::Str(low), Entry::Str(high)) = (&item[0], &item[1]) else {
                        return None;
                    };
                    let (low, high) = (number(low)?, number(high)?);
                    if high < low || map.len() + (high - low) as usize >= MAX_CODES {
                        return None;
                    }
                    match &item[2] {
                        Entry::Str(dest) => {
                            let mut dest = dest.clone();
                            for code in low..=high {
                                map.insert(code, utf16(&dest)?);
                                // The last byte counts up, as the CMap rules say.
                                let last = dest.last_mut()?;
                                *last = last.wrapping_add(1);
                            }
                        }
                        Entry::Array(dests) => {
                            if dests.len() != (high - low + 1) as usize {
                                return None;
                            }
                            for (code, dest) in (low..=high).zip(dests) {
                                map.insert(code, utf16(dest)?);
                            }
                        }
                    }
                }
            }
            if map.len() > MAX_CODES {
                return None;
            }
        }
        at = close + 1;
    }
    Some(ToUnicode { width: width?, map })
}

/// A CMap block's operands.
enum Entry {
    Str(Vec<u8>),
    Array(Vec<Vec<u8>>),
}

fn entries(body: &[Token]) -> Option<Vec<Entry>> {
    let mut out = Vec::new();
    let mut array: Option<Vec<Vec<u8>>> = None;
    for token in body {
        match token {
            Token::Str(bytes) => match array.as_mut() {
                Some(items) => items.push(bytes.clone()),
                None => out.push(Entry::Str(bytes.clone())),
            },
            Token::ArrayStart if array.is_none() => array = Some(Vec::new()),
            Token::ArrayEnd => out.push(Entry::Array(array.take()?)),
            _ => return None,
        }
    }
    array.is_none().then_some(out)
}

fn number(bytes: &[u8]) -> Option<u32> {
    (1..=4)
        .contains(&bytes.len())
        .then(|| bytes.iter().fold(0u32, |acc, b| acc << 8 | u32::from(*b)))
}

fn utf16(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Lexes content-stream syntax into the tokens text needs. Inline image data
/// is passed over from `ID` to `EI`.
fn tokenize(content: &[u8]) -> Vec<Token> {
    let delimiter = |byte: u8| {
        byte.is_ascii_whitespace()
            || byte == 0
            || matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
            )
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at < content.len() {
        let byte = content[at];
        match byte {
            b'%' => {
                while at < content.len() && !matches!(content[at], b'\n' | b'\r') {
                    at += 1;
                }
            }
            b'(' => {
                let mut bytes = Vec::new();
                at = literal_string(content, at + 1, &mut bytes);
                out.push(Token::Str(bytes));
            }
            b'<' if content.get(at + 1) == Some(&b'<') => {
                out.push(Token::Other);
                at += 2;
            }
            b'>' if content.get(at + 1) == Some(&b'>') => {
                out.push(Token::Other);
                at += 2;
            }
            b'<' => {
                let mut bytes = Vec::new();
                at = hex_string(content, at + 1, &mut bytes);
                out.push(Token::Str(bytes));
            }
            b'[' => {
                out.push(Token::ArrayStart);
                at += 1;
            }
            b']' => {
                out.push(Token::ArrayEnd);
                at += 1;
            }
            b'/' => {
                let start = at + 1;
                at = start;
                while at < content.len() && !delimiter(content[at]) {
                    at += 1;
                }
                out.push(Token::Name(content[start..at].to_vec()));
            }
            _ if delimiter(byte) => at += 1,
            _ => {
                let start = at;
                while at < content.len() && !delimiter(content[at]) {
                    at += 1;
                }
                let word = &content[start..at];
                let is_number = word
                    .iter()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+'));
                if is_number {
                    out.push(Token::Other);
                } else if word == b"ID" {
                    // Inline image data: skip to an `EI` standing alone.
                    let mut end = at;
                    while end + 2 <= content.len() {
                        if &content[end..end + 2] == b"EI"
                            && content[end - 1].is_ascii_whitespace()
                            && content.get(end + 2).is_none_or(|b| delimiter(*b))
                        {
                            break;
                        }
                        end += 1;
                    }
                    at = (end + 2).min(content.len());
                    out.push(Token::Op(b"EI".to_vec()));
                } else {
                    out.push(Token::Op(word.to_vec()));
                }
            }
        }
    }
    out
}

/// Follows a value through one reference.
fn resolve<'a>(doc: &'a Document, value: Option<&'a Object>) -> Option<&'a Object> {
    match value? {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

/// The page's resources, inherited from the page tree when it has none.
fn inherited_resources(doc: &Document, page: ObjectId) -> Option<Dictionary> {
    let mut at = page;
    for _ in 0..MAX_DEPTH {
        let dict = doc.get_dictionary(at).ok()?;
        if let Some(Object::Dictionary(resources)) = resolve(doc, dict.get(b"Resources").ok()) {
            return Some(resources.clone());
        }
        at = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// The XObject `name` in `resources`, as an object.
fn xobject(doc: &Document, resources: Option<&Dictionary>, name: &[u8]) -> Option<ObjectId> {
    let Some(Object::Dictionary(xobjects)) = resolve(doc, resources?.get(b"XObject").ok()) else {
        return None;
    };
    xobjects.get(name).ok()?.as_reference().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cmap_reads_its_code_space_chars_and_both_range_forms() {
        let map = parse_cmap(
            b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
              1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
              2 beginbfchar <0003> <0020> <0010> <00FC> endbfchar\n\
              2 beginbfrange <0024> <0026> <0041> <0030> <0031> [<0066 0069> <004D>] endbfrange\n\
              endcmap CMapName currentdict /CMap defineresource pop end end",
        )
        .expect("parses");
        assert_eq!(map.width, 2);
        let text = |codes: &[u8]| {
            let mut out = Vec::new();
            map.decode(codes, &mut out)
                .map(|()| String::from_utf8(out).unwrap())
        };
        assert_eq!(
            text(&[0, 0x24, 0, 0x25, 0, 0x26, 0, 3]).as_deref(),
            Some("ABC ")
        );
        assert_eq!(text(&[0, 0x31, 0, 0x10]).as_deref(), Some("M\u{fc}"));
        assert_eq!(text(&[0, 0x30]).as_deref(), Some("fi"));
        // A code the map lacks, and a string that is not whole codes.
        assert_eq!(text(&[0, 0x99]), None);
        assert_eq!(text(&[0, 0x24, 0]), None);
    }

    #[test]
    fn a_cmap_this_does_not_read_whole_is_refused() {
        // `usecmap`, mixed widths, an odd UTF-16 destination.
        assert!(parse_cmap(b"/Base usecmap 1 beginbfchar <01> <0041> endbfchar").is_none());
        assert!(
            parse_cmap(b"2 begincodespacerange <00> <7F> <8000> <FFFF> endcodespacerange")
                .is_none()
        );
        assert!(parse_cmap(
            b"1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <01> <00> endbfchar"
        )
        .is_none());
        // No code space at all.
        assert!(parse_cmap(b"1 beginbfchar <01> <0041> endbfchar").is_none());
    }
}
