//! Whether a file can be certified clean, and what the answer rests on.
//!
//! `docs/PLAN.md` §6 makes verification mandatory after a redaction and forbids
//! a bare success: the result is *verified*, or *not verified* with specifics.
//! The rule it states for getting there is deny by default --- anything the
//! sanitizer does not understand is a failure rather than a shrug --- and spike
//! 0.4 found that rule, taken literally, refuses almost every real document.
//!
//! **The refusal rate is the finding, not a detail.** `lopdf` decodes
//! `FlateDecode`, `LZWDecode` and `ASCII85Decode`, and answers
//! `Unimplemented("decompression algorithms")` for everything else. Everything
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
//! * [`Report::deferred`] --- a raster image. Its *encoded bytes* were scanned
//!   like every other byte in the file, so a needle sitting literally in the
//!   stream is found; what was not read is the **picture**, and text that exists
//!   only as pixels is exactly what OCR is for.
//!
//! **That instrument exists now, and it is narrower than this list.**
//! [`crate::ocr_gate`] renders the regions a redaction removed from and has an
//! engine read them, so a *region* whose carrier is a picture is answered. A
//! deferred image sitting anywhere else in the file is still exactly what this
//! bullet says it is: bytes nobody read, reported rather than waved through.
//!
//! A caller that wants a single word gets [`Verdict`], which says *not verified*
//! for either list. What the split buys is that the reason names the next step
//! rather than ending the conversation.
//!
//! **Two things the byte scan can see and the graph walk cannot**, both from
//! spike 0.4 and both preserved here: bytes past the last `%%EOF` belong to no
//! object at all, and a file with more than one `%%EOF` has revisions that no
//! parser resolves --- an object a later revision *overwrote* sits at its old
//! offset addressable by nothing, so it is invisible to any graph walk and, if
//! it is compressed, to the byte scan as well. Such a file cannot be certified;
//! it can only be rewritten and then certified.

use std::collections::BTreeSet;

use lopdf::{Document, Object};

/// Ceiling on any single decoded stream.
///
/// The same bound `save::MAX_DECODE` uses, and for the same reason: a carrier
/// that will not fit is a blind spot rather than a reason to allocate. Stated
/// here rather than imported so that this module's own bound is visible in the
/// module that enforces it.
pub const MAX_DECODE: usize = 64 * 1024 * 1024;

/// The filters `lopdf` 0.44 can actually decode.
///
/// Read out of `Stream::decode_filters`, which dispatches on exactly these three
/// and returns `Unimplemented` otherwise. Written down rather than discovered at
/// run time because the classification below has to distinguish "this failed to
/// decode" from "this was never going to decode", and only the second is a fact
/// about the library.
const DECODABLE: &[&[u8]] = &[b"FlateDecode", b"LZWDecode", b"ASCII85Decode"];

