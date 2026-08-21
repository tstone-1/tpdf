//! Indefinite lengths made definite, so a BER signature can be read at all.
//!
//! A PDF signature's `/Contents` is a CMS blob, and the specification that
//! governs it (RFC 5652 §5.1) requires DER. Real signers do not all obey: a
//! producer that streams its output cannot know a value's length before it has
//! written the value, so it writes the **indefinite form** --- `80` where the
//! length belongs, and a two-byte end-of-contents marker where the value stops.
//! That is legal BER and it is not DER, and `der` refuses it outright with
//! *indefinite length disallowed*.
//!
//! Measured on a real signed contract: five indefinite values, nineteen levels
//! deep, in a 46 KB blob --- and every reader in this repository saw nothing at
//! all. [`to_definite_length`] rewrites those lengths and hands back bytes the
//! rest of the pipeline can parse.
//!
//! # It also decides where the blob ends, and that is the larger half
//!
//! A signature is written by reserving a fixed span and filling it, so the blob
//! arrives right-padded with zeros. Trimming those by scanning back to the last
//! non-zero byte is what this module replaces, and that scan was wrong twice
//! over. It loses a legitimate trailing `0x00`, which is about one DER blob in
//! 256. Worse, **an end-of-contents marker is two zero bytes**, so on the very
//! blobs this module exists for the scan eats the terminators and leaves
//! something no parser can read --- measured on that contract as six bytes
//! short, three nested markers gone.
//!
//! Reading the structure answers both exactly: the first value's extent is
//! where the blob ends, whatever bytes follow it are padding, and a value
//! ending in a zero keeps it.
//!
//! # What this is not
//!
//! It is not a BER-to-DER canonicaliser. DER constrains more than the length
//! form --- a `SET OF` must be sorted, a `BOOLEAN` must be `0xff`, a string
//! must be primitive rather than assembled from constructed segments --- and
//! none of that is touched here. A blob violating one of those comes out of
//! this module unchanged in that respect and is refused by the parser after it,
//! which is reported as unread rather than passed off as absent. The scope is
//! the one thing measured in the wild and the one thing that made a whole
//! document unreadable.
//!
//! # Hostile input
//!
//! The bytes are attacker-chosen, so the walk is bounded on every axis it can
//! be: nesting at [`MAX_DEPTH`], a length field at [`MAX_LENGTH_BYTES`], a tag
//! at [`MAX_TAG_BYTES`], and every read through `get` rather than an index. A
//! value that claims more bytes than it has, a child that overruns its parent
//! and an indefinite value that never terminates are all refused rather than
//! trusted. See `docs/THREAT-MODEL.md` §T6.8.

/// Deepest nesting the walk follows.
///
/// A real signature reaches about twenty-five: a certificate chain inside a
/// timestamp token inside a signature. Sixty-four leaves room for a document
/// nobody has written yet and refuses one built to exhaust the stack.
pub const MAX_DEPTH: usize = 64;

/// Most bytes a length field may occupy, after the byte announcing its size.
///
/// Four is 4 GB, which no `/Contents` blob approaches; X.690 §8.1.3.5(c)
/// reserves `0xff`, and this refuses that along with everything above four.
const MAX_LENGTH_BYTES: usize = 4;

/// Most bytes a tag may occupy, the leading byte included.
///
/// Every tag CMS uses is one byte. The high-tag-number form is accepted because
/// refusing a legal encoding on the grounds that we have not seen it is how a
/// reader acquires a document it cannot open.
const MAX_TAG_BYTES: usize = 5;

/// One value, as the input holds it and as it will be written.
struct Span {
    /// Bytes it occupies in the input, header included.
    input: usize,
    /// Bytes its *value* will occupy once every length inside is definite.
    content: usize,
    /// Bytes it will occupy in full, header included.
    output: usize,
}

