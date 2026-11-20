//! Whether a file can be certified clean, and what the answer rests on.
//!
//! `docs/PLAN.md` §6 makes verification mandatory after a redaction and forbids
//! a bare success: the result is *verified*, or *not verified* with specifics.
//! The rule it states for getting there is deny by default --- anything the
//! sanitizer does not understand is a failure rather than a shrug --- and spike
//! 0.4 found that rule, taken literally, refuses almost every real document.
//!
//! **The refusal rate is the finding, not a detail.** `lopdf` decoded
//! `FlateDecode`, `LZWDecode` and `ASCII85Decode` when the spike ran, and
//! answered `Unimplemented("decompression algorithms")` for everything else
//! (0.45 added three, none of them an image filter, and 0.46 none; see
//! `DECODABLE`). Everything
//! else includes `/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and
//! `/JPXDecode`, which is what every scanner on earth emits. On the `filters`
//! fixture all six of the spike's rewrite routes reported *not verified*,
//! QPDF's own output among them.
//!
//! **So the split is by remedy, and deliberately not by verdict.** This is the
//! part that is easy to get wrong in the direction that ships a lie: calling an
//! image carrier "fine" would make a scanned document certifiable without
//! anything having read it. It is not fine, and it is not the same as a stream
//! nothing can decode either --- one needs a different instrument, the other
//! needs a different file. A [`Report`] therefore carries two lists, and both
//! of them withhold certification:
//!
//! * [`Report::blind`] --- nothing here can account for these bytes. There is no
//!   instrument that would change the answer.
//! * [`Report::deferred`] --- a raster image, whether a JPEG, a deflated
//!   picture or one drawn inline in a content stream. Its *encoded bytes* were
//!   scanned like every other byte in the file, so a needle sitting literally in
//!   the stream is found; what was not read is the **picture**, and text that
//!   exists only as pixels is exactly what OCR is for.
//!
//! **That instrument exists now, and it is narrower than this list.**
//! [`crate::ocr_gate`] renders the regions a redaction removed from and has an
//! engine read them, so a *region* whose carrier is a picture is answered. A
//! deferred image sitting anywhere else in the file is still exactly what this
//! bullet says it is: bytes nobody read, reported rather than waved through.
//!
//! **The byte scan reads characters, and a composite font does not store
//! them.** Under a `/Type0` font a content stream carries glyph numbers, so a
//! word drawn through one is never in the file as its own bytes and the scan
//! finds nothing whether the word is there or not. A file holding such a font is
//! therefore blind, once, for the file: the scan cannot account for a copy of
//! the words anywhere it draws. The command-line tool adds a text-extraction
//! search of the pages it was asked about, which reads them in their own
//! encoding; it does not reach an appearance stream or a form no page draws,
//! so it narrows this reason and does not withdraw it.
//!
//! A caller that wants a single word gets [`Verdict`], which says *not verified*
//! for either list. What the split buys is that the reason names the next step
//! rather than ending the conversation.
//!
//! **Since 2026-09-21 a hit carries a page, when one can be earned.** Until
//! then a report said *"4711-0815 is still in the file"* and nothing more, which
//! a reader cannot act on: a removal that failed and a second copy on a page
//! nobody marked produce the identical sentence. [`Located`] is the answer, and
//! it has **three** values rather than two because the middle one is where a
//! two-valued version would lie --- an object more than one page draws belongs
//! to no page, and naming the first page that reached it would turn *still in
//! the file* into *on a page you did not mark*. So: placed on a page set, shared
//! between pages and therefore unplaceable, or reached by no page at all. A
//! bound tripping anywhere withholds every answer rather than shortening one,
//! because a truncated walk does not lose an answer, it invents a wrong one.
//!
//! What that does **not** change is the verdict. A word still in the file is
//! still a leak whatever page it is on, and this module will not certify one:
//! the location makes the finding actionable, not forgivable.
//!
//! **Two things the byte scan can see and the graph walk cannot**, both from
//! spike 0.4 and both preserved here: bytes past the last `%%EOF` belong to no
//! object at all, and a file with more than one `%%EOF` has revisions that no
//! parser resolves --- an object a later revision *overwrote* sits at its old
//! offset addressable by nothing, so it is invisible to any graph walk and, if
//! it is compressed, to the byte scan as well. Such a file cannot be certified;
//! it can only be rewritten and then certified.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Document, Object};

/// The ceiling on any single decoded stream, which every other scan uses too.
///
/// **This module declared its own literal from 2026-08-26 until 2026-08-31**,
/// with a comment saying it was stated rather than imported so that the bound
/// would be visible where it is enforced --- and citing `save::MAX_DECODE`,
/// which is not a definition but `save.rs`'s import of this one. Two copies of a
/// security bound with nothing asserting they agree, one day after
/// `encoding.rs` recorded that having six of them was the defect. Visibility is
/// what a `use` line is for; agreement is what one value is for.
use crate::encoding::MAX_DECODE;

/// The filters whose decoded bytes this scan reads.
///
/// Read out of `lopdf` 0.44's `Stream::decode_filters`, which dispatched on
/// exactly these three and returned `Unimplemented` otherwise. Written down
/// rather than discovered at run time because the classification below has to
/// distinguish "this failed to decode" from "this was never going to decode",
/// and only the second is a fact about the library.
///
/// **Narrower than the library since 0.45.** 0.45.0 dispatches on six: these,
/// `BrotliDecode`, `ASCIIHexDecode` and `RunLengthDecode`; 0.46.0, which the
/// lock file holds, dispatches on the same six. This list was not widened with it, so [`classify`] still
/// answers [`Carrier::Undecodable`] for a stream ending in `ASCIIHexDecode` or
/// `RunLengthDecode` and [`Carrier::Unrecognised`] for one ending in
/// `BrotliDecode`, and the scan withholds certification from bytes the library
/// could hand it. That is the direction this module may err in --- less
/// reassuring than the file deserves, never more --- and it is recorded here
/// so that nobody reads the three names as the library's whole answer.
const DECODABLE: &[&[u8]] = &[b"FlateDecode", b"LZWDecode", b"ASCII85Decode"];

/// How many per-object reasons one report carries before it summarises the rest.
///
/// **A bound that had no reason to exist while the scan ran here, and has one
/// now that it runs in a worker.** A [`Report`] is the reply
/// [`crate::worker_proto::Reply::Verified`] carries, and a reply is read under
/// [`crate::worker_proto::MAX_REPLY_BYTES`], which is 32 MB. Each per-object
/// reason is about a hundred bytes and there is one per object, so a file with a
/// few hundred thousand undecodable objects --- reachable inside 32 MB of PDF,
/// since an object with a filter and an empty stream costs about forty bytes ---
/// produces a report that will not fit down the pipe. The reader would then be
/// told the verification *failed*, which is a different and much worse sentence
/// than the one the file has earned.
///
/// It bounds the worker's own memory for the same input, which the coordinator
/// never bounded either.
///
/// **The suppressed reasons still withhold certification.** What replaces them
/// is one more blind reason saying how many there were, so the verdict is
/// unchanged and only the enumeration is shortened. That is the direction this
/// module is built around: a report may be less specific than the file deserves,
/// and it may never be more reassuring.
///
/// A thousand rather than a hundred because the list is a reader's evidence, and
/// rather than a million because nobody reads the millionth line either.
const MAX_OBJECT_REASONS: usize = 1_000;

/// Filters whose content is a raster image rather than bytes worth scanning.
///
/// Not a list of things we forgive. A needle cannot be *found* in a JPEG by
/// looking at its decoded output, because the decoded output is pixels --- so
/// even a decoder for these would not answer the question the byte scan asks.
/// That is what makes them a different instrument's problem rather than a
/// weaker version of the same one.
///
/// **Not how a picture is recognised, only how a filter is.** A picture stored
/// losslessly carries `/FlateDecode` or no filter at all, which this list
/// cannot tell from a content stream; [`as_drawn`] reads `/Subtype /Image`
/// first, and that is what makes such a stream a picture here.
const IMAGE: &[&[u8]] = &[
    b"DCTDecode",
    b"JPXDecode",
    b"JBIG2Decode",
    b"CCITTFaxDecode",
];

/// What a stream's contents rest on, once the scan has done what it can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// Decoded and scanned. The only outcome that accounts for the bytes.
    Scanned,
    /// A raster image: its encoded bytes were scanned, its picture was not read.
    Image {
        /// The filter that says so, for a report a person reads.
        filter: String,
    },
    /// An encoding this build understands to exist and cannot decode.
    ///
    /// `/ASCIIHexDecode` and `/RunLengthDecode` are the common ones, and they
    /// carry arbitrary bytes --- including text --- which is what separates them
    /// from [`Carrier::Image`].
    Undecodable {
        /// The filter that stopped it.
        filter: String,
    },
    /// A filter nothing here recognises at all.
    ///
    /// Distinct from [`Carrier::Undecodable`] because the remedies differ: one
    /// is a decoder this build lacks, the other is a construct nobody has looked
    /// at. Deny by default covers both, and a report that conflated them would
    /// hide the second behind the first.
    Unrecognised {
        /// The filter as the file spells it.
        filter: String,
    },
    /// A filter this build decodes, and a stream it still did not read the way
    /// a viewer reads it: a key the decoder consults in a form `lopdf` passes
    /// over, a decode that failed, or one that stopped part way. Never produced
    /// by [`classify`], which sees only the filter names; [`as_drawn`] says it.
    Unread {
        /// The sentence, with the object it is about.
        why: String,
    },
}

/// What a stream's filter chain means for the scan.
///
/// **Classified by the LAST filter**, which is the one that decides what the
/// content is: `/Filter [/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG,
/// and it is the JPEG that matters. `lopdf` decodes the chain in order and
/// stops at the first it cannot do, so a chain whose earlier entries are
/// decodable and whose last is not still reaches this function's answer.
///
/// An empty chain is [`Carrier::Scanned`]: a stream with no `/Filter` is stored
/// as it is, and `lopdf` returns its content unchanged.
#[must_use]
pub fn classify(filters: &[&[u8]]) -> Carrier {
    let Some(last) = filters.last() else {
        return Carrier::Scanned;
    };
    let name = || String::from_utf8_lossy(last).into_owned();
    if IMAGE.contains(last) {
        return Carrier::Image { filter: name() };
    }
    if DECODABLE.contains(last) {
        return Carrier::Scanned;
    }
    // Known to the format, absent from this build's decoder. The two are worth
    // separating even though both withhold certification, because "we lack a
    // decoder" is a thing that can be fixed here and "we have never heard of
    // this" is a thing that has to be read first.
    const KNOWN: &[&[u8]] = &[b"ASCIIHexDecode", b"RunLengthDecode", b"Crypt"];
    if KNOWN.contains(last) {
        Carrier::Undecodable { filter: name() }
    } else {
        Carrier::Unrecognised { filter: name() }
    }
}

/// One stream as the scan has it: what it rests on, and its bytes when read.
struct Drawn {
    carrier: Carrier,
    /// The decoded content. Empty unless `carrier` is [`Carrier::Scanned`].
    decoded: Vec<u8>,
    /// The decoded content draws an inline image (`BI` ... `ID` ... `EI`).
    inline_picture: bool,
}

/// Whether one stream was read the way a viewer reads it, and if so its bytes.
///
/// **The one question this scan has to ask of every stream, asked in one
/// place.** A stream counts as read only when it is not a picture, its filters
/// are ones this build decodes, every key the decoder consults is one `lopdf`
/// reads as PDFium does, and the decode ran to the end of the data. Anything
/// else is a picture nobody read ([`Carrier::Image`]) or bytes nobody read
/// ([`Carrier::Unread`] and the filter cases), and both withhold the verdict.
///
/// Each clause was a way to certify a file the reader can still see words in:
///
/// * **A picture is a picture whatever its filter.** The filter list
///   ([`IMAGE`]) names the lossy and fax codecs, and a picture stored with
///   `/FlateDecode`, `/LZWDecode` or nothing at all --- what img2pdf, a PNG and
///   most 1-bit scanners write --- was inflated, searched for the words as if
///   its samples were text, and passed. `/Subtype /Image` is what makes PDFium
///   draw it as a picture, so that is what decides, through a reference too.
/// * **`lopdf` reads `/DecodeParms` only when it is direct**, and its
///   `/Predictor`, `/Columns`, `/Colors`, `/BitsPerComponent` and
///   `/EarlyChange` only as direct integers; PDFium resolves all of them. A
///   PNG-predicted stream whose parameters are `9 0 R` decodes to row-differenced
///   bytes, the words are not in those, and the stream was counted as read.
///   See [`parameters_unread`].
/// * **A decode that stops part way is `Ok`.** `lopdf` logs the zlib error
///   and returns what it produced, so a stream cut short was searched only up
///   to the cut. See [`inflates_whole`]. Its LZW and ASCII85 decoders end the
///   same way, so since 2026-11-20 each of those stages has to reach its own
///   end marker: [`lzw_ends`] and [`ascii85_ends`].
/// * **A content stream can hold a picture of its own**, an inline image, and
///   reading the stream's bytes reads the words around it, not the picture.
fn as_drawn(doc: &Document, object: u32, stream: &lopdf::Stream, filters: &[&[u8]]) -> Drawn {
    let not_read = |carrier| Drawn {
        carrier,
        decoded: Vec::new(),
        inline_picture: false,
    };
    if subtype_is(doc, &stream.dict, b"Image") {
        // The filter names the picture's encoding in the sentence; an empty
        // one says it has none.
        let filter = filters
            .last()
            .map(|last| String::from_utf8_lossy(last).into_owned())
            .unwrap_or_default();
        return not_read(Carrier::Image { filter });
    }
    let carrier = classify(filters);
    if carrier != Carrier::Scanned {
        return not_read(carrier);
    }
    if let Some(why) = parameters_unread(object, stream) {
        return not_read(Carrier::Unread { why });
    }
    // Classified as decodable and then would not decode: a bomb over the
    // bound, or damage. Blind either way, and it must not be mistaken for the
    // filter cases above --- those are decided before any decoding is
    // attempted.
    let decoded = match stream.decompressed_content_with_limit(MAX_DECODE) {
        Ok(decoded) => decoded,
        Err(why) => {
            return not_read(Carrier::Unread {
                why: said!(
                    "object {object} could not be decoded, so its contents are unknown: {why}"
                ),
            })
        }
    };
    if stopped_part_way(stream, filters) {
        return not_read(Carrier::Unread {
            why: said!(
                "object {object} stopped decoding part way, so whatever follows the point it \
                 stopped at was not read"
            ),
        });
    }
    let inline_picture = draws_inline_picture(&decoded);
    Drawn {
        carrier: Carrier::Scanned,
        decoded,
        inline_picture,
    }
}

/// Whether a dictionary's `/Subtype`, followed through references as PDFium
/// follows it, is `subtype`.
fn subtype_is(doc: &Document, dict: &lopdf::Dictionary, subtype: &[u8]) -> bool {
    dict.get(b"Subtype")
        .and_then(|value| doc.dereference(value))
        .and_then(|(_, value)| value.as_name())
        .is_ok_and(|name| name == subtype)
}

/// The sentence for a stream whose decoding parameters `lopdf` would not read
/// as a viewer reads them, or `None` when it reads all of them.
///
/// `lopdf` 0.46.0 (`Stream::decode_filters`) takes `/DecodeParms` when it is a
/// dictionary or an array of dictionaries, and anything else --- a reference,
/// an array holding one, a number --- as no parameters at all. Inside the
/// dictionary it reads each key with `as_i64`, so a reference or a real there
/// is the default. PDFium resolves every one of them. A `/Predictor` above 15
/// is the one value the two read differently when it is direct: PDFium takes
/// every value from 10 up as PNG, `lopdf` only 10 to 15. And when a predictor
/// applies, a sample size outside what a picture row can have is one the two
/// libraries guard differently, so it is not read either.
///
/// **Every key, not the one the last defect used.** A `/Filter` written as a
/// reference was the first spelling of this, and closing it left these open.
fn parameters_unread(object: u32, stream: &lopdf::Stream) -> Option<String> {
    let direct = |params: &lopdf::Dictionary| -> bool {
        const INTEGERS: &[&[u8]] = &[
            b"Predictor",
            b"Columns",
            b"Colors",
            b"BitsPerComponent",
            b"EarlyChange",
        ];
        let integer = |key: &[u8]| match params.get(key) {
            Err(_) => Some(None),
            Ok(Object::Integer(value)) => Some(Some(*value)),
            Ok(_) => None,
        };
        let Some(values) = INTEGERS
            .iter()
            .map(|key| integer(key))
            .collect::<Option<Vec<Option<i64>>>>()
        else {
            return false;
        };
        let &[predictor, columns, colors, bits, _] = values.as_slice() else {
            return false;
        };
        match predictor.unwrap_or(1) {
            2 | 10..=15 => {}
            // Above 15 PDFium still applies PNG and `lopdf` does not.
            16.. => return false,
            // No predictor in either library: the sample keys are not read.
            _ => return true,
        }
        columns.is_none_or(|n| n > 0)
            && colors.is_none_or(|n| (1..=32).contains(&n))
            && bits.is_none_or(|n| matches!(n, 1 | 2 | 4 | 8 | 16))
    };
    let read = match stream.dict.get(b"DecodeParms") {
        Err(_) | Ok(Object::Null) => true,
        Ok(Object::Dictionary(params)) => direct(params),
        Ok(Object::Array(items)) => items.iter().all(|item| match item {
            Object::Null => true,
            Object::Dictionary(params) => direct(params),
            _ => false,
        }),
        Ok(_) => false,
    };
    if read {
        None
    } else {
        Some(said!(
            "object {object} gives how to decode it (/DecodeParms) in a form this build does not \
             read, so its contents were not decoded"
        ))
    }
}

/// Whether a stage of `filters` stopped before the end of its data.
///
/// `lopdf` answers `Ok` with whatever its decoder produced when the decoder
/// stops early, for all three filters it reads, so a stream cut short reaches
/// the search as its first part and nothing says the rest was never read. This
/// asks each stage the same question again and listens to the answer: `flate2`
/// whether the inflate ran to its end ([`inflates_whole`]), `weezl` whether the
/// LZW data reached its end code ([`lzw_ends`]), and the ASCII85 text whether
/// it reached `~>` with nothing unreadable before it ([`ascii85_ends`]). Each
/// runs once over the stage's input into a sink, under the same bound.
///
/// A stage after the first reads what the stages before it produced, so those
/// are decoded again on their own; a prefix that will not decode is a stage
/// that cannot be shown whole.
fn stopped_part_way(stream: &lopdf::Stream, filters: &[&[u8]]) -> bool {
    for (at, filter) in filters.iter().enumerate() {
        if !matches!(*filter, b"FlateDecode" | b"LZWDecode" | b"ASCII85Decode") {
            continue;
        }
        let decoded;
        let input: &[u8] = if at == 0 {
            &stream.content
        } else {
            let mut before = stream.clone();
            let names: Vec<Object> = filters[..at]
                .iter()
                .map(|name| Object::Name(name.to_vec()))
                .collect();
            before.dict.set("Filter", Object::Array(names));
            match before.decompressed_content_with_limit(MAX_DECODE) {
                Ok(bytes) => {
                    decoded = bytes;
                    &decoded
                }
                Err(_) => return true,
            }
        };
        let whole = match *filter {
            b"FlateDecode" => inflates_whole(input),
            b"LZWDecode" => lzw_ends(input, early_change(stream, at)),
            _ => ascii85_ends(input),
        };
        if !whole {
            return true;
        }
    }
    false
}

/// The `/EarlyChange` `lopdf` decodes stage `at` with: from the stage's own
/// entry of a `/DecodeParms` array, or from a single dictionary, and 1 when
/// absent. [`parameters_unread`] has already refused any form it would not
/// read as a direct integer.
fn early_change(stream: &lopdf::Stream, at: usize) -> bool {
    let params = match stream.dict.get(b"DecodeParms") {
        Ok(Object::Array(items)) => items.get(at).and_then(|item| item.as_dict().ok()),
        Ok(Object::Dictionary(params)) => Some(params),
        _ => None,
    };
    params
        .and_then(|params| params.get(b"EarlyChange").ok())
        .and_then(|value| value.as_i64().ok())
        .is_none_or(|value| value != 0)
}

