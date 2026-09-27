//! The worker's half of signing: the revision a signature goes into.
//!
//! ## What this writes, and what it leaves for somebody else
//!
//! Phase 6 step 2 (`docs/PLAN.md` §9) splits signing across the process
//! boundary on purpose. **This half parses the document and holds no key**; the
//! app process holds the key --- or rather asks the OS, which holds it --- and
//! never parses the document. What crosses between them is bytes and numbers:
//! the incremental update this module builds, where its reserved `/Contents`
//! hole sits, the `/ByteRange` around it, and the SHA-256 of the bytes that
//! range covers. `sign_cms.rs` is the other half and recomputes that digest
//! over the bytes it will actually write before it believes this one.
//!
//! The revision is an **approval signature** and nothing else:
//!
//! - a signature dictionary, `/SubFilter /ETSI.CAdES.detached`, with the
//!   signing time in `/M` (PAdES B-B puts it there rather than in a signed
//!   attribute), a `/ByteRange` of fixed-width placeholders and a `/Contents`
//!   hex string of [`RESERVED`] zero bytes;
//! - an invisible widget --- a zero rectangle, `/F 132` (print and locked) ---
//!   that is also the field, named `SignatureN` for the first `N` no top-level
//!   field already uses, on the first page;
//! - that widget added to the page's `/Annots` and to the form's `/Fields`, and
//!   `/SigFlags` given bits 1 and 2 (signatures exist, append only).
//!
//! Only those objects change. A page whose `/Annots` is its own object has the
//! array rewritten and not the page, the same rule `save::append_update` keeps
//! for a mark, and for the same reason: an earlier signature's difference
//! analysis reads every rewritten object as a change it must account for.
//!
//! ## Refused, each with the reason in its message
//!
//! - **An encrypted document.** `lopdf` encrypts every string it appends with
//!   the document's key, and a signature's `/Contents` is the one string PDF
//!   32000-1 §7.6.2 says is *not* encrypted. Signing one would write a value no
//!   reader can decode. Whether an encrypted document can be signed correctly is
//!   a question for a writer that can leave that string alone; this one cannot.
//! - **A certification that permits no changes** (DocMDP `/P 1`). Any further
//!   revision breaks it, and a second signature is a further revision.
//! - **A placeholder that is not where it was put.** The two holes are found in
//!   the serialised update by their bytes, and exactly one of each must be
//!   there. A cloned page dictionary is the document's own bytes, and a string
//!   in it could spell a placeholder; that is a refusal, never a guess.

use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, StringFormat};
use sha2::{Digest as _, Sha256};

use crate::encoding::{resolve, MAX_DECODE};
use crate::pagetree::ordered_pages;

/// The bytes of DER the `/Contents` hole holds: **32 KiB**, 65,536 hex digits.
///
/// Chosen before any signature exists, because the hole's size is fixed the
/// moment the revision is serialised, and chosen so that Phase 6 step 3 needs no
/// second format. What fills it:
///
/// - step 2's CMS --- signed attributes, one signature value, the signer's
///   certificate and whatever intermediates the OS returns. Measured by
///   `sign_cms.rs`'s tests at about 1.3 KiB for an RSA-2048 self-signed
///   certificate; a real chain of three 4096-bit certificates is under 6 KiB;
/// - step 3's RFC 3161 token, an unsigned attribute added later, which carries
///   the TSA's own certificate chain: typically 3 to 8 KiB.
///
/// So step 2 refuses a CMS larger than [`STEP_TWO_LIMIT`], half of this, and
/// the other half is kept for the token. Acrobat and pyHanko reserve between
/// 8 and 32 KiB by default; the cost of the upper end is 64 KB of zeros per
/// signature in the file.
pub const RESERVED: usize = 32 * 1024;

/// The largest CMS step 2 may write, leaving the rest of [`RESERVED`] free for
/// the timestamp token step 3 adds.
pub const STEP_TWO_LIMIT: usize = RESERVED / 2;

