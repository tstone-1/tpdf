//! The sentences the command-line tool says about a signature: the app's own.
//!
//! **A port, held to its original by a test, not a second author.** The words
//! live in `src/lib/integrity.ts` (`integrityRow`, `trustRow`, `WHY`, `DOUBT`)
//! and `src/lib/signing.ts` (`afterSigning`), where the properties dialog and
//! the signing panel say them. The command-line tool has no webview to ask, so
//! the functions are restated here --- and `words_sample` writes every case this
//! module can produce to `src-tauri/testdata/cli/wording.json`, which
//! `src/lib/cliwording.test.ts` reads and holds against the TypeScript
//! functions, sentence for sentence. A word changed on either side is a red
//! test on the other, so "exactly the wording the app uses" is a checked claim
//! rather than a hope.
//!
//! The rule `properties.test.ts`'s `VERDICT_WORDS` states holds here because it
//! holds there: none of these sentences says valid, verified, authentic or
//! genuine.

use crate::integrity::{Integrity, Verdict, Why};
use crate::trust::{Doubt, Standing, Store, Trust};

/// Said after every answer that could be read as "this signer is who they say".
/// `integrity.ts`'s `TRUST_NOT_CHECKED`.
pub const TRUST_NOT_CHECKED: &str =
    "Whether that key belongs to the person the certificate names was not checked.";

/// Said after every standing that says the chain reached a trusted root.
/// `integrity.ts`'s `REVOCATION_NOT_CHECKED`.
pub const REVOCATION_NOT_CHECKED: &str =
    "Revocation was not checked: tpdf does not go online, so a certificate its \
     issuer has since withdrawn reads the same as one it has not.";

/// Why a signature was not checked, as a clause that follows "not checked —".
/// `integrity.ts`'s `WHY`.
#[must_use]
pub fn why(why: Why) -> &'static str {
    match why {
        Why::Format => "tpdf does not check signatures in this format",
        Why::Range => {
            "the signed range does not leave out exactly this signature's own value, \
             so the signature does not protect the document the way it should"
        }
        Why::Unreadable => "the signature's data could not be read",
        Why::Certificate => {
            "the certificate the signature names is not in it, or its key could not be read"
        }
        Why::Algorithm => "it uses an algorithm tpdf does not implement",
        Why::Attributes => "its signed attributes are not in the form the CMS standard requires",
        Why::Budget => {
            "the document's signatures together cover more data than tpdf checks at once"
        }
    }
}

/// The computer whose store answered. `integrity.ts`'s `COMPUTER`.
#[must_use]
pub fn computer(store: Option<Store>) -> &'static str {
    match store {
        Some(Store::Mac) => "this Mac",
        Some(Store::Windows) => "this PC",
        None => "this computer",
    }
}

/// Why the store does not vouch, as a clause. `integrity.ts`'s `DOUBT`.
#[must_use]
pub fn doubt(doubt: Doubt, computer: &str) -> String {
    match doubt {
        Doubt::Incomplete => format!(
            "a certificate between the signer's and a root is in neither the signature \
             nor on {computer}, and tpdf does not look it up"
        ),
        Doubt::Root => format!(
            "its chain ends at a root {computer} does not trust. A certificate somebody \
             issued to themselves reads this way, and so does one whose root only \
             Adobe's trust list carries"
        ),
        Doubt::Dates => "a certificate above the signer's is outside its dates".into(),
        Doubt::Purpose => {
            "the signer's certificate was issued for something other than signing documents".into()
        }
        Doubt::Rejected => format!("{computer} refused its chain"),
        Doubt::Certificate => {
            "the signature's certificates could not be prepared for the check".into()
        }
        Doubt::Unavailable => "the operating system's trust check could not be run".into(),
    }
}

/// The trust row's value: `integrity.ts`'s `trustRow(trust, from, until).value`.
///
/// `from` and `until` are the signer's certificate's dates as the certificate
/// rows show them, empty when unknown.
#[must_use]
pub fn trust_sentence(trust: &Trust, from: &str, until: &str) -> String {
    let computer = computer(trust.store);
    let why = trust
        .why
        .map_or_else(|| "no reason was given".to_string(), |d| doubt(d, computer));
    let chained = format!("the signer's certificate chains to a root {computer} trusts");
    match trust.standing {
        Standing::Trusted => format!(
            "trusted — {chained}, so an issuer {computer} trusts vouches that the key \
             belongs to the person the certificate names. {REVOCATION_NOT_CHECKED}"
        ),
        Standing::Expired => format!(
            "expired — {chained}, and it ran out{}. The date a signature gives is the \
             signer's own claim, so tpdf cannot tell whether it was made before then. \
             {REVOCATION_NOT_CHECKED}",
            if until.is_empty() {
                String::new()
            } else {
                format!(" on {until}")
            }
        ),
        Standing::NotYetValid => format!(
            "not yet in force — {chained}, but it only comes into force{}. \
             {REVOCATION_NOT_CHECKED}",
            if from.is_empty() {
                " later".to_string()
            } else {
                format!(" on {from}")
            }
        ),
        Standing::Untrusted => format!(
            "not trusted — {why}. So nothing establishes that the key belongs to the \
             person the certificate names."
        ),
        Standing::Unchecked => {
            format!("not checked — {why}. This says nothing either way about who holds the key.")
        }
    }
}

