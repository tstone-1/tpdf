//! Do PDFium and `docinfo.rs` say the same things about the same signatures?
//!
//! `docinfo.rs` reads signatures out of the object graph with `lopdf`: it walks
//! `/AcroForm /Fields`, recurses through `/Kids`, keeps the nodes whose `/FT` is
//! `/Sig`, and reads each one's `/V`. PDFium implements that walk itself, in C++,
//! and exports the result through `FPDF_GetSignatureCount` and friends. Neither
//! knows anything about the other.
//!
//! That makes this the instrument `links-probe --mode agree` is, for the other
//! subsystem the same reasoning applies to. `docinfo.rs`'s own tests build most
//! of their documents, and `AGENTS.md` is explicit about why that is not enough:
//! *a writer and its own reader agree about a document that is wrong*. The five
//! `incr-*` fixtures are written by pyhanko, which helps; this helps differently,
//! because it is a second **reader** rather than a second writer.
//!
//! ## What is compared, and why each one could be wrong
//!
//! * **How many signatures.** Ours is a tree walk with a depth bound and a
//!   `MAX_SIGNATURES` cap. A `/Kids` shape we mishandle shows up here and almost
//!   nowhere else.
//! * **The certificate.** PDFium hands over the `/Contents` blob it found;
//!   `docinfo::parse_certificate` is run on *that* and the answer compared with
//!   the one `docinfo` produced from `lopdf`'s blob. This is the check that
//!   matters most: picking a different signature's blob means showing a reader
//!   the wrong signer, and every other assertion here would still pass.
//! * **`/SubFilter`**, a name we decode ourselves.
//! * **`/Reason`**, a text string, which is where PDF's several string encodings
//!   get a chance to disagree.
//! * **`/M`**, compared digit for digit --- ours is reformatted for a reader and
//!   PDFium's is raw, so the digits are the part both must agree on.
//! * **The DocMDP level.** `certification_of` deliberately takes the `/Reference`
//!   entry whose `/TransformMethod` is `/DocMDP` rather than the first one, and
//!   `FPDFSignatureObj_GetDocMDPPermission` is an independent implementation of
//!   that same rule.
//! * **The signed byte range**, summed.
//!
//! ## What it cannot catch, which is not obvious
//!
//! **A bug inside `parse_certificate` is invisible here, because both sides use
//! it.** PDFium hands over the `/Contents` bytes and nothing above them --- it
//! exposes no view of the certificate set --- so the certificate comparison runs
//! our parser twice, on two independently *found* blobs. Measured, not reasoned
//! about: replacing the signer-matching logic with `certificates[0]` leaves this
//! probe at 13 of 13 on `incr-two-signers.pdf`, where both blobs carry a leaf and
//! a root, because both sides pick the same wrong element. Reversing the order
//! `docinfo` lists its fields in reddens four checks immediately.
//!
//! So this is a differential over **which blob**, not over **what the blob
//! says**. The second is the unit tests' job, and
//! `each_signed_fixture_carries_its_own_certificate` is what actually catches
//! that mutation.
//!
//! ## `--mode nested` asserts a disagreement rather than agreement
//!
//! `/AcroForm /Fields` is a *tree*, and **PDFium's signature enumeration does
//! not walk it** --- it reads the array's entries and stops, so a signature field
//! sitting under a `/Kids` node is invisible to `FPDF_GetSignatureCount`.
//! `docinfo.rs` recurses and finds it.
//!
//! That was established by control rather than inferred: `signed-nested-field.pdf`
//! and a variant differing **only** in whether the leaf sits directly in
//! `/Fields` or two `/Kids` nodes down --- same signature dictionary, byte for
//! byte --- give PDFium 1 and 0 respectively. `qpdf --check` passes the fixture.
//!
//! So on that document the two readers *must* disagree, and `--mode agree` would
//! report a count mismatch that reads like a defect in us. This mode asserts the
//! disagreement instead: one signature here, none there, and a certificate we can
//! still read. **It goes red if PDFium ever starts recursing**, which is the point
//! of writing a known limitation down as an assertion rather than as a comment.
//!
//! ## `--mode clean` is not optional
//!
//! Two readers that both find nothing agree perfectly. On a document with no
//! signatures this asserts that both report zero *and* that the run said so ---
//! without it, a probe whose PDFium half was silently broken would report full
//! agreement on every unsigned file in the corpus and look like coverage.
//!
//! Usage:
//!   signature-probe <file.pdf> [--mode read|agree|nested|clean] [--lib DIR]