/// Filters whose content is a raster image rather than bytes worth scanning.
///
/// Not a list of things we forgive. A needle cannot be *found* in a JPEG by
/// looking at its decoded output, because the decoded output is pixels --- so
/// even a decoder for these would not answer the question the byte scan asks.
/// That is what makes them a different instrument's problem rather than a
/// weaker version of the same one.
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
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Needles still present. For a redaction, any of these is a leak.
    pub found: BTreeSet<String>,
    /// Bytes nothing here can account for. No instrument would change this.
    pub blind: Vec<String>,
    /// Pictures nobody read. OCR is the instrument that would.
    pub deferred: Vec<String>,
    /// How many objects the graph walk reached.
    pub objects: usize,
    /// `%%EOF` markers, so more than one revision is visible.
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
            why.push(format!("{needle} is still in the file"));
        }
        why.extend(self.blind.iter().cloned());
        why.extend(self.deferred.iter().cloned());
        if why.is_empty() {
            Verdict::Verified
        } else {
            Verdict::NotVerified(why)
        }
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
        wrong.push("the file does not begin with a PDF header".to_string());
    }

    let eofs = count(bytes, b"%%EOF");
    match eofs {
        0 => wrong.push("the file has no %%EOF marker".to_string()),
        1 => {}
        many => wrong.push(format!(
            "the file has {many} %%EOF markers, so it holds more than one revision --- a rewrite writes exactly one, and an earlier revision is content no parser will show and no scan can decode"
        )),
    }

    if let Some(last) = rfind(bytes, b"%%EOF") {
        let trailing = bytes[last + 5..]
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
        if trailing > 0 {
            wrong.push(format!(
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
        None => wrong.push("the file has no startxref".to_string()),
        Some(at) => match start_offset(&bytes[at + 9..]) {
            None => wrong.push("the file's startxref has no offset after it".to_string()),
            Some(offset) if offset >= bytes.len() => wrong.push(format!(
                "startxref points at byte {offset} of a {}-byte file",
                bytes.len()
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
        eofs: count(bytes, b"%%EOF"),
        bytes: bytes.len() as u64,
        ..Default::default()
    };

    for needle in needles {
        if find(bytes, needle.as_bytes()) {
            report.found.insert(needle.clone());
        }
    }

    if report.eofs > 1 {
        report.blind.push(format!(
            "the file has {} %%EOF markers, so earlier revisions exist that no parser will \
             resolve and no scan can decode",
            report.eofs
        ));
    }

    match rfind(bytes, b"%%EOF") {
        Some(last) => {
            report.trailing = bytes[last + 5..]
                .iter()
                .filter(|byte| !byte.is_ascii_whitespace())
                .count();
        }
        None => report
            .blind
            .push("the file has no %%EOF marker".to_string()),
    }

    match Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    ) {
        Err(why) => report.blind.push(format!(
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
                report.blind.push(
                    "the file is encrypted and no password opened it, so nothing in it was \
                     decoded and nothing it contains was checked"
                        .to_string(),
                );
            } else if doc.objects.is_empty() {
                report.blind.push(
                    "the file parsed to no objects at all, so nothing in it was accounted for"
                        .to_string(),
                );
            }
            for (id, object) in &doc.objects {
                let strings = flatten_strings(object);
                for needle in needles {
                    if find(&strings, needle.as_bytes()) {
                        report.found.insert(needle.clone());
                    }
                }
                let Object::Stream(stream) = object else {
                    continue;
                };
                let filters = stream.filters().unwrap_or_default();
                match classify(&filters) {
                    Carrier::Image { filter } => report.deferred.push(format!(
                        "object {} is a {filter} image, so its encoded bytes were scanned and \
                         its picture was not read --- text visible only as pixels needs OCR",
                        id.0
                    )),
                    Carrier::Undecodable { filter } => report.blind.push(format!(
                        "object {} is {filter}, which this build cannot decode, so its contents \
                         are unknown",
                        id.0
                    )),
                    Carrier::Unrecognised { filter } => report.blind.push(format!(
                        "object {} uses {filter}, which nothing here recognises",
                        id.0
                    )),
                    Carrier::Scanned => match stream.decompressed_content_with_limit(MAX_DECODE) {
                        Ok(decoded) => {
                            for needle in needles {
                                if find(&decoded, needle.as_bytes()) {
                                    report.found.insert(needle.clone());
                                }
                            }
                        }
                        // Classified as decodable and then would not decode: a
                        // bomb over the bound, or damage. Blind either way, and
                        // it must not be mistaken for the filter cases above ---
                        // those are decided before any decoding is attempted.
                        Err(why) => report.blind.push(format!(
                            "object {} could not be decoded, so its contents are unknown: {why}",
                            id.0
                        )),
                    },
                }
            }
        }
    }

    report.blind.sort();
    report.blind.dedup();
    report.deferred.sort();
    report.deferred.dedup();
    report
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

fn collect_strings(object: &Object, out: &mut Vec<u8>) {
    match object {
        Object::String(bytes, _) => {
            out.extend_from_slice(bytes);
            out.push(b'\n');
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
    use super::{classify, Carrier, Report, Verdict};

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
}
