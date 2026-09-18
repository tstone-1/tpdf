//! Adobe Type 1 font programs (Adobe Type 1 Font Format, 1990) as embedded in
//! a PDF FontFile stream: a cleartext part, an eexec-encrypted private part and
//! an optional trailer, split by Length1/Length2/Length3 (ISO 32000-1 9.9).
//!
//! No PostScript is executed. The cleartext is read for a handful of literal
//! declarations, the private part for the Subrs and CharStrings binary blobs,
//! and each charstring is interpreted only far enough to learn its advance and
//! the control-point hull of its outline. Anything else refuses the font.

use std::collections::BTreeMap;

const INVALID: &str = "unsupported embedded Type 1 font program";
// Type 1 charstrings are at most 65,535 bytes; the font is already bounded by
// the 1 MiB decoded-stream limit, so these bound the index sizes and the work.
const MAX_GLYPHS: usize = 4096;
const MAX_SUBRS: usize = 65_536;
// Adobe Type 1 Font Format 6.1: the argument stack holds at most 24 numbers;
// subroutines nest at most 10 deep.
const MAX_STACK: usize = 24;
const MAX_DEPTH: usize = 10;
// Operations executed for one glyph, subroutines included. Ordinary glyphs
// use tens to a few hundred; this only stops recursion and loops through Subrs.
const MAX_OPERATIONS: usize = 65_536;

pub(super) struct Glyph {
    // Advance in glyph space (thousandths of an em for the accepted matrix).
    pub width: f64,
    // Control-point hull of the outline, or None for a glyph that paints nothing.
    pub bounds: Option<[f64; 4]>,
}

pub(super) struct Program {
    pub glyphs: BTreeMap<Vec<u8>, Glyph>,
    // FontInfo /FSType, when the program declares one (OpenType OS/2 semantics).
    pub rights: Option<i64>,
    pub encoding: Builtin,
}

// The program's own /Encoding: the implicit base of a PDF encoding for an
// embedded font (ISO 32000-1 Table 114).
pub(super) enum Builtin {
    Standard,
    Custom(Box<[Option<Vec<u8>>; 256]>),
}

/// `bytes` is the decoded FontFile stream; the lengths are its dictionary's.
pub(super) fn parse(
    bytes: &[u8],
    length1: usize,
    length2: usize,
    length3: usize,
) -> Result<Program, String> {
    let clear_end = length1;
    let private_end = length1.checked_add(length2).ok_or(INVALID)?;
    if length1 == 0 || length2 < 4 || private_end > bytes.len() {
        return Err(INVALID.into());
    }
    // Length3 counts the 512 zeros and cleartomark of the original file. A
    // producer may keep them or drop them (pdfTeX writes 0); whatever remains
    // must be that trailer and nothing a renderer could execute.
    let trailer = &bytes[private_end..];
    let _ = length3; // Informational: PDF 2.0 makes it optional.
    if trailer.len() > 4096
        || !(trailer
            .iter()
            .all(|b| b.is_ascii_whitespace() || *b == b'0')
            || trailer_is_cleartomark(trailer))
    {
        return Err(INVALID.into());
    }
    let (rights, encoding) = cleartext(&bytes[..clear_end])?;
    let encrypted = &bytes[clear_end..private_end];
    // ISO 32000-1 9.9: the private part is embedded in binary. A hexadecimal
    // private part starts with four hex digits; refuse rather than guess.
    if encrypted[..4].iter().all(u8::is_ascii_hexdigit) {
        return Err("hexadecimal Type 1 private data is not editable yet".into());
    }
    let private = decrypt(encrypted, 55665);
    let private = &private[4..];
    let private = Private::read(private)?;
    let mut glyphs = BTreeMap::new();
    for (name, charstring) in &private.charstrings {
        let program = private.charstring(charstring)?;
        // A glyph this interpreter cannot follow is not offered. Its absence
        // refuses any text that uses it; other glyphs remain available.
        if let Ok(glyph) = Interpreter::run(&program, &private) {
            glyphs.insert(name.clone(), glyph);
        }
    }
    Ok(Program {
        glyphs,
        rights,
        encoding,
    })
}

