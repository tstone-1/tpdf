//! QR codes and other 2D codes in a redacted file, and whether they still carry
//! what the redaction removed.
//!
//! **A code is a carrier the other two readers cannot see.** The byte scan
//! (`verify.rs`) reads the file's bytes, and a code is a picture or a grid of
//! filled rectangles that spells nothing in them. The OCR gate (`ocr_gate.rs`)
//! reads the marked areas, and an invoice's payment code sits beside the name it
//! carries, not under it: an EPC QR code ("GiroCode") holds the payee's name, the
//! IBAN and the amount, a Swiss QR-bill the full address, a rail ticket's Aztec
//! code the traveller's name. Marking the printed name and leaving the code
//! removes nothing, and until this module the result could read *verified*.
//!
//! **It reads, compares and says; it never shows or opens what a code holds.**
//! A decoded payload is a string from a stranger --- usually a URL --- and the
//! only thing that leaves this module is a sentence naming the removed term a
//! code still carries, which the reader already knows. So this is a check inside
//! the redaction's verification and nothing else: there is no command that
//! reads a code.
//!
//! The decode runs in the worker, on pixels PDFium drew there ([`read_page`]);
//! only the decoded strings cross, bounded by [`MAX_CODES`] and [`MAX_TEXT`].
//! The comparison ([`reasons`]) runs in the app, against the words the
//! redaction removed.
//!
//! **What it cannot see, said here rather than discovered.** rxing reports the
//! codes it decoded and nothing about a code it found and could not decode, so a
//! damaged, tiny or partly covered code adds no reason; a clean result says how
//! many codes were read ([`read_note`]), which is what a reader holds against
//! the codes on the page. The formats are the four 2D
//! ones that carry text ([`FORMATS`]), and a linear barcode is not read. A code
//! whose content is not text --- a ticket's compressed Aztec payload --- cannot be
//! compared, and is a reason of its own rather than a pass.

use std::collections::HashSet;

/// The formats read: the four 2D codes that carry free text.
///
/// Linear barcodes are left out: an EAN or a Code 128 on a document is a product
/// or a parcel number far more often than a name, and reading them would make
/// every invoice with an article number a candidate for a false finding.
pub const FORMATS: [rxing::BarcodeFormat; 4] = [
    rxing::BarcodeFormat::QR_CODE,
    rxing::BarcodeFormat::AZTEC,
    rxing::BarcodeFormat::DATA_MATRIX,
    rxing::BarcodeFormat::PDF_417,
];

/// The most codes one page reports. A page of labels can hold dozens; a page
/// holding more than this is answered with the first ones and a note that the
/// rest were not compared, so the reply stays bounded.
pub const MAX_CODES: usize = 64;

/// The most characters kept of one code's content. A QR code holds at most
/// 7,089 digits; an EPC code is under 331 bytes. Longer text is cut, and a cut
/// code is compared on what was kept.
pub const MAX_TEXT: usize = 8_192;

/// The most pixels the page is drawn at, on its longer side. Three times an A4
/// sheet's 842 pt, which puts a 1 mm module of a payment code at about 8.5 px.
pub const PAGE_SIDE: f32 = 2_600.0;

/// The scale ceiling, in pixels per point. Past 3, a page's area grows with the
/// square and the smallest codes on documents are already several pixels a module.
pub const MAX_SCALE: f32 = 3.0;

/// The scale for a page `width_pt` by `height_pt`.
#[must_use]
pub fn scale_for(width_pt: f32, height_pt: f32) -> f32 {
    let longer = width_pt.max(height_pt).max(1.0);
    MAX_SCALE.min(PAGE_SIDE / longer)
}

/// One code found on a page.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Code {
    /// What kind of code, in words: `QR code`, `Aztec code`, ...
    pub kind: String,
    /// What it holds, cut to [`MAX_TEXT`] characters.
    pub text: String,
    /// Whether the content read as text. A ticket's compressed payload decodes
    /// to bytes, and comparing words against bytes would pass by accident.
    pub plain: bool,
}

/// What one page's read found.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageCodes {
    /// The codes, at most [`MAX_CODES`].
    pub codes: Vec<Code>,
    /// Whether more were found than were kept.
    pub more: bool,
}

/// The code's kind, as a reader would name it.
///
/// A `said!` each, because the kind is put inside a reason and the window
/// translates the two together.
fn kind_of(format: &rxing::BarcodeFormat) -> String {
    match format {
        rxing::BarcodeFormat::QR_CODE => said!("QR code"),
        rxing::BarcodeFormat::AZTEC => said!("Aztec code"),
        rxing::BarcodeFormat::DATA_MATRIX => said!("Data Matrix code"),
        rxing::BarcodeFormat::PDF_417 => said!("PDF417 code"),
        _ => said!("two-dimensional code"),
    }
}

