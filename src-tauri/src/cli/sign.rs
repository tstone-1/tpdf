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
use crate::commands::sign::{asking_the_world, finish, Asking, Finished, Stage, Stopped};
use crate::docinfo;
use crate::save;
use crate::sign_cms;
use crate::sign_prepare::{Options, Visible};

/// `sign`, registered.
pub const COMMAND: Registered = Registered {
    name: "sign",
    usage: "sign <in.pdf> -o <out.pdf> --identity <subject | sha256 | sha1>\n        [--visible (--rect x,y,w,h | --anchor TEXT --size w,h [--offset dx,dy]\n         [--anchor-match N]) [--page N] [--image FILE | --no-image]\n         [--lines label,name,date | --text LINE ...] [--date-format FORMAT]\n         [--hide reason,location]]\n        [--reason TEXT] [--location TEXT] [--contact TEXT] [--field NAME]\n        [--timestamp digicert|sectigo|globalsign|<url> [--long-term]]\n        [--force] [--json]",
    summary: "Signs with a certificate from your keychain (macOS) or your\n            certificate store (Windows). The key never leaves the operating\n            system, which may ask you to allow its use. The original is never\n            changed; the signed copy is written to -o, which must not exist\n            unless --force is given. --visible draws it on a page: --rect is\n            x,y,w,h in points from the top-left corner of the page as\n            displayed, --page counts from 1, and the saved signature image is\n            drawn unless --no-image is given or --image names a PNG or JPEG\n            file to draw instead, for this signature only; the saved image is\n            then neither read nor changed. --reason, --location and --contact are\n            written to the signature, with or without --visible; a visible\n            signature draws the reason and location as lines of it. --hide writes the ones\n            it names without drawing them, and nothing is hidden without it.\n            --anchor puts it beside text on the page instead: its top-left\n            corner is the text's, moved by --offset, and --size is its width\n            and height; text found more than once needs --anchor-match.\n            --text draws your own lines instead of the standard ones: {name},\n            {date}, {reason} and {location} are filled in, \\n starts a new\n            line, and --text may be given more than once. --date-format writes\n            the date with YYYY, MM, DD, HH, mm and ss; the time is UTC.\n            --field signs an empty signature field the document has, by its\n            name: the signature is written into that field, and with --visible\n            it is drawn in the field's rectangle, so --rect, --page and --anchor\n            are left out.\n            --timestamp asks that\n            timestamp authority for an RFC 3161 timestamp over the new\n            signature; nothing is sent anywhere without it, and if the\n            authority does not answer with one that checks out, nothing\n            is written.",
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
    /// or SHA-1 of the certificate in hex.
    pub identity: String,
    /// Where a visible signature goes; `None` for an invisible one.
    pub visible: Option<Placement>,
    /// The image a visible signature draws.
    pub image: Picture,
    /// Which of the three lines a visible signature draws.
    pub lines: Lines,
    /// `/Reason`, written, and drawn by a visible signature; empty for none.
    pub reason: String,
    /// `/Location`, written, and drawn by a visible signature; empty for none.
    pub location: String,
    /// `--contact`: `/ContactInfo`, written and never drawn; empty for none.
    pub contact: String,
    /// `--field`: the empty signature field to sign, by its full name; empty
    /// for a field of the signature's own.
    pub field: String,
    /// `--hide`: which of the two are written and not drawn.
    pub hide: Hidden,
    /// `--text`: the lines drawn instead of the standard ones, one template a
    /// line (`sign_prepare::Options::text`). Empty without it.
    pub text: Vec<String>,
    /// `--date-format`; empty for the standard one.
    pub date_format: String,
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

/// Which image a visible signature draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    /// The saved signature image, when one is saved. The default.
    Saved,
    /// None: `--no-image`.
    None,
    /// The PNG or JPEG file `--image` names, for this signature only. The
    /// saved image is neither read nor changed.
    File(PathBuf),
}

/// Where a visible signature goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The page, counted from 1 as a reader counts them.
    pub page: u32,
    /// The rectangle, given or found.
    pub at: Where,
}