/// Whether LZW data reaches its end code (257) inside the decode bound.
///
/// `lopdf` decodes with `weezl` and reads its status only to log it, so data
/// cut short, or a code the table does not hold, ends the decode with what it
/// had and `Ok`. The same decoder, set up as `lopdf` sets it up, is asked here
/// whether it met the end code. Data without one is counted as stopped part
/// way even where nothing follows: a missing end code and a cut are the same
/// bytes, and the safe reading of both is "not read whole". Empty data holds
/// nothing and is whole, as [`inflates_whole`] answers for an empty inflate.
fn lzw_ends(input: &[u8], early_change: bool) -> bool {
    use weezl::{decode::Decoder, BitOrder, LzwStatus};
    if input.is_empty() {
        return true;
    }
    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };
    let mut sink = vec![0u8; 1 << 16];
    let mut rest = input;
    let mut produced = 0usize;
    loop {
        let step = decoder.decode_bytes(rest, &mut sink);
        rest = &rest[step.consumed_in..];
        produced = produced.saturating_add(step.consumed_out);
        match step.status {
            Ok(LzwStatus::Done) => return true,
            Ok(LzwStatus::Ok) if produced <= MAX_DECODE => {}
            _ => return false,
        }
        if step.consumed_in == 0 && step.consumed_out == 0 {
            return false;
        }
    }
}

/// Whether ASCII85 text reaches its end marker `~>` with every character
/// before it one `lopdf` decodes.
///
/// `lopdf` stops at the first character outside `!`..`u`, `z` and white
/// space, and keeps what it had: a stray byte mid-stream ends the decode with
/// the rest unread, and a stream with no `~>` is decoded to wherever it was
/// cut. Both are counted as stopped part way.
fn ascii85_ends(input: &[u8]) -> bool {
    if input.is_empty() {
        return true;
    }
    let stop = input
        .iter()
        .position(|&ch| !(ch.is_ascii_whitespace() || ch == b'z' || (b'!'..=b'u').contains(&ch)));
    matches!(stop, Some(at) if input[at..].starts_with(b"~>"))
}

/// Whether zlib data inflates to its end, as `lopdf` would read it.
///
/// `lopdf` falls back to raw deflate after the two header bytes when zlib
/// fails, and so does this: a stream whose only fault is its checksum ---
/// which is what an encrypted file with a broken Adler-32 looks like --- has
/// been read whole by both, and calling it blind would refuse real documents
/// for a fault that hides nothing. A stream that fails both ways stopped in its
/// data, and that is the one this reports.
fn inflates_whole(input: &[u8]) -> bool {
    use std::io::Read;
    if input.is_empty() {
        return true;
    }
    let bound = u64::try_from(MAX_DECODE)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let whole = |reader: &mut dyn Read| {
        std::io::copy(&mut reader.take(bound), &mut std::io::sink()).is_ok()
    };
    whole(&mut flate2::read::ZlibDecoder::new(input))
        || (input.len() > 2 && whole(&mut flate2::read::DeflateDecoder::new(&input[2..])))
}

/// Whether decoded content draws an inline image.
///
/// A token test first, because nearly every stream has neither `BI` nor `ID`
/// as a word and parsing every stream to find that out would cost the scan a
/// second pass. When both occur, the content parser decides, so `(BI ID)` in a
/// string is not a picture; and when the parser refuses bytes that hold both,
/// they are counted as a picture, which withholds the verdict rather than
/// waving through a stream nothing could read.
fn draws_inline_picture(content: &[u8]) -> bool {
    if !has_token(content, b"BI") || !has_token(content, b"ID") {
        return false;
    }
    lopdf::content::Content::decode(content).map_or(true, |parsed| {
        parsed.operations.iter().any(|op| op.operator == "BI")
    })
}

/// Whether `token` occurs in `content` as a word of PDF syntax.
pub(crate) fn has_token(content: &[u8], token: &[u8]) -> bool {
    let apart = |byte: u8| {
        matches!(
            byte,
            b'\0'
                | b'\t'
                | b'\n'
                | b'\x0c'
                | b'\r'
                | b' '
                | b'('
                | b')'
                | b'<'
                | b'>'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'/'
                | b'%'
        )
    };
    content
        .windows(token.len())
        .enumerate()
        .any(|(at, window)| {
            window == token
                && (at == 0 || apart(content[at - 1]))
                && content
                    .get(at + token.len())
                    .is_none_or(|next| apart(*next))
        })
}

/// How deep one page's reachability walk may follow references.
///
/// A page reaches its content stream at depth 1 and a form's resources two
/// levels further down, so a document nesting forms eight deep --- which is the
/// bound `crate::textedit::forms` enforces for the editor --- sits at about 17.
/// Thirty-two leaves room for that and stops a reference cycle that the
/// per-page visited set somehow did not.
const MAX_REACH_DEPTH: usize = 32;

/// How many distinct objects one page's walk may reach.
///
/// Per page rather than per document, because the question the walk answers is
/// per page: a document of ten thousand pages is not more suspicious than one
/// page, but a single page that reaches a hundred thousand objects is.
const MAX_REACH_OBJECTS: usize = 100_000;

/// How much work the whole walk may do, across every page.
///
/// The per-page bound above cannot bound the document: a hostile file can pay
/// its cost once per page. This is the one that makes the walk's total cost a
/// function of nothing the file controls without limit.
const MAX_REACH_STEPS: usize = 4_000_000;

/// How many objects may carry one needle before the walk stops placing it.
///
/// Not a performance bound --- it is the point at which an answer stops being
/// one. A word in a thousand objects is not *on* a page, and the honest answer
/// for it is [`Located::Unplaced`] rather than a list nobody can read.
const MAX_CARRIERS: usize = 1_000;

/// How many pages one answer names before it counts the rest.
///
/// [`MAX_OBJECT_REASONS`]'s rule, in the place the same pressure shows up: the
/// report crosses a pipe under [`crate::worker_proto::MAX_REPLY_BYTES`], and a
/// needle on every page of a ten-thousand-page file would otherwise put ten
/// thousand numbers in it, once per needle. The enumeration is shortened and
/// the count survives; the verdict is untouched either way.
const MAX_LOCATED_PAGES: usize = 64;

/// Dictionary keys a page's walk must not follow, because they leave the page.
///
/// **The whole soundness of attribution is this list.** A page dictionary names
/// its `/Parent`, an annotation names its `/P`, a link names a `/Dest` on some
/// other page, and an outline entry names its `/Next`. Following any one of
/// them walks out of this page and into the rest of the document, and the walk
/// would then answer *every page* for every object --- which is not a weaker
/// answer than none, it is a **wrong** one, and a wrong attribution turns
/// *"still in the file"* into *"on a page you did not mark"*.
///
/// So this errs towards reaching too little. An appearance stream under `/D`
/// (an annotation's *down* look) is skipped with the actions that share the
/// key, and a needle sitting only there is reported [`Located::Unplaced`]
/// rather than placed --- which withholds an answer instead of inventing one.
/// That is the direction this module is built around.
const NOT_CONTENT: &[&[u8]] = &[
    // Up and across the page tree.
    b"Parent",
    b"Kids",
    b"Root",
    b"Pages",
    b"PageLabels",
    // An annotation's page, and a reply's antecedent.
    b"P",
    b"IRT",
    // Destinations and actions, which name another page by design.
    b"Dest",
    b"D",
    b"A",
    b"AA",
    b"OpenAction",
    b"Names",
    // The outline and the article threads, which are document-wide chains.
    b"Outlines",
    b"First",
    b"Last",
    b"Next",
    b"Prev",
    b"Threads",
    b"B",
    // The structure tree, which reaches every page from any element.
    b"StructTreeRoot",
    b"StructParent",
    b"StructParents",
    b"K",
    // The form, whose field tree spans the document.
    b"AcroForm",
];

/// Pages an answer names, with however many it did not name.
///
/// Slots are **0-based positions in the file that was scanned**, which for a
/// redaction is the file that was just written --- not page numbers of the
/// document the reader opened, and not the baseline numbers the plan is in
/// terms of. [`Placed::sentence`] is the only thing that turns one into a page
/// number a person reads, and it is the only place the `+ 1` happens.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Placed {
    /// Ascending, at most [`MAX_LOCATED_PAGES`] of them.
    pub pages: Vec<u32>,
    /// How many further pages there were.
    pub more: usize,
}

impl Placed {
    /// Takes the first [`MAX_LOCATED_PAGES`] and counts the rest.
    #[must_use]
    pub fn of(pages: &BTreeSet<u32>) -> Self {
        Placed {
            pages: pages.iter().copied().take(MAX_LOCATED_PAGES).collect(),
            more: pages.len().saturating_sub(MAX_LOCATED_PAGES),
        }
    }

    /// The pages as a person reads them, one-based.
    #[must_use]
    pub fn sentence(&self) -> String {
        let numbers: Vec<String> = self
            .pages
            .iter()
            .map(|slot| (slot + 1).to_string())
            .collect();
        if self.more > 0 {
            return said!(
                "pages {pages}, and {more} more",
                pages = numbers.join(", "),
                more = self.more
            );
        }
        match numbers.as_slice() {
            [] => said!("no page"),
            [one] => format!("page {one}"),
            [first, second] => said!("pages {first} and {last}", last = second),
            [rest @ .., last] => said!("pages {first} and {last}", first = rest.join(", ")),
        }
    }
}

/// Where a needle the scan found still sits, as far as the walk can prove.
///
/// **Three answers rather than two, and the third is the point.** A scan that
/// could only say *found* or *not found* was what made *"4711-0815 is still in
/// the file"* unactionable: a reader could not tell a removal that failed from
/// a second copy on a page nobody marked. Two answers would have been *placed*
/// and *not placed*, and that is the version that ships a lie --- an object
/// more than one page draws has no page, and calling it the first page that
/// reached it is exactly the wrong attribution this type exists to refuse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Located {
    /// Every object carrying it is reached by exactly one page, and these are
    /// those pages.
    Pages(Placed),
    /// Something carrying it is drawn by more than one page --- a shared form,
    /// a shared font --- so no page owns it. The pages that share it.
    Shared(Placed),
    /// Something carrying it is reached by no page at all: bytes outside the
    /// page tree, the file's own metadata, or a hit the byte scan found and the
    /// graph walk never accounted for.
    Unplaced,
}

/// Which pages of a document reach which objects.
///
/// Built once per scan and only when there is something to place. The `Vec` per
/// object is exact rather than a set because the outer loop runs slots in
/// ascending order and visits each object at most once per page, so pushing
/// when the last entry is not this slot is a deduplication.
///
/// **Keyed by the whole [`lopdf::ObjectId`], generation included, and not by the
/// object number.** The number is what a report *prints* --- every `object N`
/// message in this module drops the generation --- and for a message that is
/// fine. For attribution it is not: `lopdf` keys `Document::objects` by the
/// pair, so a file defining both `5 0 obj` and `5 1 obj` holds two objects here,
/// and a walk keyed on `5` would hand one of them the other's pages. That is a
/// *wrong* page, which is the one answer this whole walk exists to refuse.
///
/// **Defensive rather than demonstrated**, and worth saying so: an ordinary
/// cross-reference table carries one generation per number, and `lopdf`'s writer
/// emits generation 0 for everything, so no fixture here can be built with the
/// collision in it. It costs nothing, and the direction it fails in if it ever
/// does fire is the expensive one.
struct Reach {
    by_object: std::collections::HashMap<lopdf::ObjectId, Vec<u32>>,
}

/// Every page's reachable objects, or `None` when a bound stopped the walk.
///
/// **A bound anywhere withholds attribution everywhere, and that is the safe
/// direction rather than a convenience.** A walk truncated on page 400 has not
/// merely lost page 400's answer: an object it would have reached there is now
/// recorded as reached by page 1 alone, and [`locate`] would place a needle on
/// page 1 that sits on both. Under-claiming is what this module is for, so a
/// truncated walk answers nothing at all.
fn reach(doc: &Document) -> Option<Reach> {
    let pages = crate::pagetree::ordered_pages(doc);
    let mut by_object: std::collections::HashMap<lopdf::ObjectId, Vec<u32>> =
        std::collections::HashMap::new();
    let mut steps = 0usize;
    for (slot, page) in pages.iter().enumerate() {
        let slot = u32::try_from(slot).ok()?;
        let mut seen: BTreeSet<lopdf::ObjectId> = BTreeSet::new();
        let mut stack: Vec<(lopdf::ObjectId, usize)> = vec![(*page, 0)];
        while let Some((id, depth)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if seen.len() > MAX_REACH_OBJECTS {
                return None;
            }
            steps += 1;
            if steps > MAX_REACH_STEPS {
                return None;
            }
            let reached = by_object.entry(id).or_default();
            if reached.last() != Some(&slot) {
                reached.push(slot);
            }
            let Ok(object) = doc.get_object(id) else {
                // Not a truncation: a dangling reference reaches nothing, and a
                // page that names one is a page with a hole in it rather than a
                // page this walk failed to read. Anything carried by the object
                // that is not there cannot be found by the scan either.
                continue;
            };
            if !push_refs(object, depth, &mut stack, &mut steps) {
                return None;
            }
        }
    }
    Some(Reach { by_object })
}

/// Pushes every reference inside one object, skipping the keys that leave the page.
///
/// Returns `false` when a bound tripped, which the caller turns into no
/// attribution at all. The inner walk is a worklist rather than recursion
/// because its depth is the file's to choose: a directly nested array is not a
/// reference and so is not bounded by [`MAX_REACH_DEPTH`], and recursing on it
/// would put the file in charge of this process's stack.
fn push_refs(
    object: &Object,
    depth: usize,
    stack: &mut Vec<(lopdf::ObjectId, usize)>,
    steps: &mut usize,
) -> bool {
    let mut inner: Vec<&Object> = vec![object];
    while let Some(value) = inner.pop() {
        *steps += 1;
        if *steps > MAX_REACH_STEPS {
            return false;
        }
        match value {
            Object::Reference(id) => {
                if depth + 1 > MAX_REACH_DEPTH {
                    return false;
                }
                stack.push((*id, depth + 1));
            }
            Object::Array(items) => inner.extend(items.iter()),
            Object::Dictionary(dict) => push_entries(dict, &mut inner),
            Object::Stream(stream) => push_entries(&stream.dict, &mut inner),
            _ => {}
        }
    }
    true
}

/// A dictionary's values, minus the ones [`NOT_CONTENT`] names.
///
/// **One function for the two arms above, so the skip list has one call site.**
/// The obvious shape is the loop written twice, and it is what this was; a
/// content stream's dictionary carries none of these keys in any fixture here,
/// so the second copy of the guard was one no test could reach and no mutation
/// could aim at. `docs/TRAPS.md` has the general form more than once --- a check
/// bound to one caller covers only that caller, and two copies of one rule
/// drift.
fn push_entries<'a>(dict: &'a lopdf::Dictionary, inner: &mut Vec<&'a Object>) {
    for (key, value) in dict {
        if NOT_CONTENT.contains(&key.as_slice()) {
            continue;
        }
        inner.push(value);
    }
}

/// Which objects carry one needle, and whether there were too many to say.
#[derive(Default)]
struct Carriers {
    objects: BTreeSet<lopdf::ObjectId>,
    /// More than [`MAX_CARRIERS`] of them, so the list is not an answer.
    overflowed: bool,
    /// Also carried by something that is not an object at all (the trailer),
    /// which no page reaches, so no page set is the answer.
    unreached: bool,
}

impl Carriers {
    fn note(&mut self, object: lopdf::ObjectId) {
        if self.objects.len() >= MAX_CARRIERS {
            self.overflowed = true;
            return;
        }
        self.objects.insert(object);
    }
}

/// Turns each needle's carriers into the pages that reach them.
///
/// **The weakest carrier decides.** A needle carried by one page-1-only object
/// and one object nobody reaches is not on page 1: it is somewhere this walk
/// cannot account for, and saying *page 1* would be the wrong attribution. So
/// [`Located::Unplaced`] outranks [`Located::Shared`], which outranks
/// [`Located::Pages`], and a needle only gets the specific answer when every
/// one of its carriers earned it.
fn locate(
    doc: &Document,
    found: &BTreeSet<String>,
    carriers: &BTreeMap<String, Carriers>,
) -> BTreeMap<String, Located> {
    let Some(reach) = reach(doc) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for needle in found {
        // No object carries it and the byte scan found it anyway: it is in
        // bytes no page reaches. That is the one case that can never become an
        // answer, however good the walk gets.
        let Some(carried) = carriers.get(needle) else {
            out.insert(needle.clone(), Located::Unplaced);
            continue;
        };
        if carried.overflowed || carried.objects.is_empty() {
            out.insert(needle.clone(), Located::Unplaced);
            continue;
        }
        let mut pages: BTreeSet<u32> = BTreeSet::new();
        let mut shared: BTreeSet<u32> = BTreeSet::new();
        let mut unplaced = false;
        for object in &carried.objects {
            match reach.by_object.get(object) {
                None => unplaced = true,
                Some(slots) => {
                    if slots.len() > 1 {
                        shared.extend(slots.iter().copied());
                    }
                    pages.extend(slots.iter().copied());
                }
            }
        }
        let answer = if unplaced || carried.unreached {
            Located::Unplaced
        } else if !shared.is_empty() {
            Located::Shared(Placed::of(&shared))
        } else {
            Located::Pages(Placed::of(&pages))
        };
        out.insert(needle.clone(), answer);
    }
    out
}

/// One word for a caller that needs one, and never a bare success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every carrier accounted for, and nothing that should be gone is present.
    Verified,
    /// With the specifics. `docs/PLAN.md` §6 forbids reporting this any other
    /// way: a redaction that cannot be proved clean is a confident lie, and the
    /// reasons are what tell a reader whether the next step is OCR, a rewrite,
    /// or giving up on the file.
    NotVerified(Vec<String>),
}

/// What a scan of one file found, and what it could not look at.
///
/// **Serialised, because the scan runs in a worker.** This is the answer
/// [`crate::worker_proto::Reply::Verified`] carries back, and it is the whole of
/// what crosses: the bytes it was read from stay behind the boundary. That is
/// also why the per-object lists are bounded --- see [`MAX_OBJECT_REASONS`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// Needles still present. For a redaction, any of these is a leak.
    pub found: BTreeSet<String>,
    /// Where each of them is, for the ones the walk could place.
    ///
    /// **Absent is not a page, and an entry is not an excuse.** A needle with no
    /// entry here was never attributed --- either nothing was found, or the walk
    /// was withheld --- and [`Report::verdict`] then says exactly what it said
    /// before this existed. A needle with an entry is still a leak; the entry
    /// only says where, which is what makes the finding actionable.
    #[serde(default)]
    pub located: BTreeMap<String, Located>,
    /// Bytes nothing here can account for. No instrument would change this.
    pub blind: Vec<String>,
    /// Pictures nobody read. OCR is the instrument that would.
    pub deferred: Vec<String>,
    /// How many objects the graph walk reached.
    pub objects: usize,
    /// `%%EOF` markers that end a revision, as [`revision_ends`] counts them,
    /// so more than one revision is visible.
    pub eofs: usize,
    /// Non-whitespace bytes after the last `%%EOF`.
    pub trailing: usize,
    /// The file's length, so a report says what it was about.
    pub bytes: u64,
}

