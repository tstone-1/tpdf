//! Whether a page's characters mean anything, or PDFium is guessing.
//!
//! There is a **third state** between "this page has text" and "this page has no
//! text", and until now nothing in tpdf represented it. `docs/PLAN.md` Phase 1
//! records the finding: a CID font with no `/ToUnicode` is ordinary in the wild,
//! and PDFium does not fail on it. It reads the glyph ids as if they were
//! character codes and returns text of the **right length, in the right places,
//! with the right word lengths**. On `testdata/encodings.pdf` page 0,
//! `Encoding probe ABC` extracts as `(QFRGLQJ\x03SUREH\x03$%&`.
//!
//! Everything downstream then behaves correctly and is wrong. The page is not
//! textless, so `PageMatches::textless` --- which exists so that "no matches" is
//! never a lie of omission --- does not fire. A reader searching for a word they
//! can see is told there are no matches. Copy yields nonsense. The accessibility
//! tree reads the nonsense out.
//!
//! ## Why this is a `lopdf` question and not a PDFium one
//!
//! It cannot be answered from the characters. Garbage of the right length is
//! indistinguishable from text in a language nobody here reads, and any rule
//! over the code points --- "mostly punctuation", "no spaces", "outside the
//! expected block" --- is a heuristic that will call a legitimate document
//! broken. The **file** answers it directly and with certainty: a font either
//! declares what its glyphs mean or it does not.
//!
//! PDFium has no public API for it either. `FPDFFont_*` exposes the base name,
//! embeddedness and glyph data; nothing reports the presence of a `/ToUnicode`
//! stream. So this reads the object graph, which is what `lopdf` is here for.
//!
//! ## The rule, and the field it actually turns on
//!
//! A font leaves PDFium guessing when **both** hold:
//!
//! 1. it has no `/ToUnicode` CMap, and
//! 2. its descendant's `/CIDSystemInfo /Ordering` is `Identity`, or absent.
//!
//! The second is the operative one and it is easy to get wrong, because the
//! obvious candidate is the *encoding* name. `/Encoding` decides code -> CID; it
//! says nothing about CID -> Unicode. What supplies CID -> Unicode in the absence
//! of a `/ToUnicode` is the registry-and-ordering: PDFium ships tables for
//! `Adobe-Japan1`, `Adobe-GB1`, `Adobe-CNS1` and `Adobe-Korea1`, and ships
//! nothing for `Identity`, because `Identity` means "these numbers are glyph
//! indices in this font" and only the font knows what they draw.
//!
//! **The corpus cannot tell those two rules apart**, which is why this says so
//! out loud. `encodings.pdf` page 0 is `/Identity-H` *and* `/Ordering (Identity)`;
//! page 2 is `/UniJIS-UCS2-H` *and* `/Ordering (Japan1)`. The two fields covary,
//! so a mutation swapping "check the ordering" for "check the encoding" passes
//! every fixture. `AGENTS.md`: whatever a fixture is meant to discriminate, it
//! needs two of. The unit tests below supply the missing diagonal --- Identity-H
//! over Japan1, and a predefined CMap over Identity ordering --- as synthetic
//! documents, since no real-world file needs to exist for a rule to be tested.
//!
//! ## Simple fonts are not affected, and that is not an omission
//!
//! A Type1 or TrueType font addresses glyphs by *name* through `/Encoding`
//! (`/WinAnsiEncoding`, `/MacRomanEncoding`, a `/Differences` array), and a glyph
//! name maps to Unicode through the Adobe Glyph List. So a simple font with no
//! `/ToUnicode` is ordinarily fine, and flagging one would report most of the
//! world's PDFs as broken. Only composite (`/Type0`) fonts are considered.
//!
//! ## What it costs
//!
//! Mapping is computed lazily, off the first-paint path. The original
//! dictionary-only scan took 0.1 ms on a small document and 11.9 ms on a
//! 337 MB scan. Those measurements predate inspection of text operators and
//! placed forms. Image payloads are never decoded to inspect a text mapping.
//!
//! A font listed in a shared resource dictionary may be unused, and text in
//! a placed form can use a font absent from the page dictionary. Follow text
//! shows with their selected font, including graphics-state restores and
//! form-local resources. An incomplete inspection is reported separately.
//!
//! ## Hostile input
//!
//! The same document as everywhere else. The walk is bounded in every dimension
//! that a file controls --- pages visited, fonts per page, and the resource-tree
//! recursion that `/Resources` inheritance requires --- and decompression is
//! bounded by the loader, as `print.rs` does it. A document that hits a bound is
//! reported as **unknown**, never as clean: this module's whole purpose is to
//! stop a reassuring answer being given on evidence nobody has.