fn trailer_is_cleartomark(trailer: &[u8]) -> bool {
    let text: Vec<u8> = trailer
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'0')
        .collect();
    text == b"cleartomark" || text == b"cleartomark{restore}if"
}

// Adobe Type 1 Font Format 7.1: eexec and charstring encryption.
fn decrypt(bytes: &[u8], key: u16) -> Vec<u8> {
    let mut r = key;
    bytes
        .iter()
        .map(|&c| {
            let plain = c ^ (r >> 8) as u8;
            r = (u16::from(c).wrapping_add(r))
                .wrapping_mul(52845)
                .wrapping_add(22719);
            plain
        })
        .collect()
}

#[cfg(test)]
pub(super) fn encrypt(bytes: &[u8], key: u16) -> Vec<u8> {
    let mut r = key;
    bytes
        .iter()
        .map(|&plain| {
            let c = plain ^ (r >> 8) as u8;
            r = (u16::from(c).wrapping_add(r))
                .wrapping_mul(52845)
                .wrapping_add(22719);
            c
        })
        .collect()
}

// A minimal PostScript tokenizer for literal declarations: names, numbers,
// brackets, braces and bare words. Strings and comments are skipped whole.
#[derive(Debug, PartialEq)]
enum Token<'a> {
    Name(&'a [u8]),
    Word(&'a [u8]),
    Number(f64),
    Open,
    Close,
    Other,
}

struct Tokens<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Tokens<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn delimiter(byte: u8) -> bool {
        matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
    }

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let byte = *self.bytes.get(self.at)?;
            if byte.is_ascii_whitespace() || byte == 0 {
                self.at += 1;
            } else if byte == b'%' {
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|b| !matches!(b, b'\r' | b'\n'))
                {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
        let start = self.at;
        let byte = self.bytes[start];
        self.at += 1;
        Some(match byte {
            b'[' => Token::Open,
            b']' => Token::Close,
            b'{' | b'}' | b'<' | b'>' => Token::Other,
            b'(' => {
                // Balanced parentheses with backslash escapes (PLRM 3.2.2).
                let mut depth = 1;
                while depth > 0 {
                    match self.bytes.get(self.at) {
                        None => return Some(Token::Other),
                        Some(b'\\') => self.at += 1,
                        Some(b'(') => depth += 1,
                        Some(b')') => depth -= 1,
                        _ => {}
                    }
                    self.at += 1;
                }
                Token::Other
            }
            b')' => Token::Other,
            _ => {
                let mut end = self.at;
                while self
                    .bytes
                    .get(end)
                    .is_some_and(|&b| !b.is_ascii_whitespace() && b != 0 && !Self::delimiter(b))
                {
                    end += 1;
                }
                self.at = end;
                if byte == b'/' {
                    Token::Name(&self.bytes[start + 1..end])
                } else {
                    let word = &self.bytes[start..end];
                    match std::str::from_utf8(word)
                        .ok()
                        .and_then(|text| text.parse::<f64>().ok())
                    {
                        Some(value) if value.is_finite() => Token::Number(value),
                        _ => Token::Word(word),
                    }
                }
            }
        })
    }
}