impl Report {
    /// The one-word answer, with every reason that withheld it.
    ///
    /// **Order matters to a reader and not to the verdict.** A leak is reported
    /// first because it is the finding that makes the others moot: a file with a
    /// needle still in it is not going to become clean by running OCR.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let mut why = Vec::new();
        for needle in &self.found {
            why.push(match self.located.get(needle) {
                // What this said before attribution existed, and what it still
                // says whenever the walk did not run or would not answer.
                None => said!("{needle} is still in the file"),
                Some(Located::Pages(placed)) => {
                    said!(
                        "{needle} is still in the file, on {pages}",
                        pages = placed.sentence()
                    )
                }
                Some(Located::Shared(placed)) => said!(
                    "{needle} is still in the file, carried by something more than one page \
                     draws ({pages}), so which page it is on could not be established",
                    pages = placed.sentence()
                ),
                Some(Located::Unplaced) => said!(
                    "{needle} is still in the file, in something no page of it reaches, so \
                     which page it is on could not be established"
                ),
            });
        }
        why.extend(self.blind.iter().cloned());
        why.extend(self.deferred.iter().cloned());
        if why.is_empty() {
            Verdict::Verified
        } else {
            Verdict::NotVerified(why)
        }
    }

    /// Whether every word the scan found was placed on a page set.
    ///
    /// **The precondition for anything a caller says about marked pages.** A
    /// caller knows which slots the reader marked and this knows where the
    /// words are; only both together make *"none of them is on a page you
    /// marked"* a provable sentence. One needle the walk would not place makes
    /// the whole comparison unsound, so this is `all` and not `any`.
    ///
    /// False for a report that found nothing, deliberately: there is nothing to
    /// compare, and a caller reading this as *"the pages you marked are clean"*
    /// would be reading a claim out of an empty set.
    #[must_use]
    pub fn placed(&self) -> bool {
        !self.found.is_empty()
            && self
                .found
                .iter()
                .all(|needle| matches!(self.located.get(needle), Some(Located::Pages(_))))
    }

    /// Every page a placed needle sits on, as slots in the file that was scanned.
    #[must_use]
    pub fn placed_pages(&self) -> BTreeSet<u32> {
        self.located
            .values()
            .filter_map(|where_| match where_ {
                Located::Pages(placed) => Some(placed.pages.iter().copied()),
                Located::Shared(_) | Located::Unplaced => None,
            })
            .flatten()
            .collect()
    }
}

/// What is certainly wrong with a file this build just wrote.
///
/// **The narrow half of `docs/PLAN.md` §6 step 5, and the narrowness is the
/// finding.** That step asks for an independent parser to re-check a rewrite,
/// on the strength of spike 0.4 catching a `/Size` that claimed more objects
/// than the file held --- PDFium rendered it pixel-perfect, `qpdf --check`
/// named it. Four readers were put to that same defect on 2026-08-26, and the
/// result decided this function's shape:
///
/// | reader | stale `/Size` |
/// |---|---|
/// | this byte scan | silent |
/// | `lopdf`'s loader | *OK, 8 pages* |
/// | PDFKit (`print_macos::read`) | *OK, 8 pages*, in 0.2 ms |
/// | `qpdf --check` | **exit 3** |
///
/// So there is no in-app parser that catches it, and the obvious repair --- our
/// own rule that `/Size` must equal the cross-reference table's entry count ---
/// was written, run, and **condemned a healthy file**: a swept rewrite of
/// `links.pdf` has 91 entries in three subsections against `/Size 102`, because
/// object numbers go sparse when a sweep removes objects and an unlisted number
/// is free. `qpdf --check` passes it. Every `incr-*.pdf` fixture fails the same
/// rule for the same reason from the other direction, since an incremental
/// file's `/Size` counts every revision's objects and its last section lists
/// only what changed.
///
/// A validator that fires on correct input is worse than none, so this checks
/// **only what cannot be legitimate in a file we wrote a moment ago**, and says
/// so rather than implying it covers structure. Real cross-reference validation
/// is qpdf's, it is not here, and `docs/PLAN.md` §6 keeps the note that QPDF
/// still has a place.
///
/// The two revision rules are §6's own words --- *assert exactly one logical
/// revision and no trailing data* --- and they are meaningful precisely because
/// this is our output: a **source** document may legitimately have many
/// revisions, and this is never pointed at one.
///
/// Costs a scan of the bytes and nothing else: 65.8 ms on the 321 MB fixture,
/// 0.34 ms on a 1.3 MB one.
#[must_use]
pub fn structure(bytes: &[u8]) -> Vec<String> {
    let mut wrong = Vec::new();

    if !bytes.starts_with(b"%PDF-") {
        wrong.push(said!("the file does not begin with a PDF header"));
    }

    match revision_ends(bytes) {
        0 if count(bytes, b"%%EOF") == 0 => {
            wrong.push(said!("the file has no %%EOF marker"));
        }
        0 => wrong.push(
            said!("the file has a %%EOF marker and none that ends a revision: none follows a \
             startxref and its offset"),
        ),
        1 => {}
        many => wrong.push(said!(
            "the file has {many} %%EOF markers, so it holds more than one revision \u{2014} a rewrite writes exactly one, and an earlier revision is content no parser will show and no scan can decode"
        )),
    }

    if let Some(last) = rfind(bytes, b"%%EOF") {
        let trailing = bytes[last + 5..]
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
        if trailing > 0 {
            wrong.push(said!(
                "{trailing} byte(s) follow the last %%EOF, which belong to no object and which nothing here put there"
            ));
        }
    }

    // Not "is there a `startxref`" --- `rfind` would find the one inside a
    // string or a comment just as happily. What makes this worth checking is
    // the *offset*: a rewrite computes it, and one pointing past the end of the
    // file is a file no reader can open, which is the failure this is between
    // the reader and.
    match rfind(bytes, b"startxref") {
        None => wrong.push(said!("the file has no startxref")),
        Some(at) => match start_offset(&bytes[at + 9..]) {
            None => wrong.push(said!("the file's startxref has no offset after it")),
            Some(offset) if offset >= bytes.len() => wrong.push(said!(
                "startxref points at byte {offset} of a {size}-byte file",
                size = bytes.len()
            )),
            Some(_) => {}
        },
    }

    wrong
}

/// The decimal number after a `startxref`, skipping the whitespace before it.
///
/// Returns `None` for no digits at all, and for a number too large to be a file
/// offset --- which is the same answer for the purpose here, since both mean the
/// offset cannot be believed.
fn start_offset(after: &[u8]) -> Option<usize> {
    let digits: Vec<u8> = after
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take_while(|byte| byte.is_ascii_digit())
        .copied()
        .collect();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Scans a file for every needle, at the byte level and through the object graph.
///
/// Both are needed and neither is sufficient. The byte scan is the only thing
/// that can see content outside the object graph --- trailing bytes past `%%EOF`
/// belong to no object at all --- and the graph walk is the only thing that can
/// see inside a compressed stream.
///
/// Promoted from `examples/sanitize_rewrite.rs` on 2026-08-26, which now calls
/// this rather than carrying its own copy: two implementations of what counts as
/// clean is the drift this repository keeps finding in other forms.
///
/// # Errors
///
/// The file cannot be read. A file that cannot be *parsed* is not an error ---
/// it is a blind spot, reported as one, which is the whole point of the type.
pub fn scan(bytes: &[u8], needles: &[String], password: Option<&str>) -> Report {
    let mut report = Report {
        eofs: revision_ends(bytes),
        bytes: bytes.len() as u64,
        ..Default::default()
    };

    for needle in needles {
        if find(bytes, needle.as_bytes()) {
            report.found.insert(needle.clone());
        }
    }

    if report.eofs > 1 {
        report.blind.push(said!(
            "the file has {count} %%EOF markers, so earlier revisions exist that no parser will \
             resolve and no scan can decode",
            count = report.eofs
        ));
    }

    match rfind(bytes, b"%%EOF") {
        Some(last) => {
            report.trailing = bytes[last + 5..]
                .iter()
                .filter(|byte| !byte.is_ascii_whitespace())
                .count();
        }
        None => report.blind.push(said!("the file has no %%EOF marker")),
    }

    match Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    ) {
        Err(why) => report.blind.push(said!(
            "the file could not be parsed, so nothing in it was accounted for: {why}"
        )),
        Ok(doc) => {
            report.objects = doc.objects.len();
            // **Before the walk, because an empty walk finds nothing and that
            // reads exactly like a clean file.** `lopdf` answers `Ok` for a
            // document it could not authenticate, having parsed no objects at
            // all --- so the loop below would compare every needle against
            // nothing, `found` would stay empty, and `verdict` would answer
            // *Verified* about bytes this build never decoded. An absence and a
            // lock are the same reading here, and the reassuring one is wrong.
            //
            // `docs/PLAN.md` §6 forbids reporting a redaction clean that was not
            // proved clean, and this is the one scanner that claim rests on.
            if doc.is_encrypted() {
                report.blind.push(said!(
                    "the file is encrypted and no password opened it, so nothing in it was \
                     decoded and nothing it contains was checked"
                ));
            } else if doc.objects.is_empty() {
                report.blind.push(said!(
                    "the file parsed to no objects at all, so nothing in it was accounted for"
                ));
            }
            // Counted in full and listed in part: see `MAX_OBJECT_REASONS`.
            // `blind` also carries reasons that are about the *file* rather than
            // about an object, and those are never suppressed --- which is why
            // this is a counter and not `report.blind.len()`.
            let mut blind_objects = 0usize;
            let mut deferred_objects = 0usize;
            // Which objects carry which needle, so the walk below can turn that
            // into pages. Recorded here because this is the only loop that
            // decodes anything, and only for needles that actually match, so a
            // clean file pays nothing for it.
            let mut carriers: BTreeMap<String, Carriers> = BTreeMap::new();
            // Each needle in the byte forms a simple font's string can hold
            // it in, worked out once for every stream: see [`spellings_of`].
            let spellings: Vec<Vec<Vec<u8>>> =
                needles.iter().map(|needle| spellings_of(needle)).collect();
            // Asked through the document, because the `/Subtype` that says so
            // may be a reference, and PDFium follows it.
            let names_a_composite_font = |object: &Object| composite_font_in(&doc, object);
            let composite = doc.objects.values().any(names_a_composite_font);
            // **Since 2026-11-20 a composite font is read, not refused**, where
            // it can be: every page's strings decoded through the font that
            // shows them (`fontread`), and searched below beside the bytes. The
            // reason is said once for the file when any composite string could
            // not be read that way, or when a stream that shows text was not
            // reached by the walk, so whose font it used is unknown.
            let fonts = crate::fontread::read(&doc, composite);
            let mut text_unattributed = false;
            // The same gap in a simple font, where it is narrower: a
            // `/Differences` array that puts a character of a searched word at
            // another byte, or names a glyph nothing here can read. Asked per
            // word, because most such arrays only rename punctuation or add
            // accented letters, and a file-wide reason would withhold the
            // verdict of most typeset documents for a word they write plainly.
            let differences = differences_in(&doc);
            if needles
                .iter()
                .any(|needle| differences.writes_elsewhere(needle))
            {
                report.blind.push(said!(
                    "the file draws text through a font whose own encoding (/Differences) writes \
                     some of the removed characters as other bytes, so a copy of the words drawn \
                     through it cannot be found by reading its bytes"
                ));
            }
            // Carriers the scan reads only as bytes, whose contents are another
            // format: an embedded file is a document of its own --- a zipped
            // office file, an image, another PDF with its own filters --- and
            // a producer's private data is whatever that producer wrote. A
            // redaction neither removes nor reads them, so each is a reason.
            let held = carriers_held(&doc);
            if held.embedded {
                report.blind.push(said!(
                    "the file carries embedded files, which are documents of their own that \
                     nothing here reads, so a copy of the words inside one would not be found"
                ));
            }
            if held.private {
                report.blind.push(said!(
                    "the file carries an application's private data (/PieceInfo), which nothing \
                     here reads, so a copy of the words inside it would not be found"
                ));
            }
            for (id, object) in &doc.objects {
                let strings = flatten_strings(object);
                for needle in needles {
                    if find(&strings, needle.as_bytes()) {
                        report.found.insert(needle.clone());
                        carriers.entry(needle.clone()).or_default().note(*id);
                    }
                }
                let Object::Stream(stream) = object else {
                    continue;
                };
                // **A `/Filter` that cannot be read is not an empty one.**
                // `lopdf` accepts only a name or an array of names, and answers
                // `Err` for an indirect reference or an array holding one ---
                // which PDFium resolves, and draws. Read as "no filter", that
                // stream would be scanned as its compressed bytes, find nothing
                // and certify a copy of the words the reader can see. And the
                // decoder fails the same way, handing back the raw bytes rather
                // than an error, so the `Err` arm below would never fire either.
                let filters = match stream.filters() {
                    Ok(filters) => filters,
                    Err(_) if !stream.dict.has(b"Filter") => Vec::new(),
                    Err(_) => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(said!(
                                "object {object} names its filter in a form this build does not \
                                 read, so its contents were not decoded",
                                object = id.0
                            ));
                        }
                        continue;
                    }
                };
                // Read the way a viewer reads it, or counted as not read: see
                // [`as_drawn`], which is the one place that question is asked.
                let drawn = as_drawn(&doc, id.0, stream, &filters);
                match drawn.carrier {
                    Carrier::Image { filter } => {
                        deferred_objects += 1;
                        if deferred_objects <= MAX_OBJECT_REASONS {
                            report.deferred.push(if filter.is_empty() {
                                said!(
                                    "object {object} is an image stored without compression, so \
                                     its picture was not read; text visible only as pixels needs \
                                     OCR",
                                    object = id.0
                                )
                            } else {
                                said!(
                                "object {object} is a {filter} image, so its encoded bytes were \
                                 scanned and its picture was not read \u{2014} text visible only as \
                                 pixels needs OCR", object = id.0)
                            });
                        }
                    }
                    Carrier::Undecodable { filter } => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(said!(
                                "object {object} is {filter}, which this build cannot decode, so its \
                                 contents are unknown", object = id.0));
                        }
                    }
                    Carrier::Unrecognised { filter } => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(said!(
                                "object {object} uses {filter}, which nothing here recognises",
                                object = id.0
                            ));
                        }
                    }
                    Carrier::Unread { why } => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(why);
                        }
                    }
                    Carrier::Scanned => {
                        // The stream's string operands, decoded and joined:
                        // see [`strings_shown`]. Lexed only when a needle is
                        // not already in the raw bytes.
                        let mut shown: Option<Vec<u8>> = None;
                        let decoded_text = fonts.text.get(id);
                        if composite
                            && decoded_text.is_none()
                            && may_show_text(stream)
                            && shows_text(&drawn.decoded)
                        {
                            text_unattributed = true;
                        }
                        for (needle, spelled) in needles.iter().zip(&spellings) {
                            let hit = find(&drawn.decoded, needle.as_bytes())
                                || decoded_text.is_some_and(|readings| {
                                    readings.iter().any(|text| find(text, needle.as_bytes()))
                                })
                                || {
                                    let shown =
                                        shown.get_or_insert_with(|| strings_shown(&drawn.decoded));
                                    spelled.iter().any(|one| find(shown, one))
                                };
                            if hit {
                                report.found.insert(needle.clone());
                                carriers.entry(needle.clone()).or_default().note(*id);
                            }
                        }
                        // The words around it were read; the picture was not, and
                        // it is the same picture a `BI` before it would have drawn
                        // from an XObject, so it gets that picture's reason.
                        if drawn.inline_picture {
                            deferred_objects += 1;
                            if deferred_objects <= MAX_OBJECT_REASONS {
                                report.deferred.push(said!(
                                    "object {object} draws a picture inside its own content (an \
                                     inline image), so that picture was not read; text visible \
                                     only as pixels needs OCR",
                                    object = id.0
                                ));
                            }
                        }
                    }
                }
            }
            // **The trailer is not an object, and it can hold strings.** A
            // document information dictionary written into the trailer
            // directly rather than by reference is read by every viewer that
            // shows a title, and the loop above walks `doc.objects` only, so a
            // title in UTF-16BE was compared nowhere in what it says. A word
            // found only here is carried by no object, which `locate` answers
            // as reached by no page: the truth about a trailer.
            let trailer = flatten_strings(&Object::Dictionary(doc.trailer.clone()));
            for needle in needles {
                if find(&trailer, needle.as_bytes()) {
                    report.found.insert(needle.clone());
                    carriers.entry(needle.clone()).or_default().unreached = true;
                }
            }
            // Said once for the file, however many such fonts or streams there
            // are: the reason is about the instrument.
            if composite && (!fonts.composite_read || text_unattributed) {
                report.blind.push(said!(
                    "the file draws text through a composite (Type0) font, which stores glyph \
                     numbers rather than characters, so a copy of the words drawn through it \
                     cannot be found by reading its bytes"
                ));
            }
            // The suppressed ones, said once. A count is still a reason, so the
            // verdict a file with a million bad objects earns is the verdict it
            // gets --- only the enumeration is shortened.
            if deferred_objects > MAX_OBJECT_REASONS {
                report.deferred.push(said!(
                    "{count} further images were not read, and are not listed individually",
                    count = deferred_objects - MAX_OBJECT_REASONS
                ));
            }
            if blind_objects > MAX_OBJECT_REASONS {
                report.blind.push(said!(
                    "{count} further objects could not be accounted for, and are not listed \
                     individually",
                    count = blind_objects - MAX_OBJECT_REASONS
                ));
            }

            // **Only when there is something to place, and only in a file the
            // walk can account for.**
            //
            // The first half is cost: a clean scan --- the common one --- never
            // walks a page at all.
            //
            // The second is soundness, and it is this module's opening
            // paragraph arriving as a guard. A file with more than one `%%EOF`
            // holds revisions no parser resolves: an object a later revision
            // overwrote sits at its old offset, addressable by nothing, so the
            // graph walk cannot see it and neither can the byte scan if it is
            // compressed. Attributing a needle to page 5 in such a file would
            // be a claim about the *live* graph offered as a claim about the
            // file, with a dead copy of the same word invisible beside it.
            // `blind` already says the file is uncertifiable; this says the
            // location is unknowable too, rather than answering anyway.
            if !report.found.is_empty() && report.eofs <= 1 {
                report.located = locate(&doc, &report.found, &carriers);
            }
        }
    }

    report.blind.sort();
    report.blind.dedup();
    report.deferred.sort();
    report.deferred.dedup();
    report
}

/// Whether a decoded stream draws text: holds a text-showing operator as a
/// word of PDF syntax. A string holding one counts too, which errs towards
/// withholding.
fn shows_text(content: &[u8]) -> bool {
    [&b"Tj"[..], b"TJ", b"'", b"\""]
        .iter()
        .any(|op| has_token(content, op))
}

/// Whether a stream can be a content stream at all, by its dictionary: not a
/// font program, a colour profile, metadata, a CMap, an embedded file or a
/// cross-reference or object stream. Their bytes are not drawn as text, and a
/// font program's binary can hold `Tj` by chance.
fn may_show_text(stream: &lopdf::Stream) -> bool {
    let named = |key: &[u8], names: &[&[u8]]| {
        stream
            .dict
            .get(key)
            .and_then(Object::as_name)
            .is_ok_and(|name| names.contains(&name))
    };
    !(stream.dict.has(b"Length1")
        || stream.dict.has(b"N")
        || named(
            b"Subtype",
            &[b"Type1C", b"CIDFontType0C", b"OpenType", b"XML"],
        )
        || named(
            b"Type",
            &[b"Metadata", b"EmbeddedFile", b"CMap", b"XRef", b"ObjStm"],
        ))
}

/// Whether `object` is, or holds, a font dictionary of subtype `/Type0`.
///
/// Inside the object as well as at its top, because a font may be written
/// directly into a `/Resources` dictionary and PDFium draws it there too. A
/// worklist rather than recursion, for [`push_refs`]'s reason: the nesting is
/// the file's to choose. The work is linear in the object's size, which
/// [`flatten_strings`] already pays for every object.
///
/// **`/Subtype` is resolved through `doc`**, because the key a viewer reads is
/// the key this has to read: a font whose `/Subtype` is `8 0 R`, naming
/// `/Type0`, is a composite font to PDFium, and a direct-only read counted it
/// as nothing at all. The same shape as the indirect `/Filter`, one key over.
fn composite_font_in(doc: &Document, object: &Object) -> bool {
    let mut inner: Vec<&Object> = vec![object];
    while let Some(value) = inner.pop() {
        let dict = match value {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => &stream.dict,
            Object::Array(items) => {
                inner.extend(items.iter());
                continue;
            }
            _ => continue,
        };
        if subtype_is(doc, dict, b"Type0") {
            return true;
        }
        inner.extend(dict.iter().map(|(_, value)| value));
    }
    false
}

