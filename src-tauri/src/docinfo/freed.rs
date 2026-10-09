//! The objects that cross-reference sections written after a signature mark
//! free: what an append took away without writing an object for it.
//!
//! # Why this is read here and not taken from `lopdf`
//!
//! `read_appendix` compares two `lopdf` parses, and `lopdf` 0.45 does not
//! record a free entry at all: `decode_xref_stream_with_limit` reads a type 0
//! entry and inserts nothing, and the classic table's parser keeps only `n`
//! entries. Its merge of the sections then finds the entry of the revision
//! before, so an object that a later section frees is still in the whole
//! document's object table, byte for byte what it was. The comparison calls it
//! unchanged.
//!
//! Readers do not agree with that, and they do not agree with each other.
//! Measured 2026-10-09 on a one-page document with one section appended that
//! frees the page's content stream, against the same section freeing nothing:
//!
//! ```text
//!                              PDFium 8066        poppler 26
//! freed in a stream section    the text is gone   the text is gone
//! freed in a classic table     the text is there  "xref num 4 not found",
//!                                                 then rebuilt, text there
//! ```
//!
//! So a free entry is counted in either form: which reader a recipient opens
//! the file in is not something a signature report can know.
//!
//! # What is read, and what a failure means
//!
//! The chain a reader follows: the last `startxref`, then each `/Prev`, for as
//! long as the section lies after the signed range. A section before it was
//! signed and is the signed parse's business. Each section is parsed by the
//! small reader below, which knows direct objects and nothing else --- a
//! trailer and a cross-reference stream's dictionary hold no others
//! (ISO 32000-1 7.5.8.2).
//!
//! **Anything this cannot follow is `None`, and the appendix is then reported
//! as unread.** A section that will not parse may be the one that frees
//! something, and "nothing was freed" is the reassuring answer.

use lopdf::{Dictionary, Object, Stream};
use std::collections::{BTreeMap, BTreeSet};

/// How many sections a chain may have before it is not followed further.
/// Every signature, timestamp and validation append adds one; a document with
/// a thousand is not one this was written for, and a `/Prev` that leads in a
/// circle is ended by the set of offsets already read.
const MAX_SECTIONS: usize = 1024;

/// How deep a direct object may nest. A trailer's `/ID` is an array in a
/// dictionary; a cross-reference stream's `/DecodeParms` is a dictionary in
/// one.
const MAX_DEPTH: u8 = 12;

/// What one cross-reference section says of each object it lists: `true` for
/// an object in use, `false` for a free one.
type Entries = BTreeMap<u32, bool>;

/// The numbers of the objects that are free according to the sections written
/// at or after `end`, starting from the section at `last`.
///
/// The newest section that lists an object decides. `None` when a section
/// could not be read.
pub(super) fn after(bytes: &[u8], end: usize, last: usize, limit: usize) -> Option<BTreeSet<u32>> {
    let mut decided: Entries = Entries::new();
    let mut seen = BTreeSet::new();
    let mut at = last;
    while at >= end {
        if !seen.insert(at) {
            break;
        }
        if seen.len() > MAX_SECTIONS {
            return None;
        }
        let (entries, prev) = revision(bytes, at, limit)?;
        for (id, in_use) in entries {
            decided.entry(id).or_insert(in_use);
        }
        match prev {
            Some(prev) => at = prev,
            None => break,
        }
    }
    Some(
        decided
            .into_iter()
            .filter_map(|(id, in_use)| (!in_use).then_some(id))
            .collect(),
    )
}

/// One revision's entries and where the section before it starts.
///
/// A classic table may name a cross-reference stream of its own
/// (`/XRefStm`, 7.5.8.4): the table then lists as free what the stream holds
/// in object streams, so within one revision an object in use by either is in
/// use.
fn revision(bytes: &[u8], at: usize, limit: usize) -> Option<(Entries, Option<usize>)> {
    let mut scan = Scan {
        bytes,
        at: at.min(bytes.len()),
    };
    scan.skip_space();
    if !scan.bytes[scan.at..].starts_with(b"xref") {
        let (entries, trailer) = stream_section(&mut scan, limit)?;
        return Some((entries, offset(&trailer, b"Prev")?));
    }
    scan.at += b"xref".len();
    let mut entries = table(&mut scan)?;
    let Object::Dictionary(trailer) = scan.object(0)? else {
        return None;
    };
    if let Some(hybrid) = offset(&trailer, b"XRefStm")? {
        let mut scan = Scan {
            bytes,
            at: hybrid.min(bytes.len()),
        };
        let (stream, _) = stream_section(&mut scan, limit)?;
        for (id, in_use) in stream {
            let entry = entries.entry(id).or_insert(in_use);
            *entry |= in_use;
        }
    }
    Some((entries, offset(&trailer, b"Prev")?))
}

