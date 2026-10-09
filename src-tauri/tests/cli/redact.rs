//! `tpdf redact` against real workers and real documents.
//!
//! Part of `tests/cli.rs`'s binary, which is its own worker --- and, for this
//! file, its own OCR worker too, since the application's redaction path runs
//! the OCR gate. The helpers it uses (`tool`, `fixture`, `scratch`) are that
//! file's.
//!
//! Five things, each with the control that keeps it from passing vacuously:
//!
//! 1. **What it finds is gone and what it does not find is not.** A document
//!    built here carries an e-mail address, an IBAN, a code word on two pages
//!    (one of them cropped) and a line reachable only by a rectangle, beside two
//!    `CONTROL-KEEP` lines nothing matches. By `--text`, by `--pattern`, by
//!    `--regions` and all three at once, the written file's text --- read by
//!    the built tool, by Poppler's `pdftotext`, and byte for byte through
//!    `qpdf --qdf` --- holds none of the matched strings and both controls.
//!    Control: the same three readers find every matched string in the input.
//! 2. **`--dry-run` writes nothing, and says what the write then does**: its
//!    regions, removals and pages equal the real run's.
//! 3. **The verdict is the application's.** For three documents, the regions
//!    the tool marks are marked again in this process and handed to
//!    `redaction::redact_copy_asked` --- the body of the window's *Redact and
//!    save as* --- and the tool's `verified` and every reason equal what that
//!    returns, and its exit code is 0 exactly when that is verified.
//!    `text-marked.pdf` is the fixture the application reports *not verified*
//!    for a reason that is not the platform's: an annotation keeps a copy of the
//!    removed line. Its exit code is 1, and the file is kept.
//! 4. **Refusals write nothing**: a signed document without
//!    `--invalidate-signatures` (and with it, the copy's signature no longer
//!    verifies), an XFA form, an output that exists, an output that is the
//!    input under another name, a locked document without its password (and
//!    with it, the copy stays encrypted).
//! 5. With `TPDF_REDACT_PROBE=<dir>`, the built document, its redacted copy and
//!    the strings expected gone and kept are left there for pypdf
//!    (`scripts/redact_pdf_check.py`).

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{dictionary, Document, Object, Stream};
use tpdf_lib::cli::redact::{match_regions, search_pages};
use tpdf_lib::cli::report::SearchKind;
use tpdf_lib::render::{Backend, RenderService};
use tpdf_lib::search::{Options, Prepared};

use super::{fixture, library_dir, scratch, strings, tool, Report};

/// Each line of each page: the words `contacts_pdf` draws, one text object a
/// line, at `(x, y)` in points up from the bottom.
const PAGE_ONE: [(&str, i64); 5] = [
    ("Contact: jane.doe@example.com", 700),
    ("IBAN DE89 3704 0044 0532 0130 00 for payroll", 670),
    ("CONTROL-KEEP alpha", 640),
    ("Codeword Rumpelstilzchen", 610),
    ("Sign here: J. Doe", 580),
];
const PAGE_TWO: [(&str, i64); 3] = [
    ("Second copy: jane.doe@example.com", 700),
    ("CONTROL-KEEP beta", 670),
    ("Codeword Rumpelstilzchen", 640),
];

/// The e-mail pattern the README shows.
pub(super) const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
/// The IBAN-shaped pattern the README shows.
pub(super) const IBAN: &str = r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){3,7}(?: ?[A-Z0-9]{1,3})?\b";

/// Two pages of Helvetica, each line its own `BT ... ET`, so a removal that
/// takes a whole operation takes one line and never its neighbour. The second
/// page has a crop box that does not start at the sheet's corner, which is
/// what makes `--regions`' "as displayed" a claim that can be wrong.
pub(super) fn contacts_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for (lines, crop) in [
        (&PAGE_ONE[..], None),
        (&PAGE_TWO[..], Some([36, 36, 576, 756])),
    ] {
        let mut body = String::new();
        for (text, y) in lines {
            body.push_str(&format!("BT /F1 12 Tf 72 {y} Td ({text}) Tj ET\n"));
        }
        let content = doc.add_object(Stream::new(dictionary! {}, body.into_bytes()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
        };
        if let Some(crop) = crop {
            page.set(
                "CropBox",
                crop.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
            );
        }
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// Two pages whose content has show operators that draw nothing, on the page
/// and inside a Form XObject.
///
/// A removal addresses the nth text object PDFium reports as the nth show
/// operator, and PDFium makes no object of a show without text. Page 1 holds
/// the well-formed shapes, which a phone company's invoices have on every
/// page. Page 2 holds the malformed ones, and beside them the nearest shapes
/// that do make an object. If the pinned engine counted any of them the other
/// way, the two counts would differ and the removal would refuse.
///
/// The malformed shapes are on a page of their own because `pdftotext` reads
/// nothing at all from a page that has them, and page 1 is what the other
/// readers are asked about.
///
/// Every secret has an empty show **before** it in its own stream. With the
/// empty shows only after the words, a count that ignored them and one that
/// did not would address the same operator.
fn empty_shows_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = dictionary! { "Font" => dictionary! { "F1" => font } };
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 400.into(), 80.into()],
            "Resources" => resources.clone(),
        },
        b"BT /F1 12 Tf 14 TL 0 40 Td () Tj (FORM-SECRET-7731) Tj () ' (FORM-KEEP delta) Tj ET"
            .to_vec(),
    ));
    let mut with_form = resources.clone();
    with_form.set("XObject", dictionary! { "Fm0" => form });
    let one = "BT /F1 12 Tf 14 TL 72 100 Td () Tj <> Tj [] TJ [-200] TJ [()] TJ \
               [() -200 ()] TJ () ' 0 0 () \" ET\n\
               BT /F1 12 Tf 72 700 Td (CONTROL-KEEP alpha) Tj ET\n\
               BT /F1 12 Tf 72 680 Td () Tj (Codeword Rumpelstilzchen) Tj ET\n\
               BT /F1 12 Tf 72 660 Td (CONTROL-KEEP beta) Tj () Tj ET\n\
               q 1 0 0 1 72 500 cm /Fm0 Do Q\n";
    let two = "BT /F1 12 Tf 72 100 Td [/N] TJ (x) TJ 5 Tj [(x)] Tj Tj (x) () Tj ET\n\
               BT /F1 12 Tf 72 60 Td /N Tj ( ) Tj () (C) Tj [() -200 (D)] TJ ET\n\
               BT /F1 12 Tf 72 700 Td (CONTROL-KEEP gamma) Tj ET\n\
               BT /F1 12 Tf 72 680 Td 5 Tj (MALFORMED-SECRET-5512) Tj ET\n";
    let mut kids = Vec::new();
    for (body, resources) in [(one, with_form), (two, resources)] {
        let content = doc.add_object(Stream::new(dictionary! {}, body.as_bytes().to_vec()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
        })));
    }
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// A page with empty show operators is redacted, and the right lines go.
pub(super) fn empty_shows_are_passed_over(report: &mut Report) {
    let dir = scratch("redact-empty-shows");
    let input = dir.join("empty-shows.pdf");
    std::fs::write(&input, empty_shows_pdf()).expect("fixture");
    let secrets = ["Rumpelstilzchen", "FORM-SECRET-7731"];
    let kept = ["CONTROL-KEEP alpha", "CONTROL-KEEP beta", "FORM-KEEP delta"];
    // Page 2, which only tpdf's own extraction is asked about.
    let (odd_secret, odd_kept) = ("MALFORMED-SECRET-5512", "CONTROL-KEEP gamma");

    let before = readers(&input, &dir, &[], &[&secrets[..], &kept[..]].concat());
    let odd_before = text_of(&input, &[], &[]);
    report.check(
        &format!("control: {} find every string in the input", installed()),
        before.is_empty() && odd_before.contains(odd_secret) && odd_before.contains(odd_kept),
        &before.join("; "),
    );

    let out = dir.join("redacted.pdf");
    let line = [
        vec![
            "redact".to_string(),
            s(&input),
            "-o".into(),
            s(&out),
            "--json".into(),
        ],
        query_args(&[
            (SearchKind::Text, "Rumpelstilzchen"),
            (SearchKind::Text, "FORM-SECRET-7731"),
            (SearchKind::Text, odd_secret),
        ]),
    ]
    .concat();
    let (code, json, stderr) = run(&line, &[]);
    // Written, and that is the whole claim: the count no longer refuses.
    // Not `verified`. Page 2's control word is 8.9 points tall and the
    // recogniser on Windows does not read it back, so the copy is written and
    // reported as not proved there (measured 2026-10-07; `docs/TRAPS.md` has
    // the entry about that engine and small type). The exit code and the
    // verdict still have to agree.
    report.check(
        "the copy is written, not refused over a count",
        (code == 0 || code == 1)
            && json["written"] == true
            && json["verified"] == (code == 0)
            && out.exists(),
        &format!("exit {code}: {stderr} {json}"),
    );
    if !out.exists() {
        return;
    }
    let wrong = readers(&out, &dir, &secrets, &kept);
    report.check(
        "no reader finds either secret of page 1, and every one finds its three controls",
        wrong.is_empty(),
        &wrong.join("; "),
    );
    let odd_after = text_of(&out, &[], &[]);
    report.check(
        "the secret beside the malformed shows is gone and its control is not",
        !odd_after.contains(odd_secret) && odd_after.contains(odd_kept),
        &odd_after,
    );
    // The shows that draw nothing are still in the page's stream: they were
    // passed over, not swept out with the line that was asked for.
    let after = content_of(&out);
    report.check(
        "the empty shows are left in the page's content",
        after.contains("[-200] TJ") && after.contains("() '") && !after.contains("Rumpel"),
        &after,
    );
}