/// Whether a decoded string is text: no control character but tab and line
/// ends, and no replacement character. Bytes decoded as Latin-1 put C0 and C1
/// controls in the string, which is how compressed content shows.
#[must_use]
pub fn is_plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c == '\u{FFFD}' || (c.is_control() && !matches!(c, '\t' | '\n' | '\r')))
}

/// Grey values from RGBA pixels, by the Rec. 601 weights. `rgba` shorter than
/// `width * height * 4` yields fewer values, which the decoder then refuses.
#[must_use]
pub fn luma_of_rgba(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|p| {
            let grey =
                (299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])) / 1000;
            #[allow(clippy::cast_possible_truncation)]
            let grey = grey.min(255) as u8;
            grey
        })
        .collect()
}

/// Every code rxing decodes in a grey image of `width` by `height`.
///
/// # Errors
///
/// When the image is empty or `luma` does not hold `width * height` values.
/// Finding nothing is `Ok` with no codes, never an error: rxing reports
/// "not found" as an error, and that is translated here.
pub fn read(luma: Vec<u8>, width: u32, height: u32) -> Result<PageCodes, String> {
    let wanted = u64::from(width) * u64::from(height);
    if wanted == 0 || luma.len() as u64 != wanted {
        return Err(said!(
            "the page image is {got} values for {width} x {height}",
            got = luma.len()
        ));
    }
    let mut hints = rxing::DecodeHints {
        PossibleFormats: Some(FORMATS.iter().copied().collect::<HashSet<_>>()),
        TryHarder: Some(true),
        ..rxing::DecodeHints::default()
    };
    let found =
        match rxing::helpers::detect_multiple_in_luma_with_hints(luma, width, height, &mut hints) {
            Ok(found) => found,
            Err(rxing::Exceptions::NotFoundException(_)) => Vec::new(),
            Err(e) => return Err(said!("the codes on the page could not be read: {e}")),
        };
    let more = found.len() > MAX_CODES;
    let codes = found
        .iter()
        .take(MAX_CODES)
        .map(|result| {
            let text: String = result.getText().chars().take(MAX_TEXT).collect();
            Code {
                kind: kind_of(result.getBarcodeFormat()),
                plain: is_plain(&text),
                text,
            }
        })
        .collect();
    Ok(PageCodes { codes, more })
}