pub(crate) mod complete;

use std::collections::HashMap;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};

/// Check the declared size, colour space and signature before retaining an ICC profile.
pub(crate) fn icc_header_matches(bytes: &[u8], space: &[u8; 4]) -> bool {
    bytes.len() >= 132
        && bytes[..4] == (bytes.len() as u32).to_be_bytes()
        && &bytes[16..20] == space
        && &bytes[36..40] == b"acsp"
}

/// Largest decompressed stream any scan here will accept.
///
/// **The one copy.** `AGENTS.md` states "all `lopdf` stream decoding is
/// bounded" as a single hard constraint, and it was six private constants in
/// six modules, each documented as matching a *different* one of the other
/// five --- so the constraint held by six values agreeing rather than by there
/// being one value. `save.rs` reached into `print.rs` for it, which put the
/// writer downstream of the printer for a security bound.
///
/// It lives here because decoding is this module's subject: `resolve` is what
/// every one of those scans reaches a stream through. 64 MB is far past any
/// real content stream and far under what a decompression bomb wants --- spike
/// 0.4 measured a 2,879-byte input inflating to 1 GiB.
///
/// ⚠ **"The one copy" was false for five days**, and nothing could go red about
/// it: `verify.rs` declared a second literal on 2026-08-26, one day after this
/// paragraph was written, with a comment arguing that a module's own bound
/// should be visible where it is enforced. It was removed on 2026-08-31 and
/// that module imports this. A sentence in a doc comment is a claim about the
/// whole crate made in one file, and the only thing keeping this one true is
/// that a bound written here is imported rather than restated --- which is
/// exactly the shape `docs/TRAPS.md` records as a rule written down and not
/// enforced.
pub(crate) const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Maximum nesting of a placed form during a mapping scan.
const MAX_FORM_DEPTH: usize = 32;

/// Most fonts a single page's resource dictionary may declare.
const MAX_FONTS: usize = 4096;

/// CID orderings PDFium can map to Unicode without a `/ToUnicode`.
///
/// These are the registries it ships CID-to-Unicode tables for. Anything else ---
/// `Identity` above all --- leaves it reading glyph indices as character codes.
/// Compared case-sensitively because these are Adobe's own spellings and a
/// document using a different case is not naming one of these collections.
const MAPPABLE_ORDERINGS: [&str; 4] = ["Japan1", "GB1", "CNS1", "Korea1"];

/// What a page's fonts say about whether its text means anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageMapping {
    /// Distinct composite fonts encountered when the page shows text.
    pub composite: usize,
    /// Of those, how many leave PDFium guessing.
    pub guessing: usize,
    /// Whether the scan or an explicit text replacement could be read only in part.
    ///
    /// Reported rather than folded into the numbers: a page that was only partly
    /// examined and a page that was examined and found clean are different
    /// facts, and this module exists because those two were being conflated one
    /// level up.
    pub truncated: bool,
}

impl PageMapping {
    /// Whether text extracted from this page is PDFium's guess rather than the
    /// document's statement.
    ///
    /// Deliberately not "the page is broken": a page can mix a guessed font with
    /// a stated one, and a reader searching for a word set in the stated one
    /// will still find it. What this says is that *some* text on the page cannot
    /// be searched, which is what the reader has to be told.
    #[must_use]
    pub fn unreadable(&self) -> bool {
        self.guessing > 0
    }

    /// Whether the scan finished, so a negative answer can be trusted.
    ///
    /// `!unreadable() && certain()` is the only combination that means "this
    /// page is fine". `!unreadable() && !certain()` means nobody knows, and must
    /// never be reported as fine --- the same rule `docs/PLAN.md` §6 sets for a
    /// redaction verification, arriving in a much smaller place.
    #[must_use]
    pub fn certain(&self) -> bool {
        !self.truncated
    }
}