/// What each line of [`cuts_pdf`] loses and keeps: the secret, the words
/// before it and the words after it. One line per way a show is written.
const CUT_LINES: [(&str, &str, &str); 7] = [
    // One string, which is how the invoice this was reported on writes a line.
    ("555000123456", "Tarif Muster Eins", "Ende"),
    // A `TJ`, with a kern before what goes, one after it and one further on.
    // The secret is one string of it, so a reader of the bytes finds it too.
    ("DE44 5001", "Konto", "alpha"),
    // Character spacing, word spacing and horizontal scaling.
    ("ZZ00000001", "Kunde", "bleibt"),
    // `'`, which moves to the next line before it shows.
    ("QUOTE-SECRET", "Zeile", "danach"),
    // The end of a show, with the next show starting at its pen.
    ("TAIL-SECRET", "Anfang", "weiter beta"),
    // A scaled page matrix and a scaled text matrix.
    ("SCHRAEG77", "Schief", "gamma"),
    // The start of a show.
    ("START-SECRET", "", "hinten delta"),
];

/// An eighth line, turned thirty degrees. Apart from [`CUT_LINES`] because
/// `pdftotext` does not read a tilted line whole, so only tpdf's own
/// extraction is asked about this one.
const TILTED: (&str, &str, &str) = ("TILTED-SECRET", "Schraeg", "omega");

/// One page, eight lines, each a different way of writing a line of which a
/// reader marks a part.
fn cuts_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let body = "BT /F1 12 Tf 72 700 Td (Tarif Muster Eins - 555000123456 Ende) Tj ET\n\
                BT /F1 12 Tf 72 670 Td [(Konto ) -30 (DE44 5001) 20 ( rest) -10 ( alpha)] TJ ET\n\
                BT /F1 12 Tf 2 Tc 3 Tw 80 Tz 72 640 Td (Kunde ZZ00000001 bleibt) Tj \
                0 Tc 0 Tw 100 Tz ET\n\
                BT /F1 12 Tf 14 TL 72 624 Td (Zeile QUOTE-SECRET danach) ' ET\n\
                BT /F1 12 Tf 72 580 Td (Anfang TAIL-SECRET) Tj ( weiter beta) Tj ET\n\
                q 1.5 0 0 1.5 0 0 cm BT /F1 8 Tf 1.2 0 0 1.2 60 300 Tm \
                (Schief SCHRAEG77 gamma) Tj ET Q\n\
                BT /F1 12 Tf 72 400 Td (START-SECRET hinten delta) Tj ET\n\
                BT /F1 12 Tf 0.866 0.5 -0.5 0.866 300 80 Tm \
                (Schraeg TILTED-SECRET omega) Tj ET\n";
    let content = doc.add_object(Stream::new(dictionary! {}, body.as_bytes().to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    // A bookmark that repeats the first line. The cut takes twelve digits out
    // of the line, and the title is not a substring of twelve digits: it goes
    // because it is a substring of the **line**.
    let outline = doc.new_object_id();
    let entry = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Tarif Muster Eins - 555000123456 Ende"),
        "Parent" => outline,
        "Dest" => vec![page.into(), "Fit".into()],
    });
    doc.objects.insert(
        outline,
        dictionary! { "Type" => "Outlines", "First" => entry, "Last" => entry, "Count" => 1 }
            .into(),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages, "Outlines" => outline,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// A marked part of a line goes, and the rest of the line stays where it was.
///
/// The removal used to take the whole show operator, so marking an account
/// number took the line it stood in. Reported from use.
pub(super) fn part_of_a_line_is_removed(report: &mut Report) {
    let dir = scratch("redact-cuts");
    let input = dir.join("cuts.pdf");
    std::fs::write(&input, cuts_pdf()).expect("fixture");
    let secrets: Vec<&str> = CUT_LINES.iter().map(|line| line.0).collect();
    let kept: Vec<&str> = CUT_LINES
        .iter()
        .flat_map(|line| [line.1, line.2])
        .filter(|word| !word.is_empty())
        .collect();

    let before = readers(&input, &dir, &[], &[&secrets[..], &kept[..]].concat());
    report.check(
        &format!("control: {} find every string in the input", installed()),
        before.is_empty(),
        &before.join("; "),
    );
    // Where each kept word is before anything is removed.
    let placed: Vec<&str> = kept.iter().copied().chain([TILTED.1, TILTED.2]).collect();
    let places = |path: &Path| -> Vec<serde_json::Value> {
        placed
            .iter()
            .map(|word| region_for(path, 1, word))
            .collect()
    };
    let was = places(&input);

    let out = dir.join("redacted.pdf");
    let queries: Vec<(SearchKind, &str)> = secrets
        .iter()
        .chain([&TILTED.0])
        .map(|secret| (SearchKind::Text, *secret))
        .collect();
    let line = [
        vec![
            "redact".to_string(),
            s(&input),
            "-o".into(),
            s(&out),
            "--json".into(),
        ],
        query_args(&queries),
    ]
    .concat();
    let (code, json, stderr) = run(&line, &[]);
    report.check(
        "the copy is written and verified",
        code == 0 && json["written"] == true && json["verified"] == true && out.exists(),
        &format!("exit {code}: {stderr} {json}"),
    );
    if !out.exists() {
        return;
    }
    report.check(
        "the report counts one text removal for each of the eight lines",
        json["pages"][0]["text_removals"] == 8,
        &json["pages"][0].to_string(),
    );
    let taking = json["pages"][0]["taking"].to_string();
    report.check(
        "the report says it takes the secrets and none of the words beside them",
        secrets.iter().all(|secret| taking.contains(secret))
            && kept.iter().all(|word| !taking.contains(word)),
        &taking,
    );
    let wrong = readers(&out, &dir, &secrets, &kept);
    report.check(
        "no reader finds a secret, and every one finds the words before and after each",
        wrong.is_empty(),
        &wrong.join("; "),
    );

    // **The words that stay did not move.** A cut that left no gap would pass
    // every check above with the rest of each line pulled left. A twentieth
    // of a point, where the narrowest letter here is 2.7 points wide.
    let now = places(&out);
    let tilted = text_of(&out, &[], &[]);
    report.check(
        "the tilted line lost its secret and kept the words beside it",
        !tilted.contains(TILTED.0) && tilted.contains(TILTED.1) && tilted.contains(TILTED.2),
        &tilted,
    );
    let moved: Vec<String> = placed
        .iter()
        .zip(was.iter().zip(&now))
        .filter(|(_, (a, b))| {
            (0..4).any(|at| {
                let (a, b) = (a["rect"][at].as_f64(), b["rect"][at].as_f64());
                !matches!((a, b), (Some(a), Some(b)) if (a - b).abs() <= 0.05)
            })
        })
        .map(|(word, (a, b))| format!("{word:?} was at {} and is at {}", a["rect"], b["rect"]))
        .collect();
    report.check(
        "every word that stays is where it was, to a twentieth of a point",
        moved.is_empty(),
        &moved.join("; "),
    );
}

/// What each line of [`whole_shows_pdf`] loses and keeps. The secret is a
/// show operator of its own on every line, so the removal takes all of it,
/// and what follows it is written in a different way on each.
const WHOLE_LINES: [(&str, &str); 10] = [
    // The next show starts at the pen.
    ("SECRETA", "NEXTA"),
    // A `TJ` that draws nothing moves the pen, and then a show starts there.
    ("SECRETB", "NEXTB"),
    // The last show of its text object: nothing follows.
    ("SECRETC", "LEADC"),
    // `Td`, `Tm` and `T*` place the next show from the start of the line.
    ("SECRETD", "NEXTD"),
    ("SECRETE", "NEXTE"),
    ("SECRETF", "NEXTF"),
    // The secret is shown by `'`, which moves to the next line first, and
    // the line after it is another `'`.
    ("SECRETG", "NEXTG"),
    // Turned thirty degrees: the pen moves along the line, not along x. Two
    // spaces end the secret's show, which keep the next word out of the
    // upright box a search draws round a tilted one.
    ("SECRETH", "NEXTH"),
    // `"`, which also sets the two spacings the next show is drawn with.
    ("SECRETI", "NEXTI"),
    // Another size for the show that follows.
    ("SECRETJ", "NEXTJ"),
];

/// One page, ten lines, on each a show that goes whole and something after
/// it. Synthetic words throughout.
fn whole_shows_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let body = "BT /F1 20 Tf 100 740 Td (SECRETA) Tj (NEXTA) Tj ET\n\
                BT /F1 20 Tf 100 700 Td (SECRETB) Tj [-500] TJ (NEXTB) Tj ET\n\
                BT /F1 20 Tf 100 660 Td (LEADC ) Tj (SECRETC) Tj ET\n\
                BT /F1 20 Tf 100 620 Td (SECRETD) Tj 0 -30 Td (NEXTD) Tj ET\n\
                BT /F1 20 Tf 100 550 Td (SECRETE) Tj 1 0 0 1 100 520 Tm (NEXTE) Tj ET\n\
                BT /F1 20 Tf 30 TL 100 480 Td (SECRETF) Tj T* (NEXTF) Tj ET\n\
                BT /F1 20 Tf 30 TL 100 440 Td (SECRETG) ' (NEXTG) ' ET\n\
                BT /F1 20 Tf 0.866 0.5 -0.5 0.866 300 80 Tm (SECRETH  ) Tj (NEXTH) Tj ET\n\
                BT /F1 20 Tf 30 TL 100 340 Td 4 1 (SECRETI) \" (NEXTI) Tj 0 Tw 0 Tc ET\n\
                BT /F1 20 Tf 100 250 Td (SECRETJ) Tj /F1 11 Tf (NEXTJ) Tj ET\n";
    let content = doc.add_object(Stream::new(dictionary! {}, body.as_bytes().to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// A show that goes whole takes its words and nothing else: what is drawn
/// after it stays where it was.
///
/// Taking the operator out took what it did to the pen and to the line with
/// it. Measured 2026-10-09 on this fixture: the show after one that went was
/// drawn where the one that went had started, and the line after a removed
/// `'` one line up.
pub(super) fn a_whole_show_goes_and_the_rest_stays(report: &mut Report) {
    let dir = scratch("redact-whole");
    let input = dir.join("whole.pdf");
    std::fs::write(&input, whole_shows_pdf()).expect("fixture");
    let secrets: Vec<&str> = WHOLE_LINES.iter().map(|line| line.0).collect();
    let kept: Vec<&str> = WHOLE_LINES.iter().map(|line| line.1).collect();
    let places = |path: &Path| -> Vec<serde_json::Value> {
        kept.iter().map(|word| region_for(path, 1, word)).collect()
    };
    let was = places(&input);

    let out = dir.join("redacted.pdf");
    let queries: Vec<(SearchKind, &str)> = secrets
        .iter()
        .map(|secret| (SearchKind::Text, *secret))
        .collect();
    let line = [
        vec![
            "redact".to_string(),
            s(&input),
            "-o".into(),
            s(&out),
            "--json".into(),
        ],
        query_args(&queries),
    ]
    .concat();
    let (code, json, stderr) = run(&line, &[]);
    report.check(
        "the copy is written and verified",
        code == 0 && json["written"] == true && json["verified"] == true && out.exists(),
        &format!("exit {code}: {stderr} {json}"),
    );
    if !out.exists() {
        return;
    }
    let taking = json["pages"][0]["taking"].to_string();
    report.check(
        "the report takes each secret, and none of the words after them",
        json["pages"][0]["text_removals"] == 10
            && secrets.iter().all(|secret| taking.contains(secret))
            && kept.iter().all(|word| !taking.contains(word)),
        &json["pages"][0].to_string(),
    );
    let text = text_of(&out, &[], &[]);
    report.check(
        "no secret is in the copy's text, and every word after one is",
        secrets.iter().all(|secret| !text.contains(secret))
            && kept.iter().all(|word| text.contains(word)),
        &text,
    );
    let now = places(&out);
    let moved: Vec<String> = kept
        .iter()
        .zip(was.iter().zip(&now))
        .filter(|(_, (a, b))| {
            (0..4).any(|at| {
                let (a, b) = (a["rect"][at].as_f64(), b["rect"][at].as_f64());
                !matches!((a, b), (Some(a), Some(b)) if (a - b).abs() <= 0.05)
            })
        })
        .map(|(word, (a, b))| format!("{word:?} was at {} and is at {}", a["rect"], b["rect"]))
        .collect();
    report.check(
        "every word after a show that went is where it was, to a twentieth of a point",
        moved.is_empty(),
        &moved.join("; "),
    );
    // What the page's content holds now, so that "where it was" is not a
    // position the fixture happened to give.
    let content = content_of(&out);
    report.check(
        "the removed `'` and `\"` still move to their lines, and no secret is a string",
        content.matches("T*").count() == 3
            && secrets.iter().all(|secret| !content.contains(secret)),
        &content,
    );
}

/// One page drawing the same Form XObject twice, and a line of its own.
///
/// The removal leaves a form the page draws more than once --- taking the one
/// drawing a region covers would leave the other, and the object itself ---
/// so the copy's text still holds the word, which is the case the tool's
/// search of the written file exists for.
pub(super) fn shared_form_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        b"BT /F1 12 Tf 0 4 Td (Codeword Rumpelstilzchen) Tj ET".to_vec(),
    ));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 1 0 0 1 72 700 cm /Fm1 Do Q\nq 1 0 0 1 72 600 cm /Fm1 Do Q\n\
          BT /F1 12 Tf 72 500 Td (CONTROL-KEEP gamma) Tj ET\n"
            .to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Fm1" => form },
        },
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// A field value and default shared by widgets on two different pages.
/// Each widget has its own appearance; an unrelated field and page text must
/// survive. The region covers only the first widget and a printed label that
/// gives the OCR gate a measurable font size. The field secret is never page
/// text, so removing the label cannot supply the missing field-value needle.
fn multi_widget_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let field = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let mut kids = Vec::new();
    let mut page_ids = Vec::new();
    let mut retained = None;
    for at in 0..2 {
        let page = doc.new_object_id();
        let appearance = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 250.into(), 24.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            },
            b"BT /F1 12 Tf 4 6 Td (WIDGET-SECRET-4827) Tj ET".to_vec(),
        ));
        let widget = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => field, "P" => page,
            "Rect" => vec![72.into(), 700.into(), 322.into(), 724.into()], "F" => 4,
            "AP" => dictionary! { "N" => appearance },
        });
        kids.push(Object::Reference(widget));
        let mut annots = vec![Object::Reference(widget)];
        if at == 1 {
            let control = doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
                "T" => Object::string_literal("Retained"), "V" => Object::string_literal("CONTROL-ANSWER"),
                "P" => page, "Rect" => vec![72.into(), 600.into(), 322.into(), 624.into()], "F" => 4,
            });
            annots.push(Object::Reference(control));
            retained = Some(control);
        }
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!(
                "BT /F1 8 Tf 72 500 Td (CONTROL-KEEP page {}) Tj ET\n{}",
                at + 1,
                if at == 0 {
                    "BT /F1 12 Tf 72 730 Td (REMOVE FIELD) Tj ET"
                } else {
                    ""
                }
            )
            .into_bytes(),
        ));
        doc.objects.insert(
            page,
            dictionary! {
                "Type" => "Page", "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                "Contents" => content, "Annots" => annots,
            }
            .into(),
        );
        page_ids.push(Object::Reference(page));
    }
    doc.objects.insert(
        field,
        dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("Repeated"),
            "V" => Object::string_literal("WIDGET-SECRET-4827"),
            "DV" => Object::string_literal("WIDGET-SECRET-4827"), "Kids" => kids,
        }
        .into(),
    );
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => page_ids, "Count" => 2 }.into(),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages,
        "AcroForm" => dictionary! { "Fields" => vec![Object::Reference(field), Object::Reference(retained.expect("control field"))] },
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