// Returns FontInfo /FSType when present. The matrix must be the standard one:
// PDF widths and the editor's bounds are thousandths of text space.
fn cleartext(bytes: &[u8]) -> Result<(Option<i64>, Builtin), String> {
    if !(bytes.starts_with(b"%!PS-AdobeFont-1.") || bytes.starts_with(b"%!FontType1-1.")) {
        return Err(INVALID.into());
    }
    let trimmed = bytes.trim_ascii_end();
    if !trimmed.ends_with(b"currentfile eexec") {
        return Err(INVALID.into());
    }
    let mut tokens = Tokens::new(bytes);
    let mut matrix = None;
    let mut font_type = None;
    let mut paint_type = None;
    let mut rights = None;
    let mut encoding = None;
    let mut previous: Option<Token<'_>> = None;
    while let Some(token) = tokens.next() {
        if let Some(Token::Name(key)) = previous {
            match key {
                b"FontMatrix" => {
                    if token != Token::Open {
                        return Err(INVALID.into());
                    }
                    let mut values = Vec::new();
                    loop {
                        match tokens.next().ok_or(INVALID)? {
                            Token::Number(value) if values.len() < 6 => values.push(value),
                            Token::Close => break,
                            _ => return Err(INVALID.into()),
                        }
                    }
                    if matrix.replace(values).is_some() {
                        return Err(INVALID.into());
                    }
                    previous = None;
                    continue;
                }
                b"Encoding" => {
                    let value = builtin(token, &mut tokens)?;
                    if encoding.replace(value).is_some() {
                        return Err(INVALID.into());
                    }
                    previous = None;
                    continue;
                }
                b"FontType" | b"PaintType" | b"FSType" => {
                    let Token::Number(value) = token else {
                        return Err(INVALID.into());
                    };
                    if value.fract() != 0. || tokens.next() != Some(Token::Word(b"def")) {
                        return Err(INVALID.into());
                    }
                    let slot = match key {
                        b"FontType" => &mut font_type,
                        b"PaintType" => &mut paint_type,
                        _ => &mut rights,
                    };
                    if slot.replace(value as i64).is_some() {
                        return Err(INVALID.into());
                    }
                    previous = None;
                    continue;
                }
                _ => {}
            }
        }
        previous = Some(token);
    }
    // Type 1 only; PaintType 2 strokes the outlines, which the fill-only
    // bounds would underestimate.
    if font_type != Some(1)
        || paint_type.is_some_and(|value| value != 0)
        || matrix.as_deref() != Some(&[0.001, 0., 0., 0.001, 0., 0.][..])
    {
        return Err(INVALID.into());
    }
    Ok((rights, encoding.ok_or(INVALID)?))
}

// Adobe Type 1 Font Format 2.3: `/Encoding StandardEncoding def`, or a literal
// array filled by `dup <code> /<name> put` after the conventional .notdef loop.
// The loop is matched token for token; nothing else is executed.
fn builtin<'a>(first: Token<'a>, tokens: &mut Tokens<'a>) -> Result<Builtin, String> {
    if first == Token::Word(b"StandardEncoding") {
        if tokens.next() != Some(Token::Word(b"def")) {
            return Err(INVALID.into());
        }
        return Ok(Builtin::Standard);
    }
    if first != Token::Number(256.) || tokens.next() != Some(Token::Word(b"array")) {
        return Err(INVALID.into());
    }
    let mut names: Box<[Option<Vec<u8>>; 256]> = Box::new(std::array::from_fn(|_| None));
    let mut token = tokens.next().ok_or(INVALID)?;
    if token == Token::Number(0.) {
        for expected in [
            Token::Number(1.),
            Token::Number(255.),
            Token::Other,
            Token::Number(1.),
            Token::Word(b"index"),
            Token::Word(b"exch"),
            Token::Name(b".notdef"),
            Token::Word(b"put"),
            Token::Other,
            Token::Word(b"for"),
        ] {
            if tokens.next() != Some(expected) {
                return Err(INVALID.into());
            }
        }
        token = tokens.next().ok_or(INVALID)?;
    }
    loop {
        match token {
            Token::Word(b"dup") => {
                let code = match tokens.next() {
                    Some(Token::Number(code))
                        if code.fract() == 0. && (0. ..=255.).contains(&code) =>
                    {
                        code as usize
                    }
                    _ => return Err(INVALID.into()),
                };
                let Some(Token::Name(name)) = tokens.next() else {
                    return Err(INVALID.into());
                };
                if tokens.next() != Some(Token::Word(b"put")) || name.len() > 127 {
                    return Err(INVALID.into());
                }
                names[code] = (name != b".notdef").then(|| name.to_vec());
            }
            Token::Word(b"readonly") => {
                if tokens.next() != Some(Token::Word(b"def")) {
                    return Err(INVALID.into());
                }
                break;
            }
            Token::Word(b"def") => break,
            _ => return Err(INVALID.into()),
        }
        token = tokens.next().ok_or(INVALID)?;
    }
    Ok(Builtin::Custom(names))
}

