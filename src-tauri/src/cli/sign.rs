//! `tpdf sign <in.pdf> -o <out.pdf> --identity <subject | sha256> ...`: the
//! application's signing --- a worker's revision, the OS's signature, the
//! application's writer, a worker's read-back --- from a terminal.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, unknown, value};
use super::identities::{listing, resolve, store_identities, usable_of};
use super::report::{self, SCHEMA};
use super::verify::signature_report;
use super::{json, opened, say, words, Env, Exit, Failure, Registered, Subcommand};
use crate::docinfo;
use crate::save;
use crate::sign_cms;
use crate::sign_prepare::{Options, Visible};

/// `sign`, registered.
pub const COMMAND: Registered = Registered {
    name: "sign",
    usage: "sign <in.pdf> -o <out.pdf> --identity <subject | sha256>\n        [--visible --rect x,y,w,h [--page N] [--no-image]\n         [--lines label,name,date] [--reason TEXT] [--location TEXT]]\n        [--timestamp digicert|sectigo|globalsign|<url> [--long-term]]\n        [--force] [--json]",
    summary: "Signs with a certificate from your keychain (macOS) or your\n            certificate store (Windows). The key never leaves the operating\n            system, which may ask you to allow its use. The original is never\n            changed; the signed copy is written to -o, which must not exist\n            unless --force is given. --visible draws it on a page: --rect is\n            x,y,w,h in points from the top-left corner of the page as\n            displayed, --page counts from 1, and the saved signature image is\n            drawn unless --no-image is given. --timestamp asks that\n            timestamp authority for an RFC 3161 timestamp over the new\n            signature; nothing is sent anywhere without it, and if the\n            authority does not answer with one that checks out, nothing\n            is written.",
    parse: boxed,
};

/// `tpdf sign <in.pdf> -o <out.pdf> --identity <id> ...`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sign {
    /// The document to sign. It is never written.
    pub input: PathBuf,
    /// Where the signed copy goes.
    pub output: PathBuf,
    /// A certificate's subject as `tpdf identities` prints it, or the SHA-256
    /// of the certificate in hex.
    pub identity: String,
    /// Where a visible signature goes; `None` for an invisible one.
    pub visible: Option<Placement>,
    /// Whether a visible signature draws the saved signature image, when one is
    /// saved. `--no-image` turns it off.
    pub image: bool,
    /// Which of the three lines a visible signature draws.
    pub lines: Lines,
    /// `/Reason`, drawn and written; empty for none.
    pub reason: String,
    /// `/Location`, drawn and written; empty for none.
    pub location: String,
    /// The timestamp authority `--timestamp` names, already judged by
    /// `tsa::authority`; `None` asks nobody for anything.
    pub timestamp: Option<url::Url>,
    /// `--long-term`: gather and add long-term validation data. Only with a
    /// timestamp, which the parse enforces.
    pub long_term: bool,
    /// `--json`.
    pub json: bool,
    /// `--force`: replace an existing output file.
    pub force: bool,
}

/// Where a visible signature goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The page, counted from 1 as a reader counts them.
    pub page: u32,
    /// `[left, top, right, bottom]`, points, measured from the top-left corner
    /// of the page as it is displayed --- the space the viewer's own placement
    /// is measured in (`sign_prepare::Visible::rect`).
    pub rect: [f32; 4],
}

/// The three lines a visible signature can draw. All three by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Lines {
    /// *Digitally signed by*.
    pub label: bool,
    /// The certificate's subject name.
    pub name: bool,
    /// The signing time.
    pub date: bool,
}

