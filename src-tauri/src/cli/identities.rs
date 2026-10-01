//! `tpdf identities [--json]`: the certificates that can sign, and the ones
//! that cannot with the reason --- `sign_cms::usable`'s rules, the chooser's.
//! Also [`resolve`], which is how `sign`'s `--identity` names one of them.

use std::io::Write;

use super::args::unknown;
use super::report::{self, SCHEMA};
use super::{json, say, Env, Exit, Failure, Held, Registered, Subcommand};
use crate::sign_cms::{self, Offer};

/// `identities`, registered.
pub const COMMAND: Registered = Registered {
    name: "identities",
    usage: "identities [--json]",
    summary: "Lists the certificates that can sign, and the ones that cannot\n            with the reason.",
    parse: boxed,
};

/// `tpdf identities`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identities {
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `identities`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Identities, String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(unknown("identities", other)),
        }
    }
    Ok(Identities { json })
}

impl Subcommand for Identities {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        identities(env, self.json, out)
    }
}

/// One certificate in the store, judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// `keystore::id_of` its certificate.
    pub id: String,
    /// `keystore::thumbprint_of` its certificate.
    pub sha1: String,
    /// What `sign_cms::usable` said.
    pub offer: Result<Offer, String>,
}

impl Listed {
    /// The name `--identity` matches and `identities` prints.
    fn subject(&self, certificate: &[u8]) -> String {
        match &self.offer {
            Ok(offer) => offer.subject.clone(),
            Err(_) => sign_cms::named(certificate),
        }
    }
}

/// Judges every certificate at `now`, by the chooser's rule
/// (`sign_cms::usable`), keeping each one's id.
#[must_use]
pub fn listing(found: &[(String, Vec<u8>)], now: u64) -> Vec<(Listed, String)> {
    found
        .iter()
        .map(|(id, der)| {
            let listed = Listed {
                id: id.clone(),
                sha1: crate::keystore::thumbprint_of(der),
                offer: sign_cms::usable(der, now),
            };
            let subject = listed.subject(der);
            (listed, subject)
        })
        .collect()
}

/// The `identities` report for a listing.
#[must_use]
pub fn identities_report(listed: &[(Listed, String)]) -> report::Identities {
    let mut out = report::Identities {
        schema: SCHEMA,
        command: "identities".into(),
        usable: Vec::new(),
        not_usable: Vec::new(),
    };
    for (entry, subject) in listed {
        match &entry.offer {
            Ok(offer) => out.usable.push(usable_of(entry, offer)),
            Err(why) => out.not_usable.push(report::NotUsable {
                id: entry.id.clone(),
                sha1: entry.sha1.clone(),
                subject: subject.clone(),
                why: why.clone(),
            }),
        }
    }
    out
}

pub(crate) fn usable_of(entry: &Listed, offer: &Offer) -> report::Usable {
    report::Usable {
        id: entry.id.clone(),
        sha1: entry.sha1.clone(),
        subject: offer.subject.clone(),
        issuer: offer.issuer.clone(),
        expires: offer.expires.clone(),
        method: offer.method.clone(),
    }
}

