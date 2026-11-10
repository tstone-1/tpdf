//! The rest of a signing after the OS has signed, for the window and the
//! command line alike: the timestamp, the seal, the long-term validation data,
//! the write, and a worker's read-back of the written file.
//!
//! The window's command (`commands::sign`) and `tpdf sign` (`cli::sign`) both
//! start here from a made signature, and differ only in what each ending means
//! to its reader: the window keeps a signature whose timestamp or data did not
//! come, and the command line exits. [`finish`] is the one order of the steps,
//! and [`read_back`] the one rule the written file is held to.

use std::path::Path;

use crate::sign_cms;

/// How far a signature got: what `commands::sign::Pending` holds, and where [`finish`]
/// takes a signing up from.
pub(crate) enum Stage {
    /// Made by the key; its timestamp did not come.
    Made(sign_cms::Made),
    /// Made, timestamped and sealed; its long-term validation data did not
    /// come. `cms` is the timestamped signature, which says what to ask about;
    /// `authority` is the one that stamped it, asked again for the archive
    /// timestamp --- a retry does not name it again.
    Sealed {
        bytes: Vec<u8>,
        cms: Vec<u8>,
        field: String,
        authority: Option<url::Url>,
        /// Whether the signature locks the document: `Unsigned::locked`.
        locked: bool,
    },
}

/// Who stamps a signature: the authority's token over the signature's value,
/// or why not. `tsa::ask_blocking` in the application; a test's own authority
/// in a test.
pub(crate) type Stamp<'a> = dyn Fn(&url::Url, &[u8]) -> Result<Vec<u8>, crate::tsa::Refusal> + 'a;

/// Who stamps an archive timestamp: the authority's token over the covered
/// pieces of the file, or the sentence saying why not.
pub(crate) type ArchiveBy<'a> = dyn FnMut(&url::Url, &[&[u8]]) -> Result<Vec<u8>, String> + 'a;

/// Everything the rest of a signing asks of the world outside this process,
/// after the OS has signed: the timestamp authority, the chains the OS
/// assembles, whether it vouches for the authority, the certificate
/// authorities' revocation data, and the authority again for the archive
/// timestamp. [`asking_the_world`] in the application and the tool; a test's
/// own authorities in a test.
pub(crate) struct Asking<'a> {
    /// The token over the new signature's value: `tsa::ask_blocking`.
    pub stamp: &'a Stamp<'a>,
    /// The chain above the signer's certificate: `longterm::os_chain`.
    pub os_chain: &'a crate::longterm::OsChain<'a>,
    /// Whether the timestamp authority is trusted, asked before a fresh token
    /// is sealed and again before long-term data is fetched for it:
    /// `longterm::vouched_by_os`.
    pub vouch: &'a crate::longterm::Vouch<'a>,
    /// One OCSP response or revocation list: `longterm::fetch_blocking`.
    pub fetch: &'a mut crate::longterm::Fetch<'a>,
    /// The archive timestamp: the authority's token over the covered pieces
    /// of the file, or the sentence saying why not.
    pub archive: &'a mut ArchiveBy<'a>,
    /// Seal a timestamp from an authority `vouch` does not vouch for. False
    /// everywhere but `tpdf sign --allow-untrusted-timestamp`.
    pub any_authority: bool,
}