/// Lower case, letters and digits only: `DE89 3704 0044` and `de89370400440`
/// are one IBAN, and a name keeps its letters across a line break in a payload.
#[must_use]
pub fn folded(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The shortest word of a longer removed text compared on its own. Three
/// letters match by chance in a payload of a few hundred characters; four
/// rarely do, and a region over `Jane Doe, Main Street 5` should not report a
/// code because it holds `doe` somewhere in an unrelated word.
pub const MIN_TERM: usize = 4;

/// The shortest removed text compared as a whole. A reader who removed `Doe`
/// meant it, and a false finding is the safe direction: it withholds
/// *verified*, where a skipped comparison would grant it. One character is not
/// compared --- every payment code holds a `5` --- and that gap is the one this
/// check has by rule rather than by reach.
pub const MIN_WHOLE: usize = 2;

/// What a removed text is looked for as: the whole of it, folded, from
/// [`MIN_WHOLE`] characters, and each of its words of [`MIN_TERM`] characters
/// or more, folded. A region over a name and a street removes `Jane Doe, Main
/// Street 5`; the code holds the name on a line of its own, so the words are
/// what find it.
#[must_use]
pub fn terms_of(needle: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let whole = folded(needle);
    if whole.chars().count() >= MIN_WHOLE {
        out.push(whole);
    }
    for word in needle.split(|c: char| !c.is_alphanumeric()) {
        let word = folded(word);
        if word.chars().count() >= MIN_TERM && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// The sentences one page's codes add to a redaction's reasons.
///
/// A code is a finding when its folded content holds a folded term of a removed
/// text, and it names the term as the reader removed it --- the first removed
/// text that holds it --- never the code's content. A code that is not text is a
/// finding of its own, because nothing could be compared. A code that holds none
/// of the removed words adds nothing: what else it carries is the document's,
/// and the redaction did not ask about it.
#[must_use]
pub fn reasons(page: u32, found: &PageCodes, needles: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    // Nothing was removed as text --- a region over a picture or a drawing ---
    // so there is nothing a code could still carry, and a code whose content
    // is not text is no reason either.
    if needles.is_empty() {
        return out;
    }
    let terms: Vec<(String, &str)> = needles
        .iter()
        .flat_map(|needle| {
            terms_of(needle)
                .into_iter()
                .map(move |term| (term, needle.as_str()))
        })
        .collect();
    for code in &found.codes {
        if !code.plain {
            out.push(said!(
                "page {n} has a {kind} whose content is not plain text, so whether it still \
                 carries what was removed could not be checked",
                n = page + 1,
                kind = code.kind
            ));
            continue;
        }
        let content = folded(&code.text);
        let mut carried: Vec<&str> = Vec::new();
        for (term, needle) in &terms {
            if content.contains(term.as_str()) && !carried.iter().any(|seen| seen == needle) {
                carried.push(needle);
            }
        }
        if let Some(first) = carried.first() {
            let others = carried.len() - 1;
            out.push(if others == 0 {
                said!(
                    "page {n} has a {kind} that still carries \"{first}\", which the redaction \
                     removed; redact the code too",
                    n = page + 1,
                    kind = code.kind
                )
            } else {
                said!(
                    "page {n} has a {kind} that still carries \"{first}\" and {others} more of \
                     what the redaction removed; redact the code too",
                    n = page + 1,
                    kind = code.kind
                )
            });
        }
    }
    if found.more {
        out.push(said!(
            "page {n} has more than {MAX_CODES} codes, and the ones after those were not compared \
             with what was removed",
            n = page + 1
        ));
    }
    out
}

/// What a clean verdict says about the codes: how many were read in the file.
///
/// rxing reports the codes it decoded and nothing about one it could not, so
/// *verified* cannot mean "no code carries the name"; it means "none of the
/// codes read does". The count is what lets a reader see the difference: a
/// page showing a code, beside a count of 0, is a code nobody compared.
///
/// Two sentences, because "0 codes, none carrying the name" reads as a check
/// that passed when it is a check that had nothing to look at.
#[must_use]
pub fn read_note(codes: usize) -> String {
    if codes == 0 {
        said!(
            "no QR code or other 2D code was read in the file; if a page shows one, it was not \
             compared with what was removed"
        )
    } else {
        said!(
            "2D codes read in the file and compared with what was removed: {codes}; if the \
             pages show more, the others were not compared"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        folded, is_plain, luma_of_rgba, read, read_note, reasons, scale_for, terms_of, Code,
        PageCodes,
    };

    #[test]
    fn the_note_beside_a_clean_verdict_carries_the_count() {
        assert!(read_note(0).starts_with("no QR code or other 2D code was read"));
        assert!(read_note(3).contains("compared with what was removed: 3;"));
    }

    /// A code drawn black on white, `module` pixels a module, with a quiet zone,
    /// as a grey image. Drawn by rxing's own encoder, which only the tests link.
    fn drawn(text: &str, format: &rxing::BarcodeFormat, module: u32) -> (Vec<u8>, u32, u32) {
        use rxing::Writer;
        let matrix = rxing::MultiFormatWriter
            .encode(text, format, 0, 0)
            .expect("the test's code encodes");
        let (w, h) = (matrix.getWidth(), matrix.getHeight());
        let quiet = 8 * module;
        let (width, height) = (w * module + 2 * quiet, h * module + 2 * quiet);
        let mut luma = vec![255u8; (width * height) as usize];
        for y in 0..h {
            for x in 0..w {
                if matrix.get(x, y) {
                    for dy in 0..module {
                        for dx in 0..module {
                            let px = quiet + x * module + dx;
                            let py = quiet + y * module + dy;
                            luma[(py * width + px) as usize] = 0;
                        }
                    }
                }
            }
        }
        (luma, width, height)
    }

    /// An EPC "GiroCode" payment payload, as German invoices print it.
    const EPC: &str = "BCD\n002\n1\nSCT\nCOBADEFFXXX\nJane Doe\nDE89370400440532013000\nEUR123.45\n\n\nInvoice 2026-117\n";

    fn page_with(text: &str) -> PageCodes {
        PageCodes {
            codes: vec![Code {
                kind: "QR code".into(),
                text: text.into(),
                plain: is_plain(text),
            }],
            more: false,
        }
    }

    #[test]
    fn reads_a_payment_code_drawn_at_a_few_pixels_a_module() {
        let (luma, width, height) = drawn(EPC, &rxing::BarcodeFormat::QR_CODE, 4);
        let found = read(luma, width, height).expect("a clean image reads");
        assert_eq!(found.codes.len(), 1, "{found:?}");
        assert_eq!(found.codes[0].text, EPC);
        assert_eq!(found.codes[0].kind, "QR code");
        assert!(found.codes[0].plain);
    }

    #[test]
    fn reads_the_other_three_formats() {
        for format in [
            rxing::BarcodeFormat::AZTEC,
            rxing::BarcodeFormat::DATA_MATRIX,
            rxing::BarcodeFormat::PDF_417,
        ] {
            let (luma, width, height) = drawn("Jane Doe 1974-05-02", &format, 4);
            let found = read(luma, width, height).expect("a clean image reads");
            assert_eq!(found.codes.len(), 1, "{format:?}: {found:?}");
            assert_eq!(found.codes[0].text, "Jane Doe 1974-05-02", "{format:?}");
        }
    }

    #[test]
    fn a_page_with_no_code_is_an_empty_answer_and_not_an_error() {
        let found = read(vec![255; 200 * 300], 200, 300).expect("nothing found is an answer");
        assert!(found.codes.is_empty());
        assert!(!found.more);
    }

    #[test]
    fn an_image_of_the_wrong_length_is_refused() {
        assert!(read(vec![255; 10], 200, 300).is_err());
        assert!(read(Vec::new(), 0, 0).is_err());
    }

    #[test]
    fn grey_from_rgba() {
        assert_eq!(
            luma_of_rgba(&[0, 0, 0, 255, 255, 255, 255, 255]),
            vec![0, 255]
        );
        assert_eq!(luma_of_rgba(&[255, 0, 0, 255]), vec![76]);
    }

    #[test]
    fn folding_joins_an_iban_written_in_groups() {
        assert_eq!(
            folded("DE89 3704 0044 0532 0130 00"),
            "de89370400440532013000"
        );
        assert_eq!(folded("Jane\nDoe"), "janedoe");
    }

    #[test]
    fn terms_are_the_whole_and_the_longer_words() {
        assert_eq!(
            terms_of("Jane Doe, Main Street 5"),
            vec!["janedoemainstreet5", "jane", "main", "street"]
        );
        assert_eq!(
            terms_of("Doe"),
            vec!["doe"],
            "a short removal is compared whole"
        );
        assert_eq!(terms_of("5"), Vec::<String>::new(), "one character is not");
    }

    #[test]
    fn a_short_removed_name_is_still_found() {
        let said = reasons(0, &page_with(EPC), &["Doe".to_owned()]);
        assert_eq!(said.len(), 1, "{said:?}");
    }

    #[test]
    fn nothing_removed_as_text_means_nothing_to_say() {
        assert!(reasons(0, &page_with("x\u{1}\u{9c}"), &[]).is_empty());
    }

    #[test]
    fn a_code_carrying_a_removed_name_is_a_reason_naming_it() {
        let said = reasons(0, &page_with(EPC), &["Jane Doe".to_owned()]);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("page 1 has a QR code"), "{said:?}");
        assert!(said[0].contains("\"Jane Doe\""), "{said:?}");
    }

    #[test]
    fn an_iban_removed_in_groups_is_found_in_the_code_without_them() {
        let said = reasons(
            2,
            &page_with(EPC),
            &["DE89 3704 0044 0532 0130 00".to_owned()],
        );
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("page 3 has a QR code"), "{said:?}");
    }

    #[test]
    fn a_wider_region_finds_the_name_by_its_words() {
        let said = reasons(0, &page_with(EPC), &["Jane Doe, Main Street 5".to_owned()]);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("\"Jane Doe, Main Street 5\""), "{said:?}");
    }

    #[test]
    fn several_removed_texts_in_one_code_are_one_sentence() {
        let needles = ["Jane Doe".to_owned(), "DE89370400440532013000".to_owned()];
        let said = reasons(0, &page_with(EPC), &needles);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("and 1 more"), "{said:?}");
    }

    #[test]
    fn a_code_holding_none_of_the_removed_words_adds_nothing() {
        assert!(reasons(0, &page_with(EPC), &["Richard Roe".to_owned()]).is_empty());
        assert!(reasons(0, &page_with(EPC), &[]).is_empty());
    }

    #[test]
    fn the_content_itself_is_never_in_the_sentence() {
        let said = reasons(0, &page_with(EPC), &["Jane Doe".to_owned()]);
        assert!(
            !said[0].contains("COBADEFF") && !said[0].contains("123.45"),
            "{said:?}"
        );
    }

    #[test]
    fn content_that_is_not_text_cannot_pass() {
        let binary = "x\u{1}\u{9c}\u{0}abc";
        assert!(!is_plain(binary));
        assert!(is_plain(EPC), "line ends are text");
        let said = reasons(0, &page_with(binary), &["Jane Doe".to_owned()]);
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("not plain text"), "{said:?}");
    }

    #[test]
    fn a_page_with_more_codes_than_kept_says_so() {
        let page = PageCodes {
            codes: Vec::new(),
            more: true,
        };
        assert_eq!(reasons(0, &page, &["Jane Doe".to_owned()]).len(), 1);
    }

    #[test]
    fn the_scale_keeps_a_page_within_the_side_and_the_ceiling() {
        assert!((scale_for(595.0, 842.0) - 3.0).abs() < 1e-6);
        let a0 = scale_for(2384.0, 3370.0);
        assert!(3370.0 * a0 <= super::PAGE_SIDE + 0.5, "{a0}");
    }
}