/// Which certificate `--identity` names, or why none may sign.
///
/// A 64-digit hex string is a certificate's SHA-256 (`keystore::id_of`), any
/// case, and a 40-digit one its SHA-1 (`keystore::thumbprint_of`), which is
/// the thumbprint Windows' own tools show. Anything else is a subject as
/// `identities` prints it, matched
/// exactly, **among the certificates that may sign**: a renewed certificate
/// beside the expired one it replaced is the ordinary case, and the expired one
/// cannot sign, so it is not a rival. Two that may sign are ambiguous and both
/// are listed, with the ids that tell them apart --- tpdf does not choose a key
/// for anybody. A subject that only names certificates that may not sign says
/// why each may not.
///
/// # Errors
///
/// The sentence for exit code 3.
pub fn resolve(wanted: &str, listed: &[(Listed, String)]) -> Result<usize, String> {
    let hex = wanted.chars().all(|c| c.is_ascii_hexdigit());
    // Which hash a string of hex digits is, by its length alone.
    let hash = match wanted.len() {
        64 if hex => Some("SHA-256"),
        40 if hex => Some("SHA-1"),
        _ => None,
    };
    if let Some(hash) = hash {
        let wanted = wanted.to_ascii_lowercase();
        let Some(at) = listed.iter().position(|(entry, _)| {
            if hash == "SHA-1" {
                entry.sha1 == wanted
            } else {
                entry.id == wanted
            }
        }) else {
            return Err(format!(
                "no certificate with a key in your keychain or certificate store has the \
                 {hash} {wanted} --- `identities` lists the ones there are"
            ));
        };
        return match &listed[at].0.offer {
            Ok(_) => Ok(at),
            Err(why) => Err(format!("{} cannot sign: {why}", listed[at].1)),
        };
    }

    let named: Vec<usize> = listed
        .iter()
        .enumerate()
        .filter(|(_, (_, subject))| subject == wanted)
        .map(|(at, _)| at)
        .collect();
    let usable: Vec<usize> = named
        .iter()
        .copied()
        .filter(|at| listed[*at].0.offer.is_ok())
        .collect();
    match usable.as_slice() {
        [one] => Ok(*one),
        [] if named.is_empty() => Err(format!(
            "no certificate with a key in your keychain or certificate store is issued to \
             \"{wanted}\" --- `identities` lists the ones there are"
        )),
        [] => {
            let reasons: Vec<String> = named
                .iter()
                .filter_map(|at| {
                    let (entry, _) = &listed[*at];
                    entry
                        .offer
                        .as_ref()
                        .err()
                        .map(|why| format!("{} ({why})", entry.id))
                })
                .collect();
            Err(format!(
                "\"{wanted}\" names no certificate that can sign: {}",
                reasons.join("; ")
            ))
        }
        several => {
            let ids: Vec<String> = several
                .iter()
                .map(|at| {
                    let (entry, _) = &listed[*at];
                    let expires = entry
                        .offer
                        .as_ref()
                        .map(|offer| offer.expires.clone())
                        .unwrap_or_default();
                    format!("{} (expires {expires})", entry.id)
                })
                .collect();
            Err(format!(
                "\"{wanted}\" names {} certificates that can sign, so it does not say which: {} \
                 --- give --identity the SHA-256 of the one to use",
                several.len(),
                ids.join(", ")
            ))
        }
    }
}

pub(crate) fn store_identities(env: &Env<'_>) -> Result<Vec<Held>, Failure> {
    env.store
        .identities()
        .map_err(|why| Failure::new(Exit::Internal, why))
}

fn identities(env: &Env<'_>, as_json: bool, out: &mut dyn Write) -> Result<Exit, Failure> {
    let held = store_identities(env)?;
    let found: Vec<(String, Vec<u8>)> = held
        .iter()
        .map(|h| {
            (
                crate::keystore::id_of(&h.certificate),
                h.certificate.clone(),
            )
        })
        .collect();
    let report = identities_report(&listing(&found, env.now));
    if as_json {
        json(out, &report);
    } else {
        say(out, &identities_text(&report));
    }
    Ok(Exit::Ok)
}

/// `identities` without `--json`.
#[must_use]
pub fn identities_text(report: &report::Identities) -> String {
    let mut lines = Vec::new();
    if report.usable.is_empty() {
        lines.push("No certificate here can sign a document.".to_string());
    } else {
        lines.push("Can sign:".to_string());
        for id in &report.usable {
            lines.push(format!(
                "  {} --- issued by {}, {}, expires {}",
                id.subject, id.issuer, id.method, id.expires
            ));
            lines.push(format!("    {}", id.id));
            lines.push(format!("    SHA-1 {}", id.sha1.to_ascii_uppercase()));
        }
    }
    if !report.not_usable.is_empty() {
        lines.push("Cannot sign:".to_string());
        for id in &report.not_usable {
            lines.push(format!("  {} --- {}", id.subject, id.why));
            lines.push(format!("    {}", id.id));
            lines.push(format!("    SHA-1 {}", id.sha1.to_ascii_uppercase()));
        }
    }
    lines.join("\n")
}