/// [`Asking`] as the application and the command-line tool ask: the network
/// through `tsa`, the OS for the signer's chain, and `anchors` --- the
/// system's store, or the roots the tool's environment names --- for the
/// authority.
pub(crate) fn asking_the_world<R>(
    anchors: crate::trust::Anchors<'_>,
    then: impl FnOnce(Asking<'_>) -> R,
) -> R {
    let vouch = |token: &[u8], now: u64| crate::longterm::vouched_under(token, now, anchors);
    let mut fetch = crate::longterm::fetch_blocking;
    let mut archive = ask_archive;
    then(Asking {
        stamp: &|url, value| crate::tsa::ask_blocking(url, value, &crate::tsa::LIMITS),
        os_chain: &crate::longterm::os_chain,
        vouch: &vouch,
        fetch: &mut fetch,
        archive: &mut archive,
        any_authority: false,
    })
}

/// A signing written and read back: the new field, every signature a worker
/// found in the written file, and whether they are what was written.
pub(crate) struct Finished {
    /// The new signature's field.
    pub field: String,
    /// What a worker read in the written file.
    pub signatures: Vec<crate::docinfo::Signature>,
    /// Whether that is what was asked for.
    pub read_back: ReadBack,
    /// Which of `signatures` is the one this signing made, when it is among
    /// them: [`sign_cms::ours`].
    pub ours: Option<usize>,
}

/// Why a signing stopped after the OS had signed. Nothing was written for
/// any of them but the last.
pub(crate) enum Stopped {
    /// The timestamp asked for did not come. The made signature is handed
    /// back: the window keeps it, so the key is not asked for again.
    Unstamped {
        made: Box<sign_cms::Made>,
        why: crate::tsa::Refusal,
    },
    /// The long-term data did not come, or a certificate is revoked
    /// (`why.revoked()`). The sealed, timestamped signature is handed back as
    /// the [`Stage::Sealed`] it is.
    Unextended {
        sealed: Box<Stage>,
        why: crate::longterm::Refusal,
    },
    /// Refused outright, with the sentence: the seal, or long-term data asked
    /// for with no timestamp.
    Refused(String),
    /// The signed copy could not be written.
    Unwritten(String),
    /// The signed copy was written, and could not be reopened or read back.
    Unread(String),
}

/// What the written file read back as, against what the signing asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadBack {
    /// The new signature intact, its timestamp intact when one was asked for,
    /// and --- for long-term data --- all that `longterm::check` held the
    /// same bytes to before they were written.
    Holds,
    /// The new signature is not in the file, or does not read as intact.
    Signature,
    /// The new signature is in the file and was not checked again: the
    /// document's signatures together cover more than one document's hashing
    /// budget (`integrity::MAX_HASHED`), which is spent in the order the
    /// fields are listed, and the new one is last. **Neither a failed copy nor
    /// a checked one.** [`sign_cms::Made::seal`] found this signature intact
    /// over the bytes that were then written, on a budget of its own; what
    /// did not happen is the second look at the file. Since 2026-10-09: until
    /// then this was [`ReadBack::Signature`], and a correct copy of a large
    /// document with several signatures was called one not to rely on.
    Unchecked,
    /// The new signature is intact and its timestamp is missing or is not.
    Timestamp,
    /// Both are intact and the long-term data does not read as it was
    /// checked before writing: what `longterm::check` says of the file.
    LongTerm(crate::longterm::Refusal),
    /// The new signature is intact and was to lock the document, and the
    /// file does not read as locked by it with no changes permitted.
    Lock,
}

impl ReadBack {
    /// What the reader is told about a copy that was written and does not
    /// read back as it should, for the two answers the window's own sentence
    /// (`signing.ts`'s `afterSigning`) does not word: it words a signature
    /// that is missing or not intact, and nothing after that.
    pub(crate) fn sentence(&self, name: &str, field: &str) -> Option<String> {
        let what = match self {
            // The window words these two from what was read: `afterSigning`.
            ReadBack::Holds | ReadBack::Signature | ReadBack::Unchecked => return None,
            ReadBack::Timestamp => said!(
                "reading it back did not find the timestamp of the new signature {field} intact"
            ),
            ReadBack::LongTerm(why) => why.sentence(),
            ReadBack::Lock => {
                said!("reading it back did not find the new signature {field} locking the document")
            }
        };
        Some(said!(
            "{name} was written, but {what}. Do not rely on that copy."
        ))
    }
}

