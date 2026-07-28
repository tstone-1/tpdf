//! Handing a print job to macOS.
//!
//! `print.rs` decides *which* PDF; this decides nothing at all. PDFKit
//! paginates the document and builds the `NSPrintOperation`, AppKit runs the
//! panel, and CUPS converts if the printer needs it --- measured to be a copy
//! when it does not (docs/PLAN.md, Phase 1 → Print).
//!
//! The part worth arguing for is [`read`], which is not on the printing path at
//! all. PDFKit sits on CoreGraphics, so it is a **third** parser --- independent
//! of `lopdf`, which wrote the job, and of PDFium, which drew what the reader
//! was looking at --- and it is the one the print system will itself use. Asking
//! it what it sees before opening a panel turns "the subset saved without an
//! error" into "something else can read the pages we claim to have produced",
//! which is the same standard `docs/PLAN.md` §6 sets for a redaction and the
//! same reason spike 0.4 re-parsed with `qpdf`. It found nothing here; it is
//! cheap, and a rewrite that only its own writer can read is a defect that no
//! amount of internal checking can see.
//!
//! Everything below is `unsafe` because the bindings are, and each call is
//! sound for the same reason: the receiver is a live `Retained` object and the
//! arguments outlive the call. The one genuine requirement is the main thread,
//! and that is carried in the type --- `MainThreadMarker` cannot be forged.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::NSPrintInfo;
use objc2_foundation::{NSData, NSString};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

/// What PDFKit makes of one page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageReading {
    /// The page's rotation in degrees, `/Rotate` and everything it inherits.
    pub rotation: i64,
    /// The page's text, and `None` when it was not asked for.
    ///
    /// Deliberately not a plain `String`: "we did not extract this" and "this
    /// page has no extractable text" are different facts, and an empty string
    /// for both is the same defect as a leak scanner that cannot decode a
    /// carrier reporting *clean*.
    pub text: Option<String>,
}

/// What PDFKit makes of a whole document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub pages: Vec<PageReading>,
}

/// Parses PDF bytes with PDFKit and reads the document's *shape*.
///
/// Page count and rotations only, which is everything the pre-print check
/// needs. Text is left out because extracting it is the expensive half by an
/// order of magnitude and more: on the 775-page corpus, release build, this
/// returns in **62 ms** where [`read_with_text`] takes **1,017 ms** --- a second
/// of pure waste in front of a print panel, to fill a field only the tests read.
/// On the twelve A0 pages of `vector-multi` it is 0.6 ms against 467 ms.
///
/// `None` means PDFKit refused the document outright. Note it is deliberately
/// lenient, as every shipping PDF parser is, so this proves the file is
/// *readable* and not that it is well formed --- the same distinction AGENTS.md
/// draws about PDFium rendering a document whose `/Size` is wrong.
#[must_use]
pub fn read(bytes: &[u8]) -> Option<Reading> {
    read_inner(bytes, false)
}

/// As [`read`], and every page's text.
///
/// For checks that need to know *which* pages survived a subset rather than how
/// many. Not for the print path --- see the cost above.
#[must_use]
pub fn read_with_text(bytes: &[u8]) -> Option<Reading> {
    read_inner(bytes, true)
}

/// Shared body of [`read`] and [`read_with_text`].
fn read_inner(bytes: &[u8], with_text: bool) -> Option<Reading> {
    let document = parse(bytes)?;
    let count = unsafe { document.pageCount() };
    let mut pages = Vec::with_capacity(count);
    for index in 0..count {
        let Some(page) = (unsafe { document.pageAtIndex(index) }) else {
            // A page the document counted and cannot produce. Reporting a short
            // list would read as a shorter document rather than a broken one.
            return None;
        };
        pages.push(PageReading {
            rotation: unsafe { page.rotation() } as i64,
            text: with_text.then(|| {
                unsafe { page.string() }
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            }),
        });
    }
    Some(Reading { pages })
}

/// Opens the system print panel for these bytes.
///
/// Returns whether the job was sent. `false` covers **both** a cancelled panel
/// and a failed job: `runOperation` reports one boolean and AppKit does not
/// distinguish them, so treating it as an error would put a failure in front of
/// someone who pressed Cancel.
///
/// # Errors
///
/// PDFKit refusing the document, or declining to build an operation for it.
pub fn present(bytes: &[u8], title: &str, mtm: MainThreadMarker) -> Result<bool, String> {
    let document = parse(bytes).ok_or("PDFKit could not read the print job")?;
    let info = NSPrintInfo::sharedPrintInfo();

    // PDFKit auto-rotates a page to fill the sheet. On a page that carries a
    // rotation that would spin it back --- silently undoing the turn the reader
    // asked for, which `print.rs` went to the trouble of composing onto the
    // page's inherited `/Rotate`. So it is offered only where there is no
    // rotation to discard. A document already rotated on disk is
    // indistinguishable from one the reader turned, and this errs towards
    // leaving it as it is.
    //
    // Scaled down to fit either way, or an A0 sheet on A4 paper prints its
    // top-left corner and nothing else.
    //
    // Neither half of that is verified. Both need paper.
    let upright = (0..unsafe { document.pageCount() })
        .filter_map(|index| unsafe { document.pageAtIndex(index) })
        .all(|page| unsafe { page.rotation() }.rem_euclid(360) == 0);

    let operation = unsafe {
        document.printOperationForPrintInfo_scalingMode_autoRotate(
            Some(&info),
            PDFPrintScalingMode::PageScaleDownToFit,
            upright,
            mtm,
        )
    }
    .ok_or("PDFKit would not build a print operation for this document")?;

    operation.setJobTitle(Some(&NSString::from_str(title)));
    operation.setShowsPrintPanel(true);
    operation.setShowsProgressPanel(true);
    Ok(operation.runOperation())
}

/// Loads bytes into a `PDFDocument`.
fn parse(bytes: &[u8]) -> Option<Retained<PDFDocument>> {
    let data = NSData::with_bytes(bytes);
    unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }
}

#[cfg(test)]
mod tests {
    use super::read;
    use lopdf::dictionary;

    /// The smallest thing PDFKit will accept, so the tests below have a control
    /// proving the reader answers at all.
    fn minimal() -> Vec<u8> {
        let mut doc = lopdf::Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
        });
        doc.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => 1_i64,
                "Kids" => vec![lopdf::Object::Reference(page_id)],
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save");
        out
    }

    #[test]
    fn pdfkit_reads_a_document_we_wrote() {
        let reading = read(&minimal()).expect("PDFKit refused a document it should accept");
        assert_eq!(reading.pages.len(), 1);
    }

    #[test]
    fn pdfkit_refuses_bytes_that_are_not_a_pdf() {
        // Without this the check above is satisfied by a reader that says yes to
        // everything, which is exactly what an independent oracle must not be.
        assert_eq!(read(b"this is not a PDF, and never was"), None);
    }

    #[test]
    fn pdfkit_refuses_a_truncated_document() {
        let whole = minimal();
        assert_eq!(read(&whole[..whole.len() / 2]), None);
    }
}