/// A value's identifier and length octets, decoded.
struct Header {
    /// Bytes of tag.
    tag: usize,
    /// Bytes of length field.
    length: usize,
    constructed: bool,
    /// The length, or `None` for the indefinite form.
    value: Option<usize>,
}

/// The DER encoding of a length, and how many bytes of the array it fills.
///
/// One function rather than two, because a length counted one way and written
/// another produces a blob whose header disagrees with its body --- which no
/// parser can diagnose and no test would obviously catch.
fn length_field(value: usize) -> ([u8; 9], usize) {
    let mut field = [0u8; 9];
    if value < 0x80 {
        field[0] = value as u8;
        return (field, 1);
    }
    let bytes = value.to_be_bytes();
    let used = (usize::BITS as usize - value.leading_zeros() as usize).div_ceil(8);
    field[0] = 0x80 | used as u8;
    field[1..=used].copy_from_slice(&bytes[bytes.len() - used..]);
    (field, 1 + used)
}

/// Decode the header of the value at `at`, or refuse it.
fn header(raw: &[u8], at: usize) -> Option<Header> {
    let first = *raw.get(at)?;
    let mut tag = 1;
    if first & 0x1f == 0x1f {
        loop {
            if tag >= MAX_TAG_BYTES {
                return None;
            }
            let byte = *raw.get(at + tag)?;
            tag += 1;
            if byte & 0x80 == 0 {
                break;
            }
        }
    }
    let constructed = first & 0x20 != 0;
    let marker = *raw.get(at + tag)?;
    if marker == 0x80 {
        // The indefinite form says "read until the marker", and a primitive
        // value has no marker to read to --- X.690 §8.1.3.6 allows it only for
        // the constructed form.
        if !constructed {
            return None;
        }
        return Some(Header {
            tag,
            length: 1,
            constructed,
            value: None,
        });
    }
    if marker & 0x80 == 0 {
        return Some(Header {
            tag,
            length: 1,
            constructed,
            value: Some(marker as usize),
        });
    }
    let count = (marker & 0x7f) as usize;
    if count > MAX_LENGTH_BYTES {
        return None;
    }
    let mut value = 0usize;
    for step in 0..count {
        value = (value << 8) | *raw.get(at + tag + 1 + step)? as usize;
    }
    Some(Header {
        tag,
        length: 1 + count,
        constructed,
        value: Some(value),
    })
}

/// Walk the value at `at` without writing anything, to learn how long it will
/// be once written.
fn measure(raw: &[u8], at: usize, depth: usize) -> Option<Span> {
    if depth > MAX_DEPTH {
        return None;
    }
    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;
    let mut content = 0usize;
    let mut cursor = body;

    match head.value {
        Some(length) => {
            let end = body.checked_add(length)?;
            if end > raw.len() {
                return None;
            }
            if !head.constructed {
                return Some(Span {
                    input: end - at,
                    content: length,
                    output: end - at,
                });
            }
            while cursor < end {
                let span = measure(raw, cursor, depth + 1)?;
                cursor += span.input;
                content += span.output;
                // A child is bounded by the input, not by its parent, so one
                // may legally decode and still run past the end its parent
                // declared. That is a malformed value, not a long one.
                if cursor > end {
                    return None;
                }
            }
        }
        None => loop {
            if raw.get(cursor).copied() == Some(0) && raw.get(cursor + 1).copied() == Some(0) {
                cursor += 2;
                break;
            }
            let span = measure(raw, cursor, depth + 1)?;
            cursor += span.input;
            content += span.output;
        },
    }

    let (_, length) = length_field(content);
    Some(Span {
        input: cursor - at,
        content,
        output: head.tag + length + content,
    })
}