struct Private {
    len_iv: Option<usize>,
    subrs: Vec<Vec<u8>>,
    charstrings: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Private {
    // Adobe Type 1 Font Format 5.1 and 5.3: Subrs entries are
    // `dup <index> <length> RD <binary> NP` and CharStrings entries are
    // `/<name> <length> RD <binary> ND`, with RD/NP/ND spelled either way.
    fn read(bytes: &[u8]) -> Result<Self, String> {
        let len_iv = match find_token(bytes, b"/lenIV") {
            None => Some(4),
            Some(at) => {
                let mut tokens = Tokens::new(&bytes[at..]);
                match (tokens.next(), tokens.next()) {
                    (Some(Token::Number(-1.)), Some(Token::Word(b"def"))) => None,
                    (Some(Token::Number(value)), Some(Token::Word(b"def")))
                        if value.fract() == 0. && (0. ..=4.).contains(&value) =>
                    {
                        Some(value as usize)
                    }
                    _ => return Err(INVALID.into()),
                }
            }
        };
        let mut subrs = Vec::new();
        let charstrings_at = find_token(bytes, b"/CharStrings").ok_or(INVALID)?;
        if let Some(at) = find_token(&bytes[..charstrings_at], b"/Subrs") {
            let mut cursor = Cursor { bytes, at };
            let count = cursor.count(b"array", MAX_SUBRS)?;
            subrs = vec![Vec::new(); count];
            let mut seen = vec![false; count];
            for _ in 0..count {
                cursor.word(b"dup")?;
                let index = cursor.integer()?;
                if index >= count || std::mem::replace(&mut seen[index], true) {
                    return Err(INVALID.into());
                }
                subrs[index] = cursor.binary()?.to_vec();
                cursor.word_of(&[b"NP", b"|"], Some(b"noaccess put"))?;
            }
            if cursor.at > charstrings_at {
                return Err(INVALID.into());
            }
        }
        let mut cursor = Cursor {
            bytes,
            at: charstrings_at,
        };
        let count = cursor.count(b"dict", MAX_GLYPHS)?;
        cursor.word(b"dup")?;
        cursor.word(b"begin")?;
        let mut charstrings = Vec::with_capacity(count);
        let mut names = std::collections::BTreeSet::new();
        loop {
            let token = cursor.token()?;
            match token {
                Token::Word(b"end") => break,
                Token::Name(name) => {
                    if name.is_empty() || name.len() > 127 || !names.insert(name.to_vec()) {
                        return Err(INVALID.into());
                    }
                    let program = cursor.binary()?.to_vec();
                    cursor.word_of(&[b"ND", b"|-"], Some(b"noaccess def"))?;
                    charstrings.push((name.to_vec(), program));
                    if charstrings.len() > count {
                        return Err(INVALID.into());
                    }
                }
                _ => return Err(INVALID.into()),
            }
        }
        if !names.contains(b".notdef".as_slice()) {
            return Err(INVALID.into());
        }
        Ok(Self {
            len_iv,
            subrs,
            charstrings,
        })
    }

    fn charstring(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        match self.len_iv {
            None => Ok(bytes.to_vec()),
            Some(skip) => {
                if bytes.len() < skip {
                    return Err(INVALID.into());
                }
                Ok(decrypt(bytes, 4330)[skip..].to_vec())
            }
        }
    }
}

// The first occurrence of `key` as a whole token, outside binary data only in
// the sense that the caller parses structurally from it and refuses on any
// disagreement: a match inside a charstring cannot produce a valid entry list.
fn find_token(bytes: &[u8], key: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = bytes[from..].windows(key.len()).position(|w| w == key) {
        let at = from + offset;
        let end = at + key.len();
        if bytes
            .get(end)
            .is_none_or(|&b| b.is_ascii_whitespace() || Tokens::delimiter(b))
        {
            return Some(at + key.len());
        }
        from = at + 1;
    }
    None
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn token(&mut self) -> Result<Token<'a>, String> {
        let mut tokens = Tokens::new(self.bytes);
        tokens.at = self.at;
        let token = tokens.next().ok_or(INVALID)?;
        self.at = tokens.at;
        Ok(token)
    }

