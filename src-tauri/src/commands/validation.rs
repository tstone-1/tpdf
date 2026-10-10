//! Adding long-term validation data to a document that is already signed.
//!
//! One command, [`add_validation_data`], and the tail it shares with
//! `tpdf long-term` ([`finish`]). The steps are `longterm::existing`'s: a
//! **worker** reads the file's signatures and hands their values over, this
//! **app process** --- which holds the network and parses no document ---
//! asks the certificate authorities and the timestamp authority the reader
//! chose, a worker appends the `/DSS` and builds the document timestamp's
//! revision, the copy is written (`save::write_signed`), and a **worker**
//! holding the written file reports every signature in it. What the reader is
//! told is that last reading.
//!
//! **No key is used and nothing is held.** A signing keeps the signature the
//! OS made while the reader decides about a timestamp that did not come,
//! because asking for the key again is a cost; here there is nothing made
//! that a second try could not make again, so a refusal is a sentence and
//! the command is run again.
//!
//! **The signed-save warning is not shown**, for the reason signing shows
//! none: this appends revisions and writes no byte of the earlier ones, and
//! refuses unless every signature the document held reads afterwards as it
//! did before.

use std::path::{Path, PathBuf};

use super::outside_of;
use crate::longterm::existing::{self, Refusal, VouchFor};
use crate::render::RenderService;
use crate::{edits, save};

/// Everything adding validation data asks of the world outside this process:
/// whether a signer or an authority is trusted, the certificate authorities'
/// revocation data, and the timestamp authority for the archive timestamp.
/// [`asking_the_world`] in the application and the tool; a test's own
/// authorities in a test.
pub(crate) struct Asking<'a> {
    /// Whether a chain is trusted for a purpose, asked before anything is
    /// fetched for it: `longterm::vouched_for`.
    pub vouch: &'a VouchFor<'a>,
    /// One OCSP response or revocation list: `longterm::fetch_blocking`.
    pub fetch: &'a mut crate::longterm::Fetch<'a>,
    /// The archive timestamp: the chosen authority's token over the covered
    /// pieces of the file, or the sentence saying why not.
    pub archive: &'a mut crate::longterm::Archive<'a>,
}