/// An offset a trailer states under `key`: `Some(None)` when it states none,
/// and `None` when what it states is not a direct, non-negative integer.
fn offset(trailer: &Dictionary, key: &[u8]) -> Option<Option<usize>> {
    match trailer.get(key) {
        Err(_) => Some(None),
        Ok(Object::Integer(value)) => usize::try_from(*value).ok().map(Some),
        Ok(_) => None,
    }
}

/// A classic table's entries, from after the keyword `xref` to the keyword
/// `trailer`, which is consumed.
///
/// Read as tokens rather than as twenty-byte records: the entries of real
/// files end in one byte as often as in the two the format asks for, and an
/// entry read wrongly is worse here than one read leniently.
fn table(scan: &mut Scan) -> Option<Entries> {
    let mut entries = Entries::new();
    loop {
        scan.skip_space();
        let first = scan.token();
        if first == b"trailer" {
            return Some(entries);
        }
        let start: u32 = number(first)?;
        scan.skip_space();
        let count: u32 = number(scan.token())?;
        // An entry is at least `0 0 n` and a separator.
        if count as usize > (scan.bytes.len() - scan.at) / 6 {
            return None;
        }
        for index in 0..count {
            scan.skip_space();
            number::<u64>(scan.token())?;
            scan.skip_space();
            number::<u32>(scan.token())?;
            scan.skip_space();
            let in_use = match scan.token() {
                b"n" => true,
                b"f" => false,
                _ => return None,
            };
            entries.insert(start.checked_add(index)?, in_use);
        }
    }
}

/// A token that is digits and nothing else, as a number.
fn number<T: std::str::FromStr>(token: &[u8]) -> Option<T> {
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(token).ok()?.parse().ok()
}

/// A cross-reference stream at the scan's position: its entries, and its
/// dictionary, which is the revision's trailer.
fn stream_section(scan: &mut Scan, limit: usize) -> Option<(Entries, Dictionary)> {
    scan.skip_space();
    number::<u32>(scan.token())?;
    scan.skip_space();
    number::<u32>(scan.token())?;
    scan.skip_space();
    if scan.token() != b"obj" {
        return None;
    }
    let Object::Dictionary(dict) = scan.object(0)? else {
        return None;
    };
    // Everything the decoding below reads has to be stated in the dictionary
    // itself. A reference would be resolved by a reader and not by this, and
    // a filter decoded without its parameters yields entries that are not
    // the file's.
    for key in [
        b"Length".as_slice(),
        b"W",
        b"Index",
        b"Size",
        b"Filter",
        b"DecodeParms",
    ] {
        if dict.get(key).is_ok_and(holds_a_reference) {
            return None;
        }
    }
    let length = usize::try_from(dict.get(b"Length").and_then(Object::as_i64).ok()?).ok()?;
    scan.skip_space();
    if scan.token() != b"stream" {
        return None;
    }
    // The keyword is followed by a line feed, or a carriage return and one
    // (7.3.8.1).
    if scan.bytes[scan.at..].starts_with(b"\r\n") {
        scan.at += 2;
    } else if scan.bytes.get(scan.at) == Some(&b'\n') {
        scan.at += 1;
    } else {
        return None;
    }
    let content = scan.bytes.get(scan.at..scan.at.checked_add(length)?)?;
    let stream = Stream::new(dict.clone(), content.to_vec());
    let data = match stream.filters() {
        Ok(filters) if !filters.is_empty() => stream.decompressed_content_with_limit(limit).ok()?,
        _ => stream.content,
    };

    let integers = |key: &[u8]| -> Option<Vec<u64>> {
        dict.get(key)
            .and_then(Object::as_array)
            .ok()?
            .iter()
            .map(|item| item.as_i64().ok().and_then(|n| u64::try_from(n).ok()))
            .collect()
    };
    let widths = integers(b"W")?;
    let [kind_width, second, third] = <[u64; 3]>::try_from(widths).ok()?;
    if kind_width > 8 || second > 8 || third > 8 {
        return None;
    }
    let (kind_width, record) = (kind_width as usize, (kind_width + second + third) as usize);
    if record == 0 {
        return None;
    }
    let index = match dict.get(b"Index") {
        Ok(_) => integers(b"Index")?,
        Err(_) => vec![
            0,
            u64::try_from(dict.get(b"Size").and_then(Object::as_i64).ok()?).ok()?,
        ],
    };
    if index.len() % 2 != 0 {
        return None;
    }

    let mut entries = Entries::new();
    let mut records = data.chunks_exact(record);
    for section in index.chunks_exact(2) {
        let (start, count) = (section[0], section[1]);
        for at in 0..count {
            // Fewer records than the index promises is a stream this cannot
            // speak for.
            let record = records.next()?;
            // A field of no width has its default, which for the type is 1:
            // an object in use (7.5.8.3). Type 2 is one in an object stream.
            // Any other type refers to no object, which is what free means
            // to whoever looks the object up.
            let kind = record[..kind_width]
                .iter()
                .fold(u64::from(kind_width == 0), |value, byte| {
                    (value << 8) | u64::from(*byte)
                });
            let id = u32::try_from(start.checked_add(at)?).ok()?;
            entries.insert(id, kind == 1 || kind == 2);
        }
    }
    Some((entries, dict))
}