/// Reads a document's font dictionaries and reports, per page, whether its text
/// is stated or guessed.
///
/// The vector is always exactly `page_count` long, so index `n` is page `n` and a
/// caller never has to reason about a short answer.
///
/// **`page_count` comes from PDFium, and passing it is load-bearing rather than a
/// convenience.** An empty vector reads as "no page has a problem", which is the
/// precise lie this module was written to stop, arriving through the module
/// itself. Every page `lopdf` could not account for is therefore returned
/// `truncated` --- known to be unknown --- and `certain()` is false for it.
///
/// **The guard has a demonstrated instance again as of 2026-08-23, and the two
/// corrections in a row are the interesting part.** It first read: *"`lopdf`
/// reports zero pages for `testdata/incr-encrypted-pw.pdf`, which PDFium opens
/// and paginates normally"*. That was half wrong on 2026-08-16 --- PDFium
/// refused to open it at all, so the two parsers never both saw it --- and the
/// note was rewritten to say the guard was defensive rather than demonstrated.
///
/// Then tpdf learned to ask for a password, and PDFium opens that document now.
/// So the original example became true, having been false in between: a reader
/// types `swordfish`, PDFium paginates two pages, and a `lopdf` parse *without*
/// the key reads **no objects at all** and reports zero. Which is why `password`
/// is a parameter here.
///
/// The lesson the two corrections share is worth more than either: a note
/// asserting what another component *cannot* do is a claim with a date on it,
/// and a feature is what expires it. Nothing went red when this became false ---
/// prose has no gate.
///
/// The design is unchanged and still right: two independent parsers with no
/// guarantee of agreeing is reason enough to take the count from the one whose
/// pagination the reader is actually looking at. Swept across every
/// `testdata/*.pdf` on 2026-08-16, they agreed about page count on every fixture
/// PDFium would open, and `examples/password_probe.rs` now asserts the same for
/// the locked one once it is unlocked.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`].
///
/// **`password` is the reader's, when the document needed one, and it decides
/// whether this reads anything at all.** `lopdf` tries the empty password by
/// itself, so a permission-restricted document needs nothing here --- but one
/// behind a real password parses to a `Document` with **no objects in it**,
/// which loads cleanly and reports zero pages. Without the key that is
/// indistinguishable from a document that simply has none of what is being
/// looked for. See [`crate::progressive::RawDocument::password`].
pub fn scan(
    bytes: &[u8],
    page_count: usize,
    password: Option<&str>,
) -> Result<Vec<PageMapping>, String> {
    scan_from(&load(bytes, password)?, page_count)
}

/// Parses a document's bytes under this crate's load options.
///
/// **The one place the options are written**, and the reason is the same one the
/// bound above gives for being imported rather than restated: five modules used
/// to open the same document with five copies of this block, so a change to the
/// decompression bound or to how the password is passed was five edits, and
/// nothing anywhere would go red for making four of them.
///
/// It is also what makes one parse serve five questions --- see
/// [`crate::docgraph::DocumentGraph`], which holds the result. That was five
/// parses of the same file: 5.8 ms each on the 775-page document, 11.9 ms on the
/// 337 MB scan.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`]. A
/// document whose password is wrong parses without error and yields no objects
/// --- see the note on `password` above, which is why every caller here takes one.
pub fn load(bytes: &[u8], password: Option<&str>) -> Result<Document, String> {
    Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| said!("could not parse the document: {e}"))
}

/// [`scan`] over a document somebody else parsed.
///
/// # Errors
///
/// None today; the signature matches [`scan`]'s so the two cannot drift into
/// answering differently shaped things.
pub fn scan_from(document: &Document, page_count: usize) -> Result<Vec<PageMapping>, String> {
    let mut pages: Vec<PageMapping> = document
        .get_pages()
        .values()
        .take(page_count)
        .map(|id| page_mapping(document, *id))
        .collect();

    // Whatever is missing is unknown, never clean. Also covers the opposite
    // disagreement --- `lopdf` finding *more* pages than PDFium --- since the
    // `take` above drops those and the length is pinned either way.
    pages.resize(
        page_count,
        PageMapping {
            truncated: true,
            ..PageMapping::default()
        },
    );
    Ok(pages)
}

/// Classify the fonts that actually show text, including text in placed forms.
fn page_mapping(doc: &Document, page: ObjectId) -> PageMapping {
    let mut scan = FontScan {
        doc,
        seen: HashMap::new(),
        bytes_left: 8 * 1024 * 1024,
        forms_left: MAX_FONTS,
        result: PageMapping::default(),
    };
    if scan.page(page).is_none() {
        scan.result.truncated = true;
    }
    scan.result
}

struct FontScan<'a> {
    doc: &'a Document,
    seen: HashMap<*const Dictionary, bool>,
    bytes_left: usize,
    forms_left: usize,
    result: PageMapping,
}