/// What each `/ByteRange` number is written as before the offsets are known.
///
/// Ten digits each, thirty together, and the real numbers are written into the
/// same width with spaces after them. What has to fit is the three together,
/// so offsets of eleven or twelve digits still do; a file would have to pass
/// tens of terabytes before they did not, and [`fill_range`] refuses rather
/// than overflowing into the bytes after it.
const PLACEHOLDER: i64 = 9_999_999_999;

/// The `/ByteRange` exactly as `lopdf` serialises it with placeholders.
const RANGE_TEXT: &[u8] = b"/ByteRange[0 9999999999 9999999999 9999999999]";

/// Signatures exist (bit 1) and the document is append-only (bit 2).
const SIG_FLAGS: i64 = 3;

/// The revision a signature goes into, before anybody has signed it.
///
/// **Crosses the worker boundary**, and like `save::Update` it carries only
/// what the builder built and what it says it built against. The app process
/// checks every number here against the bytes it holds before it signs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unsigned {
    /// The update section: the new objects, a cross-reference and a trailer,
    /// with the `/ByteRange` already filled in and `/Contents` all zeros.
    pub update: Vec<u8>,
    /// How long the document this was built against was. The update's offsets
    /// are measured from it.
    pub built_against: usize,
    /// `/ByteRange` as written: `[0, hole, hole_end, rest]`, in file offsets.
    pub range: [u64; 4],
    /// SHA-256 over the two pieces the range covers.
    pub digest: Vec<u8>,
    /// The new field's name, for the report after signing.
    pub field: String,
}

/// Builds the revision a signature goes into.
///
/// `signed_at` is seconds since the epoch, supplied by the caller so the
/// worker's clock is not the one the document records.
///
/// # Errors
///
/// The document cannot be parsed strictly; it is encrypted; a certification
/// forbids any change; it has no page; the form or a page's `/Annots` is a shape
/// a signature field cannot be added to; or a placeholder is not exactly once
/// where it was written.
pub fn prepare(
    original: Vec<u8>,
    signed_at: u64,
    password: Option<&str>,
) -> Result<Unsigned, String> {
    let was = original.len();
    let prev = Document::load_mem_with_options(
        &original,
        lopdf::LoadOptions {
            // An update needs a real `/Prev`; recovery can reconstruct objects
            // with no table to chain to. `save::append_update` says the same.
            strict: true,
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| format!("this document could not be parsed: {e}"))?;

    if prev.is_encrypted() || prev.was_encrypted() {
        return Err(
            "tpdf cannot sign an encrypted document yet: the signature's value \
                    would be encrypted with the document, and no reader could check it."
                .into(),
        );
    }
    if certified_without_changes(&prev) {
        return Err(
            "This document is certified with no changes permitted, so a further \
                    signature would break its certification."
                .into(),
        );
    }

    let page = *ordered_pages(&prev)
        .first()
        .ok_or("this document has no page to put a signature on")?;
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| "this document's catalog is not an object of its own".to_string())?;
    let field = field_name(&prev, root);
    let annots = annots_site(&prev, page)?;
    let form = form_site(&prev, root)?;

    let mut incremental = IncrementalDocument::create_from(original, prev);
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(signed_at);
    let signature = incremental
        .new_document
        .add_object(signature_dictionary(&crate::save::pdf_date(at)));
    let widget = incremental
        .new_document
        .add_object(widget(&field, signature, page));
    add_to_page(&mut incremental, page, annots, widget)?;
    add_to_form(&mut incremental, root, form, widget)?;

    let mut sink = crate::save::Tail {
        skip: was,
        seen: 0,
        tail: Vec::with_capacity(RESERVED * 2 + 4096),
    };
    incremental
        .save_to(&mut sink)
        .map_err(|e| format!("could not build the signature's revision: {e}"))?;
    let mut update = sink.tail;

    let range = fill_range(&mut update, was)?;
    let digest = {
        let prior = incremental.get_prev_documents_bytes();
        covered_digest(prior, &update, range)
    };
    Ok(Unsigned {
        update,
        built_against: was,
        range,
        digest,
        field,
    })
}