/// What the written file reads back as: ours intact, its timestamp intact
/// when one was asked for, and --- for long-term data --- everything
/// `longterm::check` held the same bytes to before they were written: the
/// signer's and the authority's revocation `good`, and every certificate
/// above either.
///
/// `ours` is the signature this signing made, chosen by what was written
/// ([`sign_cms::ours`]) and not by its name: until 2026-10-09 any intact
/// signature of that name answered for it, an older one included.
///
/// **One rule for the window and the command line**, decided here and not in
/// the window's TypeScript: until 2026-10-05 the window's sentence looked at
/// the new signature's verdict alone, so a timestamp or long-term data that
/// did not read back was a signing reported as done.
pub(crate) fn read_back(
    ours: Option<&crate::docinfo::Signature>,
    timestamp: bool,
    long_term: bool,
    lock: bool,
) -> ReadBack {
    use crate::integrity::{Verdict, Why};
    let intact =
        |i: Option<&crate::integrity::Integrity>| i.is_some_and(|i| i.verdict == Verdict::Intact);
    let Some(ours) = ours else {
        return ReadBack::Signature;
    };
    let unlocked = lock && ours.lock != 1;
    match ours.integrity.as_ref() {
        Some(found) if found.verdict == Verdict::Intact => {}
        // The lock needs no hashing, being read from the dictionaries, so a
        // signature whose hash ran out of time still says whether it locks.
        // Until 2026-10-15 the budget's `Unchecked` came first, and a lock
        // asked for and not written went unsaid there.
        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {
            return if unlocked {
                ReadBack::Lock
            } else {
                ReadBack::Unchecked
            };
        }
        _ => return ReadBack::Signature,
    }
    let stamped = ours
        .timestamp
        .as_ref()
        .is_some_and(|t| intact(t.integrity.as_ref()));
    if timestamp && !stamped {
        return ReadBack::Timestamp;
    }
    // Before the long-term data, which is added after the lock and is only
    // worth having on a signature that says what was asked of it.
    if unlocked {
        return ReadBack::Lock;
    }
    if long_term {
        if let Err(why) = crate::longterm::check(Some(ours)) {
            return ReadBack::LongTerm(why);
        }
    }
    ReadBack::Holds
}