/// Whether an object is a reference or has one anywhere inside it.
fn holds_a_reference(object: &Object) -> bool {
    match object {
        Object::Reference(_) => true,
        Object::Array(items) => items.iter().any(holds_a_reference),
        Object::Dictionary(dict) => dict.iter().any(|(_, value)| holds_a_reference(value)),
        _ => false,
    }
}

/// A reader of direct objects, for a trailer and a cross-reference stream's
/// dictionary.
///
/// What a string or a real number says is not kept: nothing read here is
/// either, and they are parsed only to find where they end.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// White space, as the format defines it (7.2.2).
fn is_space(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

impl<'a> Scan<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// Past white space and comments.
    fn skip_space(&mut self) {
        while let Some(byte) = self.peek() {
            if is_space(byte) {
                self.at += 1;
            } else if byte == b'%' {
                while self.peek().is_some_and(|b| b != b'\n' && b != b'\r') {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The regular characters from here on, which may be none.
    fn token(&mut self) -> &'a [u8] {
        let from = self.at;
        while self
            .peek()
            .is_some_and(|b| !is_space(b) && !is_delimiter(b))
        {
            self.at += 1;
        }
        &self.bytes[from..self.at]
    }

    /// A name's bytes after its solidus, with `#xx` decoded: `/Pre#76` is
    /// `/Prev` to every reader.
    fn name(&mut self) -> Option<Vec<u8>> {
        if self.peek() != Some(b'/') {
            return None;
        }
        self.at += 1;
        let raw = self.token();
        let mut name = Vec::with_capacity(raw.len());
        let mut rest = raw.iter();
        while let Some(&byte) = rest.next() {
            if byte != b'#' {
                name.push(byte);
                continue;
            }
            let digit = |byte: Option<&u8>| char::from(*byte?).to_digit(16);
            let (high, low) = (digit(rest.next())?, digit(rest.next())?);
            name.push((high * 16 + low) as u8);
        }
        Some(name)
    }

    /// One direct object.
    fn object(&mut self, depth: u8) -> Option<Object> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.skip_space();
        match self.peek()? {
            b'<' if self.bytes.get(self.at + 1) == Some(&b'<') => {
                self.at += 2;
                let mut dict = Dictionary::new();
                loop {
                    self.skip_space();
                    if self.bytes[self.at..].starts_with(b">>") {
                        self.at += 2;
                        return Some(Object::Dictionary(dict));
                    }
                    let key = self.name()?;
                    let value = self.object(depth + 1)?;
                    // Which of two equal keys counts is each reader's own
                    // choice, so a dictionary that states one twice is not
                    // read.
                    if dict.has(&key) {
                        return None;
                    }
                    dict.set(key, value);
                }
            }
            b'<' => {
                self.at += 1;
                while self.peek()? != b'>' {
                    self.at += 1;
                }
                self.at += 1;
                Some(Object::Null)
            }
            b'(' => {
                let mut open = 0_usize;
                loop {
                    match self.peek()? {
                        b'\\' => self.at += 1,
                        b'(' => open += 1,
                        b')' => open -= 1,
                        _ => {}
                    }
                    self.at += 1;
                    if open == 0 {
                        return Some(Object::Null);
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek()? == b']' {
                        self.at += 1;
                        return Some(Object::Array(items));
                    }
                    items.push(self.object(depth + 1)?);
                }
            }
            b'/' => self.name().map(Object::Name),
            _ => {
                let token = self.token();
                match token {
                    b"true" => return Some(Object::Boolean(true)),
                    b"false" => return Some(Object::Boolean(false)),
                    b"null" => return Some(Object::Null),
                    _ => {}
                }
                let text = std::str::from_utf8(token).ok()?;
                let Ok(integer) = text.parse::<i64>() else {
                    // A real number, kept as one so that it is never taken
                    // for an offset.
                    return text.parse::<f32>().ok().map(Object::Real);
                };
                // `12 0 R` is one object and `12 0` is two.
                let back = self.at;
                self.skip_space();
                if let Some(generation) = number::<u16>(self.token()) {
                    self.skip_space();
                    if self.token() == b"R" {
                        let id = u32::try_from(integer).ok()?;
                        return Some(Object::Reference((id, generation)));
                    }
                }
                self.at = back;
                Some(Object::Integer(integer))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of filler with sections at known places: `(offset, text)`.
    fn file(sections: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let len = sections
            .iter()
            .map(|(at, text)| at + text.len())
            .max()
            .unwrap_or(0);
        let mut bytes = vec![b'\n'; len];
        for (at, text) in sections {
            bytes[*at..at + text.len()].copy_from_slice(text);
        }
        bytes
    }

    /// A cross-reference stream's text: `entries` of one type byte each over
    /// `index`, with `more` added to the dictionary.
    fn stream(index: &str, entries: &[u8], more: &str) -> Vec<u8> {
        let mut out = format!(
            "9 0 obj\n<< /Type /XRef /Size 10 /W [1 0 0] /Index [{index}] /Length {} {more} >>\nstream\n",
            entries.len()
        )
        .into_bytes();
        out.extend_from_slice(entries);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out
    }

    fn freed(bytes: &[u8], end: usize, last: usize) -> Option<Vec<u32>> {
        after(bytes, end, last, 1 << 20).map(|ids| ids.into_iter().collect())
    }

    /// A free entry counts in a classic table and in a stream, and an entry
    /// in use does not.
    #[test]
    fn a_free_entry_is_read_in_either_form() {
        let table = file(&[(
            100,
            b"xref\n0 1\n0000000000 65535 f \n4 2\n0000000000 00001 f \n0000000050 00000 n \n\
              trailer\n<< /Size 6 /Root 1 0 R /ID [<00> (a\\)b)] >>\n"
                .to_vec(),
        )]);
        assert_eq!(freed(&table, 100, 100), Some(vec![0, 4]));

        let streamed = file(&[(100, stream("4 2", &[0, 1], ""))]);
        assert_eq!(freed(&streamed, 100, 100), Some(vec![4]));
        // Type 2 is an object in an object stream, and in use.
        let compressed = file(&[(100, stream("4 2", &[2, 1], ""))]);
        assert_eq!(freed(&compressed, 100, 100), Some(vec![]));
    }

    /// The newest section that lists an object decides, and a section before
    /// the signed range's end is not read at all.
    #[test]
    fn the_newest_section_decides_and_a_signed_one_is_left_alone() {
        let older = stream("4 1 6 1", &[0, 0], "");
        let newer = stream("4 1", &[1], "/Prev 300");
        let signed = stream("7 1", &[0], "");
        let with_prev = stream("4 1 6 1", &[0, 0], "/Prev 10");
        let bytes = file(&[(10, signed.clone()), (300, older), (600, newer)]);
        // Object 4 was freed and then written again; object 6 stays freed.
        assert_eq!(freed(&bytes, 200, 600), Some(vec![6]));
        // From the older section alone, both are free.
        assert_eq!(freed(&bytes, 200, 300), Some(vec![4, 6]));
        // A chain that leads into the signed part stops there: object 7 is
        // the signed revision's.
        let bytes = file(&[(10, signed), (300, with_prev)]);
        assert_eq!(freed(&bytes, 200, 300), Some(vec![4, 6]));
        // And with the whole file as the appended part, it is read.
        assert_eq!(freed(&bytes, 0, 300), Some(vec![4, 6, 7]));
    }

    /// A hybrid revision: the table lists as free what its stream holds.
    #[test]
    fn an_object_in_use_by_a_tables_own_stream_is_in_use() {
        let hybrid = stream("4 1", &[2], "");
        let bytes = file(&[
            (20, hybrid),
            (
                400,
                b"xref\n4 2\n0000000000 65535 f \n0000000000 00001 f \n\
                  trailer\n<< /Size 6 /XRefStm 20 >>\n"
                    .to_vec(),
            ),
        ]);
        assert_eq!(freed(&bytes, 300, 400), Some(vec![5]));
    }

    /// What cannot be followed is not "nothing was freed".
    #[test]
    fn a_section_that_cannot_be_read_is_no_answer() {
        for (what, section) in [
            ("neither a table nor an object", b"garbage".to_vec()),
            (
                "a table with no trailer",
                b"xref\n4 1\n0000000000 00001 f \n".to_vec(),
            ),
            (
                "an entry that is neither n nor f",
                b"xref\n4 1\n0000000000 00001 x \ntrailer\n<< >>\n".to_vec(),
            ),
            (
                "a previous section stated as a reference",
                b"xref\n4 1\n0000000000 00001 n \ntrailer\n<< /Prev 3 0 R >>\n".to_vec(),
            ),
            (
                "a previous section stated twice",
                b"xref\n4 1\n0000000000 00001 n \ntrailer\n<< /Prev 9999 /Prev 5 >>\n".to_vec(),
            ),
            (
                "a stream shorter than its index",
                stream("4 3", &[0, 0], ""),
            ),
            (
                "a stream whose filter is not stated in it",
                stream("4 1", &[0], "/Filter 3 0 R"),
            ),
            (
                "a previous section that is not there",
                stream("4 1", &[1], "/Prev 100000"),
            ),
        ] {
            let bytes = file(&[(100, section)]);
            assert_eq!(freed(&bytes, 50, 100), None, "{what}");
        }
        // The control: the last shape with nothing wrong in it.
        let bytes = file(&[(100, stream("4 1", &[1], ""))]);
        assert_eq!(freed(&bytes, 50, 100), Some(vec![]));
    }

    /// A name is what it decodes to: `/Pre#76` is `/Prev`.
    #[test]
    fn an_escaped_key_is_the_key_it_spells() {
        let older = stream("6 1", &[0], "");
        let newer = stream("4 1", &[1], "/Pre#76 300");
        let bytes = file(&[(300, older), (600, newer)]);
        assert_eq!(freed(&bytes, 200, 600), Some(vec![6]));
    }

    /// A compressed stream with a predictor, as real writers make them, reads
    /// as the plain one does. Built through `lopdf`, which is what decodes it.
    #[test]
    fn a_deflated_stream_is_decoded() {
        // Every third object free, over enough entries that deflating them
        // is worth it to the writer.
        let entries: Vec<u8> = (0..300_u32).map(|at| u8::from(at % 3 != 0)).collect();
        let mut packed = Stream::new(Dictionary::new(), entries);
        packed.compress().expect("deflates");
        assert!(packed.filters().is_ok_and(|filters| !filters.is_empty()));
        let mut text = format!(
            "999 0 obj\n<< /Type /XRef /Size 1000 /W [1 0 0] /Index [4 300] \
             /Filter /FlateDecode /Length {} >>\nstream\n",
            packed.content.len()
        )
        .into_bytes();
        text.extend_from_slice(&packed.content);
        text.extend_from_slice(b"\nendstream\nendobj\n");
        let bytes = file(&[(100, text)]);
        let want: Vec<u32> = (0..300).filter(|at| at % 3 == 0).map(|at| at + 4).collect();
        assert_eq!(freed(&bytes, 50, 100), Some(want));
    }
}