/// How a visible signature's rectangle is said.
#[derive(Debug, Clone, PartialEq)]
pub enum Where {
    /// `--rect`: `[left, top, right, bottom]`, points, measured from the
    /// top-left corner of the page as it is displayed --- the space the
    /// viewer's own placement is measured in (`sign_prepare::Visible::rect`).
    Rect([f32; 4]),
    /// `--anchor`: beside text the page carries.
    Anchor(Anchor),
    /// `--field` with `--visible`: in the field's own rectangle, on its page.
    Field,
}

/// `--anchor TEXT --size w,h [--offset dx,dy] [--anchor-match N]`: a
/// rectangle measured from text on the page instead of from its corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// The text to find, as the viewer's search finds it: case is ignored.
    pub text: String,
    /// Which match, counted from 1 in reading order; `None` requires the page
    /// to hold exactly one.
    pub nth: Option<u32>,
    /// How far right and down of the match's top-left corner the rectangle's
    /// own top-left corner is, in points. Either may be negative.
    pub offset: [f32; 2],
    /// The rectangle's width and height, in points.
    pub size: [f32; 2],
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

/// What `--hide` names: text written to the signature and kept out of its
/// appearance. Nothing by default, so a line without `--hide` draws what it
/// always drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hidden {
    /// `/Reason` is written and *Reason: ...* is not drawn.
    pub reason: bool,
    /// `/Location` is written and *Location: ...* is not drawn.
    pub location: bool,
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
    let mut anchor: Option<String> = None;
    let mut anchor_match: Option<u32> = None;
    let mut offset: Option<[f32; 2]> = None;
    let mut size: Option<[f32; 2]> = None;
    let mut no_image = false;
    let mut image: Option<PathBuf> = None;
    let mut lines: Option<Lines> = None;
    let mut reason: Option<String> = None;
    let mut location: Option<String> = None;
    let mut contact: Option<String> = None;
    let mut field: Option<String> = None;
    let mut hide: Option<Hidden> = None;
    let mut text: Vec<String> = Vec::new();
    let mut date_format: Option<String> = None;
    let mut timestamp: Option<url::Url> = None;
    let mut long_term = false;
    let mut json = false;
    let mut force = false;

    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "-o" | "--output") => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            (false, "--identity") => identity = Some(value(arg, &mut rest)?.clone()),
            (false, "--visible") => visible = true,
            (false, "--page") => page = Some(page_number(value(arg, &mut rest)?)?),
            (false, "--rect") => rect = Some(rectangle(value(arg, &mut rest)?)?),
            (false, "--anchor") => anchor = Some(value(arg, &mut rest)?.clone()),
            (false, "--anchor-match") => {
                anchor_match = Some(match_number(value(arg, &mut rest)?)?);
            }
            (false, "--offset") => offset = Some(pair(arg, value(arg, &mut rest)?, false)?),
            (false, "--size") => size = Some(pair(arg, value(arg, &mut rest)?, true)?),
            (false, "--no-image") => no_image = true,
            (false, "--image") => image = Some(PathBuf::from(value(arg, &mut rest)?)),
            (false, "--lines") => lines = Some(line_list(value(arg, &mut rest)?)?),
            (false, "--reason") => reason = Some(value(arg, &mut rest)?.clone()),
            (false, "--location") => location = Some(value(arg, &mut rest)?.clone()),
            (false, "--contact") => contact = Some(value(arg, &mut rest)?.clone()),
            (false, "--field") => field = Some(value(arg, &mut rest)?.clone()),
            (false, "--hide") => hide = Some(hidden_list(value(arg, &mut rest)?)?),
            (false, "--text") => text.extend(text_lines(value(arg, &mut rest)?)),
            (false, "--date-format") => date_format = Some(value(arg, &mut rest)?.clone()),
            // Judged here, so an address tpdf will not ask --- `ftp:`, a
            // typo, a URL with a password in it --- is a malformed line and
            // exit 2, before any worker, key or socket.
            (false, "--timestamp") => {
                timestamp = Some(
                    crate::tsa::authority(value(arg, &mut rest)?)
                        .map_err(|why| why.sentence(""))?,
                );
            }
            (false, "--long-term") => long_term = true,
            (false, "--json") => json = true,
            (false, "--force") => force = true,
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("sign", flag))
            }
            (_, path) => {
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
        "`sign` needs `--identity`: a certificate's subject as `identities` lists it, its \
         SHA-256, or its SHA-1 thumbprint",
    )?;

    // The appearance's options belong to a visible signature, and are refused
    // rather than ignored without one: a line with one of them and no
    // `--visible` has a mistake in it. A reason, a location and a contact are
    // the signature dictionary's, and an invisible signature carries them.
    let appearance = [
        ("--page", page.is_some()),
        ("--rect", rect.is_some()),
        ("--anchor", anchor.is_some()),
        ("--anchor-match", anchor_match.is_some()),
        ("--offset", offset.is_some()),
        ("--size", size.is_some()),
        ("--no-image", no_image),
        ("--image", image.is_some()),
        ("--lines", lines.is_some()),
        ("--hide", hide.is_some()),
        ("--text", !text.is_empty()),
        ("--date-format", date_format.is_some()),
    ];
    if !visible {
        if let Some((flag, _)) = appearance.iter().find(|(_, given)| *given) {
            return Err(format!(
                "`{flag}` describes a visible signature --- add `--visible`, or leave it out"
            ));
        }
    }
    // Hiding what was not given is a line with a mistake in it: the script
    // meant a reason to be written, and none would be.
    let hide_given = hide.is_some();
    let hide = hide.unwrap_or_default();
    for (word, hidden, given) in [
        ("reason", hide.reason, reason.is_some()),
        ("location", hide.location, location.is_some()),
    ] {
        if hidden && !given {
            return Err(format!(
                "`--hide {word}` writes the {word} without drawing it, and no `--{word}` was \
                 given"
            ));
        }
    }
    let date_drawn = wording(&text, lines, hide_given, &reason, &location)?;
    if let Some(format) = &date_format {
        if format.trim().is_empty() {
            return Err("`--date-format` was given nothing to write the date with".into());
        }
        if !date_drawn {
            return Err(
                "`--date-format` says how the date is written, and this signature draws no date"
                    .into(),
            );
        }
    }
    // Two answers to one question. Neither wins silently: a script that passes
    // both has a mistake in it, and which image was drawn would hide it.
    if no_image && image.is_some() {
        return Err(
            "`--image` names the image to draw and `--no-image` says to draw none --- give one \
             of them"
                .into(),
        );
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
    // A field is a place: it says the page and the rectangle, and a second
    // answer beside it is a line with a mistake in it.
    if let Some(name) = &field {
        if name.is_empty() {
            return Err("`--field` needs the name of the signature field to sign".into());
        }
        let placed = [
            (page.is_some(), "--page"),
            (rect.is_some(), "--rect"),
            (anchor.is_some(), "--anchor"),
            (anchor_match.is_some(), "--anchor-match"),
            (offset.is_some(), "--offset"),
            (size.is_some(), "--size"),
        ];
        for (given, flag) in placed {
            if given {
                return Err(format!(
                    "`--field` signs a field the document has, which says where the signature \
                     goes --- leave `{flag}` out"
                ));
            }
        }
    }
    let placement = if visible && field.is_some() {
        Some(Placement {
            page: 1,
            at: Where::Field,
        })
    } else if visible {
        Some(Placement {
            page: page.unwrap_or(1),
            at: placed(rect, anchor, anchor_match, offset, size)?,
        })
    } else {
        None
    };

    Ok(Sign {
        input,
        output,
        identity,
        visible: placement,
        image: match image {
            Some(path) => Picture::File(path),
            None if no_image => Picture::None,
            None => Picture::Saved,
        },
        lines: lines.unwrap_or_default(),
        reason: reason.unwrap_or_default(),
        location: location.unwrap_or_default(),
        contact: contact.unwrap_or_default(),
        field: field.unwrap_or_default(),
        hide,
        text,
        date_format: date_format.unwrap_or_default(),
        timestamp,
        long_term,
        json,
        force,
    })
}