/// [`Asking`] as the application and the command-line tool ask: the network
/// through `tsa`, with `authority` the timestamp authority the reader chose,
/// and `anchors` --- the system's store, or the roots the tool's environment
/// names --- for every signer and authority.
pub(crate) fn asking_the_world<R>(
    anchors: crate::trust::Anchors<'_>,
    authority: &url::Url,
    then: impl FnOnce(Asking<'_>) -> R,
) -> R {
    let vouch = |blob: &[u8], purpose: crate::trust::Purpose, now: u64| {
        crate::longterm::vouched_for(blob, purpose, now, anchors)
    };
    let mut fetch = crate::longterm::fetch_blocking;
    let mut archive = |pieces: &[&[u8]]| {
        crate::tsa::ask_over_range_blocking(authority, pieces, &crate::tsa::LIMITS)
            .map_err(|why| why.sentence(authority.host_str().unwrap_or_default()))
    };
    then(Asking {
        vouch: &vouch,
        fetch: &mut fetch,
        archive: &mut archive,
    })
}

/// The copy written and read back: what a worker found in it, and what was
/// added for which fields.
pub(crate) struct Finished {
    /// What a worker read in the written file.
    pub signatures: Vec<crate::docinfo::Signature>,
    /// The fields the data was added for: every signature and timestamp the
    /// document held.
    pub covered: Vec<String>,
    /// The archive timestamp's field.
    pub archive: String,
    /// `words::after_long_term`, for the window and the command line alike.
    pub summary: String,
}

/// Why it stopped. Nothing was written for the first two.
pub(crate) enum Stopped {
    /// Refused, with the reason.
    Refused(Refusal),
    /// The copy could not be written.
    Unwritten(String),
    /// The copy was written, and could not be reopened or read back.
    Unread(String),
    /// The copy was written, and does not read back as it was built.
    ReadBack(Refusal),
}

impl Stopped {
    /// What the reader is told. `name` is the copy's file name.
    pub(crate) fn sentence(&self, name: &str) -> String {
        match self {
            Stopped::Refused(why) => format!("{} --- nothing was written", why.sentence()),
            Stopped::Unwritten(why) | Stopped::Unread(why) => why.clone(),
            Stopped::ReadBack(why) => format!(
                "{name} was written, but reading it back did not find what was added: {}. Do \
                 not rely on that copy.",
                why.sentence()
            ),
        }
    }

    /// Whether this is tpdf's own failure rather than the document's or an
    /// authority's: a worker that died, a check of its own result that did
    /// not pass, or a written copy that does not read back. The command line
    /// exits 4 for these and 3 for every other.
    pub(crate) fn tpdf_failed(&self) -> bool {
        match self {
            Stopped::Refused(why) => why.tpdf_failed(),
            Stopped::Unwritten(_) => false,
            Stopped::Unread(_) | Stopped::ReadBack(_) => true,
        }
    }
}

/// The file name of `out`, as the sentences say it.
pub(crate) fn name_of(out: &Path) -> String {
    out.file_name().map_or_else(
        || out.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The whole of adding validation data to `original`, **for the window and
/// the command line alike**: what `longterm::existing::add` builds, the
/// write, and the read-back by a worker of the file at `out`.
///
/// `write` creates the copy at `out`, and is the caller's to pass for
/// `commands::sign::finish`'s reason: `scripts/check_writers.py` finds the
/// commands that write a file by the writers named in them. On a blocking
/// thread: the requests, the write and the read-back all wait.
///
/// # Errors
///
/// [`Stopped`], which says whether anything was written.
pub(crate) fn finish(
    original: &[u8],
    out: &Path,
    now: u64,
    checking: &dyn crate::save::Verifier,
    asking: Asking<'_>,
    write: &dyn Fn(&[u8]) -> Result<(), String>,
) -> Result<Finished, Stopped> {
    let added = existing::add(
        original,
        now,
        checking,
        asking.vouch,
        asking.fetch,
        asking.archive,
    )
    .map_err(Stopped::Refused)?;
    write(&added.bytes).map_err(Stopped::Unwritten)?;

    let mut written = std::fs::File::open(out).map_err(|e| {
        Stopped::Unread(format!(
            "the copy was written and could not be reopened: {e}"
        ))
    })?;
    let signatures = checking
        .signatures(&mut written, added.bytes.len())
        .map_err(|e| {
            Stopped::Unread(format!(
                "the copy was written and could not be checked: {e}"
            ))
        })?;
    let appended = (added.bytes.len() - original.len()) as u64;
    existing::read_back(&added.before, &signatures, appended).map_err(Stopped::ReadBack)?;
    let archive = existing::archive_of(&signatures)
        .map(|archive| archive.field.clone())
        .unwrap_or_default();
    // Each field with whether it is a document timestamp, which the sentence
    // names as one.
    let kinds: Vec<(String, bool)> = added
        .before
        .iter()
        .filter(|signature| signature.signed)
        .map(|signature| (signature.field.clone(), signature.kind == "ETSI.RFC3161"))
        .collect();
    let summary = crate::words::after_long_term(&name_of(out), &kinds, &archive);
    let covered: Vec<String> = kinds.into_iter().map(|(field, _)| field).collect();
    Ok(Finished {
        signatures,
        covered,
        archive,
        summary,
    })
}

/// Refuses a document too large for a worker to build a revision of: the
/// bound signing holds a document to, `save::APPEND_MAX_BYTES`, for its
/// reason (`sign_cms::refuse_too_large`).
///
/// # Errors
///
/// `len` is over the bound.
pub(crate) fn refuse_too_large(len: u64) -> Result<(), String> {
    if len > save::APPEND_MAX_BYTES {
        return Err(format!(
            "This document is {} MB, and tpdf adds validation data to documents of up to {} MB.",
            len / 1_000_000,
            save::APPEND_MAX_BYTES / 1_000_000
        ));
    }
    Ok(())
}

/// The refusal for a document with edits nobody has saved: the data is for
/// the signatures in the file, and the reader is looking at the file plus
/// their edits.
///
/// # Errors
///
/// `dirty` is true.
pub(crate) fn refuse_unsaved(dirty: bool) -> Result<(), String> {
    if dirty {
        return Err(
            "Save your changes first: validation data is added to the file as it is on disk, \
             and this document has edits that are not in it yet."
                .into(),
        );
    }
    Ok(())
}

/// Refuses the original's own name as the copy's, before anything is asked
/// of anybody.
///
/// # Errors
///
/// `out` is `source`, under this name or another.
pub(crate) fn refuse_same_file(source: &Path, out: &Path) -> Result<(), String> {
    if save::same_file(source, out) {
        return Err(
            "The document with the data added is written as a new file --- choose a name other \
             than the original's."
                .into(),
        );
    }
    Ok(())
}

/// Seconds since the epoch, now.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Adds long-term validation data for every signature of the open document,
/// and an archive timestamp from `timestamp` over the whole, and writes the
/// result to `path`. Answers the sentence the reader is shown.
///
/// **The original file is not modified**: `path` is a new file, and naming
/// the original is refused. Refused too, before anything is asked of
/// anybody: a document with unsaved edits, and a timestamp authority that is
/// not an `http` or `https` address without credentials --- judged here,
/// because the webview is not trusted to have kept to that.
///
/// # Errors
///
/// Every refusal as a sentence, which says whether anything was written.
#[tauri::command]
pub async fn add_validation_data(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    timestamp: String,
) -> Result<String, String> {
    let authority = crate::tsa::authority(&timestamp).map_err(|why| why.sentence(""))?;
    refuse_unsaved(edits.state(doc)?.dirty)?;
    let opened_as = edits.plan(doc)?.opened_as.ok_or_else(|| {
        "tpdf could not record what this file looked like when it was opened, so it cannot \
         tell that what it adds to is that file --- reopen it and try again"
            .to_string()
    })?;
    let len = std::fs::metadata(&source)
        .map_err(|e| format!("could not measure {source}: {e}"))?
        .len();
    refuse_too_large(len)?;
    // Before anybody is asked: `write_signed` refuses the original's own
    // name too, but only once the certificate authorities have answered and
    // the timestamp authority has issued a token for a copy never written.
    // (An output that exists is not refused here: the save panel asked.)
    refuse_same_file(Path::new(&source), Path::new(&path))?;

    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(source);
        let out = PathBuf::from(path);
        // The bytes the reader opened, read once: every revision is built
        // against exactly these, and the survey a worker makes is of them.
        let original = save::read_to_sign(&source, &opened_as).map_err(|why| {
            if why.changed {
                "the file on disk is not the one that was opened, so nothing was added to it \
                 --- reopen it and try again"
                    .to_string()
            } else {
                why.message
            }
        })?;
        asking_the_world(crate::trust::Anchors::System, &authority, |asking| {
            finish(
                &original,
                &out,
                now(),
                checking.as_ref(),
                asking,
                &|bytes| save::write_signed(&source, &out, bytes).map_err(|why| why.message),
            )
        })
        .map(|finished| finished.summary)
        .map_err(|stopped| stopped.sentence(&name_of(&out)))
    })
    .await
    .map_err(|e| format!("adding the validation data did not run: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::test_tsa::{mint, Imprint, Pki, Plan, Serve};
    use crate::sign_cms::testkeys::{plain_pdf, Soft};

    /// The plain document signed by the fake PKI's signer and timestamped by
    /// its authority: a document somebody else signed.
    fn signed(pki: &Pki) -> Vec<u8> {
        let at = now();
        let original = plain_pdf();
        let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
        let made = crate::sign_cms::sign(
            original,
            unsigned,
            at,
            &pki.signer.certificate,
            &pki.chain,
            &Soft::p256(pki.signer.seed),
        )
        .expect("made");
        let value = made.value().expect("a value");
        let token = mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(&value),
            None,
            at,
            &pki.tsa,
        );
        let cms = made.stamped(&token).expect("stamped");
        made.seal(Some(cms)).expect("sealed")
    }

    fn good() -> Pki {
        Pki::start(Plan {
            signer_ocsp: Some(Serve::Good),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        })
    }

    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("tpdf-validation-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        (dir.join("signed.pdf"), dir.join("kept.pdf"))
    }

    /// [`finish`] over `original`, trusting the fake PKI's two roots, read
    /// back by `checking`, and written by `write`.
    fn finished(
        pki: &Pki,
        original: &[u8],
        out: &Path,
        checking: &dyn crate::save::Verifier,
        write: &dyn Fn(&[u8]) -> Result<(), String>,
    ) -> Result<Finished, Stopped> {
        let roots = [pki.root.certificate.clone(), pki.tsa.root.clone()];
        let vouch = |blob: &[u8], purpose: crate::trust::Purpose, now: u64| {
            crate::longterm::vouched_for(blob, purpose, now, crate::trust::Anchors::Only(&roots))
        };
        let mut fetch = crate::longterm::fetch_blocking;
        let mut archive = |pieces: &[&[u8]]| {
            Ok(mint(
                Imprint::Sha256,
                &Imprint::Sha256.digest(&pieces.concat()),
                None,
                now(),
                &pki.tsa,
            ))
        };
        finish(
            original,
            out,
            now(),
            checking,
            Asking {
                vouch: &vouch,
                fetch: &mut fetch,
                archive: &mut archive,
            },
            write,
        )
    }

    #[test]
    fn the_copy_is_written_read_back_and_said_in_one_sentence() {
        let pki = good();
        let original = signed(&pki);
        let (source, out) = scratch("written");
        std::fs::write(&source, &original).expect("a source");
        let done = finished(&pki, &original, &out, &crate::save::Here, &|bytes| {
            save::write_signed(&source, &out, bytes).map_err(|why| why.message)
        });
        let Ok(done) = done else {
            panic!("stopped");
        };
        assert_eq!(done.covered, ["Signature1"]);
        assert_eq!(done.archive, "Signature2");
        assert_eq!(
            done.summary,
            "Added long-term validation data for the signature Signature1, then the archive \
             timestamp Signature2 over the whole, and saved the result to kept.pdf. Read back \
             after writing: that signature is intact, every certificate asked about reads as \
             not revoked from the document's own data, and the archive timestamp covers the \
             whole file."
        );
        let written = std::fs::read(&out).expect("the copy");
        assert_eq!(&written[..original.len()], &original[..]);
        assert_eq!(std::fs::read(&source).expect("the source"), original);
        assert_eq!(done.signatures.iter().filter(|s| s.signed).count(), 2);
    }

    #[test]
    fn a_refusal_writes_nothing_and_says_so() {
        let pki = good();
        let (_, out) = scratch("refused");
        let wrote = std::cell::Cell::new(false);
        let stopped = match finished(&pki, &plain_pdf(), &out, &crate::save::Here, &|_| {
            wrote.set(true);
            Ok(())
        }) {
            Err(stopped) => stopped,
            Ok(_) => panic!("not refused"),
        };
        assert!(matches!(stopped, Stopped::Refused(Refusal::Unsigned)));
        assert_eq!(
            stopped.sentence("kept.pdf"),
            "this document has no signature, so there is nothing to add long-term validation \
             data for --- nothing was written"
        );
        assert!(!stopped.tpdf_failed());
        assert!(!wrote.get() && !out.exists());
    }

    #[test]
    fn a_copy_that_could_not_be_written_is_not_tpdfs_failure() {
        let pki = good();
        let original = signed(&pki);
        let (_, out) = scratch("unwritten");
        let stopped = match finished(&pki, &original, &out, &crate::save::Here, &|_| {
            Err("the disk is full".into())
        }) {
            Err(stopped) => stopped,
            Ok(_) => panic!("not stopped"),
        };
        assert!(matches!(stopped, Stopped::Unwritten(_)));
        assert_eq!(stopped.sentence("kept.pdf"), "the disk is full");
        assert!(!stopped.tpdf_failed());
    }

    /// A worker that builds as `save::Here` does and reads the written file
    /// back without its last signature: a copy that does not read back as it
    /// was built.
    struct Forgetful;

    impl crate::save::Verifier for Forgetful {
        fn scan(
            &self,
            _: &mut std::fs::File,
            _: usize,
            _: &[String],
            _: Option<&str>,
        ) -> Result<crate::verify::Report, String> {
            Err("not asked".into())
        }

        fn signatures(
            &self,
            file: &mut std::fs::File,
            len: usize,
        ) -> Result<Vec<crate::docinfo::Signature>, String> {
            let mut found = crate::save::Here.signatures(file, len)?;
            found.pop();
            Ok(found)
        }

        fn validation(
            &self,
            signed: &[u8],
            gathered: &crate::sign_dss::Gathered,
        ) -> Result<crate::sign_dss::Extended, String> {
            crate::save::Here.validation(signed, gathered)
        }

        fn document_timestamp(
            &self,
            signed: &[u8],
        ) -> Result<crate::sign_prepare::Unsigned, String> {
            crate::save::Here.document_timestamp(signed)
        }

        fn survey(&self, signed: &[u8]) -> Result<crate::sign_dss::Survey, String> {
            crate::save::Here.survey(signed)
        }
    }

    #[test]
    fn a_copy_that_does_not_read_back_as_built_is_named_and_is_tpdfs_failure() {
        let pki = good();
        let original = signed(&pki);
        let (source, out) = scratch("read-back");
        std::fs::write(&source, &original).expect("a source");
        let stopped = match finished(&pki, &original, &out, &Forgetful, &|bytes| {
            save::write_signed(&source, &out, bytes).map_err(|why| why.message)
        }) {
            Err(stopped) => stopped,
            Ok(_) => panic!("reported as done"),
        };
        assert!(matches!(stopped, Stopped::ReadBack(Refusal::Archive(_))));
        assert_eq!(
            stopped.sentence("kept.pdf"),
            "kept.pdf was written, but reading it back did not find what was added: tpdf's \
             own check of the document with the data added did not pass: the archive \
             timestamp is not in it. Do not rely on that copy."
        );
        assert!(stopped.tpdf_failed());
        assert!(out.exists(), "the copy the sentence names is there");
    }

    #[test]
    fn the_originals_own_name_is_refused_before_anybody_is_asked() {
        let (source, out) = scratch("same-file");
        std::fs::write(&source, b"%PDF-1.7").expect("a source");
        let why = refuse_same_file(&source, &source).expect_err("refused");
        assert!(why.contains("a name other than the original's"), "{why}");
        // Under another name too.
        let link = source.with_file_name("link.pdf");
        std::fs::hard_link(&source, &link).expect("a second name");
        assert!(refuse_same_file(&source, &link).is_err());
        assert!(refuse_same_file(&source, &out).is_ok());
    }

    /// An earlier document timestamp is said as a timestamp, not as
    /// somebody's signature.
    #[test]
    fn the_sentence_names_a_timestamp_as_one() {
        let said = |covered: &[(&str, bool)]| {
            let covered: Vec<(String, bool)> = covered
                .iter()
                .map(|(field, timestamp)| ((*field).to_string(), *timestamp))
                .collect();
            crate::words::after_long_term("kept.pdf", &covered, "Signature9")
        };
        let both = said(&[("Signature1", false), ("Signature2", true)]);
        assert!(
            both.starts_with(
                "Added long-term validation data for the signature Signature1 and the \
                 timestamp Signature2, then the archive timestamp Signature9 over the whole"
            ),
            "{both}"
        );
        assert!(both.contains("each of them is intact"), "{both}");
        let many = said(&[("A", false), ("B", false), ("C", true), ("D", true)]);
        assert!(
            many.contains("for the signatures A, B and the timestamps C, D, then"),
            "{many}"
        );
        let alone = said(&[("Signature1", true)]);
        assert!(
            alone.contains("for the timestamp Signature1, then")
                && alone.contains("that timestamp is intact"),
            "{alone}"
        );
    }

    #[test]
    fn unsaved_edits_and_a_document_too_large_are_refused() {
        assert!(refuse_unsaved(false).is_ok());
        let why = refuse_unsaved(true).expect_err("refused");
        assert!(why.starts_with("Save your changes first"), "{why}");
        assert!(refuse_too_large(save::APPEND_MAX_BYTES).is_ok());
        let why = refuse_too_large(save::APPEND_MAX_BYTES + 1).expect_err("refused");
        assert!(
            why.contains("adds validation data to documents of up to"),
            "{why}"
        );
    }
}