use std::path::{Path, PathBuf};

use pdfium_render::prelude::{FPDF_DOCUMENT, FPDF_SIGNATURE};
use tpdf_lib::docinfo::{self, Properties};
use tpdf_lib::progressive::{self, Bindings, RawDocument};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    Agree,
    Clean,
    Nested,
}

struct Args {
    file: PathBuf,
    mode: Mode,
    library: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: signature-probe <file.pdf> [...]")?;
    let mut parsed = Args {
        file: PathBuf::from(file),
        mode: Mode::Read,
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "read" => Mode::Read,
                    "agree" => Mode::Agree,
                    "clean" => Mode::Clean,
                    "nested" => Mode::Nested,
                    other => return Err(format!("unknown mode: {other}")),
                }
            }
            "--lib" => parsed.library = PathBuf::from(value),
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(parsed)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let bindings = bind(&args.library)?;
    let document = RawDocument::open(bindings, &args.file)?;
    let ours = document.properties()?;
    let theirs = pdfium_signatures(bindings, document.handle());

    match args.mode {
        Mode::Read => {
            read(&ours, &theirs);
            Ok(true)
        }
        Mode::Agree => Ok(agree(&ours, &theirs)),
        Mode::Clean => Ok(clean(&ours, &theirs)),
        Mode::Nested => Ok(nested(&ours, &theirs)),
    }
}

fn bind(library: &Path) -> Result<Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

/// One signature as PDFium reports it.
struct Theirs {
    contents: Vec<u8>,
    sub_filter: String,
    reason: String,
    time: String,
    doc_mdp: u32,
    covered_bytes: u64,
}

/// Every signature PDFium finds, read through the raw bindings.
///
/// The high-level `PdfSignature` wrapper does not expose `/ByteRange` or
/// `/SubFilter`, and both are things we decode ourselves and could get wrong.
fn pdfium_signatures(bindings: Bindings, document: FPDF_DOCUMENT) -> Vec<Theirs> {
    let count = unsafe { bindings.FPDF_GetSignatureCount(document) };
    if count <= 0 {
        return Vec::new();
    }
    (0..count)
        .filter_map(|index| {
            let handle = unsafe { bindings.FPDF_GetSignatureObject(document, index) };
            if handle.is_null() {
                return None;
            }
            Some(one_signature(bindings, handle))
        })
        .collect()
}

fn one_signature(bindings: Bindings, handle: FPDF_SIGNATURE) -> Theirs {
    Theirs {
        contents: bytes_from(|buffer, length| unsafe {
            bindings.FPDFSignatureObj_GetContents(handle, buffer.cast(), length)
        }),
        sub_filter: {
            // 7-bit ASCII including a trailing NUL, which is not part of the name.
            let raw = bytes_from(|buffer, length| unsafe {
                bindings.FPDFSignatureObj_GetSubFilter(handle, buffer.cast(), length)
            });
            String::from_utf8_lossy(&raw)
                .trim_end_matches('\0')
                .to_string()
        },
        reason: {
            let raw = bytes_from(|buffer, length| unsafe {
                bindings.FPDFSignatureObj_GetReason(handle, buffer.cast(), length)
            });
            let wide: Vec<u16> = raw
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&wide)
                .trim_end_matches('\0')
                .to_string()
        },
        time: {
            let raw = bytes_from(|buffer, length| unsafe {
                bindings.FPDFSignatureObj_GetTime(handle, buffer.cast(), length)
            });
            String::from_utf8_lossy(&raw)
                .trim_end_matches('\0')
                .to_string()
        },
        doc_mdp: unsafe { bindings.FPDFSignatureObj_GetDocMDPPermission(handle) },
        covered_bytes: {
            // Returns a count of `int`s, not of bytes --- pairs of (offset, length).
            let wanted =
                unsafe { bindings.FPDFSignatureObj_GetByteRange(handle, std::ptr::null_mut(), 0) };
            let wanted = usize::try_from(wanted).unwrap_or(0);
            if wanted == 0 {
                0
            } else {
                let mut numbers = vec![0i32; wanted];
                let length = u64::try_from(wanted).unwrap_or(0);
                unsafe {
                    bindings.FPDFSignatureObj_GetByteRange(
                        handle,
                        numbers.as_mut_ptr(),
                        length as std::os::raw::c_ulong,
                    );
                }
                numbers
                    .chunks_exact(2)
                    .map(|pair| u64::try_from(pair[1]).unwrap_or(0))
                    .sum()
            }
        },
    }
}