/// How a long-term refusal ends the run: exit 3 for the document's, an
/// authority's or a certificate authority's refusal, with the advice to sign
/// without the data --- except after a revocation, which no signing should
/// use --- and exit 4 when the failure is tpdf's own, a worker that died or
/// did not answer included (`longterm::Refusal::tpdf_failed`).
pub(crate) fn long_term_failure(why: &crate::longterm::Refusal) -> Failure {
    let next = if why.revoked() {
        " --- nothing was written"
    } else {
        " --- nothing was written; sign again without --long-term to sign without it"
    };
    let exit = if why.tpdf_failed() {
        Exit::Internal
    } else {
        Exit::Refused
    };
    Failure::new(exit, format!("{}{next}", why.sentence()))
}

/// The rest of a signing after the OS has signed --- the timestamp, the seal,
/// the long-term data, the write and a fresh worker's read-back --- which is
/// the window's own (`commands::sign::finish`), with the tool's endings: a
/// request that fails is exit 3 with nothing written, since the authority
/// refused or could not be reached, which is neither tpdf's failure nor a
/// reason to write a signature the reader did not ask for. There is nobody
/// here to offer "sign without one" to; running again without the option is
/// it, and the sentence says so --- except after a revocation, which no
/// signing should use. A copy written and not read back is tpdf's failure (4).
///
/// # Errors
///
/// The sentence and the exit code for each way the signing stopped.
pub(crate) fn concluded(
    sign: &Sign,
    made: sign_cms::Made,
    now: u64,
    worker: &dyn save::Verifier,
    asking: Asking<'_>,
) -> Result<Finished, Failure> {
    finish(
        Stage::Made(made),
        sign.timestamp.as_ref(),
        sign.long_term,
        &sign.output,
        now,
        worker,
        asking,
        &|bytes| save::write_signed(&sign.input, &sign.output, bytes).map_err(|why| why.message),
    )
    .map_err(|stopped| match stopped {
        Stopped::Unstamped { why, .. } => {
            let host = sign
                .timestamp
                .as_ref()
                .and_then(url::Url::host_str)
                .unwrap_or_default();
            Failure::new(
                Exit::Refused,
                format!(
                    "{} --- nothing was written; sign again without --timestamp to sign without \
                     one",
                    why.sentence(host)
                ),
            )
        }
        Stopped::Unextended { why, .. } => long_term_failure(&why),
        Stopped::Refused(why) | Stopped::Unwritten(why) => Failure::new(Exit::Refused, why),
        Stopped::Unread(why) => Failure::new(Exit::Internal, why),
    })
}

