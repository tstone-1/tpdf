//! Whether what follows a signature is written so plainly that two PDF parsers
//! cannot read it differently.
//!
//! `read_appendix` compares two `lopdf` parses and PDFium draws the page, so an
//! append that the two read differently can change a signed page while the
//! comparison sees nothing. No list of differences would end, so this asks the
//! other way round: every byte of the append has to be an object, a
//! cross-reference table, a trailer or a `startxref`, in their plain forms, and
//! anything else reads as unread, which is never read as nothing.
//!
//! **What each refusal is for.** Each was traced in PDFium or `lopdf`:
//! a number over 32 characters (PDFium cuts a word at 256, `lopdf` reads it all); a
//! name byte outside ASCII (PDFium ends the name there, so `/Prev\x80` is a second
//! `/Prev`); a key given twice; a table entry not 20 bytes long (PDFium rebuilds from
//! a scan, where it finds objects no table lists) or with a generation over 65535
//! (`lopdf` drops it, PDFium takes it); `xstartxref` (`lopdf` matches it, PDFium does
//! not); a comment (it can pad a table, and hold a `startxref` after the last one),
//! except the producer's marker iText writes on the line before `startxref`;
//! a stream whose length does not reach `endstream` (PDFium looks for `endstream`
//! instead); and an array `/Filter` with a dictionary `/DecodeParms`, or the other
//! way round (`lopdf` applies the parameters, PDFium does not).
//!
//! **What it costs and what it leaves.** Over the 153 appendices in the public DSS
//! validation files it changes no reading. It asks a table's entries what they
//! name, and not a cross-reference stream's, and does not follow `/Prev`: a prototype
//! requiring the whole chain to agree accepted only 106 of 156 appends, so those stay
//! open (`docs/THREAT-MODEL.md`).

use std::collections::BTreeMap;

use lopdf::ObjectId;

/// How deep arrays and dictionaries nest in an append. Nothing honest comes close,
/// and this is recursion over the document's own bytes.
const MAX_DEPTH: usize = 64;

/// The longest number. Well short of the 256 at which PDFium cuts a word.
const MAX_NUMBER: usize = 32;

/// The longest comment taken before `startxref`.
const MAX_MARKER: usize = 64;

/// The most a generation number can be.
const MAX_GENERATION: u32 = 65535;

/// A value, as far as this asks.
enum Value {
    Dictionary(BTreeMap<Vec<u8>, Value>),
    Array,
    Integer(i64),
    Reference(ObjectId),
    Other,
}

/// Whether `bytes` after `end` is plain. `pub` for the fuzz target alone.
///
/// `length` answers for a stream's `/Length` given as a reference: the object
/// is an integer in the parse this is checking, and `None` otherwise.
pub fn reads_plainly(bytes: &[u8], end: usize, length: impl Fn(ObjectId) -> Option<i64>) -> bool {
    Reader {
        bytes,
        at: end,
        length,
    }
    .append()
    .is_some()
}

struct Reader<'a, L> {
    bytes: &'a [u8],
    at: usize,
    length: L,
}