    fn word(&mut self, word: &[u8]) -> Result<(), String> {
        if self.token()? != Token::Word(word) {
            return Err(INVALID.into());
        }
        Ok(())
    }

    // One of the short procedure names, or its expansion as two words.
    fn word_of(&mut self, words: &[&[u8]], expanded: Option<&[u8]>) -> Result<(), String> {
        match self.token()? {
            Token::Word(word) if words.contains(&word) => Ok(()),
            Token::Word(first) if expanded.is_some_and(|text| text.starts_with(first)) => {
                let Token::Word(second) = self.token()? else {
                    return Err(INVALID.into());
                };
                let mut joined = first.to_vec();
                joined.push(b' ');
                joined.extend_from_slice(second);
                if Some(joined.as_slice()) != expanded {
                    return Err(INVALID.into());
                }
                Ok(())
            }
            _ => Err(INVALID.into()),
        }
    }

    fn integer(&mut self) -> Result<usize, String> {
        match self.token()? {
            Token::Number(value) if value.fract() == 0. && (0. ..=65_535.).contains(&value) => {
                Ok(value as usize)
            }
            _ => Err(INVALID.into()),
        }
    }

    // `<count> array` or `<count> dict`, bounded.
    fn count(&mut self, kind: &[u8], limit: usize) -> Result<usize, String> {
        let count = self.integer()?;
        self.word(kind)?;
        if count > limit {
            return Err(INVALID.into());
        }
        Ok(count)
    }

    // `<length> RD <one space><length bytes>`, RD spelled either way.
    fn binary(&mut self) -> Result<&'a [u8], String> {
        let length = self.integer()?;
        match self.token()? {
            Token::Word(b"RD" | b"-|") => {}
            _ => return Err(INVALID.into()),
        }
        if self.bytes.get(self.at) != Some(&b' ') {
            return Err(INVALID.into());
        }
        let start = self.at + 1;
        let end = start.checked_add(length).ok_or(INVALID)?;
        let data = self.bytes.get(start..end).ok_or(INVALID)?;
        self.at = end;
        Ok(data)
    }
}

struct Interpreter<'a> {
    private: &'a Private,
    stack: Vec<f64>,
    // Results of an OtherSubrs call, returned one per `pop` (6.3).
    returns: Vec<f64>,
    point: (f64, f64),
    width: Option<f64>,
    bounds: Option<[f64; 4]>,
    // The last point before a drawing operation, added once it draws.
    pending: Option<(f64, f64)>,
    flex: Option<Vec<(f64, f64)>>,
    operations: usize,
}

impl<'a> Interpreter<'a> {
    fn run(program: &[u8], private: &'a Private) -> Result<Glyph, String> {
        let mut machine = Self {
            private,
            stack: Vec::new(),
            returns: Vec::new(),
            point: (0., 0.),
            width: None,
            bounds: None,
            pending: None,
            flex: None,
            operations: 0,
        };
        if !machine.execute(program, 0)? {
            return Err(INVALID.into()); // Ran off the end without endchar.
        }
        let width = machine.width.ok_or(INVALID)?;
        Ok(Glyph {
            width,
            bounds: machine.bounds,
        })
    }

    fn include(&mut self, (x, y): (f64, f64)) {
        self.bounds = Some(match self.bounds {
            Some([left, bottom, right, top]) => {
                [left.min(x), bottom.min(y), right.max(x), top.max(y)]
            }
            None => [x, y, x, y],
        });
    }

    fn draw(&mut self, points: &[(f64, f64)]) -> Result<(), String> {
        if self.width.is_none() || self.flex.is_some() {
            return Err(INVALID.into());
        }
        if let Some(start) = self.pending.take() {
            self.include(start);
        }
        for &point in points {
            if !(point.0.is_finite() && point.1.is_finite())
                || point.0.abs() > 100_000.
                || point.1.abs() > 100_000.
            {
                return Err(INVALID.into());
            }
            self.include(point);
        }
        self.point = *points.last().ok_or(INVALID)?;
        Ok(())
    }