/// Whether a DocMDP certification in the catalog permits no change at all.
fn certified_without_changes(document: &Document) -> bool {
    let Ok(catalog) = document.catalog() else {
        return false;
    };
    let Some(signature) = catalog
        .get(b"Perms")
        .ok()
        .and_then(|perms| resolve(document, perms).as_dict().ok())
        .and_then(|perms| perms.get(b"DocMDP").ok())
        .and_then(|sig| resolve(document, sig).as_dict().ok())
    else {
        return false;
    };
    crate::docinfo::certification_of(document, signature) == 1
}

/// `SignatureN` for the smallest `N` no top-level field is already called.
///
/// Top level because the new field is added there, and a field's fully
/// qualified name is its own `/T` when it has no parent: two top-level fields of
/// one name are one field to every reader, which is what makes a collision a
/// defect rather than a style.
fn field_name(document: &Document, root: ObjectId) -> String {
    let taken: Vec<Vec<u8>> = document
        .get_dictionary(root)
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|form| resolve(document, form).as_dict().ok())
        .and_then(|form| form.get(b"Fields").ok())
        .and_then(|fields| resolve(document, fields).as_array().ok())
        .map(|fields| {
            fields
                .iter()
                .filter_map(|field| resolve(document, field).as_dict().ok())
                .filter_map(|field| field.get(b"T").ok())
                .filter_map(|name| resolve(document, name).as_str().ok())
                .map(<[u8]>::to_vec)
                .collect()
        })
        .unwrap_or_default();
    (1..)
        .map(|n| format!("Signature{n}"))
        .find(|name| !taken.iter().any(|t| t == name.as_bytes()))
        .expect("an unbounded range always has a free name")
}

/// Where a page's `/Annots` is, which decides which object the widget changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    /// An object of its own: the array changes and the owner does not.
    Object(ObjectId),
    /// Written in the owner itself, so the owner changes.
    Inline,
    /// Not there: the owner gains one.
    Absent,
}

fn annots_site(document: &Document, page: ObjectId) -> Result<Site, String> {
    let dict = document
        .get_dictionary(page)
        .map_err(|_| "the first page is not a dictionary".to_string())?;
    array_site(
        document,
        dict,
        b"Annots",
        "the first page's annotation list",
    )
}

/// Where the form is, and where its `/Fields` is inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Form {
    /// The form dictionary: its own object, in the catalog, or not there.
    at: Site,
    /// Its `/Fields`, the same three ways. `Absent` when the form is.
    fields: Site,
}

fn form_site(document: &Document, root: ObjectId) -> Result<Form, String> {
    let catalog = document
        .get_dictionary(root)
        .map_err(|_| "this document's catalog is not a dictionary".to_string())?;
    let (at, dict) = match catalog.get(b"AcroForm") {
        Err(_) => {
            return Ok(Form {
                at: Site::Absent,
                fields: Site::Absent,
            })
        }
        Ok(Object::Reference(id)) => (
            Site::Object(*id),
            document
                .get_dictionary(*id)
                .map_err(|_| "this document's form is not a dictionary".to_string())?,
        ),
        Ok(Object::Dictionary(dict)) => (Site::Inline, dict),
        Ok(_) => return Err("this document's form is not a dictionary".into()),
    };
    let fields = array_site(
        document,
        dict,
        b"Fields",
        "this document's list of form fields",
    )?;
    Ok(Form { at, fields })
}

/// Where the array under `key` in `owner` is: its own object, inline, or absent.
fn array_site(
    document: &Document,
    owner: &Dictionary,
    key: &[u8],
    what: &str,
) -> Result<Site, String> {
    match owner.get(key) {
        Err(_) => Ok(Site::Absent),
        Ok(Object::Array(_)) => Ok(Site::Inline),
        Ok(Object::Reference(id)) => match document.get_object(*id) {
            Ok(Object::Array(_)) => Ok(Site::Object(*id)),
            _ => Err(format!("{what} is not an array")),
        },
        Ok(_) => Err(format!("{what} is not an array")),
    }
}