#[derive(Clone, Copy, PartialEq)]
enum ActualText {
    Absent,
    Complete,
    Incomplete,
}

impl ActualText {
    fn from_bytes(raw: &[u8]) -> Self {
        if raw.is_empty() || raw == [0xfe, 0xff] || raw == [0xef, 0xbb, 0xbf] {
            return Self::Absent;
        }
        let utf16 = raw.starts_with(&[0xfe, 0xff]);
        let utf8 = raw.starts_with(&[0xef, 0xbb, 0xbf]);
        let text = crate::annots::decode_text_string(raw);
        // PDFium drops supplementary characters and replaces ASCII controls.
        // The annotation decoder can also skip malformed or undefined bytes.
        if (!utf16 || raw.len() % 2 == 0)
            && (utf16 || utf8 || text.chars().count() == raw.len())
            && !text.is_empty()
            && text
                .chars()
                .all(|ch| (' '..='~').contains(&ch) || ('\u{81}'..'\u{fffd}').contains(&ch))
        {
            Self::Complete
        } else {
            Self::Incomplete
        }
    }
}

impl<'a> FontScan<'a> {
    fn page(&mut self, page: ObjectId) -> Option<()> {
        let doc = self.doc;
        if !doc.get_dictionary(page).ok()?.has(b"Contents") {
            return Some(());
        }
        let empty = Dictionary::new();
        let resources = crate::pagetree::resources(doc, page)
            .ok()?
            .unwrap_or(&empty);
        let content = crate::textedit::mapping_content(doc, page).ok()?;
        // Without a marked-content property list, a placed form or an
        // external font selection, ordinary simple-font text needs no walk.
        // BDC is an operator and always literal in the decoded stream. A
        // match inside data merely sends the page through the full inspection.
        if !content.windows(3).any(|word| word == b"BDC")
            && !resources.has(b"XObject")
            && !resources.has(b"ExtGState")
            && resources
                .get(b"Font")
                .ok()
                .and_then(|fonts| resolve_dict(doc, fonts).ok())
                .is_some_and(|fonts| {
                    fonts.len() <= MAX_FONTS
                        && fonts.iter().all(|(_, font)| {
                            resolve_dict(doc, font)
                                .ok()
                                .and_then(|font| font_kind(doc, font))
                                .is_some_and(|kind| {
                                    matches!(kind, b"Type1" | b"MMType1" | b"TrueType" | b"Type3")
                                })
                        })
                })
        {
            return Some(());
        }
        self.content(resources, content, None, 0)
    }