pub(super) fn widgets_are_removed(report: &mut Report) {
    let dir = scratch("redact-widgets");
    let input = dir.join("multi-widget.pdf");
    let output = dir.join("multi-widget-redacted.pdf");
    let regions = dir.join("regions.json");
    std::fs::write(&input, multi_widget_pdf()).expect("multi-widget input");
    std::fs::write(&regions, r#"[{"page":1,"rect":[72,50,250,44]}]"#).expect("regions");
    let before_fields = tool(&["fields", &s(&input), "--json"], &[]);
    let fields = parsed(&before_fields.1);
    let shared = fields["fields"]
        .as_array()
        .and_then(|fields| fields.iter().find(|field| field["name"] == "Repeated"));
    report.check(
        "control: one value has two widgets on two pages beside an unrelated field",
        before_fields.0 == 0
            && shared.is_some_and(|field| {
                field["widgets"] == 2
                    && field["pages"] == serde_json::json!([1, 2])
                    && field["value"] == "WIDGET-SECRET-4827"
            })
            && before_fields.1.contains("CONTROL-ANSWER"),
        &format!("exit {}: {}", before_fields.0, before_fields.2),
    );
    let (code, json, stderr) = run(
        &strings(&[
            "redact",
            &s(&input),
            "-o",
            &s(&output),
            "--regions",
            &s(&regions),
            "--json",
        ]),
        &[],
    );
    report.check(
        "covering one widget produces a verified rewrite",
        code == 0 && json["written"] == true && json["verified"] == true,
        &format!("exit {code}: {stderr}; {json}"),
    );
    let after_fields = tool(&["fields", &s(&output), "--json"], &[]);
    report.check(
        "the shared field is gone and the unrelated answer remains",
        after_fields.0 == 0
            && !after_fields.1.contains("WIDGET-SECRET-4827")
            && !after_fields.1.contains("Repeated")
            && after_fields.1.contains("CONTROL-ANSWER"),
        &format!(
            "exit {}: {} {}",
            after_fields.0, after_fields.1, after_fields.2
        ),
    );
    let kept = ["CONTROL-KEEP page 1", "CONTROL-KEEP page 2"];
    let wrong = readers(&output, &dir, &["WIDGET-SECRET-4827"], &kept);
    report.check(
        "every reader loses the widget secret and retains both page controls",
        wrong.is_empty(),
        &wrong.join("; "),
    );
    match (qdf(&input, &dir), qdf(&output, &dir)) {
        (Some(before), Some(after)) => report.check(
            "independent qpdf control: a stored field value and appearance become absent after rewriting",
            contains(&before, "WIDGET-SECRET-4827") && !contains(&after, "WIDGET-SECRET-4827") && contains(&after, "CONTROL-ANSWER"),
            "qpdf did not observe the expected before/after values"),
        _ => report.skip("independent qpdf field/appearance readback", "qpdf is unavailable or could not decode the files"),
    }
    if let Some(probe) = std::env::var_os("TPDF_REDACT_PROBE").map(PathBuf::from) {
        std::fs::create_dir_all(&probe).expect("probe directory");
        std::fs::copy(&input, probe.join("multi-widget.pdf")).expect("probe input");
        std::fs::copy(&output, probe.join("multi-widget-redacted.pdf")).expect("probe output");
        std::fs::write(
            probe.join("multi-widget-expected.json"),
            serde_json::json!({"gone": ["WIDGET-SECRET-4827"], "kept": kept}).to_string(),
        )
        .expect("probe expectation");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// One page with a line of text and five drawings: a rule across the page, a
/// scribble over a typed name with a small box beside it, a square that is
/// also a clip, and a grey panel.
///
/// The coordinates are odd on purpose, so each drawing can be looked for in the
/// written content by a number nothing else on the page uses.
fn drawings_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let body = "BT /F1 10 Tf 72 700 Td (CONTROL-KEEP alpha) Tj ET\n\
                BT /F1 12 Tf 200 465 Td (Signed J. Doe) Tj ET\n\
                72 600.25 m 539.5 600.25 l S\n\
                q 2 w 200 500 m 217.31 533.77 240 470 260 500 c 280 520 l S Q\n\
                210 480 6.125 6.125 re f\n\
                100 300 7.375 7.375 re f\n\
                q 400 480 20.375 20.375 re W f Q\n\
                q 0.5 g 300 640 200.625 40 re f Q\n";
    let content = doc.add_object(Stream::new(dictionary! {}, body.as_bytes().to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }
            .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialises");
    bytes
}

/// The first page's content as it is stored in `path`, decoded.
fn content_of(path: &Path) -> String {
    let doc = Document::load(path).expect("the written file loads");
    let page = *doc.get_pages().values().next().expect("a page");
    String::from_utf8_lossy(&doc.get_page_content(page)).into_owned()
}

/// A drawing the region holds all of is taken out of the content; a rule and a
/// filled rectangle the region crosses are cut at its edge; a curve that
/// reaches beyond the region, and a drawing that is also a clip, are left and
/// said.
pub(super) fn drawings_are_removed(report: &mut Report) {
    const SCRIBBLE: &str = "217.31";
    const BOX: &str = "6.125";
    const RULE: &str = "539.5";
    const CLIP: &str = "20.375";
    let dir = scratch("redact-drawings");
    let input = dir.join("drawings.pdf");
    std::fs::write(&input, drawings_pdf()).expect("drawings input");
    let before = content_of(&input);
    report.check(
        "control: the page's content holds all four drawings",
        [SCRIBBLE, BOX, RULE, CLIP]
            .iter()
            .all(|mark| before.contains(mark)),
        &before,
    );

    // `[x, y, width, height]` as displayed, y down from the top of a 792 pt page.
    let redact = |name: &str, rect: &str| {
        let output = dir.join(format!("{name}.pdf"));
        let regions = dir.join(format!("{name}.json"));
        std::fs::write(&regions, format!(r#"[{{"page":1,"rect":{rect}}}]"#)).expect("regions");
        let (code, json, stderr) = run(
            &strings(&[
                "redact",
                &s(&input),
                "-o",
                &s(&output),
                "--regions",
                &s(&regions),
                "--json",
            ]),
            &[],
        );
        let after = if output.exists() {
            content_of(&output)
        } else {
            String::new()
        };
        (code, json, stderr, after)
    };

    // Around the scribble, the box and the typed name under them.
    let (code, json, stderr, after) = redact("inside", "[190,247,110,85]");
    report.check(
        "a region holding a name and two drawings removes all three and is verified",
        code == 0
            && json["written"] == true
            && json["verified"] == true
            && json["pages"][0]["text_removals"] == 1
            && json["pages"][0]["path_removals"] == 2
            && json["notes"].as_array().is_some_and(Vec::is_empty)
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty),
        &format!("exit {code}: {stderr}; {json}"),
    );
    report.check(
        "their outlines are gone from the content, and the other drawings and the text are not",
        !after.contains(SCRIBBLE)
            && !after.contains(BOX)
            && !after.contains("J. Doe")
            && after.contains(RULE)
            && after.contains(CLIP)
            && after.contains("CONTROL-KEEP alpha"),
        &after,
    );

    // Around a box that stands alone: no word is inside the region, so the
    // read-back is sized from the smallest print left on the page and says so.
    let (code, json, stderr, after) = redact("alone", "[95,480,20,20]");
    let notes = json["notes"].to_string();
    report.check(
        "a region holding a drawing and no text is verified, at a size the report names",
        code == 0
            && json["verified"] == true
            && json["pages"][0]["path_removals"] == 1
            && notes.contains("held no text")
            && notes.contains("pt or larger")
            && json["summary"]
                .as_str()
                .is_some_and(|s| s.contains(" Note: page 1:"))
            && !after.contains("7.375")
            && after.contains(SCRIBBLE),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // What the page draws at a point, at one pixel a point: `[r, g, b]`.
    let drawn = |name: &str| {
        let picture = dir.join(format!("{name}.png"));
        let (code, _, stderr) = tool(
            &[
                "render",
                &s(&dir.join(format!("{name}.pdf"))),
                "--dpi",
                "72",
                "-o",
                &s(&picture),
            ],
            &[],
        );
        assert_eq!(code, 0, "the written copy renders: {stderr}");
        let (width, _, data) = super::images::pixels(&s(&picture));
        move |x: u32, from_top: u32| -> [u8; 3] {
            let at = ((from_top * width + x) * 4) as usize;
            [data[at], data[at + 1], data[at + 2]]
        }
    };
    let dark = |pixel: [u8; 3]| pixel.iter().all(|channel| *channel < 128);
    // The copy paints its own black mark over each region, so what a region
    // shows afterwards says nothing about what was under it. The content
    // checks say that; the pixels say what is still drawn beside the region.
    const MARK: [u8; 3] = [0, 0, 0];
    const PAPER: [u8; 3] = [255, 255, 255];

    // Over 50 pt of the rule, which runs 467 pt across the page at y 600.25:
    // 191.75 pt from the top, so it darkens pixel rows 191 and 192.
    let (code, json, stderr, after) = redact("rule", "[100,182,50,20]");
    let flat = after.split_whitespace().collect::<Vec<_>>().join(" ");
    report.check(
        "a rule the region crosses is cut at both of its edges, and the copy is verified",
        code == 0
            && json["verified"] == true
            && json["pages"][0]["path_cuts"] == 1
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && flat.contains("72 600.25 m 100 600.25 l 150 600.25 m 539.5 600.25 l S")
            && [SCRIBBLE, BOX, CLIP]
                .iter()
                .all(|mark| after.contains(mark)),
        &format!("exit {code}: {stderr}; {json}; {flat}"),
    );
    let pixel = drawn("rule");
    report.check(
        "the page still draws the rule up to both edges of the region",
        (73..99).all(|x| dark(pixel(x, 191)) || dark(pixel(x, 192)))
            && (151..538).all(|x| dark(pixel(x, 191)) || dark(pixel(x, 192)))
            && pixel(98, 185) == PAPER,
        &format!(
            "above {:?}, before {:?} {:?}, after {:?} {:?}",
            pixel(98, 185),
            pixel(98, 191),
            pixel(98, 192),
            pixel(151, 191),
            pixel(151, 192)
        ),
    );
    let whole = {
        std::fs::copy(&input, dir.join("whole.pdf")).expect("a copy of the input");
        drawn("whole")
    };
    report.check(
        "control: before the cut the page draws the rule across the region too",
        (73..538).all(|x| dark(whole(x, 191)) || dark(whole(x, 192))),
        &format!("{:?} {:?}", whole(125, 191), whole(125, 192)),
    );

    // Beside the rule: the region's lower edge is 0.15 pt above its ink, which
    // is 599.75 to 600.75, and inside the bounds PDFium reports for it.
    let (code, json, stderr, after) = redact("beside", "[100,171,50,20.1]");
    report.check(
        "a region that overlaps a rule's bounds and not its ink leaves it whole and unreported",
        code == 0
            && json["pages"][0]["path_cuts"] == 0
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && after
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("72 600.25 m 539.5 600.25 l S"),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // The copy that was cut, redacted again over the same region.
    let again = dir.join("again.pdf");
    let regions = dir.join("rule.json");
    let (code, json, stderr) = run(
        &strings(&[
            "redact",
            &s(&dir.join("rule.pdf")),
            "-o",
            &s(&again),
            "--regions",
            &s(&regions),
            "--json",
        ]),
        &[],
    );
    report.check(
        "a second redaction over a region already cut finds nothing of the rule there",
        code == 0
            && json["pages"][0]["path_cuts"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // Inside the grey panel, which is 300 to 500.625 across and 112 to 152 pt
    // from the top. The hole is 350 to 380 across and 122 to 142 from the top.
    let (code, json, stderr, after) = redact("panel", "[350,122,30,20]");
    let flat = after.split_whitespace().collect::<Vec<_>>().join(" ");
    let pixel = drawn("panel");
    let grey = whole(310, 115);
    report.check(
        "a filled rectangle loses what the region covers and is drawn around it without a seam",
        code == 0
            && json["verified"] == true
            && json["pages"][0]["path_cuts"] == 1
            && flat.contains("300 640 50 40 re 380 640 120.625 40 re 350 640 30 10 re 350 670 30 10 re f")
            && grey != PAPER
            && grey != MARK
            && (350..380).all(|x| (122..142).all(|y| pixel(x, y) == MARK))
            // Everywhere else in the panel is the grey it was, the lines where
            // two of the four pieces meet included.
            && (301..500).all(|x| {
                (113..151).all(|y| {
                    let hole = (350..380).contains(&x) && (122..142).contains(&y);
                    hole || pixel(x, y) == grey
                })
            }),
        &format!("exit {code}: {stderr}; {json}; {flat}; grey {grey:?}"),
    );

    // Over the first 30 pt of the scribble, which is a curve.
    let (code, json, stderr, after) = redact("curve", "[195,252,35,40]");
    let left = json["pages"][0]["left"].to_string();
    report.check(
        "a curve reaching beyond the region is left, said, and the copy is not called clean",
        json["written"] == true
            && json["verified"] == false
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["path_cuts"] == 0
            && left.contains("reaches beyond the region")
            && json["notes"].as_array().is_some_and(Vec::is_empty)
            && [SCRIBBLE, BOX, RULE, CLIP]
                .iter()
                .all(|mark| after.contains(mark)),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // Around the square that is also a clip.
    let (code, json, stderr, after) = redact("clip", "[395,287,30,30]");
    let left = json["pages"][0]["left"].to_string();
    report.check(
        "a drawing that also clips is left, said, and the copy is not called clean",
        json["written"] == true
            && json["verified"] == false
            && json["pages"][0]["path_removals"] == 0
            && left.contains("also clips")
            && after.contains(CLIP),
        &format!("exit {code}: {stderr}; {json}"),
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Two pages. The first keeps its resources in an object of their own and
/// draws one reusable block, which holds a word and three pictures; the third
/// picture is drawn by the second page as well.
///
/// Each picture's data is a run of bytes nothing else in the file has, so the
/// written file can be searched for it.
fn block_pictures_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let picture = |doc: &mut Document, data: &str| {
        doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => data.len() as i64,
                "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            data.as_bytes().to_vec(),
        ))
    };
    let first = picture(&mut doc, "PICTURE-A-BYTES");
    let second = picture(&mut doc, "PICTURE-B-BYTES");
    let shared = picture(&mut doc, "PICTURE-C-BYTES");
    // The block's own space: placed on the page at (100, 500).
    let inside = "BT /F1 12 Tf 0 60 Td (INSIDE-BLOCK) Tj ET\n\
                  q 40 0 0 40 0 0 cm /ImA Do Q\n\
                  q 40 0 0 40 100 0 cm /ImB Do Q\n\
                  q 40 0 0 40 200 0 cm /ImC Do Q\n";
    let block = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 100.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "XObject" => dictionary! { "ImA" => first, "ImB" => second, "ImC" => shared },
            },
        },
        inside.as_bytes().to_vec(),
    ));
    let body =
        "BT /F1 10 Tf 72 700 Td (CONTROL-KEEP alpha) Tj ET\nq 1 0 0 1 100 500 cm /Fm0 Do Q\n";
    let content = doc.add_object(Stream::new(dictionary! {}, body.as_bytes().to_vec()));
    // An object of its own, which is how most producers write a page's resources.
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
        "XObject" => dictionary! { "Fm0" => block },
    });
    let one = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    let other = doc.add_object(Stream::new(
        dictionary! {},
        b"q 40 0 0 40 72 600 cm /ImC Do Q\n".to_vec(),
    ));
    let two = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! { "XObject" => dictionary! { "ImC" => shared } },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => other,
    });
    doc.objects.insert(
        pages,
        dictionary! {
            "Type" => "Pages", "Count" => 2,
            "Kids" => vec![Object::Reference(one), Object::Reference(two)],
        }
        .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialises");
    bytes
}

/// A picture inside a reusable block is taken out of the block, on a page that
/// keeps its resources in an object of their own; one another page draws too
/// is left and said.
pub(super) fn pictures_in_blocks_are_removed(report: &mut Report) {
    let dir = scratch("redact-block-pictures");
    let input = dir.join("blocks.pdf");
    std::fs::write(&input, block_pictures_pdf()).expect("the input");
    let holds =
        |path: &Path, needle: &str| std::fs::read(path).is_ok_and(|bytes| contains(&bytes, needle));
    report.check(
        "control: the input holds all three pictures",
        ["PICTURE-A-BYTES", "PICTURE-B-BYTES", "PICTURE-C-BYTES"]
            .iter()
            .all(|needle| holds(&input, needle)),
        "",
    );
    // `[x, y, width, height]` as displayed, y down from the top of a 792 pt page.
    let redact = |name: &str, rect: &str| {
        let output = dir.join(format!("{name}.pdf"));
        let regions = dir.join(format!("{name}.json"));
        std::fs::write(&regions, format!(r#"[{{"page":1,"rect":{rect}}}]"#)).expect("regions");
        let (code, json, stderr) = run(
            &strings(&[
                "redact",
                &s(&input),
                "-o",
                &s(&output),
                "--regions",
                &s(&regions),
                "--json",
            ]),
            &[],
        );
        (code, json, stderr, output)
    };

    // The first picture sits at (100..140, 500..540): 252..292 from the top.
    let (code, json, stderr, output) = redact("first", "[105,257,20,20]");
    report.check(
        "a region on a picture inside a block removes that picture and reports nothing left",
        json["written"] == true
            && json["pages"][0]["image_removals"] == 1
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty),
        &format!("exit {code}: {stderr}; {json}"),
    );
    report.check(
        "its bytes are gone from the file, and the other two pictures' are not",
        !holds(&output, "PICTURE-A-BYTES")
            && holds(&output, "PICTURE-B-BYTES")
            && holds(&output, "PICTURE-C-BYTES"),
        "",
    );
    let (_, words, _) = tool(&["text", &s(&output)], &[]);
    report.check(
        "the block's own word and the page's are still there",
        words.contains("INSIDE-BLOCK") && words.contains("CONTROL-KEEP"),
        &words,
    );

    // The third picture, at (300..340, 500..540), is drawn by page 2 as well.
    let (code, json, stderr, output) = redact("shared", "[305,257,20,20]");
    let notes = json["notes"].to_string();
    report.check(
        "a picture another page draws too is taken off this page, and the report says it stays in the file",
        json["written"] == true
            && json["pages"][0]["image_removals"] == 1
            && json["pages"][0]["left"].as_array().is_some_and(Vec::is_empty)
            && notes.contains("draws 2 time(s)")
            && notes.contains("taken off this page")
            && notes.contains("still in the file"),
        &format!("exit {code}: {stderr}; {json}"),
    );
    let block_draws = |path: &Path| {
        let doc = Document::load(path).expect("the written file loads");
        let page = *doc.get_pages().values().next().expect("a page");
        let resources = doc.get_page_resources(page).expect("resources");
        // The page's resources are an object of their own.
        let holder = doc
            .get_dictionary(resources.1[0])
            .expect("the resources object");
        let list = holder
            .get(b"XObject")
            .and_then(Object::as_dict)
            .expect("a list");
        let block = list
            .get(b"Fm0")
            .and_then(Object::as_reference)
            .expect("the block");
        let stream = doc
            .get_object(block)
            .and_then(Object::as_stream)
            .expect("a stream");
        let body = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        String::from_utf8_lossy(&body).into_owned()
    };
    report.check(
        "the block no longer draws it, and its bytes are still in the file for the other page",
        !block_draws(&output).contains("/ImC")
            && block_draws(&output).contains("/ImA")
            && holds(&output, "PICTURE-C-BYTES"),
        &block_draws(&output),
    );
    // Marked on the other page as well, in the file just written: now nothing
    // draws it, and its bytes go. It sits at (72..112, 600..640) there.
    let everywhere = dir.join("everywhere.pdf");
    let regions = dir.join("everywhere.json");
    std::fs::write(&regions, r#"[{"page":2,"rect":[77,157,20,20]}]"#).expect("regions");
    let (code, json, stderr) = run(
        &strings(&[
            "redact",
            &s(&output),
            "-o",
            &s(&everywhere),
            "--regions",
            &s(&regions),
            "--json",
        ]),
        &[],
    );
    report.check(
        "marked on every page that draws it, the picture's bytes leave the file",
        json["written"] == true
            && json["pages"][0]["image_removals"] == 1
            && json["notes"].as_array().is_some_and(Vec::is_empty)
            && !holds(&everywhere, "PICTURE-C-BYTES")
            && holds(&everywhere, "PICTURE-B-BYTES"),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // Over the first two pictures at once.
    let (code, json, stderr, output) = redact("both", "[105,257,120,20]");
    report.check(
        "a region over two pictures of one block removes both",
        json["written"] == true
            && json["pages"][0]["image_removals"] == 2
            && !holds(&output, "PICTURE-A-BYTES")
            && !holds(&output, "PICTURE-B-BYTES")
            && holds(&output, "PICTURE-C-BYTES"),
        &format!("exit {code}: {stderr}; {json}"),
    );
    match Command::new("qpdf").arg("--check").arg(&output).output() {
        Ok(out) => report.check(
            "qpdf finds nothing wrong with the copy",
            out.status.code() == Some(0),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(_) => report.skip(
            "qpdf checks the copy without its pictures",
            "qpdf is unavailable",
        ),
    }
}

/// `contacts_pdf` with an `/AcroForm` carrying an `/XFA` packet.
fn xfa_pdf() -> Vec<u8> {
    let mut doc = Document::load_mem(&contacts_pdf()).expect("parses");
    let packet = doc.add_object(Stream::new(dictionary! {}, b"<xdp:xdp/>".to_vec()));
    let form = doc.add_object(dictionary! { "Fields" => Vec::<Object>::new(), "XFA" => packet });
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("root");
    doc.get_dictionary_mut(root)
        .expect("catalog")
        .set("AcroForm", form);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

fn parsed(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or(serde_json::Value::Null)
}

/// The built tool's text for `path`.
fn text_of(path: &Path, env: &[(&str, &str)], extra: &[&str]) -> String {
    let at = path.display().to_string();
    let mut args = vec!["text", at.as_str()];
    args.extend_from_slice(extra);
    tool(&args, env).1
}

/// Poppler's reading, or `None` when it is not installed.
fn pdftotext(path: &Path) -> Option<String> {
    let out = Command::new("pdftotext")
        .args([path.display().to_string().as_str(), "-"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The file's bytes with every stream uncompressed, or `None` without qpdf.
fn qdf(path: &Path, dir: &Path) -> Option<Vec<u8>> {
    let out = dir.join(format!("{}.qdf", path.file_name()?.to_string_lossy()));
    let status = Command::new("qpdf")
        .args(["--qdf", "--object-streams=disable"])
        .arg(path)
        .arg(&out)
        .status()
        .ok()?;
    // 0 or 3 (warnings): the file was written.
    (status.code() == Some(0) || status.code() == Some(3))
        .then(|| std::fs::read(&out).ok())
        .flatten()
}

/// One page drawing a reusable block `times` times. The block holds a rule,
/// a small square and a curve, and the page places it doubled: a point
/// `(x, y)` of the block lands at `(2x + 120, 2y + 340)`, and a stroke 1.5
/// wide is 3 on the page.
fn block_drawings_pdf(times: usize) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let inside = "0 0 m 200 0 l S\n\
                  20 20 10 10 re f\n\
                  0 60 m 20 80 60 80 80 60 c S\n";
    let block = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![(-5).into(), (-5).into(), 205.into(), 100.into()],
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 10.into(), 20.into()],
        },
        inside.as_bytes().to_vec(),
    ));
    // A line of text the regions do not cover, so the copy has a word left to
    // prove the page can still be read.
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let mut body = String::from(
        "BT /F1 10 Tf 72 700 Td (CONTROL-KEEP alpha) Tj ET\n\
         q 2 0 0 2 100 300 cm 1.5 w /Fm0 Do Q\n",
    );
    for _ in 1..times {
        body.push_str("q 1 0 0 1 300 40 cm /Fm0 Do Q\n");
    }
    let content = doc.add_object(Stream::new(dictionary! {}, body.into_bytes()));
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
        "XObject" => dictionary! { "Fm0" => block },
    });
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }
        .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialises");
    bytes
}

/// A drawing inside a reusable block goes whole when the region holds all of
/// it and is cut at the region's edge when it is a rule that runs on; a curve
/// that runs on stays and is reported, and so does everything in a block the
/// page draws twice.
pub(super) fn drawings_in_blocks_are_removed(report: &mut Report) {
    let dir = scratch("redact-block-drawings");
    let input = dir.join("blocks.pdf");
    std::fs::write(&input, block_drawings_pdf(1)).expect("the input");
    // `[x, y, width, height]` as displayed, y down from the top of a 792 pt page.
    let redact = |from: &Path, name: &str, rect: &str| {
        let output = dir.join(format!("{name}.pdf"));
        let regions = dir.join(format!("{name}.json"));
        std::fs::write(&regions, format!(r#"[{{"page":1,"rect":{rect}}}]"#)).expect("regions");
        let (code, json, stderr) = run(
            &strings(&[
                "redact",
                &s(from),
                "-o",
                &s(&output),
                "--regions",
                &s(&regions),
                "--json",
            ]),
            &[],
        );
        (code, json, stderr, output)
    };
    // What the block's content says, on one line.
    let block = |path: &Path| {
        let doc = Document::load(path).expect("the written file loads");
        let page = *doc.get_pages().values().next().expect("a page");
        let resources = doc.get_page_resources(page).expect("resources");
        let holder = doc
            .get_dictionary(resources.1[0])
            .expect("the resources object");
        let list = holder
            .get(b"XObject")
            .and_then(Object::as_dict)
            .expect("a list");
        let id = list
            .get(b"Fm0")
            .and_then(Object::as_reference)
            .expect("the block");
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .expect("a stream");
        let body = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        String::from_utf8_lossy(&body)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    // What the page draws at a point, at one pixel a point.
    let drawn = |path: &Path, name: &str| {
        let picture = dir.join(format!("{name}.png"));
        let (code, _, stderr) = tool(
            &["render", &s(path), "--dpi", "72", "-o", &s(&picture)],
            &[],
        );
        assert_eq!(code, 0, "the copy renders: {stderr}");
        let (width, _, data) = super::images::pixels(&s(&picture));
        move |x: u32, from_top: u32| -> [u8; 3] {
            let at = ((from_top * width + x) * 4) as usize;
            [data[at], data[at + 1], data[at + 2]]
        }
    };
    let dark = |pixel: [u8; 3]| pixel.iter().all(|channel| *channel < 128);
    const PAPER: [u8; 3] = [255, 255, 255];

    // The rule runs from (120, 340) to (520, 340) on the page, 3 thick: 452
    // from the top, so it darkens pixel rows 451 and 452. The region takes
    // 200..250 of it, which is 40..65 in the block's own numbers.
    let (code, json, stderr, output) = redact(&input, "rule", "[200,442,50,20]");
    let after = block(&output);
    report.check(
        "a rule inside a block is cut at both edges of the region, in the block's own numbers",
        code == 0
            && json["verified"] == true
            && json["pages"][0]["path_cuts"] == 1
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && after.contains("0 0 m 40 0 l 65 0 m 200 0 l S")
            && after.contains("20 20 10 10 re f")
            && after.contains(" c S"),
        &format!("exit {code}: {stderr}; {json}; {after}"),
    );
    let pixel = drawn(&output, "rule");
    let before = drawn(&input, "before");
    report.check(
        "the page still draws the rule up to both edges of the region, and the rest of the block",
        (122..199).all(|x| dark(pixel(x, 451)) || dark(pixel(x, 452)))
            && (252..518).all(|x| dark(pixel(x, 451)) || dark(pixel(x, 452)))
            && pixel(198, 445) == PAPER
            // The square, at (160..180, 380..400): 392..412 from the top.
            && dark(pixel(170, 402))
            // Every row above the region is drawn as it was.
            && (300..440).all(|y| (100..540).all(|x| pixel(x, y) == before(x, y))),
        &format!(
            "{:?} {:?} {:?}",
            pixel(198, 451),
            pixel(252, 452),
            pixel(170, 402)
        ),
    );
    report.check(
        "control: before the cut the page draws the rule across the region too",
        (122..518).all(|x| dark(before(x, 451)) || dark(before(x, 452))),
        &format!("{:?} {:?}", before(225, 451), before(225, 452)),
    );

    // Around the square.
    let (code, json, stderr, output) = redact(&input, "square", "[155,387,30,30]");
    let after = block(&output);
    report.check(
        "a square the region holds all of is taken out of the block",
        code == 0
            && json["pages"][0]["path_removals"] == 1
            && json["pages"][0]["path_cuts"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && !after.contains(" re")
            && after.contains("0 0 m 200 0 l S")
            && after.contains(" c S"),
        &format!("exit {code}: {stderr}; {json}; {after}"),
    );

    // Across the curve, whose arch runs from (120, 460) to (280, 490) on the
    // page: 302..332 from the top.
    let (code, json, stderr, output) = redact(&input, "curve", "[190,300,20,34]");
    let left = json["pages"][0]["left"].to_string();
    report.check(
        "a curve the region crosses is left in the block, and the report says so",
        json["verified"] == false
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["path_cuts"] == 0
            && left.contains("drawing that reaches beyond the region")
            && (!output.exists() || block(&output).contains(" c S")),
        &format!("exit {code}: {stderr}; {json}"),
    );

    // The same block drawn a second time: changing it would change a place
    // nobody marked.
    let twice = dir.join("twice.pdf");
    std::fs::write(&twice, block_drawings_pdf(2)).expect("the input");
    let (code, json, stderr, output) = redact(&twice, "twice-rule", "[200,442,50,20]");
    report.check(
        "nothing is cut in a block the page draws twice, and the report says so",
        json["verified"] == false
            && json["pages"][0]["path_cuts"] == 0
            && json["pages"][0]["path_removals"] == 0
            && json["pages"][0]["left"]
                .as_array()
                .is_some_and(|left| !left.is_empty())
            && (!output.exists() || block(&output).contains("0 0 m 200 0 l S")),
        &format!("exit {code}: {stderr}; {json}"),
    );
    let _ = std::fs::remove_dir_all(dir);
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// What each reader finds of `gone` and `kept` in `path`: a list of
/// complaints, empty when every matched string is absent and every control
/// present, for every reader that is installed.
fn readers(path: &Path, dir: &Path, gone: &[&str], kept: &[&str]) -> Vec<String> {
    let mut wrong = Vec::new();
    let mut texts: Vec<(&str, Vec<u8>)> = vec![("tpdf text", text_of(path, &[], &[]).into_bytes())];
    if let Some(poppler) = pdftotext(path) {
        texts.push(("pdftotext", poppler.into_bytes()));
    }
    if let Some(bytes) = qdf(path, dir) {
        texts.push(("qpdf --qdf", bytes));
    }
    for (reader, text) in &texts {
        for word in gone {
            if contains(text, word) {
                wrong.push(format!("{reader} still finds {word:?}"));
            }
        }
        for word in kept {
            if !contains(text, word) {
                wrong.push(format!("{reader} lost the control {word:?}"));
            }
        }
    }
    wrong
}

/// Which readers are installed, for the report's names.
fn installed() -> String {
    let mut have = vec!["tpdf text"];
    if Command::new("pdftotext").arg("-v").output().is_ok() {
        have.push("pdftotext");
    }
    if Command::new("qpdf").arg("--version").output().is_ok() {
        have.push("qpdf --qdf");
    }
    have.join(", ")
}

/// A page's text as the service's workers extract it.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .unwrap_or_else(|_| Err(E::from("the render service did not answer".to_string())))
}

/// The regions file for `line` on `page` (counted from 1), measured from the
/// page's own extraction: each character's box, in the displayed page's
/// points, unioned, as `x, y, w, h`.
fn region_for(path: &Path, page: u32, line: &str) -> serde_json::Value {
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let info = wait(|r| service.open(path.to_path_buf(), true, None, r)).expect("opens");
    let text = wait(|r| service.text(info.id, page - 1, None, r)).expect("text");
    let codes: Vec<char> = text
        .codes
        .iter()
        .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
        .collect();
    let joined: String = codes.iter().collect();
    let start = joined.find(line).expect("the line is on the page");
    let start = joined[..start].chars().count();
    let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for at in start..start + line.chars().count() {
        let q = &text.boxes[at * 4..at * 4 + 4];
        if q[2] > q[0] {
            l = l.min(q[0]);
            t = t.min(q[1]);
            r = r.max(q[2]);
            b = b.max(q[3]);
        }
    }
    serde_json::json!({ "page": page, "rect": [l, t, r - l, b - t] })
}

/// The application's path, in this process, on the regions the tool marks for
/// `queries`: marked on an `Edits`, asked, written, verified.
fn app_path(
    path: &Path,
    out: &Path,
    queries: &[(SearchKind, &str)],
    password: Option<&str>,
) -> Result<tpdf_lib::redact::Applied, String> {
    use tpdf_lib::redaction::{ask_redactions, redact_copy_asked};
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let hashing = file.try_clone().map_err(|e| e.to_string())?;
    let info = wait(|r| {
        service.open_handed(
            path.to_path_buf(),
            Some(file),
            true,
            password.map(str::to_string),
            r,
        );
    })
    .map_err(|refusal| refusal.reason)?;
    let count = u32::try_from(info.page_count).map_err(|e| e.to_string())?;
    let compiled: Vec<(SearchKind, String, Prepared)> = queries
        .iter()
        .map(|(kind, q)| {
            let options = Options {
                regex: *kind == SearchKind::Pattern,
                ..Options::default()
            };
            (
                *kind,
                (*q).to_string(),
                Prepared::new(q, options).expect("compiles"),
            )
        })
        .collect();
    let pages: Vec<u32> = (1..=count).collect();
    let found = search_pages(&compiled, &pages, |at| {
        wait(|r| service.text(info.id, at, None, r))
    })?;
    let edits = tpdf_lib::edits::Edits::default();
    edits.open(
        info.id,
        count,
        Some(tpdf_lib::fingerprint::Opened {
            file: hashing,
            what: path.to_path_buf(),
        }),
    );
    for (page, areas) in match_regions(&found) {
        for area in areas {
            edits.redact(info.id, u64::from(page) + 1, area)?;
        }
    }
    let asked = tauri::async_runtime::block_on(ask_redactions(&edits, &service, info.id))?;
    tauri::async_runtime::block_on(redact_copy_asked(
        &service,
        library_dir(),
        info.id,
        asked,
        path.display().to_string(),
        out.display().to_string(),
        None,
    ))
    .map_err(tpdf_lib::redaction::Stopped::into_message)
}

/// The tool's arguments for `queries`.
fn query_args(queries: &[(SearchKind, &str)]) -> Vec<String> {
    queries
        .iter()
        .flat_map(|(kind, q)| {
            [
                match kind {
                    SearchKind::Text => "--text".to_string(),
                    SearchKind::Pattern => "--pattern".to_string(),
                },
                (*q).to_string(),
            ]
        })
        .collect()
}

fn run(args: &[String], env: &[(&str, &str)]) -> (i32, serde_json::Value, String) {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = tool(&refs, env);
    (code, parsed(&stdout), stderr)
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// 1 and 2: what goes, what stays, and what a dry run predicts.
pub(super) fn removes_what_it_finds(report: &mut Report) {
    let dir = scratch("redact");
    let input = dir.join("contacts.pdf");
    std::fs::write(&input, contacts_pdf()).expect("contacts");
    let kept = ["CONTROL-KEEP alpha", "CONTROL-KEEP beta"];
    let secrets = [
        "jane.doe@example.com",
        "DE89 3704 0044 0532 0130 00",
        "Rumpelstilzchen",
        "Sign here: J. Doe",
    ];

    // The control: every reader finds every string in the input.
    let before = readers(&input, &dir, &[], &secrets);
    report.check(
        &format!("control: {} find every string in the input", installed()),
        before.is_empty(),
        &before.join("; "),
    );

    let regions_file = dir.join("regions.json");
    std::fs::write(
        &regions_file,
        serde_json::json!([region_for(&input, 1, "Sign here: J. Doe")]).to_string(),
    )
    .expect("regions");

    let cases: [(&str, Vec<String>, Vec<&str>); 4] = [
        (
            "by --text",
            query_args(&[(SearchKind::Text, "rumpelstilzchen")]),
            vec!["Rumpelstilzchen"],
        ),
        (
            "by --pattern, an e-mail address and an IBAN",
            query_args(&[(SearchKind::Pattern, EMAIL), (SearchKind::Pattern, IBAN)]),
            vec!["jane.doe@example.com", "DE89 3704 0044 0532 0130 00"],
        ),
        (
            "by --regions",
            vec!["--regions".into(), s(&regions_file)],
            vec!["Sign here: J. Doe"],
        ),
        (
            "by all three at once",
            [
                query_args(&[
                    (SearchKind::Text, "Rumpelstilzchen"),
                    (SearchKind::Pattern, EMAIL),
                    (SearchKind::Pattern, IBAN),
                ]),
                vec!["--regions".into(), s(&regions_file)],
            ]
            .concat(),
            secrets.to_vec(),
        ),
    ];
    let probe = std::env::var_os("TPDF_REDACT_PROBE").map(PathBuf::from);
    for (at, (what, args, gone)) in cases.iter().enumerate() {
        let out = dir.join(format!("redacted-{at}.pdf"));
        let line = [
            vec![
                "redact".to_string(),
                s(&input),
                "-o".into(),
                s(&out),
                "--json".into(),
            ],
            args.clone(),
        ]
        .concat();
        let (code, json, stderr) = run(&line, &[]);
        report.check(
            &format!("{what}: the copy is written and the report says so"),
            (code == 0 || code == 1)
                && json["written"] == true
                && json["verified"] == (code == 0)
                && out.exists(),
            &format!("exit {code}: {stderr}"),
        );
        // Nothing on stderr: a verdict is not an error, and every worker the
        // run spawned shares this stream --- including a pre-spawned spare
        // that ends without a document (`docs/TRAPS.md`).
        report.check(
            &format!("{what}: nothing is printed on stderr"),
            stderr.is_empty(),
            &stderr,
        );
        let wrong = readers(&out, &dir, gone, &kept);
        report.check(
            &format!("{what}: no reader finds a matched string, and every one finds both controls"),
            wrong.is_empty(),
            &wrong.join("; "),
        );
        // The other secrets are untouched: the removal took what was asked.
        let others: Vec<&str> = secrets
            .iter()
            .copied()
            .filter(|x| !gone.contains(x))
            .collect();
        let over = readers(&out, &dir, &[], &others);
        report.check(
            &format!("{what}: what was not asked for is still there"),
            over.is_empty(),
            &over.join("; "),
        );

        // 2: a dry run of the same line writes nothing and predicts this run.
        let dry_out = dir.join(format!("dry-{at}.pdf"));
        let mut dry = line.clone();
        dry[3] = s(&dry_out);
        dry.push("--dry-run".into());
        let (dry_code, dry_json, _) = run(&dry, &[]);
        let same = |key: &str| dry_json[key] == json[key];
        let pages_agree = dry_json["pages"]
            .as_array()
            .zip(json["pages"].as_array())
            .is_some_and(|(a, b)| {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| {
                        [
                            "page",
                            "hits",
                            "regions",
                            "text_removals",
                            "form_text_removals",
                            "image_removals",
                            "taking",
                            "left",
                        ]
                        .iter()
                        .all(|k| x[k] == y[k])
                    })
            });
        report.check(
            &format!("{what}: a dry run writes nothing and counts what the write took"),
            dry_code == 0
                && !dry_out.exists()
                && dry_json["written"] == false
                && dry_json["dry_run"] == true
                && same("regions")
                && same("removals")
                && same("searches")
                && pages_agree
                && json["regions"].as_u64().is_some_and(|n| n > 0),
            &format!("exit {dry_code}: {dry_json} against {json}"),
        );
        if let (Some(probe), 3) = (&probe, at) {
            let _ = std::fs::create_dir_all(probe);
            let _ = std::fs::copy(&input, probe.join("contacts.pdf"));
            let _ = std::fs::copy(&out, probe.join("contacts-redacted.pdf"));
            let _ = std::fs::write(
                probe.join("contacts-expected.json"),
                serde_json::json!({ "gone": secrets, "kept": kept }).to_string(),
            );
        }
    }

    // Nothing matched: nothing written, exit 0, and the report says zero.
    let none = dir.join("none.pdf");
    let (code, json, stderr) = run(
        &[
            "redact".into(),
            s(&input),
            "-o".into(),
            s(&none),
            "--text".into(),
            "Zanzibar".into(),
            "--json".into(),
        ],
        &[],
    );
    report.check(
        "a query that matches nothing is not an error: exit 0, zero matches, nothing written",
        code == 0
            && json["written"] == false
            && json["searches"][0]["matches"] == 0
            && json["regions"] == 0
            && !none.exists(),
        &format!("exit {code}: {stderr}{json}"),
    );

    // `--pages` limits the search: page 2's copy of the word stays.
    let first = dir.join("first-page.pdf");
    let (code, json, stderr) = run(
        &[
            "redact".into(),
            s(&input),
            "-o".into(),
            s(&first),
            "--text".into(),
            "Rumpelstilzchen".into(),
            "--pages".into(),
            "1".into(),
            "--json".into(),
        ],
        &[],
    );
    let page_one = text_of(&first, &[], &["--pages", "1"]);
    let page_two = text_of(&first, &[], &["--pages", "2"]);
    report.check(
        "--pages 1 removes the word from page 1 and leaves page 2's",
        (code == 0 || code == 1)
            && json["searches"][0]["matches"] == 1
            && !page_one.contains("Rumpelstilzchen")
            && page_two.contains("Rumpelstilzchen"),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document, where it is, and what to search it for.
type Parity<'a> = (String, PathBuf, Vec<(SearchKind, &'a str)>);

/// 3: the tool's verdict is the application's, on three documents.
pub(super) fn verdict_is_the_applications(report: &mut Report) {
    let dir = scratch("redact-parity");
    let contacts = dir.join("contacts.pdf");
    std::fs::write(&contacts, contacts_pdf()).expect("contacts");
    let shared = dir.join("shared-form.pdf");
    std::fs::write(&shared, shared_form_pdf()).expect("shared form");
    let mut cases: Vec<Parity<'_>> = vec![
        (
            "contacts.pdf".into(),
            contacts,
            vec![
                (SearchKind::Pattern, EMAIL),
                (SearchKind::Text, "Rumpelstilzchen"),
            ],
        ),
        (
            "shared-form.pdf".into(),
            shared,
            vec![(SearchKind::Text, "Rumpelstilzchen")],
        ),
    ];
    for (name, query) in [
        ("text-base14.pdf", "4711-0815"),
        ("text-marked.pdf", "4711-0815"),
    ] {
        match fixture(name) {
            Some(path) => cases.push((name.into(), path, vec![(SearchKind::Text, query)])),
            None => report.skip(&format!("the parity check on {name}"), "not generated"),
        }
    }
    for (name, path, queries) in &cases {
        println!("[INFO] parity {name}: application path");
        let theirs_out = dir.join(format!("app-{name}"));
        let ours_out = dir.join(format!("tool-{name}"));
        let app = app_path(path, &theirs_out, queries, None);
        println!("[INFO] parity {name}: command-line path");
        let line = [
            vec![
                "redact".to_string(),
                s(path),
                "-o".into(),
                s(&ours_out),
                "--json".into(),
            ],
            query_args(queries),
        ]
        .concat();
        let (code, json, stderr) = run(&line, &[]);
        let Ok(app) = app else {
            report.check(
                &format!("{name}: the application's path ran"),
                false,
                &format!("{app:?}"),
            );
            continue;
        };
        let all: Vec<String> = json["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
        // The tool's own reader, a search of the written file, can only add
        // reasons; everything else is the application's, in its order.
        let (own, reasons): (Vec<String>, Vec<String>) = all
            .iter()
            .cloned()
            .partition(|r| r.contains("searching the written file for"));
        let clean = app.verified && own.is_empty();
        report.check(
            &format!(
                "{name}: verified is {clean} and every other reason is the application's, word for word"
            ),
            json["verified"] == clean && reasons == app.why && json["regions"] == app.regions,
            &format!(
                "exit {code}: {stderr}\n       tool {all:?}\n       app  {:?}",
                app.why
            ),
        );
        let leaves = *name == "shared-form.pdf";
        report.check(
            &format!(
                "{name}: the search of the written file {}",
                if leaves {
                    "finds the word in the form the removal left, beside the application's reason"
                } else {
                    "finds nothing"
                }
            ),
            own.is_empty() != leaves
                && (!leaves || app.why.iter().any(|r| r.contains("drawn 2 time(s)"))),
            &format!("{own:?} beside {:?}", app.why),
        );
        report.check(
            &format!("{name}: the exit code is {}", if clean { 0 } else { 1 }),
            code == if clean { 0 } else { 1 } && ours_out.exists(),
            &format!("exit {code}"),
        );
        if *name == "text-marked.pdf" {
            report.check(
                "text-marked.pdf: not verified for a copy the removal leaves, which is not the platform's doing",
                !app.verified && reasons.iter().any(|r| r.contains("is still in the file")),
                &format!("{reasons:?}"),
            );
        }
        if !app.verified
            && app
                .why
                .iter()
                .all(|r| r.contains("could not be shown unreadable"))
        {
            println!(
                "[INFO] {name}: every reason is the OCR gate's --- no fixture here can reach exit 0 \
                 while the platform's engine cannot read (docs/TRAPS.md)"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 4: refusals, and what the two opt-ins do.
pub(super) fn refusals_write_nothing(report: &mut Report) {
    let dir = scratch("redact-refused");
    let input = dir.join("contacts.pdf");
    std::fs::write(&input, contacts_pdf()).expect("contacts");
    let out = dir.join("out.pdf");
    let redact = |extra: &[&str], output: &Path| -> (i32, String) {
        let mut args = vec!["redact".to_string(), s(&input), "-o".into(), s(output)];
        args.extend(extra.iter().map(|a| (*a).to_string()));
        let (code, _, stderr) = run(&args, &[]);
        (code, stderr)
    };

    // An output that exists: refused and left alone, then replaced with --force.
    std::fs::write(&out, b"already here").expect("existing");
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen"], &out);
    report.check(
        "an output that exists is refused (3) and left as it was",
        code == 3 && std::fs::read(&out).ok().as_deref() == Some(&b"already here"[..]),
        &format!("exit {code}: {stderr}"),
    );
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen", "--force"], &out);
    report.check(
        "--force replaces it",
        (code == 0 || code == 1) && std::fs::read(&out).is_ok_and(|b| b.starts_with(b"%PDF")),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_file(&out);

    // The input under another name.
    let linked = dir.join("linked.pdf");
    let same = std::fs::hard_link(&input, &linked).is_ok();
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen", "--force"], &linked);
    report.check(
        "an output that is the input under another name is a malformed line (2)",
        same && code == 2 && stderr.contains("under another name"),
        &format!("exit {code}: {stderr}"),
    );

    // XFA, for the write and for the dry run.
    let xfa = dir.join("xfa.pdf");
    std::fs::write(&xfa, xfa_pdf()).expect("xfa");
    for dry in [false, true] {
        let mut args = vec![
            "redact".to_string(),
            s(&xfa),
            "-o".into(),
            s(&out),
            "--text".into(),
            "Rumpelstilzchen".into(),
        ];
        if dry {
            args.push("--dry-run".into());
        }
        let (code, _, stderr) = run(&args, &[]);
        report.check(
            &format!(
                "an XFA form is refused (3){}, nothing written",
                if dry { " by a dry run too" } else { "" }
            ),
            code == 3 && stderr.contains("XFA") && !out.exists(),
            &format!("exit {code}: {stderr}"),
        );
    }

    // A signed document.
    match fixture("incr-signed.pdf") {
        None => report.skip(
            "the signed-document checks",
            "incr-signed.pdf is not generated",
        ),
        Some(signed) => {
            let args = |extra: &[&str]| -> Vec<String> {
                let mut a = vec![
                    "redact".to_string(),
                    s(&signed),
                    "-o".into(),
                    s(&out),
                    "--text".into(),
                    "page 1".into(),
                    "--json".into(),
                ];
                a.extend(extra.iter().map(|x| (*x).to_string()));
                a
            };
            let (code, _, stderr) = run(&args(&[]), &[]);
            report.check(
                "a signed document is refused (3) and nothing is written",
                code == 3 && stderr.contains("--invalidate-signatures") && !out.exists(),
                &format!("exit {code}: {stderr}"),
            );
            let (code, json, stderr) = run(&args(&["--invalidate-signatures"]), &[]);
            let (_, verified, _) = tool(&["verify", "--json", &s(&out)], &[]);
            let verdicts: Vec<String> = parsed(&verified)["files"][0]["signatures"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| {
                    x["integrity"]["verdict"]
                        .as_str()
                        .unwrap_or("?")
                        .to_string()
                })
                .collect();
            report.check(
                "with --invalidate-signatures it is written, says so, and its signature no longer reads intact",
                (code == 0 || code == 1)
                    && json["signatures_invalidated"] == 1
                    && !verdicts.is_empty()
                    && verdicts.iter().all(|v| v != "intact"),
                &format!("exit {code}: {stderr} {verdicts:?}"),
            );
            let _ = std::fs::remove_file(&out);
        }
    }

    // A locked document: refused without its password, redacted with it, and
    // the copy is still encrypted.
    match fixture("incr-encrypted-pw.pdf") {
        None => report.skip(
            "the password checks",
            "incr-encrypted-pw.pdf is not generated",
        ),
        Some(locked) => {
            let base = vec![
                "redact".to_string(),
                s(&locked),
                "-o".into(),
                s(&out),
                "--text".into(),
                "page 2".into(),
            ];
            let (code, _, stderr) = run(&base, &[]);
            report.check(
                "a locked document without its password is refused (3)",
                code == 3 && stderr.contains("--password-env") && !out.exists(),
                &format!("exit {code}: {stderr}"),
            );
            let with = [
                base.clone(),
                vec!["--password-env".into(), "TPDF_IT_PASSWORD".into()],
            ]
            .concat();
            let (code, _, stderr) = run(&with, &[("TPDF_IT_PASSWORD", "swordfish")]);
            let encrypted = Command::new("qpdf")
                .args(["--is-encrypted", &s(&out)])
                .status()
                .map(|st| st.code() == Some(0))
                .ok();
            let text = text_of(
                &out,
                &[("TPDF_IT_PASSWORD", "swordfish")],
                &["--password-env", "TPDF_IT_PASSWORD"],
            );
            report.check(
                "with its password it is redacted, the copy is still encrypted, and the word is gone",
                (code == 0 || code == 1)
                    && encrypted.unwrap_or(true)
                    && !text.contains("page 2")
                    && text.contains("page 1"),
                &format!("exit {code}: {stderr} encrypted {encrypted:?}, text {text:?}"),
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