/// The image in the file `--image` names.
///
/// Refused, naming the file and why: it cannot be opened, it is larger than
/// the chooser's own limit, or it is not an image `signature_import` reads.
fn image_file(
    env: &Env<'_>,
    document: &std::fs::File,
    len: usize,
    path: &std::path::Path,
) -> Result<crate::signature::Image, Failure> {
    let refused = |why: &str| {
        Failure::new(
            Exit::Refused,
            format!(
                "the image {} cannot be used: {why} --- nothing was written",
                path.display()
            ),
        )
    };
    let image = std::fs::File::open(path).map_err(|e| refused(&format!("{e}")))?;
    let size = image
        .metadata()
        .map_err(|e| refused(&format!("{e}")))?
        .len();
    if size == 0 || size > crate::signature_import::MAX_BYTES {
        return Err(refused(crate::signature_import::INVALID));
    }
    env.worker()
        .signature_image(document, len, &image, size as usize)
        .map_err(|declined| match declined {
            // The image's own refusals name the image. Anything else a worker
            // refuses here is about the document it was started over, and is
            // said as the signing itself would say it.
            crate::save_outside::Declined::Refused(why)
                if [
                    crate::signature_import::INVALID,
                    crate::signature_import::CLEAR,
                ]
                .contains(&why.as_str()) =>
            {
                refused(&why)
            }
            other => other.into(),
        })
}