    fn content<'r>(
        &mut self,
        resources: &'r Dictionary,
        mut bytes: Vec<u8>,
        mut font: Option<&'r Dictionary>,
        depth: usize,
    ) -> Option<()>
    where
        'a: 'r,
    {
        self.bytes_left = self.bytes_left.checked_sub(bytes.len())?;
        if depth > MAX_FORM_DEPTH
            || resources
                .get(b"Font")
                .ok()
                .and_then(|f| resolve_dict(self.doc, f).ok())
                .is_some_and(|f| f.len() > MAX_FONTS)
        {
            return None;
        }
        let doc = self.doc;
        let resource = |kind: &[u8], name: &Object| {
            let dict = resolve_dict(doc, resources.get(kind).ok()?).ok()?;
            Some(resolve(doc, dict.get(name.as_name().ok()?).ok()?))
        };
        let mut stack = Vec::new();
        let mut marks = Vec::new();
        let mut actual_text = ActualText::Absent;
        bytes.push(b'\n'); // Delimit a final operator or comment at EOF.
        for op in lopdf::content::Content::decode_strict(&bytes)
            .ok()?
            .operations
        {
            match op.operator.as_str() {
                "BMC" => {
                    op.operands.first()?.as_name().ok()?;
                    marks.push(actual_text);
                }
                "BDC" => {
                    op.operands.first()?.as_name().ok()?;
                    let property = op.operands.get(1)?;
                    let dict = match property {
                        Object::Name(_) => resource(b"Properties", property)?.as_dict().ok()?,
                        other => other.as_dict().ok()?,
                    };
                    marks.push(actual_text);
                    // PDFium takes the last property dictionary on the current text
                    // object. A nested dictionary can shadow an outer replacement.
                    actual_text = match dict.get(b"ActualText") {
                        Ok(text) => ActualText::from_bytes(resolve(doc, text).as_str().ok()?),
                        Err(_) if actual_text != ActualText::Absent => ActualText::Incomplete,
                        Err(_) => ActualText::Absent,
                    };
                }
                "EMC" => actual_text = marks.pop()?,
                "q" => stack.push(font),
                "Q" => font = stack.pop()?,
                "Tf" => font = Some(resource(b"Font", op.operands.first()?)?.as_dict().ok()?),
                "gs" => {
                    let state = resource(b"ExtGState", op.operands.first()?)?
                        .as_dict()
                        .ok()?;
                    if let Ok(value) = state.get(b"Font") {
                        let pair = resolve(doc, value).as_array().ok()?;
                        font = Some(resolve_dict(doc, pair.first()?).ok()?);
                    }
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    if !shows_glyphs(&op.operator, &op.operands)? {
                        continue;
                    }
                    if actual_text == ActualText::Incomplete {
                        self.result.truncated = true;
                    }
                    let drawn = font?;
                    if font_kind(doc, drawn)? != b"Type0" {
                        continue;
                    }
                    let checked = self
                        .seen
                        .entry(std::ptr::from_ref(drawn))
                        .or_insert_with(|| {
                            self.result.composite += 1;
                            false
                        });
                    if actual_text == ActualText::Absent && !*checked {
                        *checked = true;
                        match guesses(doc, drawn) {
                            Some(true) => self.result.guessing += 1,
                            Some(false) => {}
                            None => self.result.truncated = true,
                        }
                    }
                }
                "Do" => {
                    let obj = resource(b"XObject", op.operands.first()?)?
                        .as_stream()
                        .ok()?;
                    if resolve(doc, obj.dict.get(b"Subtype").ok()?)
                        .as_name()
                        .ok()?
                        == b"Form"
                    {
                        self.forms_left = self.forms_left.checked_sub(1)?;
                        let nested = match obj.dict.get(b"Resources") {
                            Ok(value) => match resolve(doc, value) {
                                Object::Null => resources,
                                other => other.as_dict().ok()?,
                            },
                            Err(_) => resources,
                        };
                        if obj.content.is_empty() && !obj.dict.has(b"F") {
                            continue;
                        }
                        let data = crate::textedit::mapping_stream(
                            doc,
                            obj,
                            self.bytes_left.min(1024 * 1024),
                        )
                        .ok()?;
                        self.content(nested, data, font, depth + 1)?;
                    }
                }
                _ => {}
            }
        }
        marks.is_empty().then_some(())
    }
}

/// A spacing adjustment advances the cursor without using the font's character map.
fn shows_glyphs(operator: &str, operands: &[Object]) -> Option<bool> {
    match (operator, operands) {
        ("Tj" | "'", [Object::String(bytes, _)]) => Some(!bytes.is_empty()),
        ("\"", [word, char_space, Object::String(bytes, _)]) => {
            word.as_float().ok()?;
            char_space.as_float().ok()?;
            Some(!bytes.is_empty())
        }
        ("TJ", [Object::Array(parts)]) => {
            let mut draws = false;
            for part in parts {
                match part {
                    Object::String(bytes, _) => draws |= !bytes.is_empty(),
                    Object::Integer(_) | Object::Real(_) => (),
                    _ => return None,
                }
            }
            Some(draws)
        }
        _ => None,
    }
}

/// Resolve direct and indirect font subtypes before classifying drawn text.
fn font_kind<'a>(doc: &'a Document, font: &'a Dictionary) -> Option<&'a [u8]> {
    let name = resolve(doc, font.get(b"Subtype").ok()?).as_name().ok()?;
    matches!(
        name,
        b"Type0" | b"Type1" | b"MMType1" | b"TrueType" | b"Type3"
    )
    .then_some(name)
}

/// Whether this composite font leaves PDFium guessing.
///
/// `None` when the font cannot be judged --- an unresolvable descendant --- which
/// the caller records as a truncation rather than as either answer.
fn guesses(document: &Document, font: &Dictionary) -> Option<bool> {
    // A `/ToUnicode` settles it whatever else is true. Its *contents* may still
    // be wrong --- `encodings.pdf` page 1 is exactly that, and produces
    // replacement characters --- but that is a different defect with a different
    // remedy, and conflating the two would report a document that states its
    // mapping badly as one that states nothing.
    if let Ok(value) = font.get(b"ToUnicode") {
        match resolve(document, value) {
            Object::Stream(_) => return Some(false),
            Object::Null => {}
            _ => return None,
        }
    }

    let descendants = font.get(b"DescendantFonts").ok()?;
    let descendants = match resolve(document, descendants) {
        Object::Array(array) => array.clone(),
        _ => return None,
    };
    let first = descendants.first()?;
    let descendant = resolve_dict(document, first).ok()?;

    let info = descendant.get(b"CIDSystemInfo").ok()?;
    let info = resolve_dict(document, info).ok()?;

    // An absent `/Ordering` is the same position as `Identity`: nothing names a
    // collection, so there is no table to consult.
    let ordering = info
        .get(b"Ordering")
        .ok()
        .and_then(|object| resolve(document, object).as_str().ok())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();

    Some(!MAPPABLE_ORDERINGS.contains(&ordering.as_str()))
}