/// Write the value at `at` in definite form, and report the input it consumed.
///
/// There is no depth guard here, deliberately. This runs only after `measure`
/// walked the same bytes from the same offset and returned, so it descends
/// exactly the nesting `measure` already accepted --- and a second copy of the
/// bound would make the first one untestable, since either alone refuses the
/// blob and a mutation of either would survive.
fn emit(raw: &[u8], at: usize, depth: usize, out: &mut Vec<u8>) -> Option<usize> {
    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;

    if let Some(length) = head.value {
        let end = body.checked_add(length)?;
        if end > raw.len() {
            return None;
        }
        if !head.constructed {
            // A primitive value is its bytes; only its length field can be
            // written differently, and copying the whole thing keeps a value
            // that is already DER byte-identical.
            out.extend_from_slice(&raw[at..end]);
            return Some(end - at);
        }
    }

    let span = measure(raw, at, depth)?;
    let (field, length) = length_field(span.content);
    out.extend_from_slice(&raw[at..at + head.tag]);
    out.extend_from_slice(&field[..length]);

    let mut cursor = body;
    match head.value {
        Some(declared) => {
            let end = body + declared;
            while cursor < end {
                cursor += emit(raw, cursor, depth + 1, out)?;
            }
        }
        // The consumed count comes from `measure`, so the marker needs only to
        // stop the loop --- stepping over it here would be a write nobody reads.
        None => loop {
            if raw.get(cursor).copied() == Some(0) && raw.get(cursor + 1).copied() == Some(0) {
                break;
            }
            cursor += emit(raw, cursor, depth + 1, out)?;
        },
    }
    Some(span.input)
}