/// Where a visible signature is drawn, in the space `--rect` is given in.
///
/// The worker draws the appearance; this is the same layout over the same
/// rectangle, image size and lines, computed here because it parses nothing.
/// `at` is the signing time the worker is given, so the date line is the one
/// drawn. Rounded to a thousandth of a point.
#[must_use]
pub fn drawn(visible: &Visible, at: u64) -> report::Appearance {
    use crate::sign_prepare::appearance;
    let [left, top, right, bottom] = visible.rect.map(f64::from);
    let date = save::pdf_date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(at));
    let lines = appearance::words(&visible.name, &date, &visible.options);
    let layout = appearance::layout(
        right - left,
        bottom - top,
        visible.image.as_ref().map(|i| (i.width, i.height)),
        &lines,
    );
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    report::Appearance {
        page: visible.page + 1,
        rect: [left, top, right - left, bottom - top].map(round),
        image: layout
            .image
            .map(|[u, v, w, h]| [left + u, top + v, w, h].map(round)),
        font_size: round(layout.size),
        lines: layout
            .lines
            .iter()
            .map(|(u, baseline, text)| {
                let [u0, v0, u1, v1] = appearance::line_extent(layout.size, *u, *baseline, text);
                report::DrawnLine {
                    text: text.clone(),
                    rect: [left + u0, top + v0, u1 - u0, v1 - v0].map(round),
                    baseline: round(top + baseline),
                }
            })
            .collect(),
    }
}

/// Where a visible signature goes, from the two ways of saying it.
///
/// Exactly one of `--rect` and `--anchor`: both is two answers to one
/// question, and the three options that describe an anchored rectangle are a
/// mistake without an anchor.
fn placed(
    rect: Option<[f32; 4]>,
    anchor: Option<String>,
    nth: Option<u32>,
    offset: Option<[f32; 2]>,
    size: Option<[f32; 2]>,
) -> Result<Where, String> {
    let Some(text) = anchor else {
        for (flag, given) in [
            ("--anchor-match", nth.is_some()),
            ("--offset", offset.is_some()),
            ("--size", size.is_some()),
        ] {
            if given {
                return Err(format!(
                    "`{flag}` describes a signature placed beside text --- add `--anchor`, or \
                     leave it out"
                ));
            }
        }
        return rect.map(Where::Rect).ok_or_else(|| {
            "`--visible` needs `--rect x,y,w,h`, where the signature goes in points from the \
             page's top-left corner, or `--anchor TEXT --size w,h`, to put it beside text on \
             the page"
                .to_string()
        });
    };
    if rect.is_some() {
        return Err(
            "`--rect` says where the signature goes and `--anchor` finds where --- give one of \
             them"
                .into(),
        );
    }
    // The same rule `redact --text` holds a query to, and its sentence.
    let found = crate::search::Prepared::new(&text, ANCHOR_SEARCH)
        .map_err(|problem| format!("`--anchor {text}`: {problem}"))?;
    if found.matches_nothing() {
        return Err(format!(
            "`--anchor` needs text to find, and `{text}` can match nothing"
        ));
    }
    let size =
        size.ok_or("`--anchor` needs `--size w,h`: the signature's width and height in points")?;
    Ok(Where::Anchor(Anchor {
        text,
        nth,
        offset: offset.unwrap_or([0.0, 0.0]),
        size,
    }))
}

/// How an anchor is searched for: the viewer's plain search, case ignored.
const ANCHOR_SEARCH: crate::search::Options = crate::search::Options {
    match_case: false,
    whole_word: false,
    regex: false,
};