/// The two-call buffer dance every one of these APIs uses.
///
/// The first call with a null buffer answers how many bytes are wanted; the
/// second fills. A zero first answer means the entry is absent, which is a
/// legitimate reading and not a failure.
fn bytes_from<F>(mut call: F) -> Vec<u8>
where
    F: FnMut(*mut u8, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let wanted = call(std::ptr::null_mut(), 0);
    let wanted = usize::try_from(wanted).unwrap_or(0);
    if wanted == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; wanted];
    let length = std::os::raw::c_ulong::try_from(wanted).unwrap_or(0);
    let written = call(buffer.as_mut_ptr(), length);
    buffer.truncate(usize::try_from(written).unwrap_or(0).min(wanted));
    buffer
}

/// Every digit in a string, in order.
///
/// `/M` is `D:20260726185227+02'00'` from PDFium and reformatted for a reader by
/// us, so the two are never equal as strings and must agree on every digit.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, evidence: &str) {
        if ok {
            self.passed += 1;
            println!("[OK]   {name}: {evidence}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name}: {evidence}");
        }
    }
}

fn read(ours: &Properties, theirs: &[Theirs]) {
    println!("docinfo found {} signature field(s)", ours.signatures.len());
    for signature in &ours.signatures {
        let named = match &signature.certificate {
            Some(certificate) => certificate.subject_cn.clone(),
            None => "(no certificate)".into(),
        };
        println!(
            "  field {:?} signed={} kind={:?} docmdp={} covers={} cert={:?}",
            signature.field,
            signature.signed,
            signature.kind,
            signature.certification,
            signature.covered_bytes,
            named
        );
        // PDFium exposes no extension accessor, so nothing below can be
        // compared against it. Printed because `openssl x509 -text` can, and it
        // is the oracle the unit tests are written against.
        if let Some(certificate) = &signature.certificate {
            let listed = |usage: &Option<Vec<String>>| match usage {
                Some(names) if names.is_empty() => "(states none)".to_string(),
                Some(names) => names.join(", "),
                None => "(not stated)".to_string(),
            };
            println!(
                "    key usage: {} | issued for: {} | authority: {:?} | unread extensions: {}",
                listed(&certificate.key_usage),
                listed(&certificate.extended_usage),
                certificate.authority,
                certificate.extensions_unread
            );
        }
    }
    println!("PDFium found {} signature(s)", theirs.len());
    for signature in theirs {
        println!(
            "  kind={:?} reason={:?} time={:?} docmdp={} covers={} contents={} bytes",
            signature.sub_filter,
            signature.reason,
            signature.time,
            signature.doc_mdp,
            signature.covered_bytes,
            signature.contents.len()
        );
    }
}