/// What every `/Differences` array in a document does to the bytes a word is
/// written as.
///
/// A simple font draws the byte `c` as the glyph its encoding names at `c`, and
/// PDFium extracts the character that glyph name stands for. Where the name at
/// `c` stands for an ASCII character other than `c` itself, that character is
/// written as `c`, and the byte search, which looks for the character's own
/// byte, does not find it. A ligature takes two or three letters into one byte.
/// A name nothing here can resolve (`/g12`, a Type3 font's `/a0`) could stand
/// for any character. Those three are the gap; a renamed punctuation mark at
/// its own code, an accented letter above 127 or `.notdef` are not.
///
/// The font's own built-in encoding and a `/ToUnicode` map can move characters
/// the same way and are not read here: residual 42 in `docs/THREAT-MODEL.md`.
#[derive(Default)]
struct Differences {
    /// ASCII characters some array writes at a byte other than their own.
    moved: BTreeSet<u8>,
    /// The letters a ligature glyph in some array stands for.
    ligatures: BTreeSet<&'static str>,
    /// An array holds a glyph name, or an entry, nothing here can read.
    opaque: bool,
}

impl Differences {
    /// Whether a copy of `needle` drawn through one of these encodings would be
    /// written as bytes other than its own.
    fn writes_elsewhere(&self, needle: &str) -> bool {
        self.opaque
            || needle.bytes().any(|byte| self.moved.contains(&byte))
            || self
                .ligatures
                .iter()
                .any(|letters| needle.contains(letters))
    }
}

/// Collects [`Differences`] from every dictionary in `doc` that has one,
/// following the array and its entries through references as PDFium does.
fn differences_in(doc: &Document) -> Differences {
    let mut found = Differences::default();
    for object in doc.objects.values() {
        let mut inner: Vec<&Object> = vec![object];
        while let Some(value) = inner.pop() {
            let dict = match value {
                Object::Dictionary(dict) => dict,
                Object::Stream(stream) => &stream.dict,
                Object::Array(items) => {
                    inner.extend(items.iter());
                    continue;
                }
                _ => continue,
            };
            if let Ok(array) = dict.get(b"Differences") {
                note_differences(doc, array, &mut found);
            }
            inner.extend(
                dict.iter()
                    .filter(|(key, _)| key.as_slice() != b"Differences")
                    .map(|(_, value)| value),
            );
        }
    }
    found
}

/// Reads one `/Differences` value into `found`: a code, then the names placed
/// at it and the codes after it, then the next code. Anything else in it, or a
/// value that is not an array, is opaque.
fn note_differences(doc: &Document, array: &Object, found: &mut Differences) {
    let Ok((_, Object::Array(items))) = doc.dereference(array) else {
        found.opaque = true;
        return;
    };
    let mut code: Option<i64> = None;
    for item in items {
        match doc.dereference(item).map(|(_, value)| value) {
            Ok(Object::Integer(start)) => code = Some(*start),
            Ok(Object::Name(name)) => {
                let Some(at) = code else {
                    found.opaque = true;
                    return;
                };
                match glyph(name) {
                    Glyph::Ascii(ch) if i64::from(ch) != at => {
                        found.moved.insert(ch);
                    }
                    Glyph::Ligature(letters) => {
                        found.ligatures.insert(letters);
                    }
                    Glyph::Opaque => found.opaque = true,
                    Glyph::Ascii(_) | Glyph::Other => {}
                }
                code = Some(at.saturating_add(1));
            }
            _ => {
                found.opaque = true;
                return;
            }
        }
    }
}

/// What a glyph name stands for, as far as the byte search is concerned.
#[derive(Debug, PartialEq, Eq)]
enum Glyph {
    /// One printable ASCII character.
    Ascii(u8),
    /// A ligature of these ASCII letters.
    Ligature(&'static str),
    /// A known character outside ASCII, or nothing (`.notdef`).
    Other,
    /// A name with no known meaning.
    Opaque,
}

/// The ASCII glyph names of the Adobe Glyph List, indexed from 0x20.
const ASCII_NAMES: [&str; 95] = [
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quotesingle",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
];

/// Ligature glyph names and the letters each stands for.
const LIGATURES: &[(&str, &str)] = &[
    ("ff", "ff"),
    ("fi", "fi"),
    ("fl", "fl"),
    ("ffi", "ffi"),
    ("ffl", "ffl"),
    ("f_f", "ff"),
    ("f_i", "fi"),
    ("f_l", "fl"),
    ("f_f_i", "ffi"),
    ("f_f_l", "ffl"),
    ("uniFB00", "ff"),
    ("uniFB01", "fi"),
    ("uniFB02", "fl"),
    ("uniFB03", "ffi"),
    ("uniFB04", "ffl"),
];

/// Glyph names outside ASCII that typeset documents put in `/Differences`:
/// the Latin-1 and WinAnsi upper halves, the TeX text encodings' accents and
/// Greek capitals, and the usual typographic marks. A name not here, not
/// ASCII and not `uniXXXX` is [`Glyph::Opaque`].
const OTHER_NAMES: &[&str] = &[
    ".notdef",
    "nbspace",
    "sfthyphen",
    "minus",
    "quoteright",
    "quoteleft",
    "quotedblleft",
    "quotedblright",
    "quotesinglbase",
    "quotedblbase",
    "guilsinglleft",
    "guilsinglright",
    "guillemotleft",
    "guillemotright",
    "endash",
    "emdash",
    "bullet",
    "dagger",
    "daggerdbl",
    "ellipsis",
    "perthousand",
    "trademark",
    "florin",
    "Euro",
    "fraction",
    "periodcentered",
    "exclamdown",
    "questiondown",
    "cent",
    "sterling",
    "currency",
    "yen",
    "brokenbar",
    "section",
    "copyright",
    "ordfeminine",
    "ordmasculine",
    "logicalnot",
    "registered",
    "degree",
    "plusminus",
    "twosuperior",
    "threesuperior",
    "onesuperior",
    "mu",
    "paragraph",
    "onequarter",
    "onehalf",
    "threequarters",
    "multiply",
    "divide",
    "acute",
    "dieresis",
    "macron",
    "cedilla",
    "circumflex",
    "tilde",
    "caron",
    "breve",
    "dotaccent",
    "ring",
    "ogonek",
    "hungarumlaut",
    "dotlessi",
    "dotlessj",
    "germandbls",
    "ae",
    "AE",
    "oe",
    "OE",
    "oslash",
    "Oslash",
    "lslash",
    "Lslash",
    "eth",
    "Eth",
    "thorn",
    "Thorn",
    "Scaron",
    "scaron",
    "Zcaron",
    "zcaron",
    "Ydieresis",
    "Agrave",
    "Aacute",
    "Acircumflex",
    "Atilde",
    "Adieresis",
    "Aring",
    "Ccedilla",
    "Egrave",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Igrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Ntilde",
    "Ograve",
    "Oacute",
    "Ocircumflex",
    "Otilde",
    "Odieresis",
    "Ugrave",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Yacute",
    "agrave",
    "aacute",
    "acircumflex",
    "atilde",
    "adieresis",
    "aring",
    "ccedilla",
    "egrave",
    "eacute",
    "ecircumflex",
    "edieresis",
    "igrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "ntilde",
    "ograve",
    "oacute",
    "ocircumflex",
    "otilde",
    "odieresis",
    "ugrave",
    "uacute",
    "ucircumflex",
    "udieresis",
    "yacute",
    "ydieresis",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
    "suppress",
];

/// Resolves a glyph name the way the Adobe Glyph List specification does:
/// the part before the first `.` is the name (`a.sc` is `a`), `_` joins the
/// names of a ligature, and `uniXXXX` and `uXXXX` name code points.
fn glyph(name: &[u8]) -> Glyph {
    let Ok(name) = std::str::from_utf8(name) else {
        return Glyph::Opaque;
    };
    if name == ".notdef" {
        return Glyph::Other;
    }
    let base = name.split('.').next().unwrap_or_default();
    if base.is_empty() {
        return Glyph::Opaque;
    }
    if let Some(&(_, letters)) = LIGATURES.iter().find(|(known, _)| *known == base) {
        return Glyph::Ligature(letters);
    }
    if let Some(at) = ASCII_NAMES.iter().position(|known| *known == base) {
        return Glyph::Ascii(0x20 + at as u8);
    }
    if OTHER_NAMES.contains(&base) {
        return Glyph::Other;
    }
    if base.contains('_') {
        // A ligature this table does not list: opaque unless every part is
        // a character outside ASCII, which the byte search does not read.
        return if base
            .split('_')
            .all(|part| glyph(part.as_bytes()) == Glyph::Other)
        {
            Glyph::Other
        } else {
            Glyph::Opaque
        };
    }
    let points = if let Some(hex) = base.strip_prefix("uni") {
        (hex.len() % 4 == 0 && !hex.is_empty())
            .then(|| {
                hex.as_bytes()
                    .chunks(4)
                    .map(|chunk| {
                        std::str::from_utf8(chunk)
                            .ok()
                            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                    })
                    .collect::<Option<Vec<u32>>>()
            })
            .flatten()
    } else if let Some(hex) = base.strip_prefix('u') {
        ((4..=6).contains(&hex.len()))
            .then(|| u32::from_str_radix(hex, 16).ok().map(|point| vec![point]))
            .flatten()
    } else {
        None
    };
    match points.as_deref() {
        Some([point]) if (0x20..0x7F).contains(point) => Glyph::Ascii(*point as u8),
        // `uFB01` is the ligature `uniFB01` is.
        Some([point @ 0xFB00..=0xFB04]) => {
            Glyph::Ligature(["ff", "fi", "fl", "ffi", "ffl"][(*point - 0xFB00) as usize])
        }
        Some(points) if points.iter().all(|point| *point >= 0x80) => Glyph::Other,
        // Several code points with an ASCII one among them, or a control
        // character: a ligature this table does not list.
        Some(_) => Glyph::Opaque,
        None => Glyph::Opaque,
    }
}

/// Which carriers of another format a document holds anywhere.
#[derive(Default)]
struct Held {
    /// `/EmbeddedFiles`, a file specification's `/EF`, or an `/AF` list.
    embedded: bool,
    /// `/PieceInfo` on a page, a form or the catalog.
    private: bool,
}

/// Looks through every object for the keys that hold such carriers.
///
/// By key rather than by place, because each may sit in more than one: an
/// embedded file is reached from the name tree, from an attachment
/// annotation's file specification and from an associated-files list, and
/// `/PieceInfo` from a page, a form or the catalog.
fn carriers_held(doc: &Document) -> Held {
    let mut held = Held::default();
    for object in doc.objects.values() {
        let mut inner: Vec<&Object> = vec![object];
        while let Some(value) = inner.pop() {
            let dict = match value {
                Object::Dictionary(dict) => dict,
                Object::Stream(stream) => &stream.dict,
                Object::Array(items) => {
                    inner.extend(items.iter());
                    continue;
                }
                _ => continue,
            };
            for (key, value) in dict {
                match key.as_slice() {
                    b"EmbeddedFiles" | b"EF" | b"AF" => held.embedded = true,
                    b"PieceInfo" => held.private = true,
                    _ => {}
                }
                inner.push(value);
            }
            if held.embedded && held.private {
                return held;
            }
        }
    }
    held
}

/// How many revisions a file ends: `%%EOF` markers that follow a `startxref`
/// and its offset, with white space between the three.
///
/// **Not how many times the bytes `%%EOF` occur**, which is what this counted
/// until 2026-10-04. `%%EOF` is also a PostScript comment, and an embedded
/// CMap is a PostScript resource that ends `%%EndResource %%EOF`. A document
/// that keeps such a CMap uncompressed has the bytes once per CMap, in stream
/// data, where they end nothing. Counted raw, a rewrite of such a document was
/// refused as holding several revisions, and a redaction of it could not be
/// verified: 2 of 59 real documents, each with three CMaps.
///
/// A revision's last lines are fixed by the format (ISO 32000-1 7.5.5):
/// `startxref`, the offset, `%%EOF`. That is what is counted. A stream that
/// holds a whole PDF uncompressed still counts, and is still reported: this
/// does not read stream lengths, so it cannot tell that case from a second
/// revision, and saying *not verified* there is the safe answer.
pub(crate) fn revision_ends(bytes: &[u8]) -> usize {
    const MARK: &[u8] = b"%%EOF";
    const START: &[u8] = b"startxref";
    let mut ends = 0;
    let mut from = 0;
    while let Some(found) = bytes[from..].windows(MARK.len()).position(|w| w == MARK) {
        let at = from + found;
        from = at + MARK.len();
        // Backwards from the marker: white space, digits, white space, keyword.
        let before = &bytes[..at];
        let digits_end = before.len() - trailing(before, u8::is_ascii_whitespace);
        let digits = trailing(&before[..digits_end], u8::is_ascii_digit);
        let space_end = digits_end - digits;
        let space = trailing(&before[..space_end], u8::is_ascii_whitespace);
        let keyword_end = space_end - space;
        // No test for `digits > 0`: with no digits the white space before
        // them was already taken as the white space before the marker, so
        // `space` is zero.
        if digits_end < before.len() && space > 0 && before[..keyword_end].ends_with(START) {
            ends += 1;
        }
    }
    ends
}

/// How many bytes at the end of `bytes` satisfy `is`.
fn trailing(bytes: &[u8], is: fn(&u8) -> bool) -> usize {
    bytes.iter().rev().take_while(|byte| is(byte)).count()
}

/// How many times `needle` occurs in `haystack`.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// Whether `needle` occurs in `haystack`.
/// The bytes `needle` can be stored as in a string a simple font shows: its
/// UTF-8, and its WinAnsi (Windows-1252) and MacRoman bytes when it has a
/// character outside ASCII that the encoding holds. WinAnsi is what nearly
/// every simple font with an accented letter is encoded in, and it writes `ü`
/// as the one byte `FC`, so a UTF-8 search of `Müller` never matched a page
/// that drew it; MacRoman writes it `9F`. A font's own `/ToUnicode` is read
/// by `fontread`; a `/Differences` encoding without one is the reason
/// `differences_in` gives (residual 42, `docs/THREAT-MODEL.md`).
fn spellings_of(needle: &str) -> Vec<Vec<u8>> {
    let mut spelled = vec![needle.as_bytes().to_vec()];
    if !needle.is_ascii() {
        for encode in [winansi, macroman] {
            if let Some(bytes) = needle.chars().map(encode).collect::<Option<Vec<u8>>>() {
                if !spelled.contains(&bytes) {
                    spelled.push(bytes);
                }
            }
        }
    }
    spelled
}

/// MacRomanEncoding's characters for the bytes 0x80 to 0xFF, in order, as
/// Python's `mac_roman` codec gives them.
const MACROMAN_HIGH: [&str; 16] = [
    "\u{00C4}\u{00C5}\u{00C7}\u{00C9}\u{00D1}\u{00D6}\u{00DC}\u{00E1}",
    "\u{00E0}\u{00E2}\u{00E4}\u{00E3}\u{00E5}\u{00E7}\u{00E9}\u{00E8}",
    "\u{00EA}\u{00EB}\u{00ED}\u{00EC}\u{00EE}\u{00EF}\u{00F1}\u{00F3}",
    "\u{00F2}\u{00F4}\u{00F6}\u{00F5}\u{00FA}\u{00F9}\u{00FB}\u{00FC}",
    "\u{2020}\u{00B0}\u{00A2}\u{00A3}\u{00A7}\u{2022}\u{00B6}\u{00DF}",
    "\u{00AE}\u{00A9}\u{2122}\u{00B4}\u{00A8}\u{2260}\u{00C6}\u{00D8}",
    "\u{221E}\u{00B1}\u{2264}\u{2265}\u{00A5}\u{00B5}\u{2202}\u{2211}",
    "\u{220F}\u{03C0}\u{222B}\u{00AA}\u{00BA}\u{03A9}\u{00E6}\u{00F8}",
    "\u{00BF}\u{00A1}\u{00AC}\u{221A}\u{0192}\u{2248}\u{2206}\u{00AB}",
    "\u{00BB}\u{2026}\u{00A0}\u{00C0}\u{00C3}\u{00D5}\u{0152}\u{0153}",
    "\u{2013}\u{2014}\u{201C}\u{201D}\u{2018}\u{2019}\u{00F7}\u{25CA}",
    "\u{00FF}\u{0178}\u{2044}\u{20AC}\u{2039}\u{203A}\u{FB01}\u{FB02}",
    "\u{2021}\u{00B7}\u{201A}\u{201E}\u{2030}\u{00C2}\u{00CA}\u{00C1}",
    "\u{00CB}\u{00C8}\u{00CD}\u{00CE}\u{00CF}\u{00CC}\u{00D3}\u{00D4}",
    "\u{F8FF}\u{00D2}\u{00DA}\u{00DB}\u{00D9}\u{0131}\u{02C6}\u{02DC}",
    "\u{00AF}\u{02D8}\u{02D9}\u{02DA}\u{00B8}\u{02DD}\u{02DB}\u{02C7}",
];

/// A character's byte in MacRomanEncoding, if it has one.
fn macroman(ch: char) -> Option<u8> {
    if ch.is_ascii() {
        return u8::try_from(u32::from(ch)).ok();
    }
    let at = MACROMAN_HIGH
        .iter()
        .flat_map(|row| row.chars())
        .position(|known| known == ch)?;
    u8::try_from(0x80 + at).ok()
}

/// A character's byte in WinAnsiEncoding, if it has one.
fn winansi(ch: char) -> Option<u8> {
    let point = u32::from(ch);
    if point < 0x80 || (0xA0..=0xFF).contains(&point) {
        return u8::try_from(point).ok();
    }
    const HIGH: &[(char, u8)] = &[
        ('\u{20AC}', 0x80),
        ('\u{201A}', 0x82),
        ('\u{0192}', 0x83),
        ('\u{201E}', 0x84),
        ('\u{2026}', 0x85),
        ('\u{2020}', 0x86),
        ('\u{2021}', 0x87),
        ('\u{02C6}', 0x88),
        ('\u{2030}', 0x89),
        ('\u{0160}', 0x8A),
        ('\u{2039}', 0x8B),
        ('\u{0152}', 0x8C),
        ('\u{017D}', 0x8E),
        ('\u{2018}', 0x91),
        ('\u{2019}', 0x92),
        ('\u{201C}', 0x93),
        ('\u{201D}', 0x94),
        ('\u{2022}', 0x95),
        ('\u{2013}', 0x96),
        ('\u{2014}', 0x97),
        ('\u{02DC}', 0x98),
        ('\u{2122}', 0x99),
        ('\u{0161}', 0x9A),
        ('\u{203A}', 0x9B),
        ('\u{0153}', 0x9C),
        ('\u{017E}', 0x9E),
        ('\u{0178}', 0x9F),
    ];
    HIGH.iter()
        .find(|(known, _)| *known == ch)
        .map(|(_, byte)| *byte)
}

/// Every string operand in `content`, decoded and joined with nothing between.
///
/// The byte search reads a stream as stored, and a string can be stored so its
/// characters never stand next to each other: as hex (`<4A6F686E>`), with
/// octal escapes (`(J\157hn)`), or split across a kerned `TJ` array
/// (`[(Jo) -20 (hn)]`) or across shows (residual 40). This lexes the stream as
/// PDF syntax --- literal strings with their escapes and nesting, hex strings,
/// `<<` and `>>` passed over, comments skipped --- and returns the strings'
/// bytes in order, so `John` in any of those spellings is `John` here.
///
/// Joining without a separator can put the end of one string beside the start
/// of the next and match a word nobody wrote. That errs towards *found*, which
/// withholds a verdict rather than certifying one. Applied to every decoded
/// stream, a font program or a picture included, for the same reason: lexing
/// bytes that are not content can only add matches. Linear in the stream, and
/// never longer than it.
fn strings_shown(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < content.len() {
        match content[at] {
            b'%' => {
                while at < content.len() && !matches!(content[at], b'\n' | b'\r') {
                    at += 1;
                }
            }
            b'(' => at = literal_string(content, at + 1, &mut out),
            b'<' if content.get(at + 1) == Some(&b'<') => at += 2,
            b'<' => at = hex_string(content, at + 1, &mut out),
            _ => at += 1,
        }
    }
    out
}

/// Decodes the literal string whose body starts at `at` into `out`, and
/// returns where it ends. Unterminated, it runs to the end of `content`.
pub(crate) fn literal_string(content: &[u8], mut at: usize, out: &mut Vec<u8>) -> usize {
    let mut depth = 1usize;
    while at < content.len() {
        let byte = content[at];
        at += 1;
        match byte {
            b'(' => {
                depth += 1;
                out.push(byte);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
                out.push(byte);
            }
            b'\\' => {
                let Some(&next) = content.get(at) else {
                    return at;
                };
                at += 1;
                match next {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'\r' => {
                        // A line continuation: `\` CR LF is one break.
                        if content.get(at) == Some(&b'\n') {
                            at += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut value = u32::from(next - b'0');
                        for _ in 0..2 {
                            match content.get(at) {
                                Some(&digit @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(digit - b'0');
                                    at += 1;
                                }
                                _ => break,
                            }
                        }
                        // A value past 255 keeps its low byte, as PDF readers do.
                        out.push((value & 0xFF) as u8);
                    }
                    // `\(`, `\)`, `\\`, and any other character stand for themselves.
                    other => out.push(other),
                }
            }
            _ => out.push(byte),
        }
    }
    at
}

/// Decodes the hex string whose body starts at `at` into `out`, and returns
/// where it ends: white space is skipped, an odd last digit is followed by 0,
/// and a byte that is neither ends the string where it stands.
pub(crate) fn hex_string(content: &[u8], mut at: usize, out: &mut Vec<u8>) -> usize {
    let mut high: Option<u8> = None;
    while at < content.len() {
        let byte = content[at];
        at += 1;
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            b'>' => break,
            _ if byte.is_ascii_whitespace() || byte == 0 => continue,
            _ => {
                at -= 1;
                break;
            }
        };
        match high.take() {
            Some(first) => out.push(first << 4 | nibble),
            None => high = Some(nibble),
        }
    }
    if let Some(first) = high {
        out.push(first << 4);
    }
    at
}

fn find(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Where `needle` last occurs in `haystack`.
fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// Every string in an object, flattened, so a needle in one is found.
fn flatten_strings(object: &Object) -> Vec<u8> {
    let mut out = Vec::new();
    collect_strings(object, &mut out);
    out
}

/// Whether a string's bytes are already the text it holds.
///
/// True for printable ASCII with tabs and line ends, which PDFDocEncoding maps
/// to themselves and which carries no byte-order mark. Everything else says
/// something other than its bytes: a UTF-16BE string, a byte above `7E`, and
/// the control bytes a text string's decoding drops.
fn reads_as_stored(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| matches!(byte, 0x20..=0x7E | b'\t' | b'\n' | b'\r'))
}

fn collect_strings(object: &Object, out: &mut Vec<u8>) {
    match object {
        Object::String(bytes, _) => {
            out.extend_from_slice(bytes);
            out.push(b'\n');
            // **And what the bytes say, when that is something else.** A needle
            // is the removed words in UTF-8, and a PDF text string is UTF-16BE
            // behind a byte-order mark or PDFDocEncoding without one. Compared
            // as stored, a note quoting the line in UTF-16BE never matched ---
            // not even for ASCII, with a zero byte in front of every letter ---
            // and a name with an umlaut in it matched in neither encoding.
            //
            // Beside the stored bytes rather than instead of them: a string
            // that is not a text string at all, a show operand's among them,
            // is still compared as it stands. And only for a string the
            // decoding would change, so the clean file that is nearly all
            // plain ASCII pays one pass over each string to learn there is
            // nothing to decode.
            if !reads_as_stored(bytes) {
                out.extend_from_slice(crate::annots::decode_text_string(bytes).as_bytes());
                out.push(b'\n');
            }
        }
        Object::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict {
                collect_strings(value, out);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in &stream.dict {
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {

    use super::structure;
    use super::{classify, Carrier, Located, Report, Verdict};

    /// A minimal file with the shape `structure` expects, to perturb.
    ///
    /// Built by hand rather than serialised, and it has to be: `lopdf` writes a
    /// well-formed file for every document it will accept --- an empty one comes
    /// out as 125 valid bytes --- so a fixture from the writer cannot carry any
    /// of the defects below. `docs/TRAPS.md` records building the malformed
    /// fixture by hand as the answer when the model forbids the input.
    fn well_formed() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n");
        let start = bytes.len();
        bytes.extend_from_slice(b"xref\n0 2\n");
        bytes.extend_from_slice(b"0000000000 65535 f \n0000000009 00000 n \n");
        bytes.extend_from_slice(b"trailer\n<</Size 2/Root 1 0 R>>\n");
        bytes.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
        bytes
    }

    /// The control, and without it every check below is satisfied by a function
    /// that complains about everything.
    #[test]
    fn a_well_formed_file_draws_no_complaint() {
        assert_eq!(structure(&well_formed()), Vec::<String>::new());
    }

    /// **The corpus control is in `save.rs`, deliberately, and it is the one
    /// that matters.** A hand-built fixture agrees with whatever the writer of
    /// the check had in mind, which is the writer-and-its-own-reader shape; what
    /// catches a rule that is wrong *about PDF* is real documents. The first
    /// draft of a `/Size` rule was killed exactly that way, condemning a healthy
    /// swept rewrite of `links.pdf` that `qpdf --check` passes.
    ///
    /// It sweeps rewritten **output** rather than source documents, because that
    /// is the only population this function is ever pointed at. Sweeping sources
    /// was tried first and reported `hostile-trailing.pdf` --- correctly, since
    /// that fixture exists to carry 84 bytes past its `%%EOF`. A control whose
    /// population includes deliberately malformed files has to exclude them, and
    /// an exclusion list is a thing that rots; changing the population removes
    /// the question. See `every_rewritten_fixture_is_structurally_sound`.
    #[test]
    fn a_file_that_does_not_begin_with_a_pdf_header_is_refused() {
        let mut bytes = well_formed();
        bytes.splice(0..0, *b"junk");
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("PDF header")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_file_with_no_eof_marker_is_refused() {
        let bytes = well_formed();
        let at = bytes.len() - 6;
        let wrong = structure(&bytes[..at]);
        assert!(
            wrong.iter().any(|why| why.contains("no %%EOF")),
            "{wrong:?}"
        );
    }

    /// `docs/PLAN.md` §6: a rewrite writes exactly one logical revision.
    #[test]
    fn a_second_revision_is_refused_and_the_count_is_reported() {
        let mut bytes = well_formed();
        let again = bytes.clone();
        bytes.extend_from_slice(&again);
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("2 %%EOF markers")),
            "{wrong:?}"
        );
    }

    /// The defect this rule had until 2026-10-04: an embedded CMap is a
    /// PostScript resource, and PostScript ends a file with the same comment.
    #[test]
    fn the_same_bytes_inside_a_stream_end_no_revision() {
        let cmap = b"2 0 obj\n<</Length 44>>\nstream\nend\nend\n%%EndResource\n%%EOF\n\nendstream\nendobj\n";
        let mut bytes = b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n".to_vec();
        for _ in 0..3 {
            bytes.extend_from_slice(cmap);
        }
        let start = bytes.len();
        bytes.extend_from_slice(
            b"xref\n0 1\n0000000000 65535 f \ntrailer\n<</Size 3/Root 1 0 R>>\n",
        );
        bytes.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
        assert_eq!(
            super::count(&bytes, b"%%EOF"),
            4,
            "the fixture holds the bytes four times"
        );
        assert_eq!(super::revision_ends(&bytes), 1);
        assert_eq!(structure(&bytes), Vec::<String>::new());
        let report = super::scan(&bytes, &[], None);
        assert_eq!(report.eofs, 1);
        assert!(
            !report.blind.iter().any(|why| why.contains("%%EOF markers")),
            "{:?}",
            report.blind
        );
    }

    #[test]
    fn a_revision_ends_with_startxref_an_offset_and_the_marker_and_with_nothing_less() {
        let ends = |tail: &str| super::revision_ends(tail.as_bytes());
        assert_eq!(ends("startxref\n123\n%%EOF\n"), 1);
        assert_eq!(ends("startxref\r\n123\r\n%%EOF"), 1);
        assert_eq!(ends("startxref 123 %%EOF"), 1);
        assert_eq!(ends("startxref\n1\n%%EOF\nstartxref\n2\n%%EOF\n"), 2);
        // The marker alone, the marker after a number alone, no offset, no
        // space before the marker, and a keyword that is only the end of one.
        assert_eq!(ends("%%EOF"), 0);
        assert_eq!(ends("123\n%%EOF"), 0);
        assert_eq!(ends("startxref\n%%EOF"), 0);
        assert_eq!(ends("startxref\n123%%EOF"), 0);
        assert_eq!(ends("startxref123\n%%EOF"), 0);
        assert_eq!(ends("xref\n123\n%%EOF"), 0);
        // A file whose only marker ends nothing says so.
        let wrong = structure(b"%PDF-1.7\n%%EOF\n");
        assert!(
            wrong
                .iter()
                .any(|why| why.contains("none that ends a revision")),
            "{wrong:?}"
        );
    }

    /// §6 again: no trailing data.
    #[test]
    fn bytes_after_the_last_eof_are_refused_and_whitespace_is_not() {
        let mut bytes = well_formed();
        bytes.extend_from_slice(b"\n\r\t   \n");
        assert_eq!(
            structure(&bytes),
            Vec::<String>::new(),
            "whitespace after %%EOF is how every writer ends a file"
        );
        bytes.extend_from_slice(b"leftover");
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("8 byte(s) follow")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_file_with_no_startxref_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let wrong = structure(text.replace("startxref", "startxxxx").as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no startxref")),
            "{wrong:?}"
        );
    }

    /// The offset is what makes this worth checking rather than the word.
    #[test]
    fn a_startxref_pointing_past_the_end_of_the_file_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!("{}startxref\n999999999\n%%EOF\n", &text[..at]);
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong
                .iter()
                .any(|why| why.contains("startxref points at byte 999999999")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_startxref_with_no_number_after_it_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!("{}startxref\n%%EOF\n", &text[..at]);
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no offset after it")),
            "{wrong:?}"
        );
    }

    /// An offset too large for a `usize` is unbelievable rather than large.
    #[test]
    fn a_startxref_offset_too_large_to_be_an_offset_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!(
            "{}startxref\n99999999999999999999999999\n%%EOF\n",
            &text[..at]
        );
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no offset after it")),
            "{wrong:?}"
        );
    }