/// Follows a reference once, or returns the object itself.
pub(crate) fn resolve<'a>(document: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => document.get_object(*id).unwrap_or(object),
        other => other,
    }
}

/// Resolves an object to a dictionary, following one reference.
pub(crate) fn resolve_dict<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<&'a Dictionary, ()> {
    resolve(document, object).as_dict().map_err(|_| ())
}

/// `bytes` as two upper-case hex digits each, with nothing between them: a
/// certificate serial, a PDF hex string's digits.
pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    hex_in(bytes, b"0123456789ABCDEF")
}

/// `bytes` as two lower-case hex digits each, with nothing between them: a
/// digest as a name.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    hex_in(bytes, b"0123456789abcdef")
}

/// The two spellings above, from one loop.
fn hex_in(bytes: &[u8], digits: &[u8; 16]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(digits[usize::from(byte >> 4)]));
        out.push(char::from(digits[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    /// Both spellings of every nibble, against `format!`'s, which the call
    /// sites used before.
    #[test]
    fn hex_spells_each_byte_as_two_digits_in_the_case_asked_for() {
        assert_eq!(hex_upper(&[0x00, 0x0f, 0xa5, 0xff]), "000FA5FF");
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex_upper(&[]), "");
        let every: Vec<u8> = (0..=255).collect();
        let upper: String = every.iter().map(|b| format!("{b:02X}")).collect();
        let lower: String = every.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex_upper(&every), upper);
        assert_eq!(hex_lower(&every), lower);
    }

    /// Builds a one-page document whose single font is the dictionary given.
    ///
    /// Synthetic rather than a fixture on disk, because what is being tested is a
    /// rule over four combinations of two fields and only two of them occur in
    /// any real document we have. A generated PDF is the cheapest way to reach
    /// the other two, and it needs no font program: nothing here renders.
    fn document_with_font(font: Dictionary) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let mut font = font;
        if let Ok(Object::Stream(map)) = font.get(b"ToUnicode") {
            let id = doc.add_object(map.clone());
            font.set("ToUnicode", id);
        }
        let font_id = doc.add_object(font);
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf (x) Tj ET".to_vec(),
        ));
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("the fixture must serialise");
        out
    }

    /// A `/Type0` font with the ordering given, and a `/ToUnicode` or not.
    fn composite(encoding: &str, ordering: &str, to_unicode: bool) -> Dictionary {
        let descendant = dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "BaseFont" => "Test",
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal(ordering),
                "Supplement" => 0,
            },
        };
        let mut font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Test",
            "Encoding" => encoding,
            "DescendantFonts" => vec![Object::Dictionary(descendant)],
        };
        if to_unicode {
            font.set(
                "ToUnicode",
                Stream::new(dictionary! {}, b"begincmap endcmap".to_vec()),
            );
        }
        font
    }

    fn scan_one(font: Dictionary) -> PageMapping {
        let bytes = document_with_font(font);
        let pages = scan(&bytes, 1, None).expect("the fixture must parse");
        assert_eq!(pages.len(), 1, "the fixture is one page");
        pages[0]
    }

    #[test]
    fn actual_text_covers_only_the_shows_inside_its_scope() {
        let bytes = document_with_font(composite("Identity-H", "KR", false));
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page = doc.get_pages()[&1];
        for (content, guessing) in [
            (
                "BT /F1 10 Tf /Span << /ActualText (Invoice) >> BDC (A) Tj EMC ET",
                0,
            ),
            (
                "BT /F1 10 Tf /Span << /ActualText (Invoice) >> BDC (A) Tj EMC (B) Tj ET",
                1,
            ),
            (
                "BT /F1 10 Tf /Span << /ActualText () >> BDC (A) Tj EMC ET",
                1,
            ),
        ] {
            let stream = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Contents", stream);
            let mapping = page_mapping(&doc, page);
            assert_eq!(
                (mapping.composite, mapping.guessing, mapping.truncated),
                (1, guessing, false)
            );
        }
        assert_eq!(
            scan_one(composite("Identity-H", "Korea1", false)).guessing,
            0
        );
    }

    #[test]
    fn an_incomplete_replacement_cannot_certify_even_a_mapped_font() {
        for font in [
            composite("Identity-H", "Identity", false),
            dictionary! {"Subtype" => "Type1"},
        ] {
            let mut doc = Document::load_mem(&document_with_font(font)).unwrap();
            let page = doc.get_pages()[&1];
            for (actual, certain) in [
                ("FEFF672C", true),
                ("FEFF672CD840DC00", false),
                ("FEFFD840DC00", false),
            ] {
                let content =
                    format!("BT /F1 10 Tf /Span << /ActualText <{actual}> >> BDC (A) Tj EMC ET");
                let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
                doc.get_dictionary_mut(page)
                    .unwrap()
                    .set("Contents", stream);
                assert_eq!(page_mapping(&doc, page).certain(), certain, "{actual}");
            }
        }
    }

    #[test]
    fn only_shown_fonts_count_in_a_placed_form() {
        let bytes = document_with_font(composite("Identity-H", "Identity", false));
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page = doc.get_pages()[&1];
        let mut resources = crate::pagetree::resources(&doc, page)
            .unwrap()
            .unwrap()
            .clone();
        let plain = doc.add_object(
            dictionary! { "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica" },
        );
        let mut fonts = resolve_dict(&doc, resources.get(b"Font").unwrap())
            .unwrap()
            .clone();
        fonts.set("Body", plain);
        resources.set("Font", fonts);
        let form = doc.add_object(Stream::new(
            dictionary! { "Subtype"=>"Form", "Resources"=>resources },
            Vec::new(),
        ));
        let paint = doc.add_object(Stream::new(dictionary! {}, b"/Placed Do".to_vec()));
        let dict = doc.get_dictionary_mut(page).unwrap();
        dict.set("Contents", paint);
        dict.set(
            "Resources",
            dictionary! { "XObject"=>dictionary! { "Placed"=>form } },
        );
        for (name, guesses) in [("Body", 0), ("F1", 1)] {
            doc.get_object_mut(form)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(format!("BT /{name} 12 Tf (example) Tj ET").into_bytes());
            let mapping = page_mapping(&doc, page);
            assert_eq!(mapping.composite, guesses);
            assert!(mapping.certain());
            assert_eq!(mapping.guessing, guesses);
        }
    }

    /// The case the whole module exists for: Identity ordering, no `/ToUnicode`.
    ///
    /// This is `encodings.pdf` page 0 in miniature.
    #[test]
    fn identity_ordering_without_a_tounicode_is_a_guess() {
        let page = scan_one(composite("Identity-H", "Identity", false));
        assert_eq!(page.composite, 1);
        assert_eq!(page.guessing, 1);
        assert!(page.unreadable() && page.certain());
    }

    /// `encodings.pdf` page 2 in miniature: a collection PDFium has tables for.
    #[test]
    fn a_known_ordering_without_a_tounicode_is_not_a_guess() {
        let page = scan_one(composite("UniJIS-UCS2-H", "Japan1", false));
        assert_eq!(page.composite, 1);
        assert_eq!(page.guessing, 0);
        assert!(!page.unreadable() && page.certain());
    }

    /// The first half of the diagonal the corpus cannot reach.
    ///
    /// Identity-H **encoding** over a Japan1 **ordering**. A rule that keyed on
    /// the encoding name would call this a guess; it is not one, because the
    /// ordering is what supplies CID-to-Unicode. Both real fixtures have the two
    /// fields agreeing, so without this test that mutation survives.
    #[test]
    fn identity_encoding_over_a_known_ordering_is_not_a_guess() {
        let page = scan_one(composite("Identity-H", "Japan1", false));
        assert_eq!(page.guessing, 0, "the ordering decides, not the encoding");
    }

    /// The second half: a predefined **encoding** over an Identity **ordering**.
    ///
    /// The mirror image, and it fails the other way round --- a rule keyed on the
    /// encoding name would pass this and it is a guess.
    #[test]
    fn a_known_encoding_over_identity_ordering_is_a_guess() {
        let page = scan_one(composite("UniJIS-UCS2-H", "Identity", false));
        assert_eq!(page.guessing, 1, "the ordering decides, not the encoding");
    }

    /// A `/ToUnicode` settles it whatever the ordering says.
    #[test]
    fn a_tounicode_settles_it_even_over_identity_ordering() {
        let page = scan_one(composite("Identity-H", "Identity", true));
        assert_eq!(page.guessing, 0);
    }

    #[test]
    fn indirect_font_subtype_and_null_unicode_map_leave_identity_text_guessed() {
        let bytes = document_with_font(composite("Identity-H", "Identity", false));
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page = doc.get_pages()[&1];
        let fonts = crate::pagetree::resources(&doc, page)
            .unwrap()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap();
        let font = fonts.get(b"F1").unwrap().as_reference().unwrap();
        let subtype = doc.add_object(Object::Name(b"Type0".to_vec()));
        let null = doc.add_object(Object::Null);
        doc.get_dictionary_mut(font)
            .unwrap()
            .set("Subtype", subtype);
        assert_eq!(scan_from(&doc, 1).unwrap()[0].guessing, 1);
        doc.get_dictionary_mut(font).unwrap().set("ToUnicode", null);
        assert_eq!(scan_from(&doc, 1).unwrap()[0].guessing, 1);
        doc.get_dictionary_mut(font).unwrap().set("Subtype", null);
        assert!(scan_from(&doc, 1).unwrap()[0].truncated);
    }

    /// An absent `/Ordering` is the same position as `Identity`: no table.
    #[test]
    fn an_absent_ordering_is_a_guess() {
        let descendant = dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe") },
        };
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Dictionary(descendant)],
        };
        assert_eq!(scan_one(font).guessing, 1);
    }

    /// A simple font is never counted, whatever it declares.
    ///
    /// The control on the composite rule: a Type1 font with no `/ToUnicode` is
    /// the overwhelming majority of PDFs ever made, and a rule that flagged it
    /// would report the whole world as broken while passing every test above.
    #[test]
    fn a_simple_font_is_not_considered() {
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        };
        let page = scan_one(font);
        assert_eq!(page.composite, 0);
        assert!(!page.unreadable() && page.certain());
    }

    /// A page whose fonts cannot be reached is unknown, never clean.
    ///
    /// The `/Type0` resolves, and its `/DescendantFonts` points at an object
    /// that is not there. The answer must be "nobody knows", because a page
    /// reported clean on evidence nobody has is the failure this module was
    /// written to stop one level up.
    #[test]
    fn a_font_that_cannot_be_judged_is_reported_as_unknown() {
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "Encoding" => "Identity-H",
            "DescendantFonts" => Object::Reference((9999, 0)),
        };
        let page = scan_one(font);
        assert!(page.truncated, "an unreachable descendant must truncate");
        assert!(!page.certain(), "and certain() must therefore be false");
    }

    /// A page PDFium has and `lopdf` does not is unknown, not clean.
    ///
    /// This reproduces the shape without needing an encrypted fixture, which is
    /// the right way round: a short answer must not read as a clean one whether
    /// or not a document on disk currently produces one.
    ///
    /// It used to cite `incr-encrypted-pw.pdf` as "the real instance"; that was
    /// wrong on 2026-08-16, because PDFium would not open it at all, and is
    /// right again now that tpdf can ask for a password --- but only for a scan
    /// given no key. See the note on [`scan`], and
    /// `examples/password_probe.rs`, which measures the real instance through
    /// the worker.
    #[test]
    fn a_page_lopdf_cannot_account_for_is_unknown() {
        let bytes = document_with_font(composite("Identity-H", "Identity", false));
        let pages = scan(&bytes, 4, None).expect("the fixture must parse");
        assert_eq!(pages.len(), 4, "the answer is always page_count long");
        assert!(
            pages[0].unreadable(),
            "the page it did read is still judged"
        );
        for page in &pages[1..] {
            assert!(page.truncated, "a page lopdf never saw must be truncated");
            assert!(!page.certain(), "and therefore not certain");
            assert!(!page.unreadable(), "but not asserted broken either");
        }
    }

    /// And the opposite disagreement cannot produce a long answer.
    #[test]
    fn more_pages_in_the_file_than_asked_for_are_dropped() {
        let bytes = document_with_font(composite("Identity-H", "Identity", false));
        assert_eq!(scan(&bytes, 0, None).expect("parses").len(), 0);
    }
}
