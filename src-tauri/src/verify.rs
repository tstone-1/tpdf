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
    use super::{classify, Carrier, Report, Verdict};

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
}