/// Finds the anchor on its page and returns the signature's rectangle,
/// `[left, top, right, bottom]` in the page's display space.
///
/// A worker reads the page's text; nothing of the document is parsed here.
/// Refused, with nothing written: the page does not exist, the text is not on
/// it, it is there more than once and no `--anchor-match` says which, or the
/// rectangle measured from it is not one a page can hold.
/// Where `--field` puts a visible signature: the field's page, counted from
/// nought, and its rectangle as the page is displayed.
///
/// Asked of the worker's form reader, for the report of what is drawn. The
/// worker that builds the signature finds the field again by its name and
/// takes its rectangle from the document, so nothing here decides where the
/// signature is written.
///
/// # Errors
///
/// Exit 3 for a document whose form cannot be read, a name it does not have,
/// a field that is not an empty signature field, or one in several places.
fn field_place(env: &Env<'_>, sign: &Sign) -> Result<(u32, [f32; 4]), Failure> {
    let refused = |why: String| Failure::new(Exit::Refused, why);
    let shown = sign.input.display().to_string();
    let name = &sign.field;
    let (file, len) = opened(&sign.input).map_err(refused)?;
    let mut session = env.worker().session(&file, len, None).map_err(|why| {
        refused(format!(
            "{shown} could not be opened to find `{name}`: {why:?}"
        ))
    })?;
    let form = super::fields::ask_form(&mut session, &shown, false)?;
    let found: Vec<_> = form.widgets.iter().filter(|w| &w.name == name).collect();
    let [widget] = found.as_slice() else {
        return Err(refused(if found.is_empty() {
            format!("{shown} has no field called `{name}` --- `tpdf fields` lists the ones it has")
        } else {
            format!(
                "`{name}` is shown in {} places, and a signature goes in one",
                found.len()
            )
        }));
    };
    match widget.control {
        crate::forms::Control::Signature { signed: false } => {}
        crate::forms::Control::Signature { signed: true } => {
            return Err(refused(format!("`{name}` already holds a signature")))
        }
        _ => return Err(refused(format!("`{name}` is not a signature field"))),
    }
    Ok((widget.page, widget.display_rect))
}

fn anchored(env: &Env<'_>, sign: &Sign, page: u32, anchor: &Anchor) -> Result<[f32; 4], Failure> {
    use super::redact::{search_pages, wait};
    use super::report::SearchKind;
    use crate::render::{Backend, RenderService};
    let refused = |why: String| Failure::new(Exit::Refused, why);
    let shown = sign.input.display().to_string();
    let (file, _) = opened(&sign.input).map_err(refused)?;
    let service = RenderService::start_with(env.library_dir.clone(), Backend::Worker);
    let info = wait(|reply| {
        service.open_handed(sign.input.clone(), Some(file), true, None, reply);
    })
    .map_err(|why: crate::progressive::Refusal| {
        refused(format!(
            "{shown} could not be opened to find the anchor: {}",
            why.reason
        ))
    })?;
    if page as usize > info.page_count {
        return Err(refused(format!(
            "{shown} has {} page{}, and `--page {page}` is not one of them",
            info.page_count,
            if info.page_count == 1 { "" } else { "s" }
        )));
    }
    let compiled = crate::search::Prepared::new(&anchor.text, ANCHOR_SEARCH).map_err(refused)?;
    let queries = [(SearchKind::Text, anchor.text.clone(), compiled)];
    let found = search_pages(&queries, &[page], |at| {
        wait(|reply| service.text(info.id, at, None, reply))
    })
    .map_err(|why| refused(format!("{shown}: {why}, so the anchor could not be found")))?;
    // Each match's first box: where its text starts.
    let boxes: Vec<[f32; 4]> = found
        .matches
        .iter()
        .filter_map(|hit| {
            let halves = super::regions::halves(std::slice::from_ref(hit));
            let text = found.texts.get(&(page - 1))?;
            super::regions::regions_on(text, page - 1, &halves)
                .first()
                .copied()
        })
        .collect();
    let text = &anchor.text;
    let chosen = match (anchor.nth, boxes.len()) {
        (_, 0) => {
            return Err(refused(format!(
                "page {page} of {shown} has no text `{text}` to put the signature beside --- \
                 nothing was written"
            )))
        }
        (None, 1) => boxes[0],
        (None, many) => {
            return Err(refused(format!(
                "page {page} of {shown} has `{text}` {many} times, so it does not say where the \
                 signature goes --- give `--anchor-match N` to choose one, counted from 1; \
                 nothing was written"
            )))
        }
        (Some(nth), many) => *boxes.get(nth as usize - 1).ok_or_else(|| {
            refused(format!(
                "page {page} of {shown} has `{text}` {many} time{}, and `--anchor-match {nth}` \
                 is not one of them --- nothing was written",
                if many == 1 { "" } else { "s" }
            ))
        })?,
    };
    let [dx, dy] = anchor.offset;
    let [w, h] = anchor.size;
    rectangle_of(chosen[0] + dx, chosen[1] + dy, w, h).ok_or_else(|| {
        refused(format!(
            "`{text}` is at {},{} on page {page}, and the offset puts the signature off the \
             page's top or left edge --- nothing was written",
            chosen[0], chosen[1]
        ))
    })
}

