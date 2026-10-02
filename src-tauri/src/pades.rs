//! Which PAdES baseline level a signature has the parts of.
//!
//! People who receive a signed document are asked for its level by name ---
//! "is it B-LT?" --- and until 2026-10-02 the properties showed every part of
//! the answer and never the word. This names it.
//!
//! **By its parts, and it says so.** ETSI EN 319 142-1 defines each level by
//! what the document carries, and that much is read here from verdicts the
//! worker already reached: the signature holds over its bytes, a timestamp
//! token is attested, the document's own revocation data answered `good` for
//! every certificate, a later document timestamp is attested. What is **not**
//! done is a conformance test --- the signed attributes a profile requires,
//! the algorithms it permits, the order the standard fixes for adding
//! validation data. A validator does those; this does not, and
//! [`crate::cli::words::pades_sentence`] ends every answer by saying so.
//!
//! **Nothing is named for a signature that does not hold.** A level is a
//! statement about a signature, and one that is broken, altered or unchecked
//! has nothing to be at a level of.
//!
//! **A level is never rounded up.** Each rung needs the one below it, and a
//! rung whose evidence is absent or could not be judged stops the climb: an
//! archive timestamp over a signature whose revocation data is missing is
//! B-T, not B-LTA.

use crate::docinfo::Signature;
use crate::integrity::Verdict;
use crate::revocation::Status;

/// `/SubFilter` of a CAdES signature in a PDF, which is what PAdES is.
const CADES: &str = "ETSI.CAdES.detached";
/// `/SubFilter` of a document timestamp.
const DOCUMENT_TIMESTAMP: &str = "ETSI.RFC3161";

/// A PAdES baseline level, ETSI EN 319 142-1 §6.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Level {
    /// B-B: a CAdES signature, and nothing that dates it.
    #[serde(rename = "B-B")]
    B,
    /// B-T: and a trusted time for it.
    #[serde(rename = "B-T")]
    T,
    /// B-LT: and the revocation data to check it later, in the document.
    #[serde(rename = "B-LT")]
    Lt,
    /// B-LTA: and a document timestamp over that data.
    #[serde(rename = "B-LTA")]
    Lta,
}

impl Level {
    /// The level's name, as a reader is asked for it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Level::B => "B-B",
            Level::T => "B-T",
            Level::Lt => "B-LT",
            Level::Lta => "B-LTA",
        }
    }
}

/// Whether a verdict leaves something to be at a level of.
fn holds(signature: &Signature) -> bool {
    signature
        .integrity
        .as_ref()
        .is_some_and(|i| matches!(i.verdict, Verdict::Intact | Verdict::Weak))
}

/// Where a signature's covered range ends, as an offset from the file's end.
///
/// `appended_bytes` is the file's size less that end, so of two signatures the
/// one with **fewer** appended bytes reaches further.
fn reaches_past(later: &Signature, earlier: &Signature) -> bool {
    later.appended_bytes < earlier.appended_bytes
}

/// Document timestamps that are attested and reach past `signature`'s range.
fn sealing<'a>(
    signature: &'a Signature,
    all: &'a [Signature],
) -> impl Iterator<Item = &'a Signature> {
    all.iter().filter(move |other| {
        other.kind == DOCUMENT_TIMESTAMP
            && holds(other)
            && other.timestamp.as_ref().is_some_and(|t| t.attested)
            && reaches_past(other, signature)
    })
}

/// The level `signature` has the parts of, among `all` the document's
/// signatures, or `None` when it is not a PAdES signature that holds.
#[must_use]
pub fn level(signature: &Signature, all: &[Signature]) -> Option<Level> {
    if !signature.signed || signature.kind != CADES || !holds(signature) {
        return None;
    }
    // B-T: a time somebody else attests --- the signature's own timestamp
    // token, or a document timestamp made over it afterwards.
    let own = signature.timestamp.as_ref().filter(|t| t.attested);
    if own.is_none() && sealing(signature, all).next().is_none() {
        return Some(Level::B);
    }
    // B-LT: the document's own data answers for every certificate, the
    // signer's chain and the chain of the authority that dated it. `Good` is
    // the only answer that counts: `none` is no data and `unchecked` is data
    // nothing could be concluded from.
    let good = |chain: Option<&crate::revocation::chain::Chain>| {
        chain.is_some_and(|c| c.standing == Status::Good && c.dropped == 0)
    };
    let dated_by_good_authority = match own {
        Some(token) => good(token.revocation_chain.as_ref()),
        None => sealing(signature, all).any(|stamp| good(stamp.revocation_chain.as_ref())),
    };
    if !good(signature.revocation_chain.as_ref()) || !dated_by_good_authority {
        return Some(Level::T);
    }
    // B-LTA: a document timestamp, itself answered for, over all of that.
    if sealing(signature, all).any(|stamp| good(stamp.revocation_chain.as_ref())) {
        Some(Level::Lta)
    } else {
        Some(Level::Lt)
    }
}