    /// A stream with no `/Filter` is stored as it is, so it is scannable.
    #[test]
    fn a_stream_with_no_filter_is_scanned() {
        assert_eq!(classify(&[]), Carrier::Scanned);
    }

    /// The three `lopdf` actually implements, read out of its dispatch.
    #[test]
    fn the_filters_lopdf_implements_are_scanned() {
        for filter in [&b"FlateDecode"[..], b"LZWDecode", b"ASCII85Decode"] {
            assert_eq!(classify(&[filter]), Carrier::Scanned, "{filter:?}");
        }
    }

    /// What every scanner emits, and the reason this module exists.
    #[test]
    fn the_raster_filters_are_deferred_to_a_different_instrument() {
        for filter in [
            &b"DCTDecode"[..],
            b"JPXDecode",
            b"JBIG2Decode",
            b"CCITTFaxDecode",
        ] {
            assert!(
                matches!(classify(&[filter]), Carrier::Image { .. }),
                "{filter:?} should be an image carrier"
            );
        }
    }

    /// The last filter decides, because it is the one that produces the content.
    ///
    /// `/Filter [/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG: the
    /// chain is applied in order, so what comes out at the end is a picture.
    /// Classifying by the *first* would call this scannable and then hand the
    /// byte scan a decoded JPEG to look for words in.
    #[test]
    fn an_armoured_image_is_still_an_image() {
        assert!(matches!(
            classify(&[b"ASCII85Decode", b"DCTDecode"]),
            Carrier::Image { .. }
        ));
    }

    /// Understood by the format, absent from this build's decoder.
    ///
    /// Separated from an image because these carry arbitrary bytes --- text
    /// included --- so a decoder for them *would* answer the byte scan's
    /// question, where a decoder for a JPEG would not. Both withhold
    /// certification; only one of them could stop doing so.
    #[test]
    fn a_filter_we_cannot_decode_is_blind_rather_than_deferred() {
        for filter in [&b"ASCIIHexDecode"[..], b"RunLengthDecode", b"Crypt"] {
            assert!(
                matches!(classify(&[filter]), Carrier::Undecodable { .. }),
                "{filter:?}"
            );
        }
    }

    /// Deny by default, and it names the filter so a reader can go and look.
    #[test]
    fn a_filter_nothing_recognises_is_its_own_answer() {
        let Carrier::Unrecognised { filter } = classify(&[b"SomeVendorDecode"]) else {
            panic!("an unknown filter must not be classified as anything else");
        };
        assert_eq!(filter, "SomeVendorDecode");
    }

    #[test]
    fn a_clean_report_is_the_only_thing_that_verifies() {
        assert_eq!(Report::default().verdict(), Verdict::Verified);
    }