/// The rest of a signing, from a made or sealed signature, **for the window
/// and the command line alike**: the timestamp when one was asked for, the
/// seal, the long-term data when it was asked for, the write, and the
/// read-back by a worker of the file at `out` --- so the verdict a reader is
/// shown is the one the properties dialog would give, computed where every
/// other parse of the file happens. **`out` is reopened by name after the
/// write has put it in place**, so the verdict is about the file of that name
/// at that moment: the one just written, unless something replaced it in
/// between. `write_signed` stages and renames and hands back no handle.
///
/// What each ending means to its reader is the caller's: the window keeps a
/// signature whose timestamp or data did not come (`commands::sign::conclude`), and the
/// command line exits (`cli::sign`). The order, and what is checked before
/// anything is written, is here once.
///
/// `write` creates the signed copy at `out`, and is the caller's to pass
/// because `scripts/check_writers.py` finds the commands that write a file by
/// the writers named one call below them. On a blocking thread: the requests,
/// the write and the read-back all wait.
///
/// # Errors
///
/// [`Stopped`], which says what, if anything, is left to write.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish(
    stage: Stage,
    authority: Option<&url::Url>,
    long_term: bool,
    out: &Path,
    now: u64,
    checking: &dyn crate::save::Verifier,
    asking: Asking<'_>,
    write: &dyn Fn(&[u8]) -> Result<(), String>,
) -> Result<Finished, Stopped> {
    let (bytes, cms, field, stamped_by, locked) = match stage {
        Stage::Made(made) => {
            let stamped = match crate::tsa::stamp(&made, authority, |url, value| {
                (asking.stamp)(url, value)
            }) {
                Ok(stamped) => stamped,
                Err(why) => {
                    return Err(Stopped::Unstamped {
                        made: Box::new(made),
                        why,
                    })
                }
            };
            // Before anything is sealed, with long-term data asked for or not.
            // The token's arithmetic says nothing of who made it, and over
            // `http://` anybody on the path can answer. Long-term data asks
            // again before it fetches anything, but only after the seal: a
            // refusal there holds a sealed stage that *Sign without long-term
            // data* writes as it is, so an unvouched token must never reach
            // `seal`. Every `Stage::Sealed` the window holds therefore carries
            // a token vouched for here, and `resumed` need not judge it again.
            if let (Some(cms), false) = (&stamped, asking.any_authority) {
                if let Err(refusal) = crate::longterm::vouched(cms, now, asking.vouch) {
                    let why = match refusal {
                        crate::longterm::Refusal::Untrusted { name, why } => said!(
                            "the timestamp authority {name} is not trusted by this computer: {why}"
                        ),
                        other => other.sentence(),
                    };
                    return Err(Stopped::Unstamped {
                        made: Box::new(made),
                        why: crate::tsa::Refusal::Untrusted(why),
                    });
                }
            }
            let field = made.field.clone();
            let locked = made.locked;
            let cms = stamped.clone();
            (
                made.seal(stamped).map_err(Stopped::Refused)?,
                cms,
                field,
                authority.cloned(),
                locked,
            )
        }
        Stage::Sealed {
            bytes,
            cms,
            field,
            authority,
            locked,
        } => (bytes, Some(cms), field, authority, locked),
    };
    let timestamped = cms.is_some();
    // Where the new signature's own revision ends: what a long-term signing
    // appends after it is how the read-back knows which signature is ours.
    let sealed = bytes.len();
    let bytes = match (long_term, cms) {
        (false, _) => bytes,
        (true, None) => {
            return Err(Stopped::Refused(
                crate::longterm::Refusal::NoTimestamp.sentence(),
            ))
        }
        (true, Some(cms)) => {
            let archive = asking.archive;
            match crate::longterm::extend(
                &bytes,
                &cms,
                &field,
                now,
                checking,
                asking.os_chain,
                asking.vouch,
                asking.fetch,
                // The archive timestamp, from the authority that stamped the
                // signature.
                &mut |pieces| match stamped_by.as_ref() {
                    Some(url) => archive(url, pieces),
                    None => Err(crate::longterm::Refusal::NoTimestamp.sentence()),
                },
            ) {
                Ok(extended) => extended,
                Err(why) => {
                    return Err(Stopped::Unextended {
                        sealed: Box::new(Stage::Sealed {
                            bytes,
                            cms,
                            field,
                            authority: stamped_by,
                            locked,
                        }),
                        why,
                    })
                }
            }
        }
    };
    write(&bytes).map_err(Stopped::Unwritten)?;

    let mut written = std::fs::File::open(out).map_err(|e| {
        Stopped::Unread(said!(
            "the signed file was written and could not be reopened: {e}"
        ))
    })?;
    let signatures = checking
        .signatures(&mut written, bytes.len())
        .map_err(|e| {
            Stopped::Unread(said!(
                "the signed file was written and could not be checked: {e}"
            ))
        })?;
    let ours = sign_cms::ours(&signatures, &field, (bytes.len() - sealed) as u64);
    let read_back = read_back(
        ours.map(|at| &signatures[at]),
        timestamped,
        long_term,
        locked,
    );
    Ok(Finished {
        field,
        signatures,
        read_back,
        ours,
    })
}

/// The archive timestamp for the application and the tool: `tsa::ask_over_range`
/// against `url`.
fn ask_archive(url: &url::Url, pieces: &[&[u8]]) -> Result<Vec<u8>, String> {
    crate::tsa::ask_over_range_blocking(url, pieces, &crate::tsa::LIMITS)
        .map_err(|why| why.sentence(url.host_str().unwrap_or_default()))
}