/// The signature dictionary with both holes still unfilled.
fn signature_dictionary(date: &str) -> Dictionary {
    let mut sig = Dictionary::new();
    sig.set("Type", Object::Name(b"Sig".to_vec()));
    sig.set("Filter", Object::Name(b"Adobe.PPKLite".to_vec()));
    sig.set("SubFilter", Object::Name(b"ETSI.CAdES.detached".to_vec()));
    sig.set(
        "M",
        Object::String(date.as_bytes().to_vec(), StringFormat::Literal),
    );
    sig.set(
        "ByteRange",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(PLACEHOLDER),
            Object::Integer(PLACEHOLDER),
            Object::Integer(PLACEHOLDER),
        ]),
    );
    sig.set(
        "Contents",
        Object::String(vec![0; RESERVED], StringFormat::Hexadecimal),
    );
    sig
}

/// The field and its one widget, merged, with no appearance and no area.
///
/// `/F 132` is Print (4) and Locked (128). A zero rectangle is how PDF 32000-1
/// §12.7.4.5 spells an invisible signature; a visible one is the next step.
fn widget(field: &str, signature: ObjectId, page: ObjectId) -> Dictionary {
    let mut widget = Dictionary::new();
    widget.set("Type", Object::Name(b"Annot".to_vec()));
    widget.set("Subtype", Object::Name(b"Widget".to_vec()));
    widget.set("FT", Object::Name(b"Sig".to_vec()));
    widget.set(
        "T",
        Object::String(field.as_bytes().to_vec(), StringFormat::Literal),
    );
    widget.set("V", Object::Reference(signature));
    widget.set("F", Object::Integer(132));
    widget.set(
        "Rect",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    widget.set("P", Object::Reference(page));
    widget
}

/// Adds the widget to the first page's `/Annots`, changing only what holds it.
fn add_to_page(
    incremental: &mut IncrementalDocument,
    page: ObjectId,
    site: Site,
    widget: ObjectId,
) -> Result<(), String> {
    let changed = match site {
        Site::Object(array) => array,
        Site::Inline | Site::Absent => page,
    };
    incremental
        .opt_clone_object_to_new_document(changed)
        .map_err(|e| format!("could not bring the first page across: {e}"))?;
    let object = incremental
        .new_document
        .get_object_mut(changed)
        .map_err(|e| e.to_string())?;
    match site {
        Site::Object(_) => push(object, widget),
        Site::Inline | Site::Absent => {
            let dict = object.as_dict_mut().map_err(|e| e.to_string())?;
            push_under(dict, site, b"Annots", widget)
        }
    }
}

/// Adds the widget to the form's `/Fields`, creating the form if there is none,
/// and sets the two `/SigFlags` bits.
fn add_to_form(
    incremental: &mut IncrementalDocument,
    root: ObjectId,
    form: Form,
    widget: ObjectId,
) -> Result<(), String> {
    // The existing flags, read from the previous revision, where an indirect
    // value still resolves.
    let flags = {
        let prev = incremental.get_prev_documents();
        let dict = match form.at {
            Site::Object(id) => prev.get_dictionary(id).ok(),
            Site::Inline => prev
                .get_dictionary(root)
                .ok()
                .and_then(|catalog| catalog.get(b"AcroForm").ok())
                .and_then(|form| form.as_dict().ok()),
            Site::Absent => None,
        };
        dict.and_then(|form| form.get(b"SigFlags").ok())
            .and_then(|flags| resolve(prev, flags).as_i64().ok())
            .unwrap_or(0)
    };

    // The array first, when it is an object of its own: the form still changes
    // for its flags, but the list of fields is written once, where it lives.
    if let Site::Object(array) = form.fields {
        incremental
            .opt_clone_object_to_new_document(array)
            .map_err(|e| format!("could not bring the field list across: {e}"))?;
        let object = incremental
            .new_document
            .get_object_mut(array)
            .map_err(|e| e.to_string())?;
        push(object, widget)?;
    }

    let owner = match form.at {
        Site::Object(id) => id,
        Site::Inline | Site::Absent => root,
    };
    incremental
        .opt_clone_object_to_new_document(owner)
        .map_err(|e| format!("could not bring the form across: {e}"))?;
    let object = incremental
        .new_document
        .get_object_mut(owner)
        .map_err(|e| e.to_string())?;
    let owner = object.as_dict_mut().map_err(|e| e.to_string())?;
    let dict = match form.at {
        Site::Object(_) => owner,
        Site::Inline => owner
            .get_mut(b"AcroForm")
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?,
        Site::Absent => {
            owner.set("AcroForm", Dictionary::new());
            owner
                .get_mut(b"AcroForm")
                .and_then(Object::as_dict_mut)
                .map_err(|e| e.to_string())?
        }
    };
    if !matches!(form.fields, Site::Object(_)) {
        push_under(dict, form.fields, b"Fields", widget)?;
    }
    dict.set("SigFlags", Object::Integer(flags | SIG_FLAGS));
    Ok(())
}

/// Appends `widget` to an array that is an object of its own.
fn push(array: &mut Object, widget: ObjectId) -> Result<(), String> {
    array
        .as_array_mut()
        .map(|array| array.push(Object::Reference(widget)))
        .map_err(|e| e.to_string())
}

/// Appends `widget` to the array under `key` in `dict`, or creates one holding
/// only it. `site` is [`Site::Inline`] or [`Site::Absent`].
fn push_under(
    dict: &mut Dictionary,
    site: Site,
    key: &[u8],
    widget: ObjectId,
) -> Result<(), String> {
    let reference = Object::Reference(widget);
    match site {
        Site::Inline => dict
            .get_mut(key)
            .and_then(Object::as_array_mut)
            .map(|array| array.push(reference))
            .map_err(|e| e.to_string()),
        Site::Absent | Site::Object(_) => {
            dict.set(key.to_vec(), Object::Array(vec![reference]));
            Ok(())
        }
    }
}

/// The one position of `needle` in `hay`, or `None` when it is absent or
/// occurs more than once.
fn only(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut found = hay
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// Finds both holes in the serialised update, writes the real `/ByteRange`
/// over its placeholder, and returns it.
///
/// `was` is the length of the file the update follows, so a position in the
/// update is `was` further along in the file.
fn fill_range(update: &mut [u8], was: usize) -> Result<[u64; 4], String> {
    let mut hole = Vec::with_capacity(RESERVED * 2 + 11);
    hole.extend_from_slice(b"/Contents<");
    hole.resize(hole.len() + RESERVED * 2, b'0');
    hole.push(b'>');
    let placed = |what: &str| {
        format!(
            "tpdf could not find the signature's {what} exactly once in the revision it \
             built, so it will not guess where to sign"
        )
    };
    let contents = only(update, &hole).ok_or_else(|| placed("value"))?;
    let range_at = only(update, RANGE_TEXT).ok_or_else(|| placed("byte range"))?;

    // `<` and `>` are part of the hole: the signature's value is the whole hex
    // string, delimiters included, which is what `integrity::covered` checks.
    let first = was + contents + b"/Contents".len();
    let second = first + RESERVED * 2 + 2;
    let end = was + update.len();
    let range = [0, first as u64, second as u64, (end - second) as u64];

    let width = RANGE_TEXT.len() - b"/ByteRange".len();
    let mut text = format!("[0 {} {} {}", range[1], range[2], range[3]).into_bytes();
    if text.len() + 1 > width {
        return Err("this document is too large for tpdf to sign".into());
    }
    text.resize(width - 1, b' ');
    text.push(b']');
    let at = range_at + b"/ByteRange".len();
    update[at..at + width].copy_from_slice(&text);
    Ok(range)
}

/// SHA-256 over the two pieces `range` covers, of `prior` followed by `update`.
///
/// Written against the two buffers rather than a joined copy, which on a
/// 300 MB document is 300 MB nobody needs.
fn covered_digest(prior: &[u8], update: &[u8], range: [u64; 4]) -> Vec<u8> {
    let was = prior.len();
    let (first, second) = (range[1] as usize, range[2] as usize);
    let mut hasher = Sha256::new();
    hasher.update(prior);
    hasher.update(&update[..first - was]);
    hasher.update(&update[second - was..]);
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests;