    /// **The keystone.** An image carrier withholds certification.
    ///
    /// This is the whole "split by remedy, not by verdict" claim, and it is the
    /// one that would ship a lie if it went the other way: a scanned document is
    /// nothing *but* image carriers, so a `deferred` list that certified would
    /// hand a reader the word "verified" for a file where nothing read the only
    /// thing in it.
    #[test]
    fn an_image_carrier_does_not_certify() {
        let report = Report {
            deferred: vec!["object 4 is a DCTDecode image".to_string()],
            ..Default::default()
        };
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a picture nobody read must not verify");
        };
        assert_eq!(why.len(), 1);
        assert!(why[0].contains("DCTDecode"), "{why:?}");
    }

    /// A needle still present is reported first, because it makes the rest moot.
    #[test]
    fn a_leak_is_reported_before_the_things_that_could_still_be_looked_at() {
        let mut report = Report {
            blind: vec!["object 9 is ASCIIHexDecode".to_string()],
            deferred: vec!["object 4 is a DCTDecode image".to_string()],
            ..Default::default()
        };
        report.found.insert("SECRET".to_string());
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a leak must not verify");
        };
        assert_eq!(why.len(), 3);
        assert!(
            why[0].contains("SECRET") && why[0].contains("still in the file"),
            "the leak comes first: {why:?}"
        );
    }

    /// A scan that decoded nothing must never be the reassuring answer.
    ///
    /// **The one claim `docs/PLAN.md` §6 will not let this build make.** `lopdf`
    /// answers `Ok` for a document it could not authenticate, having decoded
    /// none of it --- so every needle is compared against nothing, `found` stays
    /// empty, and without the guard `verdict()` says *Verified* about bytes this
    /// build never read. Measured on the fixture below: one blind entry, which
    /// is the guard's own, so the verdict really was `Verified` before it.
    ///
    /// A report lists at most [`MAX_OBJECT_REASONS`] objects, and counts the rest.
    ///
    /// **The bound exists because the report now crosses a pipe**, and a check
    /// that only asserted the cap would be satisfied by a scan that stopped
    /// looking. So the assertion is in two halves: the list is bounded, *and*
    /// the count in the summary line accounts for every object that was not
    /// listed. A scan that gave up at the cap would fail the second.
    ///
    /// Built rather than taken from `testdata/`: no fixture has a thousand
    /// objects nothing can decode, and one written to have them would be a
    /// fixture for this test alone.
    #[test]
    fn a_report_lists_a_bounded_number_of_objects_and_counts_the_rest() {
        use lopdf::{dictionary, Document, Object, Stream};

        // Two hundred over the cap, so the summary's number is a value only the
        // full walk could produce --- an off-by-one or a cap-and-stop both miss
        // it, and a rounder excess would not.
        const EXTRA: usize = 200;

        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        for _ in 0..super::MAX_OBJECT_REASONS + EXTRA {
            let mut stream = Stream::new(
                dictionary! { "Filter" => "AFilterNothingRecognises" },
                b"..".to_vec(),
            );
            stream.allows_compression = false;
            document.add_object(stream);
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");

        let report = super::scan(&bytes, &[], None);
        let listed = report
            .blind
            .iter()
            .filter(|reason| reason.starts_with("object "))
            .count();
        assert_eq!(
            listed,
            super::MAX_OBJECT_REASONS,
            "the per-object list is bounded: {} reasons in all",
            report.blind.len()
        );
        assert!(
            report.blind.iter().any(|reason| reason
                == &format!(
                    "{EXTRA} further objects could not be accounted for, and are not listed \
                     individually"
                )),
            "the summary must account for every object the list left out: {:?}",
            report
                .blind
                .iter()
                .filter(|reason| !reason.starts_with("object "))
                .collect::<Vec<_>>()
        );
        // And the verdict is unchanged by the shortening, which is the whole
        // point: a file with a thousand objects nothing can read is not
        // certifiable, however few of them are named.
        assert!(
            matches!(report.verdict(), Verdict::NotVerified(_)),
            "a shortened report is still a refusal"
        );
    }

    /// **Two subjects, because there are two rules.** The encrypted document
    /// parses to **one** object rather than none --- so an `objects.is_empty()`
    /// guard, which is the obvious one to write, would not fire here at all. It
    /// is `is_encrypted()` that answers, and the emptiness rule needs a file of
    /// its own or neither is falsifiable.
    #[test]
    fn a_scan_that_decoded_no_object_is_not_verified() {
        let needles = vec!["a-string-that-is-in-no-document".to_string()];

        let path = std::path::Path::new("../testdata/incr-encrypted-pw.pdf");
        if !path.exists() {
            println!(
                "[SKIP] a_scan_that_decoded_no_object_is_not_verified: generate testdata/ (BUILD.md)"
            );
            return;
        }
        let bytes = std::fs::read(path).expect("read the fixture");

        let locked = super::scan(&bytes, &needles, None);
        assert_eq!(
            locked.objects, 1,
            "the fixture must be one `lopdf` opens without decoding it --- if this ever \
             becomes 0 the emptiness rule below is what catches it, and this test is measuring \
             the wrong thing"
        );
        assert!(
            locked.found.is_empty(),
            "the needle is in no document --- the point is that looking for it succeeded \
             without anything having been looked at"
        );
        let Verdict::NotVerified(why) = locked.verdict() else {
            panic!("a scan that decoded no object must never verify");
        };
        assert!(
            why.iter()
                .any(|reason| reason.contains("no password opened it")),
            "the verdict has to say the file was never decoded: {why:?}"
        );

        // The control for that one. With the key the walk happens, so the guard
        // is about what was read rather than about encryption as such.
        let seeing = super::scan(&bytes, &needles, Some("swordfish"));
        assert!(
            seeing.objects > 1,
            "with the password the scan must reach the object graph, not just its wrapper"
        );
        assert!(
            !seeing
                .blind
                .iter()
                .any(|reason| reason.contains("no password opened it")),
            "a scan that decoded the file must not report it as undecoded: {:?}",
            seeing.blind
        );

        // **The second subject.** A file that parses cleanly and holds nothing:
        // not encrypted, so the rule above cannot reach it. Built by hand for
        // `well_formed`'s reason --- `lopdf` writes at least a catalog for every
        // document it will serialise, so no fixture from the writer has this
        // shape.
        let mut hollow = Vec::new();
        hollow.extend_from_slice(b"%PDF-1.7\n");
        let start = hollow.len();
        hollow.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
        hollow.extend_from_slice(b"trailer\n<</Size 1>>\n");
        hollow.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
        let empty = super::scan(&hollow, &needles, None);
        assert_eq!(empty.objects, 0, "the hand-built file holds no objects");
        let Verdict::NotVerified(why) = empty.verdict() else {
            panic!("a scan of a document with no objects in it must never verify");
        };
        assert!(
            why.iter()
                .any(|reason| reason.contains("no objects at all")),
            "the emptiness rule has its own reason, and this is the only subject that \
             reaches it: {why:?}"
        );
    }

    /// A document of one content stream per page, serialised as the writer does.
    ///
    /// **Built here rather than taken from `testdata/`**, for the reason
    /// `well_formed` gives and one more: these fixtures differ from each other
    /// by exactly one property --- which page prints a word, whether a form is
    /// shared, whether a link points somewhere --- and a tracked corpus cannot
    /// be varied one property at a time.
    fn document(pages: &[&str]) -> Vec<u8> {
        pieces(pages, &[], false)
    }

    /// The same, with optional extras: a form every page draws, and a link from
    /// page 1 to the last page.
    fn pieces(pages: &[&str], form: &[u8], link: bool) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        // One form object, drawn by every page, so a needle inside it is
        // carried by something no single page owns.
        let shared = (!form.is_empty()).then(|| {
            doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                },
                form.to_vec(),
            ))
        });
        let mut kids: Vec<Object> = Vec::new();
        let mut ids: Vec<lopdf::ObjectId> = Vec::new();
        for text in pages {
            let content = doc.add_object(Stream::new(
                dictionary! {},
                format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET").into_bytes(),
            ));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content,
            };
            if let Some(form) = shared {
                page.set(
                    "Resources",
                    dictionary! { "XObject" => dictionary! { "X1" => form } },
                );
            }
            let id = doc.add_object(page);
            ids.push(id);
            kids.push(Object::Reference(id));
        }
        // A `GoTo` from page one to the last page. Following `/A` or `/D` would
        // walk page one's attribution into that page's content; this is the
        // control that says it does not.
        if link {
            if let (Some(first), Some(last)) = (ids.first().copied(), ids.last().copied()) {
                let annot = doc.add_object(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Link",
                    "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                    "P" => first,
                    "A" => dictionary! {
                        "S" => "GoTo",
                        "D" => vec![Object::Reference(last), "Fit".into()],
                    },
                });
                if let Ok(Object::Dictionary(page)) = doc.get_object_mut(first) {
                    page.set("Annots", vec![Object::Reference(annot)]);
                }
            }
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// The reason a report gave for one needle, so a test asserts the sentence.
    fn reason(report: &Report, needle: &str) -> String {
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a report that found {needle} must never verify");
        };
        why.into_iter()
            .find(|one| one.starts_with(needle))
            .unwrap_or_else(|| panic!("no reason named {needle}"))
    }

    /// **The measurement the 2026-09-20 redaction narrowing rests on, and there
    /// is no inserted page anywhere in it.**
    ///
    /// `Refusal::RedactionBesideImportedPages` refused a redaction for the whole
    /// document while any page came from another file, and one of the four
    /// reasons it gave was this scan: *"the scan in particular reads the whole
    /// output, so imported text that happens to repeat a removed word reads as
    /// a leak"*. Every word of that is true and none of it is about imports.
    /// This scans a two-page document that never left the opened file --- the
    /// word is gone from page 1 and still printed on page 2 --- and the answer
    /// is the same *still in the file*.
    ///
    /// So the ambiguity is a property of a whole-file scan and is already
    /// shipped: an inserted page is exactly as opaque to it as page two, which
    /// makes it a reason to say so in the report
    /// ([`crate::redact::inserted_pages_note`]) and not a reason to refuse.
    ///
    /// ⚠ **The sentence it ends on changed on 2026-09-21, and the test's old
    /// last assertion said the opposite of what is now true.** It read *"the
    /// reason names the word, not the page --- which is exactly what it cannot
    /// say"*, and the walk this file gained makes it say it: the answer here is
    /// [`Located::Pages`] naming page two. What has **not** changed is the
    /// verdict, which is the half the redaction narrowing actually rests on ---
    /// a word still in the file is still *not verified*, wherever it is.
    ///
    /// The two needles are the control pair: one the writer put on the second
    /// page only, one on neither. A scan that "found" both, or neither, would
    /// pass an assertion about only the first.
    #[test]
    fn a_needle_on_another_page_reads_as_still_in_the_file() {
        const REPEATED: &str = "SECRET-4711";
        const ABSENT: &str = "NOT-IN-THIS-FILE-AT-ALL";

        let bytes = document(&["this page was redacted", REPEATED]);
        let needles = vec![REPEATED.to_string(), ABSENT.to_string()];
        let report = super::scan(&bytes, &needles, None);
        assert!(
            report.found.contains(REPEATED),
            "a word still printed on another page of the same document is reported as still \
             in the file: {:?}",
            report.found
        );
        assert!(
            !report.found.contains(ABSENT),
            "and the control says the scan is discriminating rather than agreeable: {:?}",
            report.found
        );
        assert!(
            matches!(report.verdict(), Verdict::NotVerified(_)),
            "a report that found a needle must never verify, however well it is placed"
        );
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![1],
                more: 0
            })),
            "the walk places it on the page that prints it, and on no other"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on page 2"),
            "and the reason a reader is shown names that page"
        );
        assert!(
            !report.located.contains_key(ABSENT),
            "nothing is placed for a word that was never found"
        );
    }

    /// The word is on the page the reader marked, which is the loud failure.
    ///
    /// **The control that makes the test above mean something.** Both fixtures
    /// are two pages and one needle; only *which* page prints it differs, and
    /// the two answers have to differ with it. A walk that attributed
    /// everything to page one would pass this and fail that, and one that
    /// followed `/Parent` up to `/Pages` and back down through `/Kids` --- the
    /// route [`NOT_CONTENT`] exists to cut --- would answer *both pages* here
    /// and fail neither assertion about *a* page.
    #[test]
    fn a_needle_on_the_page_it_was_removed_from_is_placed_there() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = document(&[REPEATED, "this page was never marked"]);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![0],
                more: 0
            })),
            "page one, and not both pages --- the page tree is not a route between them"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on page 1")
        );
    }

    /// An amount is verified as one passage, and one left in place is found.
    ///
    /// The two halves of what `cli::regions`' `on_same_line` rests on. A hit
    /// on `4,250.00` that reaches this scan as the three needles `4`, `,` and
    /// `250.00` cannot verify, and that is the scan being right: a page that
    /// positions text has a `4` among its operands and a comma in its prose,
    /// and both are still in the file with the amount gone. So the hit has to
    /// arrive as one needle --- and the control is that the one needle is not
    /// a way past the scan. A file whose marked page still prints the amount
    /// is *not verified*, by name and by page.
    #[test]
    fn an_amount_is_one_needle_and_one_still_on_its_page_is_not_verified() {
        const AMOUNT: &str = "4,250.00";
        let whole = vec![AMOUNT.to_string()];

        // What the page holds once the amount is out of it.
        let removed = document(&["comes to EUR . It is paid, within 14 days"]);
        assert_eq!(
            super::scan(&removed, &whole, None).verdict(),
            Verdict::Verified
        );
        let pieces = ["4", ",", "250.00"].map(String::from);
        let split = super::scan(&removed, &pieces, None);
        assert_eq!(
            split.found.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![",", "4"],
            "the pieces of a split hit are found in a file the amount has left"
        );

        // And the removal that did not take it.
        let kept = document(&["comes to EUR 4,250.00. It is paid, within 14 days"]);
        let report = super::scan(&kept, &whole, None);
        assert_eq!(
            reason(&report, AMOUNT),
            format!("{AMOUNT} is still in the file, on page 1")
        );
    }

    /// A word on two pages is placed on both, each by its own carrier.
    ///
    /// Distinct from [`Located::Shared`], which is one carrier that two pages
    /// draw. Here there are two carriers and each is owned, so the answer is
    /// specific --- and the sentence has to read as a list rather than as a
    /// singular page.
    #[test]
    fn a_needle_on_two_pages_is_placed_on_both() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = document(&[REPEATED, "clean", REPEATED]);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![0, 2],
                more: 0
            }))
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on pages 1 and 3")
        );
    }

    /// **One object that two pages draw has no page, and says so.**
    ///
    /// The middle of the three answers, and the one a two-valued version would
    /// get wrong: a shared form reached by page one and page two would be
    /// reported as *on page one* by anything that took the first answer it
    /// found, and a reader comparing that against the page they marked would
    /// act on a fabrication. The verdict is unchanged --- the word is still in
    /// the file --- and the sentence withholds the page instead of inventing
    /// one.
    #[test]
    fn a_needle_in_something_two_pages_draw_is_not_placed_on_either() {
        const REPEATED: &str = "SECRET-4711";

        let form = format!("BT /F1 12 Tf 10 10 Td ({REPEATED}) Tj ET").into_bytes();
        let bytes = pieces(&["one", "two"], &form, false);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(
            report.found.contains(REPEATED),
            "the scan reads inside the form, which is the precondition for the rest"
        );
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Shared(super::Placed {
                pages: vec![0, 1],
                more: 0
            })),
            "shared, and naming the pages that share it"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!(
                "{REPEATED} is still in the file, carried by something more than one page \
                 draws (pages 1 and 2), so which page it is on could not be established"
            )
        );
        assert!(
            !report.placed(),
            "and a caller must not compare this against the pages a reader marked"
        );
    }

    /// A link is not a route: following `/A` would put page three's words on page one.
    ///
    /// **The [`NOT_CONTENT`] list's own control, and the only one of its
    /// entries a plain two-page fixture does not already exercise.** A `GoTo`
    /// action's destination array names another page's object, so a walk that
    /// followed it would reach that page's content stream from page one --- and
    /// the needle would come back [`Located::Shared`] between pages one and
    /// three, which is a *softer* wrong answer than a wrong page and still a
    /// wrong answer. The assertion is therefore the exact `Pages([2])`, not
    /// merely that page three is in the set.
    #[test]
    fn a_link_to_another_page_does_not_reach_that_page_s_words() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = pieces(&["one", "two", REPEATED], &[], true);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![2],
                more: 0
            })),
            "the link's own page reaches the annotation, and stops there"
        );
    }

    /// Bytes past the last `%%EOF` belong to no object, so they belong to no page.
    ///
    /// **The third answer, and the one no walk will ever improve on.** The byte
    /// scan finds the word and the graph walk never accounts for it, which is
    /// the module's opening paragraph arriving as an attribution: there is no
    /// page to name because there is no object.
    #[test]
    fn a_needle_no_object_carries_is_not_placed() {
        const REPEATED: &str = "SECRET-4711";

        let mut bytes = document(&["clean", "also clean"]);
        bytes.extend_from_slice(REPEATED.as_bytes());
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED), "the byte scan sees it");
        assert_eq!(report.located.get(REPEATED), Some(&Located::Unplaced));
        assert_eq!(
            reason(&report, REPEATED),
            format!(
                "{REPEATED} is still in the file, in something no page of it reaches, so which \
                 page it is on could not be established"
            )
        );
    }

    /// The same answer for an object the page tree does not reach at all.
    ///
    /// A different subject from the one above and it needs its own fixture: the
    /// word here *is* in an object, and the object is the document's own
    /// `/Info` --- which `save::apply_redactions` scrubs precisely because a
    /// redacted word can sit in it. A walk keyed on pages cannot place it, and
    /// the honest answer is the same one trailing bytes get.
    #[test]
    fn a_needle_in_the_file_s_own_metadata_is_not_placed() {
        use lopdf::{dictionary, Document, Object};

        const REPEATED: &str = "SECRET-4711";

        // Rebuilt through `lopdf` rather than patched into the bytes, so the
        // `/Info` really is an object in the graph rather than loose text.
        let mut doc =
            Document::load_mem(&document(&["clean", "also clean"])).expect("the fixture loads");
        let info = doc.add_object(Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(REPEATED),
        }));
        doc.trailer.set("Info", info);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");

        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED));
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Unplaced),
            "an object no page reaches cannot be attributed to a page"
        );
    }

    /// More than one revision withholds every answer, not just the ambiguous one.
    ///
    /// **This module's opening paragraph as a guard.** An object a later
    /// revision overwrote sits at its old offset addressable by nothing: the
    /// graph walk cannot see it, and nor can the byte scan if it is compressed.
    /// So the live graph can honestly answer *page two* for a file that also
    /// holds a dead copy of the same word --- an answer about the graph offered
    /// as an answer about the file. The needle is still reported; only the
    /// place is withheld.
    #[test]
    fn a_file_with_more_than_one_revision_places_nothing() {
        const REPEATED: &str = "SECRET-4711";

        let one = document(&["clean", REPEATED]);
        let report = super::scan(&one, &[REPEATED.to_string()], None);
        assert!(
            report.located.contains_key(REPEATED),
            "the control: one revision, and the walk answers"
        );

        let mut two = one.clone();
        two.extend_from_slice(&one);
        let report = super::scan(&two, &[REPEATED.to_string()], None);
        assert!(report.eofs > 1, "the fixture really does hold two of them");
        assert!(report.found.contains(REPEATED), "the finding survives");
        assert!(
            report.located.is_empty(),
            "and nothing is placed: {:?}",
            report.located
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file"),
            "the sentence falls back to the one it gave before attribution existed"
        );
    }

    /// A word in more objects than [`MAX_CARRIERS`] is not *on* a page.
    ///
    /// The list stops being an answer long before it stops being affordable, so
    /// this is not a performance bound. Two hundred over the cap, for the reason
    /// [`MAX_OBJECT_REASONS`]'s test gives: a round excess would not tell an
    /// off-by-one from a cap-and-stop.
    #[test]
    fn a_needle_in_more_objects_than_the_cap_is_not_placed() {
        use lopdf::{Document, Object};

        const REPEATED: &str = "SECRET-4711";
        const EXTRA: usize = 200;

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let mut carried: Vec<Object> = Vec::new();
        for _ in 0..super::MAX_CARRIERS + EXTRA {
            carried.push(Object::Reference(
                doc.add_object(Object::string_literal(REPEATED)),
            ));
        }
        // Hung off the page so every one of them is reachable: the point is the
        // number of carriers, not whether they could have been placed.
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", carried);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");

        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED));
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Unplaced),
            "a thousand carriers is not a location"
        );
    }

    /// The page list is shortened and the count survives it.
    ///
    /// [`MAX_OBJECT_REASONS`]'s rule in the place the same pressure shows up.
    /// Asserted in two halves for that test's reason: the list is bounded *and*
    /// the remainder accounts for every page it left out, so a walk that gave
    /// up at the cap fails the second.
    #[test]
    fn a_placed_needle_names_a_bounded_number_of_pages_and_counts_the_rest() {
        const REPEATED: &str = "SECRET-4711";
        const EXTRA: usize = 3;

        let pages = vec![REPEATED; super::MAX_LOCATED_PAGES + EXTRA];
        let bytes = document(&pages);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        let Some(Located::Pages(placed)) = report.located.get(REPEATED) else {
            panic!("every page owns its own carrier, so this is placed: {report:?}");
        };
        assert_eq!(placed.pages.len(), super::MAX_LOCATED_PAGES);
        assert_eq!(placed.more, EXTRA);
        assert!(
            placed.sentence().ends_with(", and 3 more"),
            "{}",
            placed.sentence()
        );
    }

    /// A chain of references deeper than [`MAX_REACH_DEPTH`] stops the walk.
    ///
    /// **Called against [`reach`] rather than through [`scan`], and that is what
    /// makes the three bound tests affordable.** Each of them needs a document
    /// at the bound, and serialising a hundred thousand objects to exercise a
    /// limit on how many are *visited* would spend the whole cost on the one
    /// step that is not the subject. The `None`/`Some` pair is the control:
    /// without the second, a `reach` that returned `None` for everything would
    /// pass all three.
    #[test]
    fn a_reference_chain_past_the_depth_bound_stops_the_walk() {
        assert!(
            super::reach(&chain(super::MAX_REACH_DEPTH - 2)).is_some(),
            "a chain inside the bound is walked"
        );
        assert!(
            super::reach(&chain(super::MAX_REACH_DEPTH + 2)).is_none(),
            "and one past it withholds every answer, not just its own"
        );
    }

    /// A page reaching more objects than [`MAX_REACH_OBJECTS`] stops the walk.
    #[test]
    fn a_page_reaching_past_the_object_bound_stops_the_walk() {
        assert!(
            super::reach(&fan(super::MAX_REACH_OBJECTS - 10)).is_some(),
            "a page inside the bound is walked"
        );
        assert!(super::reach(&fan(super::MAX_REACH_OBJECTS + 10)).is_none());
    }

    /// Work past [`MAX_REACH_STEPS`] stops the walk, however it was spent.
    ///
    /// **Direct objects, which is the half the per-page object bound cannot
    /// reach.** A hostile array of two million integers costs two million steps
    /// and one object, so a walk bounded only by objects visited would grind
    /// through it. The bound is on the work rather than on the graph.
    ///
    /// See [`heap`] for why its page has no content stream: with one, this test
    /// passed for the wrong reason and the mutation that deletes the guard it
    /// is named for survived.
    #[test]
    fn work_past_the_step_bound_stops_the_walk() {
        assert!(
            super::reach(&heap(super::MAX_REACH_STEPS / 2)).is_some(),
            "half the budget is spent and answered"
        );
        assert!(super::reach(&heap(super::MAX_REACH_STEPS + 10)).is_none());
    }

    /// One page whose `/Resources` is a chain of `deep` references.
    fn chain(deep: usize) -> lopdf::Document {
        use lopdf::{dictionary, Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let mut last = doc.add_object(Object::Null);
        for _ in 0..deep {
            last = doc.add_object(dictionary! { "Link" => last });
        }
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", last);
        }
        doc
    }

    /// One page whose `/Resources` references `wide` separate objects.
    fn fan(wide: usize) -> lopdf::Document {
        use lopdf::{Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let spread: Vec<Object> = (0..wide)
            .map(|_| Object::Reference(doc.add_object(Object::Null)))
            .collect();
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", spread);
        }
        doc
    }

    /// One page whose `/Resources` is a direct array of `many` integers, and
    /// which references **nothing** --- not even its own content stream.
    ///
    /// ⚠ **The `/Contents` had to go, and finding out why is the interesting
    /// part.** There are two step checks: one in [`push_refs`], which stops a
    /// single object from burning the whole budget, and one in [`reach`]'s own
    /// loop. They share a counter, so with a content stream still on the page
    /// the stack was not empty when `push_refs` returned --- `reach` popped it,
    /// incremented the shared counter past the bound and answered `None`
    /// anyway. The mutation that deletes the inner check therefore **survived**
    /// on the first run of it: the walk still truncated, for the other reason.
    ///
    /// A guard that cannot be shown to fire is not a guard, and the failure
    /// here is not hypothetical --- it is exactly what an object holding a
    /// billion direct values would exploit, spending a billion steps before the
    /// outer check gets a turn. Stripping every reference from the page is what
    /// leaves the inner check as the only thing standing between the fixture
    /// and an answer.
    fn heap(many: usize) -> lopdf::Document {
        use lopdf::{Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.remove(b"Contents");
            page.set("Resources", vec![Object::Integer(0); many]);
        }
        doc
    }

    /// A two-page document whose second page carries a note with these bytes
    /// as its `/Contents`, exactly as given.
    ///
    /// The note is on a page nothing was removed from, which is where a copy
    /// of a redacted line survives a redaction: no removal rule reaches it, so
    /// the scan is the only thing that can say it is there.
    fn noted(contents: Vec<u8>) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, StringFormat};

        let mut doc = Document::load_mem(&document(&["this page was redacted", "another page"]))
            .expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[1];
        let note = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::String(contents, StringFormat::Hexadecimal),
        });
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Annots", vec![Object::Reference(note)]);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// `text` as a PDF text string in UTF-16BE, byte-order mark first.
    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        bytes
    }

    /// A needle in a text string stored as UTF-16BE is found.
    ///
    /// **The needle is UTF-8 and the string is not**, so a comparison of bytes
    /// against bytes misses it even when every character is ASCII: the string
    /// holds a zero byte in front of each one. LibreOffice writes outline
    /// titles and the document information this way and many producers write a
    /// note's body this way, so a note quoting a removed line read as clean
    /// and the report said *verified*.
    ///
    /// Three subjects and the answer has to differ across them. The note that
    /// quotes the line is found. The same note about something else is not,
    /// which is the control against a scan that finds everything. And the line
    /// in plain ASCII is found as it always was, so decoding is an addition to
    /// the byte comparison and has not replaced it.
    #[test]
    fn a_needle_in_a_utf16_text_string_is_found() {
        const TAKEN: &str = "SECRET-4711";
        let needles = vec![TAKEN.to_string()];

        let quoting = noted(utf16("the line read SECRET-4711 before"));
        assert!(
            !super::find(&quoting, TAKEN.as_bytes()),
            "the fixture must not hold the needle as plain bytes, or the byte scan answers and \
             this tests nothing"
        );
        let report = super::scan(&quoting, &needles, None);
        assert!(
            report.found.contains(TAKEN),
            "a note quoting the removed line in UTF-16BE is a copy of it: {:?}",
            report.found
        );
        assert!(matches!(report.verdict(), Verdict::NotVerified(_)));
        assert_eq!(
            report.located.get(TAKEN),
            Some(&Located::Pages(super::Placed {
                pages: vec![1],
                more: 0
            })),
            "and it is placed on the page the note is on"
        );

        let unrelated = noted(utf16("the line read something else before"));
        let report = super::scan(&unrelated, &needles, None);
        assert!(
            report.found.is_empty(),
            "a note about something else holds no needle: {:?}",
            report.found
        );
        assert_eq!(report.verdict(), Verdict::Verified);

        let plain = noted(b"the line read SECRET-4711 before".to_vec());
        assert!(super::scan(&plain, &needles, None).found.contains(TAKEN));
    }

    /// A needle outside ASCII in a text string stored in PDFDocEncoding is
    /// found.
    ///
    /// The other half of the same comparison: a string with no byte-order mark
    /// is PDFDocEncoding, where `ö` is the one byte `F6` and an en dash is
    /// `85`, and the needle spells both in UTF-8. The en dash is in the fixture
    /// because it is where PDFDocEncoding and Latin-1 part, so a decoder that
    /// took the string for Latin-1 would find the name and miss the line.
    #[test]
    fn a_needle_outside_ascii_in_a_pdfdocencoded_string_is_found() {
        const TAKEN: &str = "Gr\u{f6}\u{df}e\u{2013}4711";
        const STORED: &[u8] = b"Gr\xF6\xDFe\x854711";
        let needles = vec![TAKEN.to_string()];

        let mut body = b"the line read ".to_vec();
        body.extend_from_slice(STORED);
        let quoting = noted(body);
        assert!(
            !super::find(&quoting, TAKEN.as_bytes()),
            "the fixture must not hold the needle's UTF-8 bytes"
        );
        let report = super::scan(&quoting, &needles, None);
        assert!(
            report.found.contains(TAKEN),
            "a note quoting the removed line in PDFDocEncoding is a copy of it: {:?}",
            report.found
        );

        let unrelated = noted(b"the line read Gr\xF6\xDFe and no number".to_vec());
        let report = super::scan(&unrelated, &needles, None);
        assert!(
            report.found.is_empty(),
            "a note without the line holds no needle: {:?}",
            report.found
        );
    }

    /// A string that is plain ASCII is compared once, as it is stored.
    ///
    /// The cost half of the two tests above: nearly every string in nearly
    /// every file is this one, and decoding it would give back the same bytes.
    #[test]
    fn only_a_string_that_decodes_to_something_else_is_decoded() {
        assert!(super::reads_as_stored(b"plain words, 4711\tand a tab\r\n"));
        assert!(super::reads_as_stored(b""));
        assert!(!super::reads_as_stored(&utf16("plain")));
        assert!(!super::reads_as_stored(b"Gr\xF6\xDFe"));
        // Below the table, where a byte is dropped rather than kept.
        assert!(!super::reads_as_stored(b"SEC\x00RET"));
        // And the one printable-range byte PDFDocEncoding leaves undefined.
        assert!(!super::reads_as_stored(b"a\x7Fb"));

        let object = lopdf::Object::string_literal("plain words");
        assert_eq!(super::flatten_strings(&object), b"plain words\n");
        let object = lopdf::Object::String(utf16("ab"), lopdf::StringFormat::Literal);
        assert_eq!(
            super::flatten_strings(&object),
            b"\xFE\xFF\x00a\x00b\nab\n",
            "the stored bytes, then what they say"
        );
    }

    /// One page drawing `page_text`, and a Form XObject holding `hidden`
    /// deflated, with its `/Filter` written directly or as a reference.
    fn filtered(page_text: &str, hidden: &str, indirect_filter: bool) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut form = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            },
            // Padded, because `compress` keeps a stream that deflating would
            // not shrink as it is, and the fixture needs it deflated.
            format!(
                "BT /F1 12 Tf 72 600 Td ({hidden}) Tj ET\n{}",
                "q Q\n".repeat(200)
            )
            .into_bytes(),
        );
        form.compress().expect("deflate the form");
        assert!(
            !form
                .content
                .windows(hidden.len())
                .any(|w| w == hidden.as_bytes()),
            "the fixture must hide the words from a reading of the raw bytes"
        );
        if indirect_filter {
            let name = doc.add_object(Object::Name(b"FlateDecode".to_vec()));
            form.dict.set("Filter", Object::Reference(name));
        }
        let form = doc.add_object(form);
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 72 700 Td ({page_text}) Tj ET /X1 Do").into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            "Resources" => dictionary! { "XObject" => dictionary! { "X1" => form } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    #[test]
    fn a_stream_whose_filter_is_an_indirect_reference_withholds_the_verdict() {
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(&filtered("kept", "4711-0815", true), &needles, None);
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a stream nothing decoded must not certify the file: {report:?}");
        };
        assert!(
            why.iter()
                .any(|one| one.contains("names its filter in a form this build does not read")),
            "and the reason names the filter: {why:?}"
        );
    }

    #[test]
    fn a_stream_whose_filter_is_written_directly_is_decoded_and_scanned() {
        // The control: the same words in the same deflated form, found once the
        // filter is one `lopdf` reads. Without it the test above passes for a
        // scan that withholds every verdict.
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(&filtered("kept", "4711-0815", false), &needles, None);
        assert!(report.found.contains("4711-0815"), "{report:?}");
        let clean = super::scan(&filtered("kept", "nothing here", false), &needles, None);
        assert_eq!(clean.verdict(), Verdict::Verified, "{clean:?}");
    }

    /// One page drawing `text` through a font of `subtype`, as glyph numbers
    /// when the font is composite.
    fn drawn_through(subtype: &str) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = if subtype == "Type0" {
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "Synthetic",
                "Encoding" => "Identity-H",
            }
        } else {
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" }
        };
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 72 700 Td <00170014001400120010> Tj ET".to_vec(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            // Written directly into the resources, which is the harder of the
            // two places for the scan to look.
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    #[test]
    fn a_file_with_a_composite_font_is_not_certified_by_the_byte_scan() {
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(&drawn_through("Type0"), &needles, None);
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("glyph numbers the scan cannot read must not certify the file");
        };
        assert!(
            why.iter().any(|one| one.contains("composite (Type0) font")),
            "{why:?}"
        );
        // The control: the same page through a simple font is certified, so the
        // reason above is the font's and not the page's.
        let simple = super::scan(&drawn_through("Type1"), &needles, None);
        assert_eq!(simple.verdict(), Verdict::Verified, "{simple:?}");
    }

    /// [`drawn_through`]'s simple-font page, with `key` set on the catalog.
    fn with_catalog_key(key: &str, value: lopdf::Object) -> Vec<u8> {
        let mut doc = lopdf::Document::load_mem(&drawn_through("Type1")).expect("load");
        doc.catalog_mut().expect("catalog").set(key, value);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        bytes
    }

    #[test]
    fn an_embedded_file_withholds_the_verdict() {
        use lopdf::{dictionary, Object};
        let needles = vec!["4711-0815".to_string()];
        let names = dictionary! {
            "EmbeddedFiles" => dictionary! { "Names" => vec![
                Object::string_literal("a.docx"),
                dictionary! { "Type" => "Filespec", "F" => Object::string_literal("a.docx") }.into(),
            ] },
        };
        let report = super::scan(&with_catalog_key("Names", names.into()), &needles, None);
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("an embedded file nothing read must not certify the file");
        };
        assert!(
            why.iter().any(|one| one.contains("embedded files")),
            "{why:?}"
        );
        let listed = super::scan(
            &with_catalog_key(
                "AF",
                vec![dictionary! { "Type" => "Filespec" }.into()].into(),
            ),
            &needles,
            None,
        );
        assert!(
            matches!(listed.verdict(), Verdict::NotVerified(_)),
            "an /AF list too"
        );
    }

    #[test]
    fn an_applications_private_data_withholds_the_verdict() {
        use lopdf::dictionary;
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(
            &with_catalog_key(
                "PieceInfo",
                dictionary! { "Vendor" => dictionary! {} }.into(),
            ),
            &needles,
            None,
        );
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("private data nothing read must not certify the file");
        };
        assert!(why.iter().any(|one| one.contains("/PieceInfo")), "{why:?}");
        // The control: the same file with an unrelated key is certified, so the
        // reason is the key's.
        let plain = super::scan(
            &with_catalog_key("Lang", lopdf::Object::string_literal("en")),
            &needles,
            None,
        );
        assert_eq!(plain.verdict(), Verdict::Verified, "{plain:?}");
    }

    #[test]
    fn a_needle_drawn_through_identity_h_is_read_through_its_to_unicode() {
        // The generated CID fixture, where PDFium extracts the word and no byte
        // of the file spells it: the document a byte scan once called clean,
        // and until 2026-11-20 one the scan refused outright for its font.
        let path = std::path::Path::new("../testdata/text-cid.pdf");
        if !path.exists() {
            crate::fixtures::missing(
                "[SKIP] a_needle_drawn_through_identity_h_is_read_through_its_to_unicode: \
                 generate testdata/ (BUILD.md)",
            );
            return;
        }
        let bytes = std::fs::read(path).expect("read the fixture");
        assert!(
            !bytes.windows(9).any(|w| w == b"4711-0815"),
            "the precondition: no byte of the file spells the word"
        );
        let report = super::scan(&bytes, &["4711-0815".to_string()], None);
        assert!(
            report.found.contains("4711-0815"),
            "decoded through the font that shows it, the word is found: {report:?}"
        );
        // The control, and the point of reading the font: a word the page does
        // not show is certified rather than refused for the font.
        let absent = super::scan(&bytes, &["absent word".to_string()], None);
        assert_eq!(absent.verdict(), Verdict::Verified, "{absent:?}");
    }

    /// A page showing `content` through `/F1`, a composite Identity-H font
    /// with `cmap` as its `/ToUnicode` when given, and an `orphan` content
    /// stream no page draws when given.
    fn composite_page(cmap: Option<&[u8]>, content: &[u8], orphan: Option<&[u8]>) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Synthetic",
            "Encoding" => "Identity-H",
        };
        if let Some(cmap) = cmap {
            let map = doc.add_object(Stream::new(dictionary! {}, cmap.to_vec()));
            font.set("ToUnicode", map);
        }
        let font = doc.add_object(font);
        let body = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        if let Some(orphan) = orphan {
            doc.add_object(Stream::new(dictionary! {}, orphan.to_vec()));
        }
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => body,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// `J`, `o`, `h`, `n` at codes 1 to 4.
    const JOHN_CMAP: &[u8] = b"1 begincodespacerange <0000> <FFFF> endcodespacerange \
        4 beginbfchar <0001> <004A> <0002> <006F> <0003> <0068> <0004> <006E> endbfchar";

    fn composite_withheld(report: &Report) -> bool {
        report
            .blind
            .iter()
            .any(|why| why.contains("composite (Type0) font"))
    }

    #[test]
    fn a_composite_font_with_a_map_is_read_and_one_it_cannot_read_is_refused() {
        let john = vec!["John".to_string()];
        let shown = b"BT /F1 12 Tf 72 700 Td <0001000200030004> Tj ET";
        // Read through its map: the word is found, and an absent word is clean.
        let read = super::scan(&composite_page(Some(JOHN_CMAP), shown, None), &john, None);
        assert!(read.found.contains("John"), "{read:?}");
        assert!(!composite_withheld(&read), "{read:?}");
        let other = super::scan(
            &composite_page(Some(JOHN_CMAP), shown, None),
            &["Smith".to_string()],
            None,
        );
        assert_eq!(other.verdict(), Verdict::Verified, "{other:?}");
        // A code the map does not hold.
        let gap = super::scan(
            &composite_page(Some(JOHN_CMAP), b"BT /F1 12 Tf <00010099> Tj ET", None),
            &john,
            None,
        );
        assert!(composite_withheld(&gap), "{gap:?}");
        // No map at all.
        let none = super::scan(&composite_page(None, shown, None), &john, None);
        assert!(composite_withheld(&none), "{none:?}");
        // A map that covers every code shown and sends one glyph to a
        // private-use code point: what it decodes to is not what is visible.
        let private = super::scan(
            &composite_page(
                Some(
                    b"1 begincodespacerange <0000> <FFFF> endcodespacerange \
                      4 beginbfchar <0001> <004A> <0002> <E001> <0003> <0068> \
                      <0004> <006E> endbfchar",
                ),
                shown,
                None,
            ),
            &["Smith".to_string()],
            None,
        );
        assert!(composite_withheld(&private), "{private:?}");
        // A font the resources do not hold.
        let unknown = super::scan(
            &composite_page(Some(JOHN_CMAP), b"BT /F9 12 Tf <0001> Tj ET", None),
            &john,
            None,
        );
        assert!(composite_withheld(&unknown), "{unknown:?}");
        // A stream that shows text and that no page draws.
        let orphan = super::scan(
            &composite_page(Some(JOHN_CMAP), shown, Some(b"BT /F1 12 Tf <0001> Tj ET")),
            &["Smith".to_string()],
            None,
        );
        assert!(composite_withheld(&orphan), "{orphan:?}");
    }

    #[test]
    fn a_form_drawn_without_a_font_of_its_own_uses_the_one_current_where_it_is_drawn() {
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::load_mem(&composite_page(
            Some(JOHN_CMAP),
            b"BT /F1 12 Tf ET /X1 Do",
            None,
        ))
        .expect("load");
        let form = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            },
            b"BT 72 700 Td <0001000200030004> Tj ET".to_vec(),
        ));
        for object in doc.objects.values_mut() {
            if let Object::Dictionary(page) = object {
                if let Ok(Object::Dictionary(resources)) = page.get_mut(b"Resources") {
                    resources.set("XObject", dictionary! { "X1" => form });
                }
            }
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        let report = super::scan(&bytes, &["John".to_string()], None);
        assert!(report.found.contains("John"), "{report:?}");
    }

    /// One page whose content is `content`, drawing `xobject` as `/X1` when
    /// there is one; `trailer` adds keys to the trailer as the writer leaves it.
    fn one_page(
        content: &[u8],
        xobject: Option<lopdf::Stream>,
        build: impl FnOnce(&mut lopdf::Document, &mut lopdf::Stream),
    ) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut resources = dictionary! {};
        if let Some(mut xobject) = xobject {
            build(&mut doc, &mut xobject);
            let id = doc.add_object(xobject);
            resources.set("XObject", dictionary! { "X1" => id });
        }
        let content = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            "Resources" => resources,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// Bytes that are both a valid content stream and 32 by 32 grey samples,
    /// so one stream can be a form in a control and a picture in the test.
    fn samples() -> Vec<u8> {
        b"q Q\n".repeat(256)
    }

    /// A stream of [`samples`] with `subtype`, deflated.
    fn deflated(subtype: &str) -> lopdf::Stream {
        use lopdf::{dictionary, Stream};
        let mut stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => subtype,
                "Width" => 32,
                "Height" => 32,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "BBox" => vec![0.into(), 0.into(), 32.into(), 32.into()],
            },
            samples(),
        );
        stream.compress().expect("deflate");
        assert!(stream.dict.has(b"Filter"), "the fixture must be deflated");
        stream
    }

    fn deferred_reasons(report: &Report) -> Vec<&String> {
        report
            .deferred
            .iter()
            .filter(|why| why.contains("needs OCR"))
            .collect()
    }

    #[test]
    fn a_deflated_image_is_deferred_like_a_jpeg() {
        let needles = vec!["4711-0815".to_string()];
        let drawn = b"q 32 0 0 32 0 0 cm /X1 Do Q";
        let deflated_picture = super::scan(
            &one_page(drawn, Some(deflated("Image")), |_, _| {}),
            &needles,
            None,
        );
        let Verdict::NotVerified(why) = deflated_picture.verdict() else {
            panic!("a picture nobody read must not certify the file: {deflated_picture:?}");
        };
        assert!(
            why.iter()
                .any(|one| one.contains("FlateDecode image") && one.contains("needs OCR")),
            "{why:?}"
        );
        // The JPEG twin: the same picture under `/DCTDecode`, which is the case
        // that always had this reason. One reason each, of one kind, so the
        // deflated picture is treated as the JPEG is and no more harshly.
        let mut jpeg = deflated("Image");
        jpeg.dict.set("Filter", "DCTDecode");
        let jpeg = super::scan(&one_page(drawn, Some(jpeg), |_, _| {}), &needles, None);
        assert_eq!(deferred_reasons(&jpeg).len(), 1, "{jpeg:?}");
        assert_eq!(deferred_reasons(&deflated_picture).len(), 1);
        assert_eq!(jpeg.blind, deflated_picture.blind);
        // The control: the same bytes as a form are content, decoded and read.
        let form = super::scan(
            &one_page(drawn, Some(deflated("Form")), |_, _| {}),
            &needles,
            None,
        );
        assert_eq!(form.verdict(), Verdict::Verified, "{form:?}");
    }

    #[test]
    fn an_image_named_by_a_reference_or_stored_raw_is_still_an_image() {
        use lopdf::Object;
        let needles = vec!["4711-0815".to_string()];
        let drawn = b"q 32 0 0 32 0 0 cm /X1 Do Q";
        let by_reference = super::scan(
            &one_page(drawn, Some(deflated("Image")), |doc, image| {
                let name = doc.add_object(Object::Name(b"Image".to_vec()));
                image.dict.set("Subtype", Object::Reference(name));
            }),
            &needles,
            None,
        );
        assert_eq!(deferred_reasons(&by_reference).len(), 1, "{by_reference:?}");
        let mut raw = deflated("Image");
        raw.dict.remove(b"Filter");
        raw.set_content(samples());
        let raw = super::scan(&one_page(drawn, Some(raw), |_, _| {}), &needles, None);
        assert!(
            raw.deferred
                .iter()
                .any(|why| why.contains("stored without compression")),
            "{raw:?}"
        );
    }

    #[test]
    fn an_inline_image_in_a_content_stream_is_deferred() {
        let needles = vec!["4711-0815".to_string()];
        let inline = super::scan(
            &one_page(
                b"BT /F1 12 Tf 72 700 Td (kept) Tj ET\nq 10 0 0 10 0 0 cm\nBI /W 2 /H 1 /CS /G /BPC 8 ID \x00\x00 EI\nQ",
                None,
                |_, _| {},
            ),
            &needles,
            None,
        );
        assert!(
            inline
                .deferred
                .iter()
                .any(|why| why.contains("inline image")),
            "{inline:?}"
        );
        // The control: the same two words in a string are text, not a picture.
        let words = super::scan(
            &one_page(
                b"BT /F1 12 Tf 72 700 Td (BI and ID EI) Tj ET",
                None,
                |_, _| {},
            ),
            &needles,
            None,
        );
        assert_eq!(words.verdict(), Verdict::Verified, "{words:?}");
    }

    /// How a predicted stream's parameters are written.
    #[derive(Clone, Copy)]
    enum Params {
        Direct,
        Reference,
        InArrayByReference,
        ColumnsByReference,
        Predictor16,
    }

    /// A deflated form holding `hidden`, stored with PNG predictor 12 (Up for
    /// every row is enough to hide nothing; Sub is used, so every byte after a
    /// row's first is a difference and the words are in no reading but the
    /// decoded one).
    fn predicted(hidden: &str, params: Params) -> Vec<u8> {
        use lopdf::{dictionary, Object, Stream};
        use std::io::Write;

        const COLUMNS: usize = 16;
        let mut plain = format!("BT /F1 12 Tf 72 600 Td ({hidden}) Tj ET\n").into_bytes();
        plain.resize(plain.len().div_ceil(COLUMNS) * COLUMNS, b' ');
        let mut rows = Vec::new();
        for row in plain.chunks(COLUMNS) {
            rows.push(1u8);
            let mut left = 0u8;
            for &byte in row {
                rows.push(byte.wrapping_sub(left));
                left = byte;
            }
        }
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&rows).expect("deflate");
        let deflated = encoder.finish().expect("deflate");
        let form = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Filter" => "FlateDecode",
            },
            deflated,
        );
        one_page(b"/X1 Do", Some(form), |doc, form| {
            let mut decode = dictionary! { "Predictor" => 12, "Columns" => COLUMNS as i64 };
            let value = match params {
                Params::Direct => Object::Dictionary(decode),
                Params::Reference => Object::Reference(doc.add_object(decode)),
                Params::InArrayByReference => {
                    Object::Array(vec![Object::Reference(doc.add_object(decode))])
                }
                Params::ColumnsByReference => {
                    let columns = doc.add_object(Object::Integer(COLUMNS as i64));
                    decode.set("Columns", Object::Reference(columns));
                    Object::Dictionary(decode)
                }
                Params::Predictor16 => {
                    decode.set("Predictor", 16);
                    Object::Dictionary(decode)
                }
            };
            form.dict.set("DecodeParms", value);
        })
    }

    #[test]
    fn a_stream_whose_decode_parms_is_a_reference_withholds_the_verdict() {
        let needles = vec!["4711-0815".to_string()];
        for params in [
            Params::Reference,
            Params::InArrayByReference,
            Params::ColumnsByReference,
            Params::Predictor16,
        ] {
            let report = super::scan(&predicted("4711-0815", params), &needles, None);
            let Verdict::NotVerified(why) = report.verdict() else {
                panic!("parameters this build does not read must not certify: {report:?}");
            };
            assert!(
                why.iter().any(|one| one.contains("(/DecodeParms)")),
                "{why:?}"
            );
        }
    }

    #[test]
    fn a_stream_whose_decode_parms_is_direct_is_decoded_and_scanned() {
        // The control, and it proves the fixture: with the parameters read the
        // predictor is undone and the words are found, and a clean one passes.
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(&predicted("4711-0815", Params::Direct), &needles, None);
        assert!(report.found.contains("4711-0815"), "{report:?}");
        let clean = super::scan(&predicted("nothing here", Params::Direct), &needles, None);
        assert_eq!(clean.verdict(), Verdict::Verified, "{clean:?}");
    }

    #[test]
    fn a_composite_font_named_by_reference_is_still_composite() {
        use lopdf::Object;
        let mut doc = lopdf::Document::load_mem(&drawn_through("Type1")).expect("load");
        let type0 = doc.add_object(Object::Name(b"Type0".to_vec()));
        for object in doc.objects.values_mut() {
            let Ok(page) = object.as_dict_mut() else {
                continue;
            };
            let Ok(font) = page
                .get_mut(b"Resources")
                .and_then(Object::as_dict_mut)
                .and_then(|resources| resources.get_mut(b"Font"))
                .and_then(Object::as_dict_mut)
                .and_then(|fonts| fonts.get_mut(b"F1"))
                .and_then(Object::as_dict_mut)
            else {
                continue;
            };
            font.set("Subtype", Object::Reference(type0));
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        let report = super::scan(&bytes, &["4711-0815".to_string()], None);
        assert!(
            report
                .blind
                .iter()
                .any(|one| one.contains("composite (Type0) font")),
            "{report:?}"
        );
    }

    /// [`filtered`]'s page with its form's deflated bytes passed through `cut`.
    fn recompressed(hidden: &str, cut: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
        let mut doc = lopdf::Document::load_mem(&filtered("kept", hidden, false)).expect("load");
        for object in doc.objects.values_mut() {
            if let lopdf::Object::Stream(stream) = object {
                if stream.dict.has(b"Filter") {
                    cut(&mut stream.content);
                    let len = stream.content.len();
                    stream.dict.set("Length", len as i64);
                }
            }
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        bytes
    }

    #[test]
    fn a_flate_stream_cut_short_withholds_the_verdict() {
        let needles = vec!["4711-0815".to_string()];
        // The words lead the form and the padding follows, so the cut takes
        // padding and the decode that `lopdf` returns still holds the words;
        // what makes the file uncertifiable is the part nothing read.
        let cut = super::scan(
            &recompressed("4711-0815", |content| {
                let keep = content.len() - 6;
                content.truncate(keep);
            }),
            &needles,
            None,
        );
        assert!(
            cut.blind
                .iter()
                .any(|why| why.contains("stopped decoding part way")),
            "{cut:?}"
        );
        // The control: a stream whose only fault is its checksum was read to
        // its end, by `lopdf` and by this, and is not blind.
        let checksum = super::scan(
            &recompressed("nothing here", |content| {
                let last = content.len() - 1;
                content[last] ^= 0xFF;
            }),
            &needles,
            None,
        );
        assert_eq!(checksum.verdict(), Verdict::Verified, "{checksum:?}");
    }

    /// One page whose content stream is `content`, drawn through Helvetica in
    /// WinAnsiEncoding.
    fn showing(content: &[u8]) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let body = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        };
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => body,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// Each spelling a string operand can take, and a word with a WinAnsi
    /// byte in it, is found; each control is the same spelling of another
    /// word, so the finding is the word's and not the spelling's.
    #[test]
    fn a_word_in_any_spelling_of_a_string_operand_is_found() {
        let cases: [(&str, &[u8], &[u8]); 5] = [
            (
                "4711-0815",
                b"BT /F1 12 Tf 72 700 Td <343731312D30383135> Tj ET",
                b"BT /F1 12 Tf 72 700 Td <6E6F7468696E67> Tj ET",
            ),
            (
                "4711-0815",
                b"BT /F1 12 Tf 72 700 Td (\\064711-\\060815) Tj ET",
                b"BT /F1 12 Tf 72 700 Td (\\064700-\\060815) Tj ET",
            ),
            (
                "4711-0815",
                b"BT /F1 12 Tf 72 700 Td [(47) -20 (11-) 15 (0815)] TJ ET",
                b"BT /F1 12 Tf 72 700 Td [(47) -20 (00-) 15 (0815)] TJ ET",
            ),
            (
                "4711-0815",
                b"BT /F1 12 Tf 72 700 Td (4711) Tj 30 0 Td (-0815) Tj ET",
                b"BT /F1 12 Tf 72 700 Td (4700) Tj 30 0 Td (-0815) Tj ET",
            ),
            (
                "M\u{fc}ller",
                b"BT /F1 12 Tf 72 700 Td (M\xfcller) Tj ET",
                b"BT /F1 12 Tf 72 700 Td (M\xfcnzer) Tj ET",
            ),
        ];
        for (needle, shown, control) in cases {
            let needles = vec![needle.to_string()];
            let report = super::scan(&showing(shown), &needles, None);
            assert!(
                report.found.contains(needle),
                "{needle} in {:?}: {report:?}",
                String::from_utf8_lossy(shown)
            );
            let clean = super::scan(&showing(control), &needles, None);
            assert_eq!(
                clean.verdict(),
                Verdict::Verified,
                "{:?}: {clean:?}",
                String::from_utf8_lossy(control)
            );
        }
    }

    #[test]
    fn a_string_operand_is_decoded_as_pdf_syntax_says() {
        use super::strings_shown;
        assert_eq!(strings_shown(b"(a(b)c\\)d) Tj"), b"a(b)c)d");
        // Three octal digits at most: `\0102` is byte 8 and then `2`.
        assert_eq!(strings_shown(b"(\\101\\1022\\n) Tj"), b"AB2\n");
        assert_eq!(strings_shown(b"(\\0102) Tj"), b"\x082");
        assert_eq!(strings_shown(b"(a\\\r\nb) Tj"), b"ab");
        assert_eq!(strings_shown(b"<4 1 4>"), b"A@");
        assert_eq!(strings_shown(b"<</MCID 0>> BDC (x) Tj"), b"x");
        assert_eq!(strings_shown(b"% (not a string)\n(y) Tj"), b"y");
        assert_eq!(super::winansi('\u{20AC}'), Some(0x80));
        assert_eq!(super::winansi('\u{0100}'), None);
        assert_eq!(super::macroman('\u{00FC}'), Some(0x9F));
        assert_eq!(super::macroman('\u{02C7}'), Some(0xFF));
        assert_eq!(super::macroman('\u{0100}'), None);
        assert_eq!(
            super::spellings_of("M\u{fc}ller"),
            vec![
                "M\u{fc}ller".as_bytes().to_vec(),
                b"M\xfcller".to_vec(),
                b"M\x9fller".to_vec()
            ]
        );
    }

    /// ASCII85 of `data` with its `~>` end marker, as a PDF writer emits it.
    fn ascii85(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            let mut value = u32::from_be_bytes(word);
            if chunk.len() == 4 && value == 0 {
                out.push(b'z');
                continue;
            }
            let mut digits = [0u8; 5];
            for digit in digits.iter_mut().rev() {
                *digit = b'!' + (value % 85) as u8;
                value /= 85;
            }
            out.extend_from_slice(&digits[..chunk.len() + 1]);
        }
        out.extend_from_slice(b"~>");
        out
    }

    /// [`filtered`]'s page with its form re-encoded under `filter` alone and
    /// then passed through `cut`.
    fn encoded_as(filter: &str, hidden: &str, cut: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
        let mut doc = lopdf::Document::load_mem(&filtered("kept", hidden, false)).expect("load");
        for object in doc.objects.values_mut() {
            if let lopdf::Object::Stream(stream) = object {
                if stream.dict.has(b"Filter") {
                    let plain = stream.decompressed_content().expect("inflate the form");
                    let mut encoded = match filter {
                        "LZWDecode" => {
                            weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                                .encode(&plain)
                                .expect("encode LZW")
                        }
                        _ => ascii85(&plain),
                    };
                    cut(&mut encoded);
                    stream
                        .dict
                        .set("Filter", lopdf::Object::Name(filter.into()));
                    stream.dict.set("Length", encoded.len() as i64);
                    stream.content = encoded;
                }
            }
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        bytes
    }

    fn stopped(report: &Report) -> bool {
        report
            .blind
            .iter()
            .any(|why| why.contains("stopped decoding part way"))
    }

    #[test]
    fn an_lzw_stream_cut_short_withholds_the_verdict() {
        let needles = vec!["4711-0815".to_string()];
        // Whole, the words are found and a clean form is verified, so the
        // encoding is the one `lopdf` decodes.
        let found = super::scan(
            &encoded_as("LZWDecode", "4711-0815", |_| {}),
            &needles,
            None,
        );
        assert!(found.found.contains("4711-0815"), "{found:?}");
        let whole = super::scan(
            &encoded_as("LZWDecode", "nothing here", |_| {}),
            &needles,
            None,
        );
        assert_eq!(whole.verdict(), Verdict::Verified, "{whole:?}");
        // Cut before its end code: `lopdf` returns the first part as success.
        let cut = super::scan(
            &encoded_as("LZWDecode", "nothing here", |content| {
                let keep = content.len() - 4;
                content.truncate(keep);
            }),
            &needles,
            None,
        );
        assert!(stopped(&cut), "{cut:?}");
    }

    #[test]
    fn an_ascii85_stream_cut_short_or_broken_withholds_the_verdict() {
        let needles = vec!["4711-0815".to_string()];
        let found = super::scan(
            &encoded_as("ASCII85Decode", "4711-0815", |_| {}),
            &needles,
            None,
        );
        assert!(found.found.contains("4711-0815"), "{found:?}");
        let whole = super::scan(
            &encoded_as("ASCII85Decode", "nothing here", |_| {}),
            &needles,
            None,
        );
        assert_eq!(whole.verdict(), Verdict::Verified, "{whole:?}");
        // White space after the marker is still a stream read whole.
        let spaced = super::scan(
            &encoded_as("ASCII85Decode", "nothing here", |content| {
                content.extend_from_slice(b"\n")
            }),
            &needles,
            None,
        );
        assert_eq!(spaced.verdict(), Verdict::Verified, "{spaced:?}");
        // Cut, so the marker is gone.
        let cut = super::scan(
            &encoded_as("ASCII85Decode", "nothing here", |content| {
                let keep = content.len() - 7;
                content.truncate(keep);
            }),
            &needles,
            None,
        );
        assert!(stopped(&cut), "{cut:?}");
        // A byte `lopdf` stops at, half way: the rest is never decoded.
        let broken = super::scan(
            &encoded_as("ASCII85Decode", "nothing here", |content| {
                let middle = content.len() / 2;
                content[middle] = b'{';
            }),
            &needles,
            None,
        );
        assert!(stopped(&broken), "{broken:?}");
    }

    /// [`drawn_through`]'s simple-font page with `/Encoding` set on its font
    /// to a dictionary whose `/Differences` is `differences`.
    fn encoded_through(differences: Vec<lopdf::Object>) -> Vec<u8> {
        use lopdf::{dictionary, Object};
        let mut doc = lopdf::Document::load_mem(&drawn_through("Type1")).expect("load");
        let encoding = doc.add_object(dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => differences,
        });
        for object in doc.objects.values_mut() {
            let Object::Dictionary(page) = object else {
                continue;
            };
            let Ok(Object::Dictionary(resources)) = page.get_mut(b"Resources") else {
                continue;
            };
            let Ok(Object::Dictionary(fonts)) = resources.get_mut(b"Font") else {
                continue;
            };
            let Ok(Object::Dictionary(font)) = fonts.get_mut(b"F1") else {
                continue;
            };
            font.set("Encoding", encoding);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        assert!(
            bytes.windows(11).any(|w| w == b"Differences"),
            "the fixture must carry the array"
        );
        bytes
    }

    fn names(code: i64, names: &[&str]) -> Vec<lopdf::Object> {
        std::iter::once(lopdf::Object::Integer(code))
            .chain(
                names
                    .iter()
                    .map(|name| lopdf::Object::Name(name.as_bytes().to_vec())),
            )
            .collect()
    }

    fn differences_reason(report: &Report) -> bool {
        report
            .blind
            .iter()
            .any(|why| why.contains("(/Differences)"))
    }

    #[test]
    fn a_differences_array_that_moves_a_searched_letter_withholds_the_verdict() {
        let john = vec!["John".to_string()];
        // "John" written as the bytes 1 to 4.
        let moved = super::scan(
            &encoded_through(names(1, &["J", "o", "h", "n"])),
            &john,
            None,
        );
        assert!(differences_reason(&moved), "{moved:?}");
        assert!(matches!(moved.verdict(), Verdict::NotVerified(_)));
        // The control: letters at their own codes, and accented letters above
        // 127, leave "John" where the byte search looks for it.
        let plain = super::scan(
            &encoded_through([names(65, &["A", "B"]), names(233, &["eacute"])].concat()),
            &john,
            None,
        );
        assert_eq!(plain.verdict(), Verdict::Verified, "{plain:?}");
    }

    #[test]
    fn a_ligature_in_a_differences_array_withholds_only_the_words_it_spells() {
        let file = encoded_through(names(12, &["fi"]));
        let office = super::scan(&file, &["profile".to_string()], None);
        assert!(differences_reason(&office), "{office:?}");
        let john = super::scan(&file, &["John".to_string()], None);
        assert_eq!(john.verdict(), Verdict::Verified, "{john:?}");
    }

    #[test]
    fn a_glyph_name_nothing_resolves_withholds_every_word() {
        let file = encoded_through(names(1, &["g12"]));
        let john = super::scan(&file, &["John".to_string()], None);
        assert!(differences_reason(&john), "{john:?}");
    }

    #[test]
    fn glyph_names_resolve_as_the_glyph_list_specification_says() {
        use super::{glyph, Glyph};
        assert_eq!(glyph(b"A"), Glyph::Ascii(b'A'));
        assert_eq!(glyph(b"a.sc"), Glyph::Ascii(b'a'));
        assert_eq!(glyph(b"uni004A"), Glyph::Ascii(b'J'));
        assert_eq!(glyph(b"u004A"), Glyph::Ascii(b'J'));
        assert_eq!(glyph(b"hyphen"), Glyph::Ascii(b'-'));
        assert_eq!(glyph(b"fi"), Glyph::Ligature("fi"));
        assert_eq!(glyph(b"uFB03"), Glyph::Ligature("ffi"));
        assert_eq!(glyph(b"f_f_l.alt"), Glyph::Ligature("ffl"));
        assert_eq!(glyph(b"eacute"), Glyph::Other);
        assert_eq!(glyph(b"uni00E9"), Glyph::Other);
        assert_eq!(glyph(b".notdef"), Glyph::Other);
        assert_eq!(glyph(b"g12"), Glyph::Opaque);
        assert_eq!(glyph(b"T_h"), Glyph::Opaque);
        assert_eq!(glyph(b"uni00410042"), Glyph::Opaque);
    }

    /// `page_text` drawn on the page, and a document information dictionary
    /// written directly into a classic trailer with `title` in UTF-16BE.
    fn titled(page_text: &str, title: &str) -> Vec<u8> {
        use lopdf::{dictionary, Object, StringFormat};
        let mut doc =
            lopdf::Document::load_mem(&filtered(page_text, "nothing here", false)).expect("load");
        let mut utf16 = vec![0xFE, 0xFF];
        for unit in title.encode_utf16() {
            utf16.extend_from_slice(&unit.to_be_bytes());
        }
        doc.trailer.set(
            "Info",
            Object::Dictionary(dictionary! {
                "Title" => Object::String(utf16, StringFormat::Hexadecimal),
            }),
        );
        // A cross-reference stream is an object, and its dictionary would be
        // read with the objects; a classic trailer is not one, which is the
        // case the objects never reach.
        doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");
        assert!(
            bytes.windows(8).any(|window| window == b"trailer\n")
                || bytes.windows(8).any(|window| window == b"trailer\r"),
            "the fixture must have a classic trailer"
        );
        bytes
    }

    #[test]
    fn a_title_in_an_info_dictionary_written_into_the_trailer_is_found() {
        let needles = vec!["4711-0815".to_string()];
        let report = super::scan(&titled("kept", "4711-0815"), &needles, None);
        assert!(report.found.contains("4711-0815"), "{report:?}");
        // And on the page as well: the page alone would place it there, and
        // the copy in the trailer, which no page reaches, has to undo that.
        let both = super::scan(&titled("4711-0815", "4711-0815"), &needles, None);
        assert_eq!(
            both.located.get("4711-0815"),
            Some(&Located::Unplaced),
            "{both:?}"
        );
        // The control: on the page alone it is placed, so the answer above is
        // the trailer's.
        let page = super::scan(&titled("4711-0815", "plain"), &needles, None);
        assert!(
            matches!(page.located.get("4711-0815"), Some(Located::Pages(_))),
            "{page:?}"
        );
    }
}