fn is_white(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

impl<L: Fn(ObjectId) -> Option<i64>> Reader<'_, L> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn white(&mut self) {
        while self.peek().is_some_and(is_white) {
            self.at += 1;
        }
    }

    /// Takes `word`, when it stands here as a word of its own.
    fn word(&mut self, word: &[u8]) -> bool {
        let Some(rest) = self.bytes.get(self.at..) else {
            return false;
        };
        let ended = rest
            .get(word.len())
            .is_none_or(|next| is_white(*next) || is_delimiter(*next));
        if rest.starts_with(word) && ended {
            self.at += word.len();
            return true;
        }
        false
    }

    /// Revisions to the end: anything else, or a revision left without its
    /// `%%EOF`, is `None`.
    fn append(&mut self) -> Option<()> {
        let mut open = false;
        loop {
            self.white();
            if self.peek().is_none() {
                return (!open).then_some(());
            }
            if self.peek() == Some(b'%') {
                self.marker()?;
            } else if self.word(b"xref") {
                self.table()?;
                open = true;
            } else if self.word(b"startxref") {
                self.white();
                self.digits(20)?;
                self.white();
                if !self.word(b"%%EOF") {
                    return None;
                }
                open = false;
            } else {
                self.object()?;
                open = true;
            }
        }
    }

    /// The one comment taken: a short line of printable ASCII naming a producer,
    /// directly before `startxref`, with no word in it that a parser searches for.
    /// iText writes one in every revision, as `%iText-5.5.3`.
    fn marker(&mut self) -> Option<()> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|byte| byte != b'\r' && byte != b'\n')
        {
            self.at += 1;
        }
        let line = &self.bytes[start..self.at];
        let names_structure = [&b"xref"[..], b"EOF", b"obj", b"trailer", b"stream"]
            .iter()
            .any(|word| line.windows(word.len()).any(|at| at == *word));
        if line.len() > MAX_MARKER
            || names_structure
            || !line.iter().all(|byte| (0x20..=0x7e).contains(byte))
        {
            return None;
        }
        self.white();
        self.bytes[self.at..]
            .starts_with(b"startxref")
            .then_some(())
    }

    /// Up to `most` digits, as a number.
    fn digits(&mut self, most: usize) -> Option<u64> {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        let digits = &self.bytes[start..self.at];
        if digits.is_empty() || digits.len() > most {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    }

    /// The subsections and the trailer of a table, after its `xref`.
    fn table(&mut self) -> Option<()> {
        loop {
            self.white();
            if self.word(b"trailer") {
                break;
            }
            let first = self.digits(10)?;
            self.white();
            for number in 0..self.digits(10)? {
                // The line's end is left to the entry; a count on a line of its
                // own has white space before the first.
                self.white();
                self.entry(first.checked_add(number)?)?;
            }
        }
        self.white();
        matches!(self.value(0)?, Value::Dictionary(_)).then_some(())
    }

    /// `nnnnnnnnnn ggggg n` and its two-byte line end: 20 bytes, as PDF 32000-1
    /// 7.5.4 has it.
    ///
    /// **An entry in use has to name the object at its offset.** `lopdf` keys an
    /// object by the number in its own header and PDFium by the table's, so an entry
    /// for an unused 999 at the offset of the signed `7 0 obj` has `lopdf` read the
    /// signed 7 while PDFium draws the 7 this append wrote.
    fn entry(&mut self, number: u64) -> Option<()> {
        let entry = self.bytes.get(self.at..self.at + 20)?;
        let digits = |range: std::ops::Range<usize>| entry[range].iter().all(u8::is_ascii_digit);
        let generation: u32 = std::str::from_utf8(&entry[11..16]).ok()?.parse().ok()?;
        let offset: usize = std::str::from_utf8(&entry[0..10]).ok()?.parse().ok()?;
        let in_use = entry[17] == b'n';
        (digits(0..10)
            && entry[10] == b' '
            && digits(11..16)
            && generation <= MAX_GENERATION
            && entry[16] == b' '
            && (in_use || entry[17] == b'f')
            && is_white(entry[18])
            && is_white(entry[19])
            && (!in_use || self.names_object(offset, number)))
        .then(|| self.at += 20)
    }

    /// Whether `n g obj` with this `number` stands at `offset`, counted from the
    /// file's first byte or from a `%PDF-` with bytes before it, as PDFium counts.
    fn names_object(&self, offset: usize, number: u64) -> bool {
        let header = self
            .bytes
            .windows(5)
            .take(1024)
            .position(|at| at == b"%PDF-")
            .unwrap_or(0);
        [offset, offset.saturating_add(header)]
            .into_iter()
            .filter(|offset| *offset < self.bytes.len())
            .any(|offset| {
                let mut there = Reader {
                    bytes: self.bytes,
                    at: offset,
                    length: |_| None,
                };
                there.white();
                let named = there.digits(10);
                there.white();
                there.digits(5).is_some() && {
                    there.white();
                    named == Some(number) && there.word(b"obj")
                }
            })
    }

    /// `n g obj`, a value, a stream if there is one, `endobj`.
    fn object(&mut self) -> Option<()> {
        self.digits(10)?;
        self.white();
        if self.digits(5)? > u64::from(MAX_GENERATION) {
            return None;
        }
        self.white();
        if !self.word(b"obj") {
            return None;
        }
        self.white();
        let value = self.value(0)?;
        self.white();
        if self.word(b"stream") {
            self.stream(&value)?;
            self.white();
        }
        self.word(b"endobj").then_some(())
    }

    /// A stream's bytes, by its `/Length`, which must end at `endstream`.
    fn stream(&mut self, value: &Value) -> Option<()> {
        let Value::Dictionary(dict) = value else {
            return None;
        };
        // Parameters of another shape than the filters they go with.
        match (
            dict.get(b"Filter".as_slice()),
            dict.get(b"DecodeParms".as_slice()),
        ) {
            (Some(Value::Array), Some(Value::Dictionary(_)))
            | (Some(Value::Other), Some(Value::Array)) => return None,
            _ => {}
        }
        let length = match dict.get(b"Length".as_slice())? {
            Value::Integer(length) => *length,
            Value::Reference(id) => (self.length)(*id)?,
            _ => return None,
        };
        if self.bytes[self.at..].starts_with(b"\r\n") {
            self.at += 2;
        } else if self.peek() == Some(b'\n') {
            self.at += 1;
        } else {
            return None;
        }
        self.at = self.at.checked_add(usize::try_from(length).ok()?)?;
        if self.at > self.bytes.len() {
            return None;
        }
        self.white();
        self.word(b"endstream").then_some(())
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek()? {
            b'<' if self.bytes.get(self.at + 1) == Some(&b'<') => {
                self.at += 2;
                self.dictionary(depth)
            }
            b'<' => self.hex(),
            b'[' => {
                self.at += 1;
                self.array(depth)
            }
            b'(' => self.string(),
            b'/' => self.name().map(|_| Value::Other),
            b'0'..=b'9' | b'+' | b'-' | b'.' => self.number(),
            _ => (self.word(b"true") || self.word(b"false") || self.word(b"null"))
                .then_some(Value::Other),
        }
    }

    fn dictionary(&mut self, depth: usize) -> Option<Value> {
        let mut entries = BTreeMap::new();
        loop {
            self.white();
            if self.bytes[self.at..].starts_with(b">>") {
                self.at += 2;
                return Some(Value::Dictionary(entries));
            }
            let key = self.name()?;
            self.white();
            let value = self.value(depth + 1)?;
            if entries.insert(key, value).is_some() {
                return None;
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        loop {
            self.white();
            if self.peek()? == b']' {
                self.at += 1;
                return Some(Value::Array);
            }
            self.value(depth + 1)?;
        }
    }

    /// A name's bytes without its slash. Printable ASCII and nothing else: a `#`
    /// escape both parsers decode alike is left to [`super::same_object`].
    fn name(&mut self) -> Option<Vec<u8>> {
        if self.peek()? != b'/' {
            return None;
        }
        self.at += 1;
        let start = self.at;
        while let Some(byte) = self.peek() {
            if is_white(byte) || is_delimiter(byte) {
                break;
            }
            if !(0x21..=0x7e).contains(&byte) {
                return None;
            }
            self.at += 1;
        }
        Some(self.bytes[start..self.at].to_vec())
    }

    fn hex(&mut self) -> Option<Value> {
        self.at += 1;
        loop {
            match self.peek()? {
                b'>' => {
                    self.at += 1;
                    return Some(Value::Other);
                }
                byte if byte.is_ascii_hexdigit() || is_white(byte) => self.at += 1,
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<Value> {
        let mut open = 0usize;
        loop {
            match self.peek()? {
                b'\\' => self.at += 1,
                b'(' => open += 1,
                b')' => open -= 1,
                _ => {}
            }
            self.at += 1;
            if open == 0 {
                return Some(Value::Other);
            }
        }
    }

    /// A number, or a reference when one begins here.
    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let digits = self.at;
        let mut dots = 0;
        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' => {}
                b'.' => dots += 1,
                _ => break,
            }
            self.at += 1;
        }
        let ended = self
            .peek()
            .is_none_or(|next| is_white(next) || is_delimiter(next));
        let word = &self.bytes[start..self.at];
        if !ended || word.len() > MAX_NUMBER || dots > 1 || self.at - digits == dots {
            return None;
        }
        if dots > 0 {
            return Some(Value::Other);
        }
        let text = std::str::from_utf8(word).ok()?;
        let integer: i64 = text.parse().ok()?;
        Some(
            self.reference(integer)
                .map_or(Value::Integer(integer), Value::Reference),
        )
    }

    /// `g R` after the number `first`, or nothing with the place unmoved.
    fn reference(&mut self, first: i64) -> Option<ObjectId> {
        let back = self.at;
        let found = (|| {
            let number = u32::try_from(first).ok()?;
            self.white();
            let generation = self.digits(5)?;
            self.white();
            self.word(b"R").then_some(())?;
            Some((number, u16::try_from(generation).ok()?))
        })();
        if found.is_none() {
            self.at = back;
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::reads_plainly;

    fn plain(append: &[u8]) -> bool {
        reads_plainly(append, 0, |id| (id == (9, 0)).then_some(4))
    }

    const TAIL: &[u8] =
        b"xref\n5 1\n0000000010 00000 f \ntrailer\n<</Size 6/Prev 9>>\nstartxref\n100\n%%EOF\n";

    fn revision(object: &[u8]) -> Vec<u8> {
        [object, b"\n", TAIL].concat()
    }

    #[test]
    fn what_a_signer_writes_is_plain() {
        for (case, append) in [
            ("nothing", b"".to_vec()),
            ("a line end", b"\r\n".to_vec()),
            ("a table", TAIL.to_vec()),
            ("an object", revision(b"5 0 obj\n<</A 1 0 R/B[1 2.5 -3 .5 (a\\)(b)) <0aF> /N true null]>>\nendobj")),
            ("no white space", revision(b"5 0 obj<</A/B>>endobj")),
            ("a name with an escape", revision(b"5 0 obj /A#20B endobj")),
            ("a stream", revision(b"5 0 obj <</Length 4>> stream\nendo\nendstream endobj")),
            ("CRLF before a stream", revision(b"5 0 obj <</Length 4>> stream\r\nabcd\r\nendstream\r\nendobj")),
            ("an indirect length", revision(b"5 0 obj <</Length 9 0 R>> stream\nabcd\nendstream endobj")),
            ("a stream section", b"5 0 obj <</Type/XRef/Length 1/Filter/FlateDecode/DecodeParms<</Columns 4>>>> stream\nx\nendstream endobj\nstartxref\n1\n%%EOF".to_vec()),
            ("two revisions", [TAIL, TAIL].concat()),
            ("a producer's marker", [b"5 0 obj null endobj\n%iText-5.5.3\nstartxref\n1\n%%EOF\n".as_slice()].concat()),
            ("binary in a stream", revision(b"5 0 obj <</Length 4>> stream\n%\xff\x80%\nendstream endobj")),
        ] {
            assert!(plain(&append), "{case}");
        }
    }

    #[test]
    fn what_two_parsers_could_read_differently_is_not_plain() {
        let digits = "1".repeat(300);
        // Numbers that parse whatever their length, so only the length refuses them.
        let zeros = format!("{}1", "0".repeat(299));
        let zeros_start = format!("startxref\n{zeros}\n%%EOF\n");
        let zeros_prev = format!("xref\n0 0\ntrailer\n<</Prev {zeros}>>\nstartxref\n1\n%%EOF\n");
        let long_start = format!("startxref\n{digits}\n%%EOF\n");
        let long_prev = format!("xref\n0 0\ntrailer\n<</Prev {digits}>>\nstartxref\n1\n%%EOF\n");
        for (case, append) in [
            ("a long startxref of zeros", zeros_start.into_bytes()),
            ("a long /Prev of zeros", zeros_prev.into_bytes()),
            ("a stream that closes as an object", revision(b"5 0 obj <</Length 0>> stream\nendobj")),
            ("a last startxref with no %%EOF", [TAIL, b"startxref\n1\n"].concat()),
            ("a word that holds startxref", b"xstartxref\n1\n%%EOF\n".to_vec()),
            ("a long startxref", long_start.into_bytes()),
            ("a long /Prev", long_prev.into_bytes()),
            ("a key with a byte past ASCII", b"xref\n0 0\ntrailer\n<</Prev 1/Prev\x80 2>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a key given twice", b"xref\n0 0\ntrailer\n<</Prev 1/Prev 2>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a short table entry", b"xref\n5 1\n000000010 00000 n \ntrailer\n<<>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a generation past 65535", b"xref\n5 1\n0000000010 70000 n \ntrailer\n<<>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("an object's generation past 65535", revision(b"5 70000 obj null endobj")),
            ("a comment in a table", b"xref\n%x\n5 1\n0000000010 00000 n \ntrailer\n<<>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a comment after the last revision", [TAIL, b"%startxref 9\n%%EOF\n"].concat()),
            ("a comment before a table", b"%iText\nxref\n0 0\ntrailer\n<<>>\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a comment at the end", [TAIL, b"%iText\n"].concat()),
            ("a marker naming a section", b"%xref 5\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a marker past ASCII", b"%\xff\nstartxref\n1\n%%EOF\n".to_vec()),
            ("a string ending in a backslash", revision(b"5 0 obj (a\\")),
            ("a comment in a value", revision(b"5 0 obj %x\n null endobj")),
            ("a length short of endstream", revision(b"5 0 obj <</Length 0>> stream\n0 0 9 9 re f\nendstream endobj")),
            ("a length past the end", revision(b"5 0 obj <</Length 99999>> stream\nx\nendstream endobj")),
            ("a negative length", revision(b"5 0 obj <</Length -1>> stream\nx\nendstream endobj")),
            ("no length", revision(b"5 0 obj <<>> stream\nx\nendstream endobj")),
            ("a length that is not an integer", revision(b"5 0 obj <</Length 8 0 R>> stream\nx\nendstream endobj")),
            ("a stream with no line end", revision(b"5 0 obj <</Length 1>> stream x\nendstream endobj")),
            ("a CR alone before a stream", revision(b"5 0 obj <</Length 1>> stream\rx\nendstream endobj")),
            ("an array of filters, one set of parameters", revision(b"5 0 obj <</Length 1/Filter[/FlateDecode]/DecodeParms<<>>>> stream\nx\nendstream endobj")),
            ("one filter, an array of parameters", revision(b"5 0 obj <</Length 1/Filter/FlateDecode/DecodeParms[<<>>]>> stream\nx\nendstream endobj")),
            ("a number that is no number", revision(b"5 0 obj [-1.#INF] endobj")),
            ("two signs", revision(b"5 0 obj --1 endobj")),
            ("a stray word", revision(b"junk")),
            ("stray bytes between revisions", [TAIL, b"junk\n", TAIL].concat()),
            ("a revision with no %%EOF", b"5 0 obj null endobj\n".to_vec()),
            ("a table with no startxref", b"xref\n0 0\ntrailer\n<<>>\n".to_vec()),
            ("a string left open", revision(b"5 0 obj (a endobj")),
            ("hex that is not hex", revision(b"5 0 obj <0g> endobj")),
            ("nesting without end", revision(&[b"5 0 obj ".as_slice(), &b"[".repeat(500)].concat())),
        ] {
            assert!(!plain(&append), "{case}");
        }
    }

    /// Past the signature's range and no sooner: what the range covers is signed.
    #[test]
    fn only_what_follows_the_range_is_asked() {
        let bytes = [b"junk".as_slice(), TAIL].concat();
        assert!(!reads_plainly(&bytes, 0, |_| None));
        assert!(reads_plainly(&bytes, 4, |_| None));
    }

    /// An entry for an unused number at the offset of another object: `lopdf` reads
    /// the signed 7 for it, while PDFium draws the 7 the table's own entry names.
    #[test]
    fn a_table_entry_has_to_name_the_object_at_its_offset() {
        let signed = b"%PDF-1.7\n7 0 obj\nnull\nendobj\n";
        let at = |entries: &str| {
            let tail = format!("xref\n{entries}trailer\n<<>>\nstartxref\n1\n%%EOF\n");
            let bytes = [signed.as_slice(), tail.as_bytes()].concat();
            reads_plainly(&bytes, signed.len(), |_| None)
        };
        assert!(at("7 1\n0000000009 00000 n \n"), "its own offset");
        assert!(
            at("7 1\n0000000008 00000 n \n"),
            "one byte short, as honest files are"
        );
        assert!(
            !at("999 1\n0000000009 00000 n \n"),
            "another object's offset"
        );
        assert!(
            !at("7 1\n0000000012 00000 n \n"),
            "the middle of the object"
        );
        assert!(!at("7 1\n0009999999 00000 n \n"), "past the end");
        // Bytes before `%PDF-`, and offsets counted from it.
        let moved = b"xxx%PDF-1.7\n7 0 obj\nnull\nendobj\n";
        let tail = b"xref\n7 1\n0000000009 00000 n \ntrailer\n<<>>\nstartxref\n1\n%%EOF\n";
        let bytes = [moved.as_slice(), tail.as_slice()].concat();
        assert!(
            reads_plainly(&bytes, moved.len(), |_| None),
            "after a prefix"
        );
    }
}
