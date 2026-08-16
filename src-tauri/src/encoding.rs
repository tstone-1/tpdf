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
//! Measured in release over the fixtures, because the first design here was
//! built on a guess: **0.1 ms** on a small document, **5.8 ms** on the 775-page
//! one, **11.9 ms** on the 337 MB scan. `lopdf` parses the cross-reference table
//! and object headers rather than every content stream, so the cost tracks the
//! **object count** and barely notices the byte count --- the 337 MB file is
//! mostly image data in very few objects.
//!
//! That is cheap enough that the original justification for computing it lazily
//! ("the dominant cost of opening a document") was simply false. It is still
//! computed lazily, for a reason that survives measurement: warm startup is
//! ~276 ms against a 300 ms target, so ~25 ms is the entire margin and 6--12 ms
//! of it is a quarter. Off the critical path this is free.
//!
//! ## Hostile input
//!
//! The same document as everywhere else. The walk is bounded in every dimension
//! that a file controls --- pages visited, fonts per page, and the resource-tree
//! recursion that `/Resources` inheritance requires --- and decompression is
//! bounded by the loader, as `print.rs` does it. A document that hits a bound is
//! reported as **unknown**, never as clean: this module's whole purpose is to
//! stop a reassuring answer being given on evidence nobody has.

use std::collections::HashSet;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};

/// Largest decompressed stream the scan will accept, matching `print.rs`.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Deepest the `/Resources` inheritance walk will follow `/Parent`.
///
/// A page tree is a few levels in any real document. This bounds a `/Parent`
/// cycle, which `lopdf` will happily follow forever.
const MAX_INHERIT: usize = 32;

/// Most fonts a single page's resource dictionary may declare.
const MAX_FONTS: usize = 4096;

/// CID orderings PDFium can map to Unicode without a `/ToUnicode`.
///
/// These are the registries it ships CID-to-Unicode tables for. Anything else ---
/// `Identity` above all --- leaves it reading glyph indices as character codes.
/// Compared case-sensitively because these are Adobe's own spellings and a
/// document using a different case is not naming one of these collections.
const MAPPABLE_ORDERINGS: [&str; 5] = ["Japan1", "GB1", "CNS1", "Korea1", "KR"];