fn agree(ours: &Properties, theirs: &[Theirs]) -> bool {
    let mut report = Report {
        passed: 0,
        failed: 0,
    };

    // PDFium counts signature *objects* --- fields that carry a `/V`. Our list
    // deliberately also carries fields nobody has signed yet, because "this
    // document has an unsigned signature field" is a fact about it, so the
    // comparable population is the signed ones.
    let signed: Vec<_> = ours.signatures.iter().filter(|s| s.signed).collect();
    report.check(
        "both readers find the same number of signatures",
        signed.len() == theirs.len(),
        &format!(
            "docinfo {} signed of {} fields, PDFium {}",
            signed.len(),
            ours.signatures.len(),
            theirs.len()
        ),
    );
    if signed.len() != theirs.len() {
        println!("[FAIL] the counts differ, so nothing below could be compared");
        return false;
    }
    if signed.is_empty() {
        println!("[FAIL] this document has no signature; --mode agree needs one");
        return false;
    }

    for (index, (ours, theirs)) in signed.iter().zip(theirs.iter()).enumerate() {
        let at = |what: &str| format!("signature {}: {what}", index + 1);

        report.check(
            &at("the same /SubFilter"),
            ours.kind == theirs.sub_filter,
            &format!("{:?} against {:?}", ours.kind, theirs.sub_filter),
        );
        report.check(
            &at("the same /Reason"),
            ours.reason == theirs.reason,
            &format!("{:?} against {:?}", ours.reason, theirs.reason),
        );
        report.check(
            &at("the same signing time, digit for digit"),
            digits(&ours.when) == digits(&theirs.time),
            &format!("{:?} against {:?}", ours.when, theirs.time),
        );
        report.check(
            &at("the same DocMDP level"),
            u32::from(ours.certification) == theirs.doc_mdp,
            &format!("{} against {}", ours.certification, theirs.doc_mdp),
        );
        report.check(
            &at("the same number of signed bytes"),
            ours.covered_bytes == theirs.covered_bytes,
            &format!("{} against {}", ours.covered_bytes, theirs.covered_bytes),
        );

        // The one that matters most. Parsing PDFium's blob and comparing the
        // result against ours tests that both readers reached the *same
        // signature* --- a wrong pick shows a reader the wrong signer, and every
        // assertion above would still pass on a document whose signatures share
        // a subfilter and a date.
        let from_theirs = docinfo::parse_certificate(strip_padding(&theirs.contents));
        match (&ours.certificate, &from_theirs) {
            (Some(mine), Some(other)) => {
                report.check(
                    &at("the same certificate, from each reader's own blob"),
                    mine.serial == other.serial && mine.subject == other.subject,
                    &format!(
                        "{:?} #{} against {:?} #{}",
                        mine.subject_cn, mine.serial, other.subject_cn, other.serial
                    ),
                );
            }
            (None, None) => report.check(
                &at("neither reader found a certificate"),
                true,
                "both empty, and the blobs agree about that",
            ),
            (mine, other) => report.check(
                &at("the same certificate, from each reader's own blob"),
                false,
                &format!(
                    "docinfo {}, PDFium's blob {}",
                    if mine.is_some() {
                        "read one"
                    } else {
                        "read none"
                    },
                    if other.is_some() {
                        "read one"
                    } else {
                        "read none"
                    }
                ),
            ),
        }
    }

    println!("\n{} passed, {} failed", report.passed, report.failed);
    report.failed == 0
}

/// A signature blob is right-padded with zeros to the span its writer reserved.
fn strip_padding(blob: &[u8]) -> &[u8] {
    match blob.iter().rposition(|b| *b != 0) {
        Some(last) => &blob[..=last],
        None => &[],
    }
}

/// The known disagreement, asserted so it cannot expire in silence.
///
/// See the module note: PDFium does not recurse into `/Kids` when enumerating
/// signatures and `docinfo.rs` does, so on a nested-field document the correct
/// outcome is a *difference*. Asserting it means the day PDFium changes, this
/// says so instead of a comment quietly becoming wrong.
fn nested(ours: &Properties, theirs: &[Theirs]) -> bool {
    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    let signed: Vec<_> = ours.signatures.iter().filter(|s| s.signed).collect();

    report.check(
        "docinfo walks the field tree and finds the nested signature",
        signed.len() == 1,
        &format!(
            "{} signed of {} field(s)",
            signed.len(),
            ours.signatures.len()
        ),
    );
    report.check(
        "PDFium does not, which is the limitation under assertion",
        theirs.is_empty(),
        &format!(
            "{} signature(s) --- if this is 1, PDFium now recurses and this mode is obsolete",
            theirs.len()
        ),
    );
    report.check(
        "and the certificate is still read out of the nested field",
        signed
            .first()
            .and_then(|signature| signature.certificate.as_ref())
            .is_some_and(|certificate| !certificate.subject_cn.is_empty()),
        &match signed.first().and_then(|s| s.certificate.as_ref()) {
            Some(certificate) => certificate.subject_cn.clone(),
            None => "no certificate".into(),
        },
    );
    println!("\n{} passed, {} failed", report.passed, report.failed);
    report.failed == 0
}

/// The control: on an unsigned document both readers must report nothing.
///
/// Two readers that both find nothing agree perfectly, so `--mode agree` on an
/// unsigned file would report a clean sweep of zero comparisons. This says out
/// loud that the file has none, which is what makes a corpus of unsigned
/// documents distinguishable from a broken PDFium half.
fn clean(ours: &Properties, theirs: &[Theirs]) -> bool {
    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    report.check(
        "docinfo reports no signature",
        ours.signatures.is_empty(),
        &format!("{} field(s)", ours.signatures.len()),
    );
    report.check(
        "PDFium reports no signature",
        theirs.is_empty(),
        &format!("{} signature(s)", theirs.len()),
    );
    report.check(
        "and nothing went unread",
        ours.limits.certificates_unread == 0,
        &format!("{} unread", ours.limits.certificates_unread),
    );
    println!("\n{} passed, {} failed", report.passed, report.failed);
    report.failed == 0
}