/// Seconds since the epoch, now.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{save, sign_prepare};

    /// A signature from the fake PKI's signer, timestamped and sealed: the
    /// stage `Pending` holds after its long-term data did not come.
    pub(crate) fn sealed(pki: &crate::integrity::test_tsa::Pki) -> Stage {
        use crate::integrity::test_tsa::{mint, Imprint};
        use crate::sign_cms::testkeys::{plain_pdf, Soft};
        let at = now();
        let original = plain_pdf();
        let unsigned = sign_prepare::prepare(original.clone(), at, None).expect("prepared");
        let made = sign_cms::sign(
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
        let field = made.field.clone();
        let bytes = made.seal(Some(cms.clone())).expect("sealed");
        Stage::Sealed {
            bytes,
            cms,
            field,
            authority: url::Url::parse("http://127.0.0.1:9/").ok(),
            locked: false,
        }
    }

    /// The archive timestamp for a test: the test authority's token over the
    /// pieces.
    pub(crate) fn test_archive(_: &url::Url, pieces: &[&[u8]]) -> Result<Vec<u8>, String> {
        use crate::integrity::test_tsa::{mint, Imprint};
        Ok(mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(&pieces.concat()),
            None,
            now(),
            &crate::integrity::test_tsa::TestTsa::new(),
        ))
    }

    /// Whether the authority is trusted, for a test: the reader's rule, with
    /// the test authority's root as the only anchor.
    pub(crate) fn test_vouch(token: &[u8], now: u64) -> crate::longterm::Vouched {
        let root = crate::integrity::test_tsa::TestTsa::new().root;
        crate::longterm::vouched_under(
            token,
            now,
            crate::trust::Anchors::Only(std::slice::from_ref(&root)),
        )
    }

    /// [`Asking`] for a test that starts from a sealed signature: no
    /// timestamp authority to ask, no OS to assemble a chain, the fake PKI's
    /// own server for revocation data, the test authority for the archive
    /// timestamp, and `vouch` for whether that authority is trusted.
    pub(crate) fn asking<R>(
        vouch: fn(&[u8], u64) -> crate::longterm::Vouched,
        then: impl FnOnce(Asking<'_>) -> R,
    ) -> R {
        let mut fetch = crate::longterm::fetch_blocking;
        let mut archive = test_archive;
        then(Asking {
            stamp: &|_, _| Err(crate::tsa::Refusal::Unreachable("nobody to ask".into())),
            os_chain: &|_, _| Vec::new(),
            vouch: &vouch,
            fetch: &mut fetch,
            archive: &mut archive,
            any_authority: false,
        })
    }

    pub(crate) fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tpdf-conclude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let source = dir.join("source.pdf");
        std::fs::write(&source, b"%PDF-1.7 the original").expect("a source");
        (source, dir.join("signed.pdf"))
    }

    /// A sound token from an authority nobody vouches for is not sealed, with
    /// long-term data or without, unless the command line says to keep it. It is
    /// what somebody on the path can answer with over `http://`.
    #[test]
    fn a_timestamp_from_an_authority_nobody_vouches_for_is_not_sealed() {
        use crate::integrity::test_tsa::{mint, Imprint, Pki};
        use crate::sign_cms::testkeys::{plain_pdf, Soft};
        let pki = Pki::start(crate::integrity::test_tsa::Plan::default());
        let authority = url::Url::parse("http://127.0.0.1:9/").expect("a URL");
        let at = now();
        let made = || {
            let original = plain_pdf();
            let unsigned = sign_prepare::prepare(original.clone(), at, None).expect("prepared");
            sign_cms::sign(
                original,
                unsigned,
                at,
                &pki.signer.certificate,
                &pki.chain,
                &Soft::p256(pki.signer.seed),
            )
            .expect("made")
        };
        let run = |long_term: bool, any_authority: bool, out: &Path| {
            let mut fetch = crate::longterm::fetch_blocking;
            let mut archive = test_archive;
            finish(
                Stage::Made(made()),
                Some(&authority),
                long_term,
                out,
                at,
                &crate::save::Here,
                Asking {
                    stamp: &|_, value| {
                        Ok(mint(
                            Imprint::Sha256,
                            &Imprint::Sha256.digest(value),
                            None,
                            at,
                            &pki.tsa,
                        ))
                    },
                    os_chain: &|_, _| Vec::new(),
                    vouch: &|token, now| {
                        crate::longterm::vouched_under(token, now, crate::trust::Anchors::Only(&[]))
                    },
                    fetch: &mut fetch,
                    archive: &mut archive,
                    any_authority,
                },
                &|bytes| std::fs::write(out, bytes).map_err(|e| e.to_string()),
            )
        };

        for long_term in [false, true] {
            let (_, out) = scratch(&format!("untrusted-refused-{long_term}"));
            let _ = std::fs::remove_file(&out);
            match run(long_term, false, &out) {
                Err(Stopped::Unstamped {
                    why: crate::tsa::Refusal::Untrusted(sentence),
                    ..
                }) => assert!(!sentence.is_empty()),
                Err(_) => panic!("long-term {long_term}: stopped for another reason"),
                Ok(_) => panic!("long-term {long_term}: sealed"),
            }
            assert!(!out.exists(), "long-term {long_term}: nothing is written");
        }
        assert!(pki.paths().is_empty(), "{:?}", pki.paths());

        let (_, out) = scratch("untrusted-kept");
        let _ = std::fs::remove_file(&out);
        assert!(run(false, true, &out).is_ok());
        assert!(out.exists(), "the command line asked to keep it");
    }

    /// A signing that was to lock the document is held to the lock when the
    /// file is read back: an intact signature that locks nothing, or at
    /// another level, is a copy not to rely on, and says so. A signing that
    /// asked for no lock is not held to one.
    #[test]
    fn a_lock_that_does_not_read_back_is_not_a_signing_that_holds() {
        use crate::integrity::{Integrity, Verdict};
        let reading = |lock: u8| crate::docinfo::Signature {
            field: "Signature1".into(),
            signed: true,
            lock,
            integrity: Some(Integrity {
                verdict: Verdict::Intact,
                ..Integrity::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            read_back(Some(&reading(1)), false, false, true),
            ReadBack::Holds
        );
        for level in [0, 2, 3] {
            assert_eq!(
                read_back(Some(&reading(level)), false, false, true),
                ReadBack::Lock,
                "level {level}"
            );
            assert_eq!(
                read_back(Some(&reading(level)), false, false, false),
                ReadBack::Holds,
                "level {level}, and no lock was asked for"
            );
        }
        let said = ReadBack::Lock
            .sentence("signed.pdf", "Signature1")
            .expect("a sentence of its own");
        assert!(
            said.contains("did not find the new signature Signature1 locking the document")
                && said.ends_with("Do not rely on that copy."),
            "{said}"
        );
    }

    /// The rule both paths hold the written file to, and what the window is
    /// told for the two answers its own sentence does not word.
    #[test]
    fn a_copy_that_does_not_read_back_as_written_is_refused_in_the_window_too() {
        use crate::docinfo::{Signature, Timestamp};
        use crate::integrity::{Integrity, Verdict};
        let verdict = |verdict| {
            Some(Integrity {
                verdict,
                ..Integrity::default()
            })
        };
        let signature = |own, stamp: Option<Verdict>| Signature {
            field: "Signature1".into(),
            signed: true,
            integrity: verdict(own),
            timestamp: stamp.map(|stamp| Timestamp {
                integrity: verdict(stamp),
                ..Timestamp::default()
            }),
            ..Signature::default()
        };
        let read = |found: &[Signature], timestamp, long_term| {
            let ours = sign_cms::ours(found, "Signature1", 0);
            read_back(ours.map(|at| &found[at]), timestamp, long_term, false)
        };
        let bare = [signature(Verdict::Intact, None)];
        let stamped = [signature(Verdict::Intact, Some(Verdict::Intact))];
        let unstamped = [signature(Verdict::Intact, Some(Verdict::Broken))];
        let altered = [signature(Verdict::Altered, Some(Verdict::Intact))];

        assert_eq!(read(&bare, false, false), ReadBack::Holds);
        assert_eq!(read(&stamped, true, false), ReadBack::Holds);
        // Where the window's own sentence already says so, the answer is the
        // signature's and no second sentence is made.
        assert_eq!(read(&altered, true, false), ReadBack::Signature);
        assert_eq!(read(&[], false, false), ReadBack::Signature);
        assert_eq!(
            read(&bare, false, false).sentence("signed.pdf", "Signature1"),
            None
        );
        assert_eq!(
            ReadBack::Signature.sentence("signed.pdf", "Signature1"),
            None
        );
        // A timestamp asked for that is not there, or does not check out: only
        // when one was asked for.
        for found in [&bare, &unstamped] {
            assert_eq!(read(found, true, false), ReadBack::Timestamp);
            assert_eq!(read(found, false, false), ReadBack::Holds);
        }
        assert_eq!(
            ReadBack::Timestamp
                .sentence("signed.pdf", "Signature1")
                .as_deref(),
            Some(
                "signed.pdf was written, but reading it back did not find the timestamp of \
                 the new signature Signature1 intact. Do not rely on that copy."
            )
        );
        // Long-term data asked for and not read back good: what the check
        // before writing says of the file, and only when it was asked for.
        let unanswered = read(&stamped, true, true);
        assert!(
            matches!(&unanswered, ReadBack::LongTerm(why) if why.tpdf_failed()),
            "{unanswered:?}"
        );
        assert_eq!(read(&stamped, true, false), ReadBack::Holds);
        let said = unanswered
            .sentence("signed.pdf", "Signature1")
            .expect("a sentence");
        assert!(
            said.starts_with(
                "signed.pdf was written, but tpdf's own check of the signed document with its \
                 long-term validation data did not pass: "
            ) && said.ends_with(". Do not rely on that copy."),
            "{said}"
        );
        // And another field is not ours, whatever it reads as.
        assert_eq!(sign_cms::ours(&stamped, "Signature9", 0), None);
        assert_eq!(read_back(None, true, false, false), ReadBack::Signature);

        // A new signature the read-back had no hashing budget left for: its
        // own answer, which is neither of the two above. Only for that
        // reason --- any other unchecked signature is one not to rely on.
        use crate::integrity::Why;
        let unchecked = |why| {
            let mut found = signature(Verdict::Unchecked, Some(Verdict::Intact));
            found.integrity.as_mut().expect("a verdict").why = Some(why);
            [found]
        };
        for (timestamp, long_term) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                read(&unchecked(Why::Budget), timestamp, long_term),
                ReadBack::Unchecked
            );
        }
        // But a lock asked for and not written is said even then: it is
        // read from the dictionaries and needs no hashing.
        let budget = unchecked(Why::Budget);
        assert_eq!(
            read_back(Some(&budget[0]), false, false, true),
            ReadBack::Lock,
            "the lock is compared before the budget's answer"
        );
        let mut locked = unchecked(Why::Budget);
        locked[0].lock = 1;
        assert_eq!(
            read_back(Some(&locked[0]), false, false, true),
            ReadBack::Unchecked
        );
        assert_eq!(
            read(&unchecked(Why::Range), false, false),
            ReadBack::Signature
        );
        assert_eq!(
            ReadBack::Unchecked.sentence("signed.pdf", "Signature1"),
            None,
            "the window words it from what was read"
        );
    }

    /// A verifier that reads the file as it is and then changes what it
    /// reports, for the answers a real file cannot be made to give in a test.
    pub(crate) struct Rewording(
        pub(crate) fn(Vec<crate::docinfo::Signature>) -> Vec<crate::docinfo::Signature>,
    );

    impl crate::save::Verifier for Rewording {
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
            crate::save::Here.signatures(file, len).map(self.0)
        }

        fn validation(
            &self,
            signed: &[u8],
            gathered: &crate::sign_dss::Gathered,
        ) -> Result<crate::sign_dss::Extended, String> {
            crate::save::Here.validation(signed, gathered)
        }

        fn document_timestamp(&self, signed: &[u8]) -> Result<sign_prepare::Unsigned, String> {
            crate::save::Here.document_timestamp(signed)
        }

        fn survey(&self, signed: &[u8]) -> Result<crate::sign_dss::Survey, String> {
            crate::save::Here.survey(signed)
        }
    }

    /// [`finish`] from a sealed signature, read back by `checking`.
    pub(crate) fn finished(name: &str, checking: &dyn crate::save::Verifier) -> Finished {
        use crate::integrity::test_tsa::{Pki, Plan};
        let pki = Pki::start(Plan::default());
        let (source, out) = scratch(name);
        let done = asking(test_vouch, |asking| {
            finish(
                sealed(&pki),
                None,
                false,
                &out,
                now(),
                checking,
                asking,
                &|bytes| save::write_signed(&source, &out, bytes).map_err(|why| why.message),
            )
        });
        match done {
            Ok(done) => done,
            Err(_) => panic!("{name}: the signing stopped"),
        }
    }

    /// The new signature is the one this signing wrote, not any intact
    /// signature of its name. Until 2026-10-09 the read-back matched by name,
    /// so an older intact signature answered for a new one that did not read
    /// back --- and a document can hold one: a field named in another
    /// encoding before that day, or a child of an unnamed field.
    #[test]
    fn an_older_intact_signature_of_the_same_name_does_not_answer_for_the_new_one() {
        use crate::integrity::Verdict;
        // The file as written, with an older signature of the same name
        // listed before the new one: intact, and followed by a later revision.
        fn with_older(mut found: Vec<crate::docinfo::Signature>) -> Vec<crate::docinfo::Signature> {
            let mut older = found[0].clone();
            older.appended_bytes = 4_321;
            older.covers_whole_file = false;
            found.insert(0, older);
            found
        }
        // The same, and the new signature does not read back.
        fn with_older_and_altered(
            found: Vec<crate::docinfo::Signature>,
        ) -> Vec<crate::docinfo::Signature> {
            let mut found = with_older(found);
            found[1].integrity.as_mut().expect("a verdict").verdict = Verdict::Altered;
            found
        }

        let done = finished("older-altered", &Rewording(with_older_and_altered));
        assert_eq!(done.signatures.len(), 2);
        assert_eq!(done.signatures[0].field, done.signatures[1].field);
        assert_eq!(
            done.signatures[0].integrity.as_ref().map(|i| i.verdict),
            Some(Verdict::Intact),
            "the older one is intact, which is what answered before"
        );
        assert_eq!(done.ours, Some(1));
        assert_eq!(done.read_back, ReadBack::Signature);
        // What the window is told: one signature is the new one, and it is
        // the altered one.
        let report = sign_cms::report(String::new(), done.field, done.signatures, done.ours);
        let ours: Vec<_> = report.signatures.iter().filter(|s| s.ours).collect();
        assert_eq!(ours.len(), 1);
        assert_eq!(
            ours[0].integrity.as_ref().map(|i| i.verdict),
            Some(Verdict::Altered)
        );

        // The control: the same older signature beside a new one that does
        // read back.
        let done = finished("older-intact", &Rewording(with_older));
        assert_eq!((done.ours, done.read_back), (Some(1), ReadBack::Holds));
    }
}
