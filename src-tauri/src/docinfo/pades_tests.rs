//! What [`crate::pades`] reads from a [`Signature`]: the `Evidence` this
//! module hands it, on the report's own types.
//!
//! `pades`'s own tests build a signature of six fields, so they cannot say
//! anything about a field `pades` is not handed. This one can, and it is here
//! because the type with a `trust` field is.

use super::{Signature, Timestamp};
use crate::integrity::{Integrity, Verdict};
use crate::pades::{level, Level};

/// The level is the document's parts, not this computer's opinion of who
/// made them: a token that checks out from an authority the store does not
/// trust is still the part B-T names, and the trust row says the rest.
#[test]
fn a_token_from_an_authority_this_computer_does_not_trust_is_still_the_part() {
    use crate::trust::{Doubt, Standing, Trust};
    let standing = |standing, why| {
        Some(Trust {
            standing,
            why,
            ..Trust::default()
        })
    };
    for trust in [
        standing(Standing::Trusted, None),
        standing(Standing::Untrusted, Some(Doubt::Root)),
        standing(Standing::Unchecked, Some(Doubt::Unavailable)),
        None,
    ] {
        // A CAdES signature that holds, carrying an attested token whose
        // authority nothing in the document answers for.
        let dated = Signature {
            signed: true,
            kind: "ETSI.CAdES.detached".into(),
            integrity: Some(Integrity {
                verdict: Verdict::Intact,
                ..Integrity::default()
            }),
            appended_bytes: 1_000,
            timestamp: Some(Timestamp {
                attested: true,
                trust: trust.clone(),
                ..Timestamp::default()
            }),
            ..Signature::default()
        };
        let all = [dated];
        assert_eq!(level(&all[0], &all), Some(Level::T), "{trust:?}");
    }
}