/// `--anchor-match`: which match, counted from 1.
fn match_number(text: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!(
            "`--anchor-match` is which match to use, counted from 1, and `{text}` is not a \
             number of one"
        )),
    }
}

/// Two numbers, `a,b`. A size is above zero; an offset is any finite pair.
fn pair(flag: &str, text: &str, size: bool) -> Result<[f32; 2], String> {
    let refused = || {
        if size {
            format!(
                "`{flag}` is a width and a height above zero, `w,h` in points --- `{text}` is not"
            )
        } else {
            format!("`{flag}` is two numbers, `dx,dy` in points --- `{text}` is not")
        }
    };
    let numbers: Vec<f32> = text
        .split(',')
        .map(|n| n.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| refused())?;
    let [a, b] = numbers.as_slice() else {
        return Err(refused());
    };
    let fine = a.is_finite() && b.is_finite() && (!size || (*a > 0.0 && *b > 0.0));
    fine.then_some([*a, *b]).ok_or_else(refused)
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

/// The lines one `--text` gives: a real line break and the two characters
/// `\n` both start a new line, since a line break is awkward to type into most
/// shells.
fn text_lines(given: &str) -> Vec<String> {
    given
        .replace("\r\n", "\n")
        .replace("\\n", "\n")
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// Holds `--text` to the rest of the line, and says whether a date is drawn.
///
/// A text is the whole wording, so `--lines` and `--hide` beside it are two
/// answers to one question. A brace that names nothing is refused here, before
/// any worker or key, and so is a `{reason}` or `{location}` that was not
/// given: it would draw as nothing, in a line that says there is one.
fn wording(
    text: &[String],
    lines: Option<Lines>,
    hide: bool,
    reason: &Option<String>,
    location: &Option<String>,
) -> Result<bool, String> {
    use crate::sign_prepare::Part;
    if text.is_empty() {
        return Ok(lines.unwrap_or_default().date);
    }
    for (flag, given) in [("--lines", lines.is_some()), ("--hide", hide)] {
        if given {
            return Err(format!(
                "`--text` is the whole wording of the signature and `{flag}` chooses among the \
                 standard lines --- give one of them"
            ));
        }
    }
    let mut date = false;
    let mut words = false;
    for line in text {
        for part in crate::sign_prepare::parts(line)? {
            match part {
                Part::Words(own) => words |= !own.trim().is_empty(),
                Part::Name => words = true,
                Part::Date => (date, words) = (true, true),
                Part::Reason | Part::Location => {
                    let (word, given) = if part == Part::Reason {
                        ("reason", reason.is_some())
                    } else {
                        ("location", location.is_some())
                    };
                    if !given {
                        return Err(format!(
                            "the text asks for `{{{word}}}`, and no `--{word}` was given"
                        ));
                    }
                    words = true;
                }
            }
        }
    }
    if !words {
        return Err(
            "`--text` was given no words to draw --- for an image alone, give `--lines \"\"`"
                .into(),
        );
    }
    Ok(date)
}

/// `reason,location`, either or both.
fn hidden_list(text: &str) -> Result<Hidden, String> {
    let mut hidden = Hidden::default();
    for word in text.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        match word {
            "reason" => hidden.reason = true,
            "location" => hidden.location = true,
            other => {
                return Err(format!(
                    "`--hide` names `reason` and `location`, and `{other}` is neither"
                ))
            }
        }
    }
    if hidden == Hidden::default() {
        return Err("`--hide` names `reason`, `location` or both, and was given neither".into());
    }
    Ok(hidden)
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

    // The image file, before the store is asked for anything: a file that is
    // missing or is not an image ends the run with no certificate listed, no
    // key touched and no keychain prompt. Decoded in a worker.
    let mut picture = match &sign.image {
        Picture::File(path) => Some(image_file(env, &file, len, path)?),
        _ => None,
    };

    // The anchor, before the store too: text that is not on the page ends
    // the run with no certificate listed and no key touched.
    let rect = match &sign.visible {
        None => None,
        Some(Placement {
            at: Where::Rect(rect),
            ..
        }) => Some(*rect),
        Some(Placement {
            page,
            at: Where::Anchor(anchor),
        }) => Some(anchored(env, sign, *page, anchor)?),
        Some(Placement {
            at: Where::Field, ..
        }) => None,
    };
    // The field, before the store as well: a name the document does not have
    // ends the run with no certificate listed and no key touched.
    let in_field = match &sign.visible {
        Some(Placement {
            at: Where::Field, ..
        }) => Some(field_place(env, sign)?),
        _ => None,
    };
    let page = in_field
        .map(|(page, _)| page)
        .or(sign.visible.as_ref().map(|placement| placement.page - 1));
    let rect = in_field.map(|(_, rect)| rect).or(rect);

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

    let visible = match page.zip(rect) {
        None => None,
        Some((page, rect)) => {
            let image = match &sign.image {
                Picture::Saved => env.store.saved_image().map_err(|why| {
                    Failure::new(
                        Exit::Refused,
                        format!(
                            "the saved signature image could not be read ({why}) --- give \
                             --no-image to sign without it"
                        ),
                    )
                })?,
                Picture::None => None,
                Picture::File(_) => picture.take(),
            };
            Some(Visible {
                page,
                rect,
                name: offer.subject.clone(),
                image,
                options: Options {
                    label: sign.lines.label,
                    name: sign.lines.name,
                    date: sign.lines.date,
                    reason: sign.reason.clone(),
                    location: sign.location.clone(),
                    hide_reason: sign.hide.reason,
                    hide_location: sign.hide.location,
                    text: sign.text.clone(),
                    date_format: sign.date_format.clone(),
                },
            })
        }
    };

    let worker = env.worker();
    let appearance = visible.as_ref().map(|visible| drawn(visible, env.now));
    let notes = crate::sign_prepare::Notes {
        reason: sign.reason.clone(),
        location: sign.location.clone(),
        contact: sign.contact.clone(),
        field: sign.field.clone(),
    };
    let unsigned = worker.prepare_signature(&file, len, env.now, visible, notes)?;
    drop(file);

    let original = save::read_to_sign(&sign.input, &opened_as)
        .map_err(|why| Failure::new(Exit::Refused, why.message))?;
    let made = sign_cms::sign(
        original,
        unsigned,
        env.now,
        &chosen.certificate,
        &chosen.chain,
        chosen.key.as_ref(),
    )
    .map_err(|why| Failure::new(Exit::Refused, why))?;
    // Everything after the OS has signed is the window's own tail. The
    // authority must be one this computer trusts before anything is fetched
    // for it: `env.anchors` is the system's store, or a test's own roots.
    let Finished {
        field,
        signatures,
        read_back,
    } = asking_the_world(env.anchors, |asking| {
        concluded(sign, made, env.now, &worker, asking)
    })?;
    let found: Vec<&docinfo::Signature> = signatures.iter().filter(|s| s.signed).collect();
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
    // with what was written, which is tpdf's failure (4). And for long-term
    // data, all that the check before writing held the same bytes to.
    let ours_intact = read_back.holds();
    let report = report::Signed {
        schema: SCHEMA,
        command: "sign".into(),
        input: sign.input.display().to_string(),
        output: sign.output.display().to_string(),
        field,
        identity: usable_of(&listed[at].0, &offer),
        visible: sign.visible.is_some(),
        appearance,
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