/// The first value in `raw`, written with definite lengths and nothing after it.
///
/// Returns `None` when the bytes are not a value this can walk: truncated, a
/// child overrunning its parent, an indefinite value that never terminates,
/// nesting past [`MAX_DEPTH`], or a length field past [`MAX_LENGTH_BYTES`].
///
/// A blob that is already DER comes back **byte-identical** up to its end, with
/// any padding after it dropped. That property is what makes this safe to put
/// in front of every signature rather than only the ones that need it, and it
/// is asserted against the real fixtures rather than reasoned about.
pub fn to_definite_length(raw: &[u8]) -> Option<Vec<u8>> {
    let span = measure(raw, 0, 0)?;
    let mut out = Vec::with_capacity(span.output);
    emit(raw, 0, 0, &mut out)?;
    debug_assert_eq!(out.len(), span.output, "measured and written disagree");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value with a definite, minimally encoded length.
    ///
    /// The length is written out here rather than by calling `length_field`,
    /// and that is not tidiness. Every fixture below is built with this, and
    /// `a_definite_encoding_comes_back_unchanged` is a check on the encoder ---
    /// so building the fixture *with* the encoder makes the writer agree with
    /// its own reader whatever either of them does. Measured: with the length
    /// rule mutated to always use the long form, sixteen tests went red and the
    /// one named for it did not.
    fn definite(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match body.len() {
            length if length < 0x80 => out.push(length as u8),
            length if length < 0x100 => out.extend_from_slice(&[0x81, length as u8]),
            length if length < 0x10000 => {
                out.extend_from_slice(&[0x82, (length >> 8) as u8, (length & 0xff) as u8]);
            }
            length => panic!("no fixture here needs a length of {length}"),
        }
        out.extend_from_slice(body);
        out
    }

    /// The same value with the indefinite length a streaming signer writes.
    fn indefinite(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, 0x80];
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// The property that lets this sit in front of every signature rather than
    /// only the ones that need it.
    #[test]
    fn a_definite_encoding_comes_back_unchanged() {
        let inner = definite(0x02, &[0x41]);
        let outer = definite(0x30, &inner);
        assert_eq!(
            to_definite_length(&outer).as_deref(),
            Some(outer.as_slice())
        );
    }

    /// The reserved span a signer fills is longer than what it writes.
    #[test]
    fn padding_after_the_first_value_is_dropped() {
        let value = definite(0x30, &definite(0x02, &[0x41]));
        let mut padded = value.clone();
        padded.extend_from_slice(&[0; 64]);
        assert_eq!(
            to_definite_length(&padded).as_deref(),
            Some(value.as_slice())
        );
    }

    /// The defect the trailing-zero scan had, and roughly one blob in 256 hits
    /// it: the last byte of the value *is* a zero, and scanning back for a
    /// non-zero byte eats it.
    #[test]
    fn a_value_whose_last_byte_is_zero_keeps_it() {
        let value = definite(0x30, &definite(0x02, &[0x41, 0x00]));
        let mut padded = value.clone();
        padded.extend_from_slice(&[0; 16]);
        let read = to_definite_length(&padded).expect("a walkable value");
        assert_eq!(read, value);
        assert_eq!(read.last(), Some(&0), "the value's own zero must survive");
    }

    /// The reason this module exists.
    #[test]
    fn an_indefinite_length_becomes_a_definite_one() {
        let body = definite(0x02, &[0x41]);
        let read = to_definite_length(&indefinite(0x30, &body)).expect("a walkable value");
        assert_eq!(read, definite(0x30, &body));
    }

    /// A real signature nests them: a token inside an attribute inside a signer.
    #[test]
    fn nested_indefinite_lengths_all_become_definite() {
        let leaf = definite(0x02, &[0x41]);
        let middle = indefinite(0x30, &leaf);
        let read = to_definite_length(&indefinite(0xa0, &middle)).expect("a walkable value");
        assert_eq!(read, definite(0xa0, &definite(0x30, &leaf)));
        assert!(
            !read.windows(2).any(|pair| pair == [0x80, 0x30]),
            "no indefinite marker may survive"
        );
    }

    /// The boundary where the short form stops being available.
    #[test]
    fn a_body_of_a_hundred_and_twenty_eight_bytes_takes_the_long_form() {
        let body = definite(0x04, &[0x41; 126]);
        assert_eq!(body.len(), 128);
        let read = to_definite_length(&indefinite(0x30, &body)).expect("a walkable value");
        assert_eq!(&read[..3], &[0x30, 0x81, 0x80]);
    }

    /// BER allows a length written in more bytes than it needs; DER does not.
    #[test]
    fn a_non_minimal_length_is_rewritten_minimally() {
        let mut wasteful = vec![0x30, 0x81, 0x03];
        wasteful.extend_from_slice(&definite(0x02, &[0x41]));
        let read = to_definite_length(&wasteful).expect("a walkable value");
        assert_eq!(read, definite(0x30, &definite(0x02, &[0x41])));
    }

    /// Both directions, because a bound proved only by its refusal could sit
    /// anywhere below the depth a real signature needs.
    #[test]
    fn nesting_is_followed_to_the_bound_and_refused_past_it() {
        let build = |levels: usize| {
            let mut value = definite(0x02, &[0x41]);
            for _ in 0..levels {
                value = definite(0x30, &value);
            }
            value
        };
        // The outermost value is depth 0, so MAX_DEPTH wrappers reach it exactly.
        assert!(to_definite_length(&build(MAX_DEPTH)).is_some());
        assert_eq!(to_definite_length(&build(MAX_DEPTH + 1)), None);
    }

    /// X.690 §8.1.3.6: there is no marker to read a primitive value up to.
    ///
    /// The empty case is the one that discriminates, and the first draft of
    /// this test had only the other. `04 80 41 00 00` is refused whether the
    /// rule is enforced or not --- with the rule gone the walk reads `41 00` as
    /// a child and then runs out of input --- so the mutation survived. An
    /// immediate marker walks cleanly, and without the rule it comes back as an
    /// empty octet string.
    #[test]
    fn an_indefinite_primitive_is_refused() {
        assert_eq!(to_definite_length(&[0x04, 0x80, 0x00, 0x00]), None);
        assert_eq!(to_definite_length(&[0x04, 0x80, 0x41, 0x00, 0x00]), None);
    }

    /// A child may decode perfectly and still run past the end its parent
    /// declared, and that is a malformed value rather than a long one.
    #[test]
    fn a_child_that_overruns_its_parent_is_refused() {
        let mut raw = vec![0x30, 0x06];
        raw.extend_from_slice(&definite(0x02, &[0x41]));
        raw.extend_from_slice(&definite(0x02, &[0x41; 5]));
        assert_eq!(to_definite_length(&raw), None);
    }

    #[test]
    fn a_value_claiming_more_bytes_than_it_has_is_refused() {
        assert_eq!(to_definite_length(&[0x30, 0x05, 0x02, 0x01]), None);
    }

    #[test]
    fn an_indefinite_value_that_never_terminates_is_refused() {
        assert_eq!(to_definite_length(&[0x30, 0x80, 0x02, 0x01, 0x41]), None);
    }

    /// Four bytes is 4 GB; `0xff` is reserved by X.690 §8.1.3.5(c) and is
    /// refused by the same rule.
    ///
    /// Both values are written out **in full and correctly**, describing a body
    /// that really is there. A width the walk refuses is otherwise refused a
    /// second time by running off the end of the input, and then the rule under
    /// test is not what produced the answer.
    fn wide_length(width: usize, content: &[u8]) -> Vec<u8> {
        let mut raw = vec![0x30, 0x80 | width as u8];
        raw.extend(std::iter::repeat_n(0u8, width - 1));
        raw.push(content.len() as u8);
        raw.extend_from_slice(content);
        raw
    }

    #[test]
    fn a_length_field_past_the_bound_is_refused() {
        let body = definite(0x02, &[0x41]);
        assert_eq!(
            to_definite_length(&wide_length(MAX_LENGTH_BYTES, &body)).as_deref(),
            Some(definite(0x30, &body).as_slice()),
            "the widest accepted form must still be read, or this bound is not \
             where the test says it is"
        );
        assert_eq!(
            to_definite_length(&wide_length(MAX_LENGTH_BYTES + 1, &body)),
            None
        );
        assert_eq!(to_definite_length(&wide_length(0x7f, &body)), None);
    }

    /// The high-tag-number form is legal and CMS never uses it; refusing a legal
    /// encoding because we have not seen it is how a reader acquires a document
    /// it cannot open.
    #[test]
    fn a_high_tag_number_is_read_up_to_the_bound_and_refused_past_it() {
        let mut reached = vec![0x1f];
        reached.extend_from_slice(&[0x81, 0x81, 0x01]);
        reached.extend_from_slice(&[0x01, 0x41]);
        assert_eq!(reached.len(), MAX_TAG_BYTES + 1);
        assert_eq!(
            to_definite_length(&reached).as_deref(),
            Some(reached.as_slice())
        );

        let mut runaway = vec![0x1f];
        runaway.extend_from_slice(&[0x81; MAX_TAG_BYTES]);
        runaway.extend_from_slice(&[0x01, 0x01, 0x41]);
        assert_eq!(to_definite_length(&runaway), None);
    }

    /// An empty input is not a value, and neither is a lone tag.
    #[test]
    fn too_few_bytes_to_hold_a_header_is_refused() {
        assert_eq!(to_definite_length(&[]), None);
        assert_eq!(to_definite_length(&[0x30]), None);
    }

    /// A constructed value may declare a definite length and still hold a child
    /// that does not, which is why the walk cannot copy a definite value whole.
    #[test]
    fn a_definite_parent_holding_an_indefinite_child_is_rewritten() {
        let leaf = definite(0x02, &[0x41]);
        let child = indefinite(0x30, &leaf);
        let raw = definite(0xa0, &child);
        let read = to_definite_length(&raw).expect("a walkable value");
        assert_eq!(read, definite(0xa0, &definite(0x30, &leaf)));
        assert!(read.len() < raw.len(), "the marker and its length byte go");
    }
}