/// What a page's fonts say about whether its text means anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageMapping {
    /// Composite fonts the page declares.
    pub composite: usize,
    /// Of those, how many leave PDFium guessing.
    pub guessing: usize,
    /// Whether a bound was hit, so `guessing` is a floor rather than a count.
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
/// **The example this used to give was half wrong, and the correction is worth
/// more than the example** (measured 2026-08-16). It read: *"the two parsers
/// disagree more often than one would like, and the disagreement is always in
/// the dangerous direction: `lopdf` reports zero pages for
/// `testdata/incr-encrypted-pw.pdf`, which PDFium opens and paginates
/// normally."* The first half is true --- `lopdf` loads that file and reports
/// **0** pages. The second is not: **PDFium refuses to open it at all**
/// (`RawDocument::open` fails, and `links-probe` exits 2 on it), because it is
/// AES-256 behind a real user password. So the two parsers never both see that
/// document, and it demonstrates no disagreement.
///
/// Swept across every `testdata/*.pdf` the same day: **the two parsers agree
/// about page count on every fixture PDFium will open.** `hostile-encrypted.pdf`
/// --- AES-256 with an *empty* user password, which opens with no prompt --- is
/// read by `lopdf` as 1 page, exactly as PDFium paginates it.
///
/// The design is unchanged and still right: two independent parsers with no
/// guarantee of agreeing is reason enough to take the count from the one whose
/// pagination the reader is actually looking at. What changed is that the guard
/// is **defensive rather than demonstrated**, and saying so is the difference
/// between a bound with a known instance and one without.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`].
pub fn scan(bytes: &[u8], page_count: usize) -> Result<Vec<PageMapping>, String> {
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse the document: {e}"))?;

    let mut pages: Vec<PageMapping> = document
        .get_pages()
        .values()
        .take(page_count)
        .map(|id| page_mapping(&document, *id))
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

/// Examines one page's fonts.
fn page_mapping(document: &Document, page: ObjectId) -> PageMapping {
    let mut mapping = PageMapping::default();

    let Some(fonts) = font_dictionary(document, page, &mut mapping) else {
        return mapping;
    };

    for (index, (_, font)) in fonts.iter().enumerate() {
        if index >= MAX_FONTS {
            mapping.truncated = true;
            break;
        }
        let Ok(font) = resolve_dict(document, font) else {
            // A font entry that does not resolve is a font we cannot judge.
            // Unknown, not clean.
            mapping.truncated = true;
            continue;
        };
        if !is_composite(font) {
            continue;
        }
        mapping.composite += 1;
        match guesses(document, font) {
            Some(true) => mapping.guessing += 1,
            Some(false) => {}
            None => mapping.truncated = true,
        }
    }

    mapping
}

/// The page's `/Font` resource dictionary, following `/Resources` inheritance.
///
/// `/Resources` is inheritable, so a page may carry none and take its parent's.
/// `lopdf` has `get_inherited_page_property`, and it is not used here because it
/// does not bound the walk --- a `/Parent` cycle is a hostile document's cheapest
/// trick and this is the walk it would spin.
fn font_dictionary(
    document: &Document,
    page: ObjectId,
    mapping: &mut PageMapping,
) -> Option<Dictionary> {
    let mut node = page;
    let mut seen: HashSet<ObjectId> = HashSet::new();

    for _ in 0..MAX_INHERIT {
        if !seen.insert(node) {
            // A cycle. Stop, and say the answer is partial.
            mapping.truncated = true;
            return None;
        }
        let Ok(dict) = document.get_dictionary(node) else {
            mapping.truncated = true;
            return None;
        };

        if let Ok(resources) = dict.get(b"Resources") {
            if let Ok(resources) = resolve_dict(document, resources) {
                if let Ok(fonts) = resources.get(b"Font") {
                    return resolve_dict(document, fonts).ok().cloned();
                }
                // Resources with no /Font: the page draws no text with a font it
                // names here. Not a truncation.
                return None;
            }
            mapping.truncated = true;
            return None;
        }

        match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => node = *parent,
            _ => return None,
        }
    }

    mapping.truncated = true;
    None
}

/// Whether a font dictionary is a composite (`/Type0`) font.
fn is_composite(font: &Dictionary) -> bool {
    font.get(b"Subtype")
        .and_then(Object::as_name)
        .map(|name| name == b"Type0")
        .unwrap_or(false)
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
    if font.get(b"ToUnicode").is_ok() {
        return Some(false);
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
        .and_then(|object| object.as_str().ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    /// Builds a one-page document whose single font is the dictionary given.
    ///
    /// Synthetic rather than a fixture on disk, because what is being tested is a
    /// rule over four combinations of two fields and only two of them occur in
    /// any real document we have. A generated PDF is the cheapest way to reach
    /// the other two, and it needs no font program: nothing here renders.
    fn document_with_font(font: Dictionary) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
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
            font.set("ToUnicode", Object::Null);
        }
        font
    }

    fn scan_one(font: Dictionary) -> PageMapping {
        let bytes = document_with_font(font);
        let pages = scan(&bytes, 1).expect("the fixture must parse");
        assert_eq!(pages.len(), 1, "the fixture is one page");
        pages[0]
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
    /// It used to cite `incr-encrypted-pw.pdf` as "the real instance", and that
    /// was half wrong --- `lopdf` does report zero pages for it, and PDFium
    /// **refuses to open it**, so the two never both see it. See the correction
    /// on [`scan`].
    #[test]
    fn a_page_lopdf_cannot_account_for_is_unknown() {
        let bytes = document_with_font(composite("Identity-H", "Identity", false));
        let pages = scan(&bytes, 4).expect("the fixture must parse");
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
        assert_eq!(scan(&bytes, 0).expect("parses").len(), 0);
    }
}