/// Sets [`Signature::pades`] on every signature of a document.
pub fn assign(signatures: &mut [Signature]) {
    let levels: Vec<Option<Level>> = signatures
        .iter()
        .map(|signature| level(signature, signatures))
        .collect();
    for (signature, level) in signatures.iter_mut().zip(levels) {
        signature.pades = level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docinfo::Timestamp;
    use crate::integrity::Integrity;
    use crate::revocation::chain::Chain;

    fn chain(standing: Status) -> Option<Chain> {
        Some(Chain {
            standing,
            ..Chain::default()
        })
    }

    fn intact() -> Option<Integrity> {
        Some(Integrity {
            verdict: Verdict::Intact,
            ..Integrity::default()
        })
    }

    fn token(attested: bool, authority: Status) -> Option<Timestamp> {
        Some(Timestamp {
            attested,
            revocation_chain: chain(authority),
            ..Timestamp::default()
        })
    }

    /// A CAdES signature that holds, with 1,000 bytes appended after it.
    fn signed() -> Signature {
        Signature {
            signed: true,
            kind: CADES.into(),
            integrity: intact(),
            appended_bytes: 1_000,
            ..Signature::default()
        }
    }

    /// A document timestamp that holds and reaches the end of the file.
    fn stamp(authority: Status) -> Signature {
        Signature {
            signed: true,
            kind: DOCUMENT_TIMESTAMP.into(),
            integrity: intact(),
            appended_bytes: 0,
            timestamp: token(true, authority),
            revocation_chain: chain(authority),
            ..Signature::default()
        }
    }

    fn of(signature: &Signature, others: &[Signature]) -> Option<Level> {
        let mut all = vec![signature.clone()];
        all.extend_from_slice(others);
        level(&all[0], &all)
    }

    #[test]
    fn each_rung_needs_its_own_evidence_and_the_one_below() {
        let bare = signed();
        assert_eq!(of(&bare, &[]), Some(Level::B));

        let dated = Signature {
            timestamp: token(true, Status::Good),
            ..signed()
        };
        assert_eq!(of(&dated, &[]), Some(Level::T), "no revocation data yet");

        let checkable = Signature {
            revocation_chain: chain(Status::Good),
            ..dated.clone()
        };
        assert_eq!(of(&checkable, &[]), Some(Level::Lt));
        assert_eq!(of(&checkable, &[stamp(Status::Good)]), Some(Level::Lta));

        // Never rounded up: an archive timestamp over a signature whose
        // revocation data is missing dates it and proves nothing more.
        assert_eq!(of(&dated, &[stamp(Status::Good)]), Some(Level::T));
        // And one whose own authority is not answered for does not seal.
        assert_eq!(of(&checkable, &[stamp(Status::None)]), Some(Level::Lt));
    }

    #[test]
    fn a_document_timestamp_alone_dates_a_signature_that_has_no_token() {
        let bare = signed();
        assert_eq!(of(&bare, &[stamp(Status::None)]), Some(Level::T));
        // With the signer's chain answered and the stamp's authority too, the
        // one stamp is both the time and the seal.
        let checkable = Signature {
            revocation_chain: chain(Status::Good),
            ..signed()
        };
        assert_eq!(of(&checkable, &[stamp(Status::Good)]), Some(Level::Lta));
        assert_eq!(of(&checkable, &[stamp(Status::None)]), Some(Level::T));
    }

    #[test]
    fn evidence_that_does_not_hold_is_not_evidence() {
        let base = Signature {
            revocation_chain: chain(Status::Good),
            ..signed()
        };
        // A token that is there and not attested.
        let unattested = Signature {
            timestamp: token(false, Status::Good),
            ..base.clone()
        };
        assert_eq!(of(&unattested, &[]), Some(Level::B));
        // The authority's chain unanswered: dated, and no further.
        let unanswered = Signature {
            timestamp: token(true, Status::Unchecked),
            ..base.clone()
        };
        assert_eq!(of(&unanswered, &[]), Some(Level::T));
        // Each answer that is not `good`, for the signer's own chain.
        for standing in [
            Status::None,
            Status::Unchecked,
            Status::Unknown,
            Status::Revoked,
        ] {
            let doubtful = Signature {
                timestamp: token(true, Status::Good),
                revocation_chain: chain(standing),
                ..signed()
            };
            assert_eq!(of(&doubtful, &[]), Some(Level::T), "{standing:?}");
        }
        // A chain with certificates past the bound was not judged whole.
        let cut = Signature {
            timestamp: token(true, Status::Good),
            revocation_chain: Some(Chain {
                standing: Status::Good,
                dropped: 1,
                ..Chain::default()
            }),
            ..signed()
        };
        assert_eq!(of(&cut, &[]), Some(Level::T));

        // A document timestamp made BEFORE the signature's range ends does not
        // cover it: it reaches no further than the signature does.
        let dated = Signature {
            timestamp: token(true, Status::Good),
            ..base
        };
        let earlier = Signature {
            appended_bytes: 1_000,
            ..stamp(Status::Good)
        };
        assert_eq!(of(&dated, &[earlier]), Some(Level::Lt));
        // And one that is broken is not a seal.
        let broken = Signature {
            integrity: Some(Integrity {
                verdict: Verdict::Broken,
                ..Integrity::default()
            }),
            ..stamp(Status::Good)
        };
        assert_eq!(of(&dated, &[broken]), Some(Level::Lt));
    }

    #[test]
    fn nothing_is_named_for_what_is_not_a_pades_signature_that_holds() {
        let fully = |change: fn(&mut Signature)| {
            let mut signature = Signature {
                timestamp: token(true, Status::Good),
                revocation_chain: chain(Status::Good),
                ..signed()
            };
            change(&mut signature);
            of(&signature, &[stamp(Status::Good)])
        };
        assert_eq!(fully(|_| {}), Some(Level::Lta), "the control");
        assert_eq!(fully(|s| s.kind = "adbe.pkcs7.detached".into()), None);
        assert_eq!(fully(|s| s.signed = false), None);
        assert_eq!(fully(|s| s.integrity = None), None);
        for verdict in [Verdict::Broken, Verdict::Altered, Verdict::Unchecked] {
            let mut signature = signed();
            signature.integrity = Some(Integrity {
                verdict,
                ..Integrity::default()
            });
            assert_eq!(of(&signature, &[]), None, "{verdict:?}");
        }
        // A document timestamp is not a signature, and has no level of its own.
        let seal = stamp(Status::Good);
        assert_eq!(of(&seal, &[]), None);
        // SHA-1 still holds, as the Integrity row says it does.
        let mut weak = signed();
        weak.integrity = Some(Integrity {
            verdict: Verdict::Weak,
            ..Integrity::default()
        });
        assert_eq!(of(&weak, &[]), Some(Level::B));
    }

    #[test]
    fn assign_gives_each_signature_its_own_level() {
        let mut all = vec![
            Signature {
                timestamp: token(true, Status::Good),
                revocation_chain: chain(Status::Good),
                ..signed()
            },
            stamp(Status::Good),
            Signature {
                appended_bytes: 0,
                ..signed()
            },
        ];
        assign(&mut all);
        let levels: Vec<Option<Level>> = all.iter().map(|s| s.pades).collect();
        // The last was signed after the stamp, which reaches no further than it.
        assert_eq!(levels, vec![Some(Level::Lta), None, Some(Level::B)]);
        assert_eq!(
            serde_json::to_string(&levels).expect("json"),
            r#"["B-LTA",null,"B-B"]"#
        );
    }
}