impl Default for Lines {
    fn default() -> Self {
        Lines {
            label: true,
            name: true,
            date: true,
        }
    }
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `sign`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Sign, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut identity: Option<String> = None;
    let mut visible = false;
    let mut page: Option<u32> = None;
    let mut rect: Option<[f32; 4]> = None;
    let mut image: Option<bool> = None;
    let mut lines: Option<Lines> = None;
    let mut reason: Option<String> = None;
    let mut location: Option<String> = None;
    let mut timestamp: Option<url::Url> = None;
    let mut long_term = false;
    let mut json = false;
    let mut force = false;

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--identity" => identity = Some(value(arg, &mut rest)?.clone()),
            "--visible" => visible = true,
            "--page" => page = Some(page_number(value(arg, &mut rest)?)?),
            "--rect" => rect = Some(rectangle(value(arg, &mut rest)?)?),
            "--no-image" => image = Some(false),
            "--lines" => lines = Some(line_list(value(arg, &mut rest)?)?),
            "--reason" => reason = Some(value(arg, &mut rest)?.clone()),
            "--location" => location = Some(value(arg, &mut rest)?.clone()),
            // Judged here, so an address tpdf will not ask --- `ftp:`, a
            // typo, a URL with a password in it --- is a malformed line and
            // exit 2, before any worker, key or socket.
            "--timestamp" => {
                timestamp = Some(
                    crate::tsa::authority(value(arg, &mut rest)?)
                        .map_err(|why| why.sentence(""))?,
                );
            }
            "--long-term" => long_term = true,
            "--json" => json = true,
            "--force" => force = true,
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown("sign", flag)),
            path => {
                if input.is_some() {
                    return Err(format!(
                        "`sign` takes one document, and `{path}` is a second --- sign them one \
                         at a time"
                    ));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }

    let input = input.ok_or("`sign` needs the document to sign")?;
    let output = output.ok_or(
        "`sign` needs `-o <out.pdf>`: the signed document is written as a new file, and the \
         original is never changed",
    )?;
    if lexically_same(&input, &output) {
        return Err(
            "the output names the input --- the signed document is written as a new file, so \
             choose another name for it"
                .into(),
        );
    }
    let identity = identity.ok_or(
        "`sign` needs `--identity`: a certificate's subject as `identities` lists it, or its \
         SHA-256",
    )?;

    // The appearance's options belong to a visible signature, and are refused
    // rather than ignored without one: a reason typed and silently dropped is a
    // signature that does not say what its signer thinks it says. (An invisible
    // signature with a reason is not built --- `docs/PLAN.md`, Phase 6.)
    let appearance = [
        ("--page", page.is_some()),
        ("--rect", rect.is_some()),
        ("--no-image", image.is_some()),
        ("--lines", lines.is_some()),
        ("--reason", reason.is_some()),
        ("--location", location.is_some()),
    ];
    if !visible {
        if let Some((flag, _)) = appearance.iter().find(|(_, given)| *given) {
            return Err(format!(
                "`{flag}` describes a visible signature --- add `--visible`, or leave it out"
            ));
        }
    }
    // Long-term validation data rests on a timestamp (B-LT is B-T with the
    // data added), so asking for it without one is a malformed line.
    if long_term && timestamp.is_none() {
        return Err(
            "`--long-term` needs `--timestamp`: long-term validation data is added to a \
             timestamped signature"
                .into(),
        );
    }
    let placement = if visible {
        let rect = rect.ok_or(
            "`--visible` needs `--rect x,y,w,h`: where the signature goes, in points from the \
             page's top-left corner",
        )?;
        Some(Placement {
            page: page.unwrap_or(1),
            rect,
        })
    } else {
        None
    };

    Ok(Sign {
        input,
        output,
        identity,
        visible: placement,
        image: image.unwrap_or(true),
        lines: lines.unwrap_or_default(),
        reason: reason.unwrap_or_default(),
        location: location.unwrap_or_default(),
        timestamp,
        long_term,
        json,
        force,
    })
}

/// A page number, counted from 1.
fn page_number(text: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!(
            "`--page` is a page number counted from 1, and `{text}` is not one"
        )),
    }
}

/// `x,y,w,h` into `[left, top, right, bottom]`.
fn rectangle(text: &str) -> Result<[f32; 4], String> {
    let refused = || {
        format!(
            "`--rect` is four numbers, `x,y,w,h` in points from the page's top-left corner, \
             with a width and height above zero --- `{text}` is not"
        )
    };
    let numbers: Vec<f32> = text
        .split(',')
        .map(|n| n.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| refused())?;
    let [x, y, w, h] = numbers.as_slice() else {
        return Err(refused());
    };
    rectangle_of(*x, *y, *w, *h).ok_or_else(refused)
}

/// `x, y, w, h` as `[left, top, right, bottom]`, when it is a rectangle a page
/// can hold: finite, not left of or above the page's corner, and with a width
/// and height above zero. `sign --rect` and `redact --regions` share the rule,
/// because they share the convention.
pub(crate) fn rectangle_of(x: f32, y: f32, w: f32, h: f32) -> Option<[f32; 4]> {
    let fine = [x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite())
        && x >= 0.0
        && y >= 0.0
        && w > 0.0
        && h > 0.0;
    fine.then_some([x, y, x + w, y + h])
}

/// `label,name,date`, any subset, or empty for none.
fn line_list(text: &str) -> Result<Lines, String> {
    let mut lines = Lines {
        label: false,
        name: false,
        date: false,
    };
    for word in text.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        match word {
            "label" => lines.label = true,
            "name" => lines.name = true,
            "date" => lines.date = true,
            other => {
                return Err(format!(
                    "`--lines` names `label`, `name` and `date`, and `{other}` is none of them"
                ))
            }
        }
    }
    Ok(lines)
}