    fn relative(&self, dx: f64, dy: f64) -> (f64, f64) {
        (self.point.0 + dx, self.point.1 + dy)
    }

    fn move_to(&mut self, point: (f64, f64)) -> Result<(), String> {
        if self.width.is_none() {
            return Err(INVALID.into());
        }
        self.point = point;
        // Adobe Type 1 Font Format 8.3: inside a flex, rmoveto only records
        // the next control point; the curves are drawn when the flex ends.
        if let Some(points) = &mut self.flex {
            // Counted where the flex ends: exactly seven, or the glyph fails.
            points.push(point);
        } else {
            self.pending = Some(point);
        }
        Ok(())
    }

    fn take<const N: usize>(&mut self) -> Result<[f64; N], String> {
        if self.stack.len() != N {
            return Err(INVALID.into());
        }
        let mut values = [0.; N];
        values.copy_from_slice(&self.stack);
        self.stack.clear();
        Ok(values)
    }

    // Returns true when the glyph ended (endchar), false on `return`.
    fn execute(&mut self, program: &[u8], depth: usize) -> Result<bool, String> {
        if depth > MAX_DEPTH {
            return Err(INVALID.into());
        }
        let mut at = 0;
        while at < program.len() {
            self.operations += 1;
            if self.operations > MAX_OPERATIONS {
                return Err(INVALID.into());
            }
            let byte = program[at];
            at += 1;
            if byte >= 32 {
                let value = match byte {
                    32..=246 => f64::from(byte) - 139.,
                    247..=250 => {
                        let next = *program.get(at).ok_or(INVALID)?;
                        at += 1;
                        f64::from(byte - 247) * 256. + f64::from(next) + 108.
                    }
                    251..=254 => {
                        let next = *program.get(at).ok_or(INVALID)?;
                        at += 1;
                        -(f64::from(byte - 251) * 256.) - f64::from(next) - 108.
                    }
                    _ => {
                        let bytes = program.get(at..at + 4).ok_or(INVALID)?;
                        at += 4;
                        f64::from(i32::from_be_bytes(bytes.try_into().map_err(|_| INVALID)?))
                    }
                };
                if self.stack.len() >= MAX_STACK {
                    return Err(INVALID.into());
                }
                self.stack.push(value);
                continue;
            }
            let operator = if byte == 12 {
                let next = *program.get(at).ok_or(INVALID)?;
                at += 1;
                1200 + u16::from(next)
            } else {
                u16::from(byte)
            };
            match operator {
                // hsbw: sidebearing x, advance x.
                13 => {
                    let [sbx, wx] = self.take()?;
                    if self.width.replace(wx).is_some() {
                        return Err(INVALID.into());
                    }
                    self.point = (sbx, 0.);
                }
                // sbw: sidebearing and advance vectors; only horizontal fonts.
                1207 => {
                    let [sbx, sby, wx, wy] = self.take()?;
                    if wy != 0. || self.width.replace(wx).is_some() {
                        return Err(INVALID.into());
                    }
                    self.point = (sbx, sby);
                }
                // Hints and dotsection change no outline.
                1 | 3 => {
                    self.take::<2>()?;
                }
                1201 | 1202 => {
                    self.take::<6>()?;
                }
                1200 => {
                    self.take::<0>()?;
                }
                21 => {
                    let [dx, dy] = self.take()?;
                    self.move_to(self.relative(dx, dy))?;
                }
                22 => {
                    let [dx] = self.take()?;
                    self.move_to(self.relative(dx, 0.))?;
                }
                4 => {
                    let [dy] = self.take()?;
                    self.move_to(self.relative(0., dy))?;
                }
                5 => {
                    let [dx, dy] = self.take()?;
                    self.draw(&[self.relative(dx, dy)])?;
                }
                6 => {
                    let [dx] = self.take()?;
                    self.draw(&[self.relative(dx, 0.)])?;
                }
                7 => {
                    let [dy] = self.take()?;
                    self.draw(&[self.relative(0., dy)])?;
                }
                8 => {
                    let [dx1, dy1, dx2, dy2, dx3, dy3] = self.take()?;
                    self.curve([dx1, dy1, dx2, dy2, dx3, dy3])?;
                }
                30 => {
                    let [dy1, dx2, dy2, dx3] = self.take()?;
                    self.curve([0., dy1, dx2, dy2, dx3, 0.])?;
                }
                31 => {
                    let [dx1, dx2, dy2, dy3] = self.take()?;
                    self.curve([dx1, 0., dx2, dy2, 0., dy3])?;
                }
                9 => {
                    self.take::<0>()?;
                    if self.width.is_none() {
                        return Err(INVALID.into());
                    }
                }
                14 => {
                    self.take::<0>()?;
                    if self.width.is_none() || self.flex.is_some() || !self.returns.is_empty() {
                        return Err(INVALID.into());
                    }
                    return Ok(true);
                }
                11 => {
                    return Ok(false);
                }
                10 => {
                    let index = self.stack.pop().ok_or(INVALID)?;
                    if index.fract() != 0. || index < 0. {
                        return Err(INVALID.into());
                    }
                    let subr = self.private.subrs.get(index as usize).ok_or(INVALID)?;
                    let subr = self.private.charstring(subr)?;
                    if self.execute(&subr, depth + 1)? {
                        return Ok(true);
                    }
                }
                // div
                1212 => {
                    let b = self.stack.pop().ok_or(INVALID)?;
                    let a = self.stack.pop().ok_or(INVALID)?;
                    if b == 0. {
                        return Err(INVALID.into());
                    }
                    self.stack.push(a / b);
                }
                // callothersubr: only the flex and hint-replacement entries
                // every Type 1 font carries (Adobe Type 1 Font Format 8).
                1216 => {
                    let number = self.stack.pop().ok_or(INVALID)?;
                    let count = self.stack.pop().ok_or(INVALID)?;
                    if count.fract() != 0. || count < 0. || count as usize > self.stack.len() {
                        return Err(INVALID.into());
                    }
                    let args = self.stack.split_off(self.stack.len() - count as usize);
                    match (number as i64, args.as_slice()) {
                        (1, []) => {
                            if self.flex.replace(Vec::new()).is_some() {
                                return Err(INVALID.into());
                            }
                        }
                        (2, []) => {
                            if self.flex.is_none() {
                                return Err(INVALID.into());
                            }
                        }
                        (0, [_, x, y]) => {
                            let points = self.flex.take().ok_or(INVALID)?;
                            if points.len() != 7 {
                                return Err(INVALID.into());
                            }
                            // Point 0 is the reference point; the six after
                            // it are two curves' controls and end points.
                            self.pending = Some(points[0]);
                            self.draw(&points[1..])?;
                            // Returned for `pop pop setcurrentpoint`.
                            self.returns = vec![*y, *x];
                        }
                        (3, [subr]) => self.returns = vec![*subr],
                        _ => return Err(INVALID.into()),
                    }
                }
                // pop
                1217 => {
                    let value = self.returns.pop().ok_or(INVALID)?;
                    if self.stack.len() >= MAX_STACK {
                        return Err(INVALID.into());
                    }
                    self.stack.push(value);
                }
                // setcurrentpoint
                1233 => {
                    let [x, y] = self.take()?;
                    self.point = (x, y);
                }
                // seac and anything else: not followed yet.
                _ => return Err(INVALID.into()),
            }
        }
        Ok(false)
    }

    fn curve(&mut self, [dx1, dy1, dx2, dy2, dx3, dy3]: [f64; 6]) -> Result<(), String> {
        let first = self.relative(dx1, dy1);
        let second = (first.0 + dx2, first.1 + dy2);
        let end = (second.0 + dx3, second.1 + dy3);
        self.draw(&[first, second, end])
    }
}