/// `(SHA-256, RSA)`, or nothing when the check stopped before either.
fn how(integrity: &Integrity) -> String {
    let parts: Vec<&str> = [integrity.digest.as_str(), integrity.method.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// The integrity row's value: `integrity.ts`'s
/// `integrityRow(integrity, appended, trustFollows).value`.
///
/// `trust_follows` says a trust sentence follows this one, which is then the
/// answer to whose key it is, and [`TRUST_NOT_CHECKED`] would contradict it.
#[must_use]
pub fn integrity_sentence(integrity: &Integrity, appended: u64, trust_follows: bool) -> String {
    let owner = if trust_follows {
        String::new()
    } else {
        format!(" {TRUST_NOT_CHECKED}")
    };
    let how = how(integrity);
    match integrity.verdict {
        Verdict::Intact => {
            let later = if appended > 0 {
                " It covers the document as it was when signed; what was appended \
                 afterwards is not part of it."
            } else {
                ""
            };
            format!(
                "intact — the signed bytes are unchanged and the signature checks out \
                 under the key in its certificate{how}.{later}{owner}"
            )
        }
        Verdict::Weak => format!(
            "unchanged under SHA-1 only — the digest and the signature match{how}, but \
             SHA-1 collisions can be manufactured, so this does not show the bytes are \
             the ones signed.{owner}"
        ),
        Verdict::Altered => format!(
            "altered — the bytes this signature covers have changed since it was \
             made{how}. The signature itself checks out, so what changed is the document."
        ),
        Verdict::Broken => format!(
            "broken — the signature does not check out under its certificate's \
             key{how}, so nothing it states can be relied on, including what it covers."
        ),
        Verdict::Unchecked => format!(
            "not checked — {}. This says nothing either way about whether the document \
             changed.",
            integrity.why.map_or("no reason was given", why)
        ),
    }
}

/// A verdict in a few words, for a list of signatures. `signing.ts`'s `verdict`.
#[must_use]
pub fn verdict_briefly(integrity: Option<&Integrity>) -> String {
    let Some(integrity) = integrity else {
        return "not checked".into();
    };
    match integrity.verdict {
        Verdict::Intact => "intact".into(),
        Verdict::Weak => "unchanged under SHA-1 only".into(),
        Verdict::Altered => "altered".into(),
        Verdict::Broken => "broken".into(),
        Verdict::Unchecked => format!(
            "not checked ({})",
            integrity.why.map_or("no reason given", why)
        ),
    }
}

/// The sentence after signing: `signing.ts`'s `afterSigning(signed)`.
///
/// `name` is the written file's base name; `signatures` is every signed field
/// the read-back found, as `(field, ours, integrity)`.
#[must_use]
pub fn after_signing(
    name: &str,
    field: &str,
    signatures: &[(String, bool, Option<Integrity>)],
) -> String {
    let ours = signatures.iter().find(|(_, ours, _)| *ours);
    let earlier: Vec<_> = signatures.iter().filter(|(_, ours, _)| !ours).collect();
    let intact = ours.is_some_and(|(_, _, integrity)| {
        integrity
            .as_ref()
            .is_some_and(|i| i.verdict == Verdict::Intact)
    });
    if !intact {
        return format!(
            "{name} was written, but reading it back did not find the new signature \
             {field} intact: {}. Do not rely on that copy.",
            ours.map_or_else(
                || "it is not in the file".to_string(),
                |(_, _, integrity)| verdict_briefly(integrity.as_ref())
            )
        );
    }
    let mut text = format!(
        "Signed as {field} and saved to {name}. Read back after writing, the signature is intact."
    );
    if !earlier.is_empty() {
        let listed: Vec<String> = earlier
            .iter()
            .map(|(field, _, integrity)| format!("{field} {}", verdict_briefly(integrity.as_ref())))
            .collect();
        text.push_str(&format!(
            " Earlier signature{}: {}.",
            if earlier.len() == 1 { "" } else { "s" },
            listed.join(", ")
        ));
    }
    text
}

/// The sentence after a redaction: `recovery.ts`'s `afterRedaction(applied)`.
///
/// **The one place a verdict is worded**, and it is the window's word for word
/// --- `tpdf redact` must never say *verified* more strongly than the
/// application does, and holding the two to one sample is what makes that a
/// checked claim. `why` is empty exactly when `verified`, as
/// `redact::Applied` promises.
#[must_use]
pub fn after_redaction(
    regions: usize,
    shows: usize,
    verified: bool,
    why: &[String],
    changed: bool,
) -> String {
    let count = |many: usize, noun: &str| {
        if many == 1 {
            format!("1 {noun}")
        } else {
            format!("{many} {noun}s")
        }
    };
    let removed = format!("{}, {}", count(regions, "region"), count(shows, "removal"));
    let verdict = if verified {
        format!(
            "Redacted {removed}. tpdf read the file back and none of the removed words are in it."
        )
    } else {
        format!(
            "Redaction not verified. Redacted {removed}, but tpdf could not prove the file is \
             clean. Checks before adding the black fill found: {}. Treat it as unredacted until \
             you have checked it.",
            why.join("; ")
        )
    };
    if !changed {
        return verdict;
    }
    format!(
        "{verdict} The original also changed on disk while you had it open, so this was built \
         from the newer version."
    )
}