impl Subcommand for Sign {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        run_sign(env, self, out, err)
    }
}

fn run_sign(
    env: &Env<'_>,
    sign: &Sign,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    // What can be refused without asking the OS for anything, first: the
    // reader hears about a missing file before any keychain prompt.
    if save::same_file(&sign.input, &sign.output) {
        return Err(Failure::new(
            Exit::Usage,
            "the output is the input under another name --- the signed document is written \
             as a new file, so choose another name for it",
        ));
    }
    if !sign.force && sign.output.exists() {
        return Err(Failure::new(
            Exit::Refused,
            format!(
                "{} already exists --- choose another name, or give --force to replace it",
                sign.output.display()
            ),
        ));
    }
    let (file, len) = opened(&sign.input).map_err(|why| Failure::new(Exit::Refused, why))?;
    sign_cms::refuse_too_large(len as u64).map_err(|why| Failure::new(Exit::Refused, why))?;

    // The certificate, from the store --- which asks the OS for certificates
    // only. The key is not touched until the digest is signed.
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
    let listed = listing(&found, env.now);
    let at = resolve(&sign.identity, &listed).map_err(|why| Failure::new(Exit::Refused, why))?;
    let chosen = &held[at];
    let offer = listed[at]
        .0
        .offer
        .clone()
        .map_err(|why| Failure::new(Exit::Refused, why))?;

    // What the file looks like now: the fingerprint `read_to_sign` holds the
    // signed bytes to, as the application records one when it opens a file.
    let opened_as = crate::fingerprint::Fingerprint::of_open(&file, &sign.input)
        .map_err(|why| Failure::new(Exit::Refused, why))?;

    let visible = match sign.visible {
        None => None,
        Some(placement) => {
            let image = if sign.image {
                env.store.saved_image().map_err(|why| {
                    Failure::new(
                        Exit::Refused,
                        format!(
                            "the saved signature image could not be read ({why}) --- give \
                             --no-image to sign without it"
                        ),
                    )
                })?
            } else {
                None
            };
            Some(Visible {
                page: placement.page - 1,
                rect: placement.rect,
                name: offer.subject.clone(),
                image,
                options: Options {
                    label: sign.lines.label,
                    name: sign.lines.name,
                    date: sign.lines.date,
                    reason: sign.reason.clone(),
                    location: sign.location.clone(),
                },
            })
        }
    };

    let worker = env.worker();
    let unsigned = worker.prepare_signature(&file, len, env.now, visible)?;
    drop(file);

    let original = save::read_to_sign(&sign.input, &opened_as)
        .map_err(|why| Failure::new(Exit::Refused, why.message))?;
    let field = unsigned.field.clone();
    let made = sign_cms::sign(
        original,
        unsigned,
        env.now,
        &chosen.certificate,
        &chosen.chain,
        chosen.key.as_ref(),
    )
    .map_err(|why| Failure::new(Exit::Refused, why))?;
    // The timestamp, when one was asked for: after the OS has signed, since it
    // is over the signature's value, and before anything is written. A request
    // that fails is exit 3 with nothing written --- the authority refused or
    // could not be reached, which is neither tpdf's failure nor a reason to
    // write a signature the reader did not ask for. There is nobody here to
    // offer "sign without one" to; running again without `--timestamp` is it.
    let stamped = crate::tsa::stamp(&made, sign.timestamp.as_ref(), |url, value| {
        crate::tsa::ask_blocking(url, value, &crate::tsa::LIMITS)
    })
    .map_err(|why| {
        let host = sign
            .timestamp
            .as_ref()
            .and_then(url::Url::host_str)
            .unwrap_or_default();
        Failure::new(
            Exit::Refused,
            format!(
                "{} --- nothing was written; sign again without --timestamp to sign without one",
                why.sentence(host)
            ),
        )
    })?;
    let cms = stamped.clone();
    let bytes = made
        .seal(stamped)
        .map_err(|why| Failure::new(Exit::Refused, why))?;
    // The long-term data, when it was asked for: after the seal, since it is
    // about the certificates in the timestamped signature, and before anything
    // is written. Refused is exit 3 with nothing written; the sentence says how
    // to sign without it --- except for a revoked certificate, which no
    // signing should use.
    let bytes = match (sign.long_term, cms) {
        (true, Some(cms)) => crate::longterm::extend(
            &bytes,
            &cms,
            &field,
            env.now,
            &worker,
            &crate::longterm::os_chain,
            &mut crate::longterm::fetch_blocking,
            // The archive timestamp, from the authority the line named.
            &mut |pieces| match sign.timestamp.as_ref() {
                Some(url) => crate::tsa::ask_over_range_blocking(url, pieces, &crate::tsa::LIMITS)
                    .map_err(|why| why.sentence(url.host_str().unwrap_or_default())),
                None => Err(crate::longterm::Refusal::NoTimestamp.sentence()),
            },
        )
        .map_err(|why| {
            let next = if why.revoked() {
                " --- nothing was written"
            } else {
                " --- nothing was written; sign again without --long-term to sign without it"
            };
            Failure::new(Exit::Refused, format!("{}{next}", why.sentence()))
        })?,
        _ => bytes,
    };
    save::write_signed(&sign.input, &sign.output, &bytes)
        .map_err(|why| Failure::new(Exit::Refused, why.message))?;

    // Read back by a fresh worker, through the handle of the file just
    // written: the verdict reported is the one the properties dialog gives.
    let (written, written_len) = opened(&sign.output).map_err(|why| {
        Failure::new(
            Exit::Internal,
            format!("the signed file was written and could not be reopened: {why}"),
        )
    })?;
    let properties = worker
        .properties(&written, written_len)
        .map_err(|declined| {
            Failure::new(
                Exit::Internal,
                format!(
                    "the signed file was written and could not be checked: {}",
                    declined.message()
                ),
            )
        })?;
    let found: Vec<&docinfo::Signature> =
        properties.signatures.iter().filter(|s| s.signed).collect();
    let name = sign.output.file_name().map_or_else(
        || sign.output.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let timestamp = found
        .iter()
        .find(|s| s.field == field)
        .and_then(|s| s.timestamp.as_ref())
        .map(|stamp| super::verify::timestamp_report(stamp, false));
    // The document timestamps written after ours are the archive a long-term
    // signing adds; everything else is the signature or one before it.
    let ours_at = found.iter().position(|s| s.field == field);
    let archive = |index: usize, s: &docinfo::Signature| {
        s.kind == "ETSI.RFC3161" && ours_at.is_some_and(|at| index > at)
    };
    let summary = words::after_signing(
        &name,
        &field,
        &found
            .iter()
            .enumerate()
            .filter(|(index, s)| !archive(*index, s))
            .map(|(_, s)| (s.field.clone(), s.field == field, s.integrity.clone()))
            .collect::<Vec<_>>(),
        timestamp.as_ref().map(|t| {
            (
                t.integrity.sentence.as_str(),
                t.trust.as_ref().map(|trust| trust.sentence.as_str()),
            )
        }),
        &found
            .iter()
            .enumerate()
            .filter(|(index, s)| archive(*index, s))
            .map(|(_, s)| (s.field.clone(), s.integrity.clone()))
            .collect::<Vec<_>>(),
    );
    // Ours must read back intact, and --- when a timestamp was asked for ---
    // carry one that reads back intact too: a timestamp `seal` checked in the
    // bytes and a worker then did not find is a written file that disagrees
    // with what was written, which is tpdf's failure (4).
    let ours_intact = found.iter().any(|s| {
        s.field == field
            && s.integrity
                .as_ref()
                .is_some_and(|i| i.verdict == crate::integrity::Verdict::Intact)
            && (sign.timestamp.is_none()
                || s.timestamp.as_ref().is_some_and(|t| {
                    t.integrity
                        .as_ref()
                        .is_some_and(|i| i.verdict == crate::integrity::Verdict::Intact)
                }))
            // And, for long-term data, the signer's revocation read back
            // `good`: the check before writing said so of the same bytes.
            && (!sign.long_term
                || s.revocation
                    .as_ref()
                    .is_some_and(|r| r.standing == crate::revocation::Status::Good))
    });
    let report = report::Signed {
        schema: SCHEMA,
        command: "sign".into(),
        input: sign.input.display().to_string(),
        output: sign.output.display().to_string(),
        field,
        identity: usable_of(&listed[at].0.id, &offer),
        visible: sign.visible.is_some(),
        signatures: found.iter().map(|s| signature_report(s)).collect(),
        summary: summary.clone(),
    };
    if sign.json {
        json(out, &report);
    } else {
        say(out, &summary);
    }
    if ours_intact {
        Ok(Exit::Ok)
    } else {
        say(err, &format!("{}: {summary}", env.program));
        Ok(Exit::Internal)
    }
}
