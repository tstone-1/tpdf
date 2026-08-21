//! What a document says about itself --- its properties, its encryption, and
//! whether anybody signed it.
//!
//! ## The question a reader actually has
//!
//! Every other panel in this application is about the *content*: what is on the
//! pages, what a stranger wrote in the margin, where a cross-reference goes.
//! This one is about the *file*, and it exists because the questions it answers
//! were reachable only from a shell. Who produced this. When. Is it encrypted,
//! and what does that encryption forbid. Did anybody sign it, and does the
//! signature still cover the whole thing.
//!
//! Those are not idle questions for a document that arrives as a supplier's
//! compliance certificate, which is the case this was built against.
//!
//! ## Why `lopdf` and not PDFium, when PDFium can do it
//!
//! `fpdf_signature.h` is compiled into the vendored build --- all eight symbols
//! are exported, checked with `nm` rather than assumed --- so the signature half
//! of this could have gone through PDFium. It does not, for the reason
//! [`crate::annots`] gives at greater length: the object graph answers the whole
//! question in one parse the worker already knows how to do, and PDFium answers
//! only part of it. `FPDFSignatureObj_*` has no accessor for the signature
//! *field's* name or page, none for `/Location`, and nothing at all for the
//! `/Info` dictionary or the encryption dictionary --- so a PDFium
//! implementation would still need this parse and would then be a second
//! resolver disagreeing with it. That trap is recorded twice already.
//!
//! What PDFium's API is genuinely good for is a **differential**: two
//! independent readers of one file, compared. That is worth building and is not
//! built here; see `docs/PLAN.md`.
//!
//! ## Encryption is readable without the password, and that is by design
//!
//! Every field this reports about encryption --- `/V`, `/R`, `/P`, `/Length` ---
//! is plaintext in the trailer's `/Encrypt` dictionary and has to be: a reader
//! needs them *before* it can decrypt anything. So the security summary is
//! always available, even for a document nothing here can open.
//!
//! The `/Info` strings are the opposite. On an encrypted document they are
//! ciphertext, and `lopdf` does not decrypt on load. [`scan`] asks it to decrypt
//! with an empty user password, which is what the common case --- a document
//! locked against *editing* rather than against reading --- actually uses. When
//! that fails the fields are **omitted and the omission is reported**, never
//! shown as the mojibake that decoding ciphertext as PDFDocEncoding produces.
//!
//! ## Nothing here says a signature is valid
//!
//! This has no crypto stack, no certificate parser and no trust store, so it
//! cannot say whether a signature verifies, whether the certificate chains to
//! anything, or whether it was revoked. `docs/TRAPS.md` is explicit that the UI
//! must never imply otherwise, and the shape of this module is what enforces it:
//! there is no field here that could carry a verdict.
//!
//! What it reports instead is what the document *claims* --- the signer's name,
//! reason and date are strings the signer wrote, and are labelled as claimed ---
//! plus exactly one fact that is checkable without cryptography and is the one
//! that catches the common real failure: [`Signature::covers_whole_file`], which
//! is whether the signed byte range reaches the file's last byte. A document
//! signed and then appended to fails that, and no certificate is needed to say
//! so.

use lopdf::{Dictionary, Document, LoadOptions, Object};

use crate::annots::decode_text_string;
use crate::encoding::resolve;

/// Ceiling on decompressed stream size, as everywhere else that parses.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Most `/Info` entries reported. A document may carry arbitrary custom keys.
const MAX_FIELDS: usize = 64;

/// Most signature fields walked.
const MAX_SIGNATURES: usize = 32;

/// Longest reported value, in characters, before it is clipped.
const MAX_VALUE_CHARS: usize = 512;

/// The largest `/Contents` blob a certificate is parsed out of.
///
/// A real PKCS#7 blob is tens of kilobytes; the largest seen here is 184 KB, on
/// a document carrying a full chain and a timestamp. This bound exists because
/// the blob is attacker-chosen DER handed to a parser, and a document is free
/// to make it a megabyte of nesting. Exceeding it is reported through
/// [`Limits::certificates_unread`] rather than passed off as "no certificate".
const MAX_SIG_BLOB: usize = 1024 * 1024;

/// The `/Info` keys PDF 32000-1 §14.3.3 defines, in the order a reader wants.
///
/// Order is the point: a document's own dictionary order is arbitrary, and
/// listing Producer above Title because that is how the file happens to be laid
/// out reads as a bug. Anything not on this list is a custom key and follows.
const STANDARD: [&str; 8] = [
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
];

/// One line of the properties readout.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Field {
    /// The key, without its slash.
    pub name: String,
    /// The value, decoded and --- for the two date keys --- reformatted.
    pub value: String,
    /// Whether this key is one PDF 32000-1 defines, rather than the document's own.
    pub standard: bool,
}

/// What a document's encryption permits.
///
/// Named rather than a bit field, because `P = -60` is not something to put in
/// front of a reader, and because the mapping from bits to meanings **depends on
/// the revision** --- see [`permissions`].
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Permission {
    pub what: String,
    pub allowed: bool,
}

/// The document's encryption, read from the trailer.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Encryption {
    /// A reader-facing name for the algorithm: `RC4 40-bit`, `AES-256`.
    pub method: String,
    /// The standard security handler's revision, `/R`.
    pub revision: i64,
    /// Whether an empty user password opened it, which is what decides whether
    /// the `/Info` fields above could be read at all.
    pub opened_without_password: bool,
    /// What it permits, in a fixed order.
    pub permissions: Vec<Permission>,
}

/// One signature field, and what the document claims about it.
///
/// **No field here is a verdict.** See the module note.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Signature {
    /// The field's name, `/T`. Empty when it has none.
    pub field: String,
    /// Whether the field carries a signature at all, or is an empty placeholder.
    pub signed: bool,
    /// `/Filter` --- the handler that wrote it, such as `Adobe.PPKLite`.
    pub handler: String,
    /// `/SubFilter` --- the encoding, such as `adbe.pkcs7.detached`.
    pub kind: String,
    /// `/Name`: who the signer says they are. Claimed, never checked.
    pub name: String,
    /// `/Reason`, as the signer wrote it.
    pub reason: String,
    /// `/Location`, as the signer wrote it.
    pub location: String,
    /// `/M`, the claimed signing time, reformatted.
    pub when: String,
    /// Whether `/ByteRange` reaches the file's last byte.
    ///
    /// The one thing in this struct that is checked rather than claimed, and it
    /// is false for a document that was signed and then appended to.
    pub covers_whole_file: bool,
    /// How many bytes the signature covers, and how many there are.
    pub covered_bytes: u64,
    /// A DocMDP certification level, 1 to 3, when the signature carries one.
    ///
    /// 1 forbids every change, 2 permits filling in forms, 3 also permits
    /// annotating. `docs/TRAPS.md` records that this is **not** the
    /// discriminator it looks like --- a validator rejects edits the
    /// specification permits --- so it is reported and not acted on.
    pub certification: u8,
    /// What the signing certificate says, when one could be read.
    ///
    /// `None` means either that the blob carried no certificate or that it
    /// could not be parsed; [`Limits::certificates_unread`] separates the two.
    pub certificate: Option<Certificate>,
}

/// What the signing certificate says, as against what the signer typed.
///
/// Read out of the DER blob in `/Contents`. **Nothing here is verified.** No
/// chain is built, no issuer is looked up, no revocation list is consulted, and
/// the signature is never tested against the bytes it covers. A certificate is
/// a document like any other and states whatever its issuer put in it.
///
/// The reason it is worth reading at all is *provenance*, not validity.
/// [`Signature::name`] is free text the signer typed into the PDF; this is what
/// somebody put into a certificate and signed with a key. The two can disagree,
/// and on the fixtures here one of them is routinely empty while the other is
/// not --- `incr-signed.pdf` has no `/Name` and a certificate reading
/// `CN=tpdf spike 0.6 test signer`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Certificate {
    /// The subject's distinguished name, near enough RFC 4514 form.
    pub subject: String,
    /// The subject's common name on its own, empty when it has none.
    pub subject_cn: String,
    /// The issuer's distinguished name.
    pub issuer: String,
    /// The issuer's common name on its own.
    pub issuer_cn: String,
    /// The serial number as uppercase hex, no separators.
    pub serial: String,
    /// `notBefore`, formatted as every other date in this module is.
    pub from: String,
    /// `notAfter`.
    pub until: String,
    /// Issuer and subject are the same name.
    ///
    /// A checked fact about two byte strings, and **not** a verdict: a
    /// self-issued certificate is how every root in every trust store starts,
    /// and it is also what an unvouched-for signer produces. Which of those
    /// this is cannot be decided without a trust store, which tpdf has not got.
    pub self_issued: bool,
    /// How many certificates the blob carried, this one included.
    pub chain: u32,
    /// The signer's certificate was identified by `SignerInfo.sid`.
    ///
    /// False when the blob held exactly one certificate and the identifier did
    /// not match it, in which case the only certificate present is reported
    /// because a set of one leaves nothing to choose between. A blob with
    /// several and no match reports no certificate at all rather than guessing.
    pub matched_signer: bool,
}

/// What could not be read, so nothing here is silently partial.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// The document is encrypted and an empty user password did not open it.
    ///
    /// Nothing inside the object graph could be read --- not the `/Info` strings,
    /// not the signatures, not the structure tree --- because all of it is
    /// ciphertext. Everything reported alongside this flag is either from the
    /// trailer, which is plaintext by necessity, or from the raw bytes.
    pub locked: bool,
    /// `/Info` entries dropped at [`MAX_FIELDS`].
    pub fields_dropped: usize,
    /// Values shortened at [`MAX_VALUE_CHARS`].
    pub values_clipped: usize,
    /// Signature fields not walked, at [`MAX_SIGNATURES`].
    pub signatures_dropped: usize,
    /// `/Fields` entries that resolved to nothing usable.
    pub unreadable: usize,
    /// Signatures whose `/Contents` blob carried a certificate we could not read.
    ///
    /// Separate from a signature that simply has none, because the two want
    /// opposite readings: absent is a fact about the document, unread is a fact
    /// about tpdf, and reporting the second as the first would be tpdf agreeing
    /// with itself.
    pub certificates_unread: usize,
}

impl Limits {
    /// Whether anything was cut. The UI shows a notice on exactly this.
    #[must_use]
    pub fn any(&self) -> bool {
        self.locked
            || self.fields_dropped > 0
            || self.values_clipped > 0
            || self.signatures_dropped > 0
            || self.unreadable > 0
            || self.certificates_unread > 0
    }
}

/// Everything a document says about itself.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Properties {
    /// The PDF version, as `1.7` --- the catalog's `/Version` where it overrides
    /// the header, which is the only place a later version may be stated.
    pub version: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// How many pages, as PDFium counts them.
    pub pages: u32,
    /// How many revisions the file holds: one, plus one per incremental update.
    pub revisions: usize,
    /// The `/Info` dictionary, standard keys first.
    pub fields: Vec<Field>,
    /// The encryption, when there is any.
    pub encryption: Option<Encryption>,
    /// Every signature field, in the order the document lists them.
    pub signatures: Vec<Signature>,
    /// Whether the document carries a structure tree, so its reading order is
    /// its own rather than one this application inferred.
    ///
    /// `None` means the question could not be asked, which on a locked document
    /// is the true answer and `false` would not be.
    pub tagged: Option<bool>,
    /// `/Lang` from the catalog, when it states one.
    pub language: String,
    /// How many files are embedded in it, where that could be counted.
    pub attachments: Option<usize>,
    /// What could not be read.
    pub limits: Limits,
    /// Time spent scanning, in milliseconds.
    pub scan_ms: f64,
}

/// Reads what a document says about itself.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`]. A
/// failure is reported rather than answered with an empty readout, for the
/// reason [`crate::annots::scan`] gives: "this document states nothing" and
/// "this document could not be read" are different things to tell a reader.
pub fn scan(bytes: &[u8], page_count: u32) -> Result<Properties, String> {
    let started = std::time::Instant::now();
    let mut document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse the document: {e}"))?;

    let mut limits = Limits::default();

    // Read the encryption before decrypting, because `lopdf::decrypt` clears the
    // trailer entry it reports on --- so asking afterwards answers "none" for a
    // document that is plainly encrypted.
    let encryption = read_encryption(&document);

    // The strings are ciphertext until this succeeds. An empty user password is
    // the common case for a document locked against editing rather than reading,
    // and it is the only password anything here has.
    let readable = if encryption.is_some() {
        let opened = document.decrypt("").is_ok();
        limits.locked = !opened;
        opened
    } else {
        true
    };

    let encryption = encryption.map(|mut e| {
        e.opened_without_password = readable;
        e
    });

    let fields = if readable {
        read_fields(&document, &mut limits)
    } else {
        Vec::new()
    };
    let signatures = if readable {
        read_signatures(&document, bytes.len() as u64, &mut limits)
    } else {
        Vec::new()
    };

    let catalog = if readable {
        document.catalog().ok()
    } else {
        None
    };
    let language = catalog
        .and_then(|c| c.get(b"Lang").ok())
        .and_then(|o| resolve(&document, o).as_str().ok())
        .map(decode_text_string)
        .unwrap_or_default();

    Ok(Properties {
        version: version_of(&document, bytes),
        bytes: bytes.len() as u64,
        pages: page_count,
        revisions: revisions_in(bytes),
        fields,
        encryption,
        signatures,
        tagged: catalog.map(|c| c.has(b"StructTreeRoot")),
        language,
        attachments: catalog.map(|_| count_attachments(&document)),
        limits,
        scan_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

/// A `/Name` entry's value, or `""`.
///
/// Names are ASCII by construction in every key read here, so the lossy
/// conversion cannot lose anything --- but it is written once rather than at
/// each of the five call sites, which is what stops one of them differing.
fn name_of(document: &Document, dict: &Dictionary, key: &[u8]) -> String {
    dict.get(key)
        .ok()
        .and_then(|o| resolve(document, o).as_name().ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .unwrap_or_default()
}

/// The version the document conforms to.
///
/// The header states one and the catalog's `/Version` may state a later one ---
/// PDF 32000-1 §7.5.2, which exists so an incremental update can raise the
/// version without rewriting the first line. The later of the two wins, and
/// "later" is a string comparison only because every version is `1.n` with a
/// single digit; a document claiming `2.0` sorts correctly under it anyway.
fn version_of(document: &Document, bytes: &[u8]) -> String {
    let header = if document.version.is_empty() {
        // `lopdf` fills this from the header, so an empty one means it could not
        // read the line. Take it from the bytes rather than reporting nothing.
        bytes
            .get(..9)
            .and_then(|head| head.strip_prefix(b"%PDF-"))
            .map(|rest| String::from_utf8_lossy(rest).trim().to_string())
            .unwrap_or_default()
    } else {
        document.version.clone()
    };

    let catalog = document
        .catalog()
        .ok()
        .map(|c| name_of(document, c, b"Version"))
        .unwrap_or_default();

    if catalog > header {
        catalog
    } else {
        header
    }
}

/// How many revisions the file holds.
///
/// Counted as end-of-file markers, which real writers emit exactly once per
/// revision. It is a count of those five bytes and nothing stronger: a content
/// stream that happens to contain them inflates it. That is tolerable here
/// because nothing decides anything on this number --- the question it hints at,
/// *was this appended to after signing*, is answered properly and separately by
/// [`Signature::covers_whole_file`].
fn revisions_in(bytes: &[u8]) -> usize {
    bytes.windows(5).filter(|w| *w == b"%%EOF").count()
}

/// Reads the trailer's encryption dictionary.
///
/// **Must be called before [`Document::decrypt`]**, which removes the trailer
/// entry and the object it points at --- so the same call afterwards reports an
/// unencrypted document, which is the opposite of the truth.
fn read_encryption(document: &Document) -> Option<Encryption> {
    let entry = document.trailer.get(b"Encrypt").ok()?;
    let dict = resolve(document, entry).as_dict().ok()?;

    let number = |key: &[u8]| -> i64 {
        dict.get(key)
            .ok()
            .and_then(|o| resolve(document, o).as_i64().ok())
            .unwrap_or_default()
    };

    let v = number(b"V");
    let r = number(b"R");
    // §7.6.3.2: absent means 40, and it is stated in bits.
    let length = match number(b"Length") {
        0 => 40,
        other => other,
    };

    Some(Encryption {
        method: method_of(document, dict, v, length),
        revision: r,
        // Filled in by the caller, which is the only place that knows.
        opened_without_password: false,
        permissions: permissions(r, number(b"P")),
    })
}

/// A reader-facing name for the algorithm.
fn method_of(document: &Document, dict: &Dictionary, v: i64, length: i64) -> String {
    // §7.6.5: from V4 on, the algorithm is named by the crypt filter rather than
    // by `/V`, and `/Length` in the filter overrides the document's.
    let filter = dict
        .get(b"CF")
        .ok()
        .and_then(|o| resolve(document, o).as_dict().ok())
        .and_then(|cf| cf.get(b"StdCF").ok())
        .and_then(|o| resolve(document, o).as_dict().ok());
    let method = filter
        .map(|f| name_of(document, f, b"CFM"))
        .unwrap_or_default();

    match (v, method.as_str()) {
        (_, "AESV3") => "AES-256".to_string(),
        (_, "AESV2") => "AES-128".to_string(),
        (_, "None") => "none".to_string(),
        (5, _) => "AES-256".to_string(),
        (4, _) | (2, _) | (1, _) => format!("RC4 {length}-bit"),
        (other, _) => format!("unknown (/V {other})"),
    }
}

/// What the permission bits mean, which depends on the revision.
///
/// PDF 32000-1 Table 22 defines bits 9 to 12 only from revision 3 --- for
/// revision 2 they are reserved and set to 1, so reading them as permissions
/// would report every old document as permitting everything it actually
/// forbids. Under revision 2 each of those four questions is answered by the
/// coarser bit that does cover it, which is what every other reader does:
/// checked against `qpdf --show-encryption` on a real `R = 2, P = -60`
/// document, where all eight lines agree.
///
/// Bit numbers are as the specification writes them, so bit 3 is value 4.
fn permissions(revision: i64, p: i64) -> Vec<Permission> {
    let bit = |n: u32| -> bool { p & (1 << (n - 1)) != 0 };
    let old = revision < 3;

    let print = bit(3);
    let modify = bit(4);
    let extract = bit(5);
    let annotate = bit(6);

    let named = [
        ("Print", print),
        (
            "Print at high resolution",
            if old { print } else { bit(12) && print },
        ),
        ("Copy text and graphics", extract),
        (
            "Extract for accessibility",
            if old { extract } else { bit(10) },
        ),
        ("Change the content", modify),
        ("Add or change comments", annotate),
        ("Fill in form fields", if old { annotate } else { bit(9) }),
        ("Assemble pages", if old { modify } else { bit(11) }),
    ];

    named
        .into_iter()
        .map(|(what, allowed)| Permission {
            what: what.to_string(),
            allowed,
        })
        .collect()
}

/// Reads the `/Info` dictionary, standard keys first.
fn read_fields(document: &Document, limits: &mut Limits) -> Vec<Field> {
    let Some(dict) = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| resolve(document, o).as_dict().ok())
    else {
        return Vec::new();
    };

    let mut fields: Vec<Field> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    let take = |name: String, object: &Object, fields: &mut Vec<Field>, limits: &mut Limits| {
        let raw = match resolve(document, object) {
            Object::String(bytes, _) => decode_text_string(bytes),
            Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Object::Integer(n) => n.to_string(),
            Object::Real(n) => n.to_string(),
            Object::Boolean(b) => b.to_string(),
            // Anything else is not a value to show a reader. Reported rather
            // than skipped silently: an `/Info` entry that is an array is a
            // malformed document, and saying nothing looks like an absent key.
            _ => {
                limits.unreadable += 1;
                return;
            }
        };
        let standard = STANDARD.contains(&name.as_str());
        let value = if standard && (name == "CreationDate" || name == "ModDate") {
            format_date(&raw)
        } else {
            raw
        };
        if value.is_empty() {
            return;
        }
        fields.push(Field {
            name,
            value: clip(value, limits),
            standard,
        });
    };

    for key in STANDARD {
        if let Ok(object) = dict.get(key.as_bytes()) {
            seen.push(key.to_string());
            take(key.to_string(), object, &mut fields, limits);
        }
    }

    for (key, object) in dict.iter() {
        let name = String::from_utf8_lossy(key).into_owned();
        if seen.contains(&name) {
            continue;
        }
        if fields.len() >= MAX_FIELDS {
            limits.fields_dropped += 1;
            continue;
        }
        take(name, object, &mut fields, limits);
    }

    fields
}

/// Shortens a value that would otherwise fill the dialog, counting what it cut.
fn clip(value: String, limits: &mut Limits) -> String {
    if value.chars().count() <= MAX_VALUE_CHARS {
        return value;
    }
    limits.values_clipped += 1;
    value.chars().take(MAX_VALUE_CHARS).collect()
}

/// Reformats a PDF date into something a person reads.
///
/// The wire format is `D:YYYYMMDDHHmmSSOHH'mm'` (§7.9.4), where everything after
/// the year is optional and `O` is `+`, `-` or `Z`. Anything that does not parse
/// is returned **unchanged** rather than dropped: a date this cannot read is
/// still a date, and showing the raw string beats showing nothing.
fn format_date(raw: &str) -> String {
    let body = raw.strip_prefix("D:").unwrap_or(raw);
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return raw.to_string();
    }
    let at = |from: usize, len: usize| -> Option<&str> { digits.get(from..from + len) };

    let mut out = digits[..4].to_string();
    if let Some(month) = at(4, 2) {
        out.push('-');
        out.push_str(month);
    }
    if let Some(day) = at(6, 2) {
        out.push('-');
        out.push_str(day);
    }
    if let Some(hour) = at(8, 2) {
        out.push(' ');
        out.push_str(hour);
        out.push(':');
        out.push_str(at(10, 2).unwrap_or("00"));
        if let Some(second) = at(12, 2) {
            out.push(':');
            out.push_str(second);
        }
    }

    // The offset, which follows the digits. `Z` means UTC and may be written
    // bare or as `Z00'00'`; the apostrophes are noise to a reader either way.
    let zone = &body[digits.len()..];
    if zone.starts_with('Z') {
        out.push_str(" UTC");
    } else if let Some(rest) = zone.strip_prefix(['+', '-']) {
        let sign = &zone[..1];
        let hours: String = rest.chars().take(2).filter(char::is_ascii_digit).collect();
        let minutes: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .skip(2)
            .skip_while(|c| !c.is_ascii_digit())
            .take(2)
            .collect();
        if hours.len() == 2 {
            out.push(' ');
            out.push_str(sign);
            out.push_str(&hours);
            out.push(':');
            out.push_str(if minutes.len() == 2 { &minutes } else { "00" });
        }
    }

    out
}

/// Counts the files embedded in a document, at the two places they may hang.
///
/// `/Names /EmbeddedFiles` is the document-level list; a `/FileAttachment`
/// annotation is the per-page one, and a document may use either. Only the
/// first is counted here, which is what "how many attachments does this file
/// carry" means to a reader --- the annotation form already appears in the
/// comments panel, where it belongs.
fn count_attachments(document: &Document) -> usize {
    let Ok(catalog) = document.catalog() else {
        return 0;
    };
    let names = catalog
        .get(b"Names")
        .ok()
        .and_then(|o| resolve(document, o).as_dict().ok());
    let Some(tree) = names
        .and_then(|n| n.get(b"EmbeddedFiles").ok())
        .and_then(|o| resolve(document, o).as_dict().ok())
    else {
        return 0;
    };
    // A name tree's leaf holds `/Names [key value key value ...]`, so the count
    // is half the array. An interior node holds `/Kids`, which is not walked:
    // the tree only branches past several hundred entries, and reporting "some"
    // as zero would be worse than reporting the leaf's own count.
    tree.get(b"Names")
        .ok()
        .and_then(|o| resolve(document, o).as_array().ok())
        .map_or(0, |array| array.len() / 2)
}

/// Reads every signature field the form declares.
///
/// The walk is bounded in both directions a form can grow --- `/Fields` is a
/// tree, so it is hostile input of the same shape [`crate::outline`] bounds its
/// depth against, and a document can declare more fields than anybody would
/// read. Both bounds report through [`Limits`].
fn read_signatures(document: &Document, size: u64, limits: &mut Limits) -> Vec<Signature> {
    let Some(form) = document
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|o| resolve(document, o).as_dict().ok())
    else {
        return Vec::new();
    };
    let Some(fields) = form
        .get(b"Fields")
        .ok()
        .and_then(|o| resolve(document, o).as_array().ok())
    else {
        return Vec::new();
    };

    let mut out: Vec<Signature> = Vec::new();
    let mut queue: Vec<(&Object, u32)> = fields.iter().map(|f| (f, 0u32)).collect();
    // Reversed so the queue pops in document order, which is the order a reader
    // sees the fields in every other application.
    queue.reverse();

    while let Some((entry, depth)) = queue.pop() {
        if out.len() >= MAX_SIGNATURES {
            limits.signatures_dropped += 1;
            continue;
        }
        let Ok(field) = resolve(document, entry).as_dict() else {
            limits.unreadable += 1;
            continue;
        };

        // A node with kids is a group; a node with `/FT /Sig` is a field. Both
        // at once is legal and means a field that also has widget children.
        if let Ok(kids) = field
            .get(b"Kids")
            .ok()
            .map_or(Err(()), |o| resolve(document, o).as_array().map_err(|_| ()))
        {
            // Eight is past any real form's nesting and short of anything that
            // could cost time. A tree deeper than this is a document trying to.
            if depth < 8 {
                for kid in kids.iter().rev() {
                    queue.push((kid, depth + 1));
                }
            } else {
                limits.unreadable += 1;
            }
        }

        if name_of(document, field, b"FT") != "Sig" {
            continue;
        }

        out.push(read_signature(document, field, size, limits));
    }

    out
}

/// Reads one signature field.
fn read_signature(
    document: &Document,
    field: &Dictionary,
    size: u64,
    limits: &mut Limits,
) -> Signature {
    let text = |dict: &Dictionary, key: &[u8]| -> String {
        dict.get(key)
            .ok()
            .and_then(|o| resolve(document, o).as_str().ok())
            .map(decode_text_string)
            .unwrap_or_default()
    };
    let mut out = Signature {
        field: text(field, b"T"),
        ..Signature::default()
    };

    // No `/V` is a signature *field* nobody has signed --- a form waiting for
    // one. Reported rather than dropped: "this document has an unsigned
    // signature field" is a fact about it, and an empty list would say the
    // opposite.
    let Some(sig) = field
        .get(b"V")
        .ok()
        .and_then(|o| resolve(document, o).as_dict().ok())
    else {
        return out;
    };

    out.signed = true;
    out.handler = name_of(document, sig, b"Filter");
    out.kind = name_of(document, sig, b"SubFilter");
    out.name = text(sig, b"Name");
    out.reason = text(sig, b"Reason");
    out.location = text(sig, b"Location");
    out.when = format_date(&text(sig, b"M"));

    if let Some(range) = sig
        .get(b"ByteRange")
        .ok()
        .and_then(|o| resolve(document, o).as_array().ok())
    {
        let numbers: Vec<i64> = range
            .iter()
            .filter_map(|o| resolve(document, o).as_i64().ok())
            .collect();
        // Pairs of (offset, length). An odd count is malformed, and the last
        // half-pair is dropped rather than read as an offset with no length.
        out.covered_bytes = numbers
            .chunks_exact(2)
            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())
            .sum();
        out.covers_whole_file = numbers.chunks_exact(2).last().is_some_and(|last| {
            let end = last[0].saturating_add(last[1]);
            u64::try_from(end).is_ok_and(|end| end == size)
        }) && numbers.first() == Some(&0);
    }

    out.certification = certification_of(document, sig);
    out.certificate = read_certificate(document, sig, limits);
    out
}

/// Reads the signer's certificate out of the `/Contents` blob.
///
/// `/Contents` holds DER: a CMS `ContentInfo` wrapping a `SignedData`, whose
/// optional `certificates` set carries the signer's certificate and usually the
/// chain above it. The set is *unordered* --- taking its first element would
/// name a certificate authority as the signer about as often as not --- so the
/// signer is identified through `SignerInfo.sid`, which is either an issuer and
/// serial pair or a subject key identifier.
///
/// Returns `None` and leaves `limits` untouched when there is simply nothing to
/// read; increments [`Limits::certificates_unread`] when there was and it could
/// not be parsed.
fn read_certificate(
    document: &Document,
    sig: &Dictionary,
    limits: &mut Limits,
) -> Option<Certificate> {
    read_certificate_bounded(document, sig, limits, MAX_SIG_BLOB)
}

/// [`read_certificate`] with its bound named, so a test can make a *valid* blob
/// exceed it.
///
/// The bound cannot be tested with an oversized piece of garbage: refusing it
/// and parsing it and failing produce the same `None` and the same increment,
/// so such a test passes whether the guard is there or not --- which is what
/// the first version of it did, and a mutation deleting the guard survived it.
/// Handing a real signature's blob a bound of a hundred bytes is the only
/// arrangement where the two outcomes differ.
fn read_certificate_bounded(
    document: &Document,
    sig: &Dictionary,
    limits: &mut Limits,
    bound: usize,
) -> Option<Certificate> {
    let raw = sig
        .get(b"Contents")
        .ok()
        .and_then(|o| resolve(document, o).as_str().ok())?;

    // A signature is written by reserving a fixed span and filling it, so the
    // blob is right-padded with zeros to whatever the writer reserved. Those
    // are not part of the DER and a decoder is entitled to reject them.
    // All zeros is a reserved-but-unwritten placeholder, not a failure --- so
    // this leaves `limits` alone, and a mutation that increments here is caught
    // by `an_untouched_placeholder_is_absent_rather_than_unread`.
    let last = raw.iter().rposition(|b| *b != 0)?;
    let der_bytes = &raw[..=last];

    if der_bytes.len() > bound {
        limits.certificates_unread += 1;
        return None;
    }

    match parse_certificate(der_bytes) {
        Some(certificate) => Some(certificate),
        None => {
            limits.certificates_unread += 1;
            None
        }
    }
}

/// The DER half of [`read_certificate`], split out so a test can hand it bytes.
///
/// Public for `signature-probe`, which parses the blob **PDFium** handed it and
/// compares the result against what this module produced from `lopdf`'s. Two
/// readers reaching the same certificate is a statement neither module's own
/// tests can make, and the failure it guards against --- picking a different
/// signature's blob, and so showing the wrong signer --- is the worst one here.
pub fn parse_certificate(der_bytes: &[u8]) -> Option<Certificate> {
    use cms::cert::CertificateChoices;
    use cms::content_info::ContentInfo;
    use cms::signed_data::{SignedData, SignerIdentifier};
    use der::{Decode, Encode};

    let info = ContentInfo::from_der(der_bytes).ok()?;
    let signed: SignedData = info.content.decode_as().ok()?;
    let set = signed.certificates.as_ref()?;

    let certificates: Vec<&x509_cert::Certificate> = set
        .0
        .iter()
        .filter_map(|choice| match choice {
            CertificateChoices::Certificate(certificate) => Some(certificate),
            CertificateChoices::Other(_) => None,
        })
        .collect();
    let chain = u32::try_from(certificates.len()).unwrap_or(u32::MAX);
    if certificates.is_empty() {
        return None;
    }

    let wanted = signed
        .signer_infos
        .0
        .as_slice()
        .first()
        .map(|info| &info.sid);
    let matched = wanted.and_then(|sid| {
        certificates.iter().copied().find(|certificate| match sid {
            SignerIdentifier::IssuerAndSerialNumber(both) => {
                certificate.tbs_certificate.serial_number == both.serial_number
                    && certificate.tbs_certificate.issuer.to_der().ok() == both.issuer.to_der().ok()
            }
            SignerIdentifier::SubjectKeyIdentifier(key) => {
                subject_key_identifier(certificate).is_some_and(|ski| ski == key.0.as_bytes())
            }
        })
    });

    // A set of one leaves nothing to choose between, so an identifier that does
    // not match it is a disagreement about naming rather than an ambiguity ---
    // report the certificate and say the match failed. Several with no match is
    // a genuine ambiguity and reports nothing.
    let (certificate, matched_signer) = match (matched, certificates.as_slice()) {
        (Some(certificate), _) => (certificate, true),
        (None, [only]) => (*only, false),
        (None, _) => return None,
    };

    let tbs = &certificate.tbs_certificate;
    let subject = distinguished_name(&tbs.subject);
    let issuer = distinguished_name(&tbs.issuer);
    Some(Certificate {
        subject_cn: common_name(&tbs.subject),
        issuer_cn: common_name(&tbs.issuer),
        self_issued: tbs.subject.to_der().ok() == tbs.issuer.to_der().ok(),
        subject,
        issuer,
        serial: hex_of(tbs.serial_number.as_bytes()),
        from: certificate_date(&tbs.validity.not_before),
        until: certificate_date(&tbs.validity.not_after),
        chain,
        matched_signer,
    })
}

/// The subject key identifier extension's octets, when the certificate has one.
fn subject_key_identifier(certificate: &x509_cert::Certificate) -> Option<Vec<u8>> {
    use der::{asn1::OctetString, Decode};

    certificate
        .tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        // 2.5.29.14, the subject key identifier, written out rather than pulled
        // from a constant database so the dependency stays to what is parsed.
        .find(|extension| extension.extn_id.to_string() == "2.5.29.14")
        .and_then(|extension| OctetString::from_der(extension.extn_value.as_bytes()).ok())
        .map(|octets| octets.as_bytes().to_vec())
}

/// A distinguished name, near enough RFC 4514: `CN=Someone, O=Something`.
///
/// Written out rather than taken from `RdnSequence`'s own `Display`, because
/// that one escapes for round-tripping and this string is read by a person.
fn distinguished_name(name: &x509_cert::name::Name) -> String {
    let mut parts: Vec<String> = Vec::new();
    for rdn in name.0.iter() {
        for attribute in rdn.0.as_slice() {
            let value = attribute_text(attribute);
            if value.is_empty() {
                continue;
            }
            parts.push(format!(
                "{}={}",
                short_oid(&attribute.oid.to_string()),
                value
            ));
        }
    }
    parts.join(", ")
}

/// The common name alone, which is what a person reads as "who signed this".
fn common_name(name: &x509_cert::name::Name) -> String {
    for rdn in name.0.iter() {
        for attribute in rdn.0.as_slice() {
            if attribute.oid.to_string() == "2.5.4.3" {
                return attribute_text(attribute);
            }
        }
    }
    String::new()
}

/// The short label for the attribute types a certificate actually uses.
///
/// Anything else keeps its numeric form, which is honest --- a made-up
/// abbreviation would read as a standard one.
fn short_oid(oid: &str) -> &str {
    match oid {
        "2.5.4.3" => "CN",
        "2.5.4.6" => "C",
        "2.5.4.7" => "L",
        "2.5.4.8" => "ST",
        "2.5.4.10" => "O",
        "2.5.4.11" => "OU",
        "2.5.4.5" => "SERIALNUMBER",
        "1.2.840.113549.1.9.1" => "E",
        other => other,
    }
}

/// One name attribute's text.
///
/// A directory string is one of five ASN.1 string types and the encoding is the
/// issuer's choice. `BMPString` is UTF-16BE and is what Windows certificate
/// authorities emit, so it is decoded rather than shown as interleaved nulls;
/// the rest carry their text as bytes. Anything undecodable comes back lossily
/// rather than empty, because a mangled name still tells a reader who it is not.
fn attribute_text(attribute: &x509_cert::attr::AttributeTypeAndValue) -> String {
    use der::Tagged as _;

    let bytes = attribute.value.value();
    // Tag 0x1E is BMPString.
    let text = if attribute.value.tag().number().value() == 0x1e {
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&wide)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    clip_text(text.trim())
}

/// `notBefore` / `notAfter`, in the shape [`format_date`] produces.
fn certificate_date(time: &x509_cert::time::Time) -> String {
    let at = time.to_date_time();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minutes(),
        at.seconds()
    )
}

/// Uppercase hex, which is how every other tool prints a serial.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes.iter().take(MAX_VALUE_CHARS / 2) {
        let _ = write!(out, "{byte:02X}");
    }
    out
}

/// Bounds one string, on the same rule the `/Info` values use.
fn clip_text(text: &str) -> String {
    if text.chars().count() <= MAX_VALUE_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_VALUE_CHARS).collect()
}

/// The DocMDP level a signature certifies at, or zero for an ordinary one.
///
/// `/Reference` is an array of transform dictionaries and only the one whose
/// `/TransformMethod` is `/DocMDP` says anything about permitted changes ---
/// `/FieldMDP` and `/UR` are the other two and mean different things, so taking
/// the first entry's `/P` would report a field-locking signature as a
/// certification of the whole document.
fn certification_of(document: &Document, sig: &Dictionary) -> u8 {
    let Some(references) = sig
        .get(b"Reference")
        .ok()
        .and_then(|o| resolve(document, o).as_array().ok())
    else {
        return 0;
    };

    for entry in references {
        let Ok(reference) = resolve(document, entry).as_dict() else {
            continue;
        };
        if name_of(document, reference, b"TransformMethod") != "DocMDP" {
            continue;
        }
        let level = reference
            .get(b"TransformParams")
            .ok()
            .and_then(|o| resolve(document, o).as_dict().ok())
            .and_then(|params| params.get(b"P").ok())
            .and_then(|o| resolve(document, o).as_i64().ok())
            .unwrap_or(0);
        // §12.8.2.2 defines 1, 2 and 3. Anything else is a document saying
        // something the specification does not define, and is not repeated.
        if (1..=3).contains(&level) {
            return level as u8;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    /// Builds a one-page document, letting the caller add to its catalog.
    ///
    /// Synthetic rather than a fixture on disk, and the reason is what this
    /// module reads: every field here is a **structural** one --- a name, an
    /// integer, an array of offsets --- so a document built to have that
    /// structure is the honest subject, not a stand-in for one. Nothing here
    /// renders, decrypts or verifies anything, so there is no property a real
    /// file would have that a built one lacks.
    ///
    /// The exception is stated where it applies: a real signature's `/Contents`
    /// is a PKCS#7 blob, and nothing in this module looks inside it.
    fn document(extra: Dictionary) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let mut catalog = dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        };
        for (key, value) in extra.iter() {
            catalog.set(key.clone(), value.clone());
        }
        let catalog_id = document.add_object(catalog);
        document.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        bytes
    }

    /// A document carrying one signature field with the entries given.
    fn document_signed(signature: Dictionary) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let sig_id = document.add_object(signature);
        let field_id = document.add_object(dictionary! {
            "FT" => "Sig",
            "T" => Object::string_literal("Signature1"),
            "V" => sig_id,
        });
        let form_id = document.add_object(dictionary! {
            "SigFlags" => 3,
            "Fields" => vec![field_id.into()],
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "AcroForm" => form_id,
        });
        document.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        bytes
    }

    /// Reads a synthetic document, which must parse.
    fn read(bytes: &[u8]) -> Properties {
        scan(bytes, 1).expect("the fixture must parse")
    }

    // ------------------------------------------------- against another writer

    /// Reads the certification level off four documents **pyhanko** signed.
    ///
    /// Every other test here builds its subject with `lopdf` and reads it with
    /// `lopdf`, which is the writer-and-its-own-reader shape `docs/TRAPS.md`
    /// warns about: a fixture that agrees with the code that made it can agree
    /// about something wrong. These four were written by a different program, in
    /// a different language, for a different purpose --- spike 0.6 built them to
    /// measure what a *validator* does with an appended edit --- and they carry
    /// real cryptographic signatures at the three DocMDP levels.
    ///
    /// So this is the one test here whose passing says the reading is right
    /// rather than merely self-consistent.
    ///
    /// The numbers come from `qpdf --json` on each file, which is a third
    /// independent reader.
    #[test]
    fn the_docmdp_levels_of_four_documents_another_program_signed() {
        let cases = [
            ("incr-signed.pdf", 0u8, 8128u64, 3054 + 668u64),
            ("incr-certified-1.pdf", 1, 8275, 3082 + 787),
            ("incr-certified-2.pdf", 2, 8275, 3082 + 787),
            ("incr-certified-3.pdf", 3, 8275, 3082 + 787),
            ("incr-certified-3-indirect.pdf", 3, 8214, 3016 + 792),
        ];

        let mut examined = 0;
        for (name, level, size, covered) in cases {
            let path = std::path::Path::new("../testdata").join(name);
            let Ok(bytes) = std::fs::read(&path) else {
                println!("[SKIP] {name}: not generated");
                continue;
            };
            assert_eq!(bytes.len() as u64, size, "{name} is not the file measured");

            let properties = scan(&bytes, 1).expect("a signed fixture must parse");
            assert_eq!(properties.signatures.len(), 1, "{name} has one signature");
            let signature = &properties.signatures[0];

            assert!(signature.signed, "{name} is signed");
            assert_eq!(signature.field, "Signature1", "{name}");
            assert_eq!(signature.handler, "Adobe.PPKLite", "{name}");
            assert_eq!(signature.kind, "adbe.pkcs7.detached", "{name}");
            assert_eq!(signature.certification, level, "{name} is DocMDP {level}");
            assert_eq!(signature.covered_bytes, covered, "{name}");
            assert!(
                signature.covers_whole_file,
                "{name}: the range ends at the file's last byte"
            );
            // Every one of them is an incremental update over a base document,
            // which is what makes them a signed-then-appended shape without
            // being a *tampered* one --- the append is the signature itself.
            assert!(properties.revisions >= 2, "{name} is an incremental save");
            examined += 1;
        }

        // Five SKIP lines and a pass look identical, and this is the check that
        // separates them --- see the same guard in `crate::print`.
        assert!(
            examined > 0,
            "no fixture was examined --- generate testdata/ (BUILD.md, Test fixtures)"
        );
    }

    /// A document appended to after signing is caught by the coverage check.
    ///
    /// The failure the whole `covers_whole_file` field exists for, and it is
    /// built by doing the thing rather than by describing it: bytes are appended
    /// to a real signed document, and the signature's own byte range then stops
    /// short of the end. No cryptography is involved on either side --- which is
    /// the point, since this is what tpdf can check without any.
    #[test]
    fn appending_to_a_signed_document_stops_the_signature_covering_it() {
        let path = std::path::Path::new("../testdata/incr-certified-1.pdf");
        let Ok(bytes) = std::fs::read(path) else {
            println!("[SKIP] incr-certified-1.pdf: not generated");
            return;
        };

        // The control first: untouched, it covers the file.
        let before = scan(&bytes, 1).expect("must parse");
        assert!(before.signatures[0].covers_whole_file);

        let mut tampered = bytes.clone();
        tampered.extend_from_slice(b"\n% and one more line\n");
        let after = scan(&tampered, 1).expect("must still parse");
        assert!(
            !after.signatures[0].covers_whole_file,
            "22 bytes now lie outside the signed range"
        );
        assert_eq!(
            after.signatures[0].covered_bytes, before.signatures[0].covered_bytes,
            "what the signature covers has not changed --- the file has"
        );
    }

    /// A document that needs a password reports that, and claims nothing else.
    ///
    /// The rule the module is built on, and the one a partial implementation
    /// gets wrong in the reassuring direction: the strings are unreadable, and
    /// an empty `/Info` list beside `tagged: Some(false)` would be four false
    /// statements about a document nothing could look inside.
    #[test]
    fn a_document_that_needs_a_password_says_so_rather_than_reporting_nothing() {
        let path = std::path::Path::new("../testdata/incr-encrypted-pw.pdf");
        let Ok(bytes) = std::fs::read(path) else {
            println!("[SKIP] incr-encrypted-pw.pdf: not generated");
            return;
        };

        let properties = scan(&bytes, 1).expect("the structure must still parse");
        assert!(
            properties.limits.locked,
            "an empty user password does not open it"
        );
        assert!(properties.limits.any(), "so the reader is told");

        // Nothing that needed the object graph may claim anything.
        assert!(properties.fields.is_empty());
        assert!(properties.signatures.is_empty());
        assert_eq!(properties.tagged, None, "not Some(false)");
        assert_eq!(properties.attachments, None, "not Some(0)");
        assert!(properties.language.is_empty());

        // What is readable without the password still is, which is the other
        // half of the claim: the security summary is always available.
        let security = properties
            .encryption
            .expect("the trailer states the encryption in plaintext");
        assert!(!security.opened_without_password);
        assert_eq!(security.permissions.len(), 8);
        assert!(properties.bytes > 0);
        assert!(!properties.version.is_empty());
    }

    // ------------------------------------------------------------ permissions

    /// Revision 2 leaves bits 9 to 12 reserved, so reading them reports
    /// permissions the document forbids.
    ///
    /// The numbers are not invented. They are `qpdf --show-encryption` on a real
    /// `R = 2, P = -60` document --- a supplier's compliance certificate --- and
    /// all eight lines here agree with all eight of its lines. That is what
    /// makes this a cross-check rather than this module agreeing with itself:
    /// qpdf implements the same table from the same specification and shares no
    /// code with any of this.
    #[test]
    fn revision_2_reads_the_reserved_bits_as_the_coarser_ones_they_stand_for() {
        let rows = permissions(2, -60);
        let allowed = |what: &str| {
            rows.iter()
                .find(|row| row.what == what)
                .unwrap_or_else(|| panic!("no permission named {what}"))
                .allowed
        };

        assert!(allowed("Print"), "bit 3 is set");
        // Bit 12 is reserved under revision 2, so a naive read makes this false
        // while qpdf says "print high resolution: allowed".
        assert!(allowed("Print at high resolution"));
        assert!(!allowed("Copy text and graphics"), "bit 5 is clear");
        // Bit 10 is reserved, and qpdf says "extract for accessibility: not
        // allowed" --- reading the reserved bit would report it as allowed.
        assert!(!allowed("Extract for accessibility"));
        assert!(!allowed("Change the content"), "bit 4 is clear");
        assert!(!allowed("Add or change comments"), "bit 6 is clear");
        assert!(!allowed("Fill in form fields"));
        assert!(!allowed("Assemble pages"));
    }

    /// From revision 3 the four fine-grained bits mean what they say.
    ///
    /// **The control for the test above, and it is the same `P`.** `-60` is a
    /// negative number, so every bit above 8 is set in it --- 9, 10, 11 and 12
    /// all read as "allowed" to anything that looks at them. That is precisely
    /// why revision 2 must not: qpdf reports accessibility extraction as *not
    /// allowed* on that document, and an implementation ignoring the revision
    /// says the opposite. So the two tests disagree on the same input, which is
    /// what makes either of them evidence.
    ///
    /// The first draft of this test wrote `-60 | (1 << 9)` to "set" bit 10 and
    /// asserted the revision-3 answer was *false*. Both halves were wrong and
    /// the second half is what failed: the bit was already set, so the `|` was a
    /// no-op and the assertion contradicted the code, correctly.
    #[test]
    fn revision_3_reads_the_fine_grained_bits() {
        let row = |rows: &[Permission], what: &str| {
            rows.iter().find(|r| r.what == what).expect("named").allowed
        };

        // The same `P` the test above uses, read under revision 3.
        let modern = permissions(3, -60);
        assert!(
            row(&modern, "Extract for accessibility"),
            "bit 10 is set in -60, and from revision 3 it means what it says"
        );
        assert!(
            !row(&permissions(2, -60), "Extract for accessibility"),
            "under revision 2 the same bit is reserved, and bit 5 decides"
        );

        // And a `P` that sets only what it names, so each answer is traceable to
        // one bit rather than to the sign of the number.
        let printing_only = permissions(3, 4);
        assert!(row(&printing_only, "Print"));
        assert!(
            !row(&printing_only, "Print at high resolution"),
            "bit 12 is clear"
        );
        assert!(
            !row(&printing_only, "Fill in form fields"),
            "bit 9 is clear"
        );
        assert!(!row(&printing_only, "Assemble pages"), "bit 11 is clear");

        // High-resolution printing needs both bits, which is the one permission
        // that is a conjunction: bit 12 alone permits nothing to be printed.
        let high_without_print = permissions(3, 1 << 11);
        assert!(!row(&high_without_print, "Print"));
        assert!(!row(&high_without_print, "Print at high resolution"));
    }

    // ------------------------------------------------------------------ dates

    /// The two dates off the real document, and the two shapes of offset.
    #[test]
    fn a_pdf_date_is_reformatted_into_something_a_person_reads() {
        assert_eq!(
            format_date("D:20260525160353+08'00'"),
            "2026-05-25 16:03:53 +08:00"
        );
        assert_eq!(format_date("D:20260525080507Z"), "2026-05-25 08:05:07 UTC");
    }

    /// Every optional tail is optional, down to the year alone.
    #[test]
    fn a_truncated_date_is_reported_as_far_as_it_goes() {
        assert_eq!(format_date("D:2026"), "2026");
        assert_eq!(format_date("D:202605"), "2026-05");
        assert_eq!(format_date("D:2026052516"), "2026-05-25 16:00");
    }

    /// A date this cannot read is shown as written rather than dropped.
    ///
    /// The alternative is worse than it looks: an empty Created row reads as a
    /// document that states no creation date, which is a different claim.
    #[test]
    fn a_date_that_does_not_parse_is_returned_unchanged() {
        assert_eq!(format_date("yesterday"), "yesterday");
        assert_eq!(format_date("D:20"), "D:20");
    }

    // -------------------------------------------------------------- structure

    /// A catalog `/Version` overrides the header, which is what it is for.
    #[test]
    fn the_later_of_the_header_and_the_catalog_version_wins() {
        let plain = read(&document(Dictionary::new()));
        assert_eq!(plain.version, "1.7");

        let raised = read(&document(dictionary! { "Version" => "2.0" }));
        assert_eq!(raised.version, "2.0");

        // And a catalog stating an *earlier* version does not lower it, which is
        // the direction a naive "the catalog wins" would get wrong.
        let lowered = read(&document(dictionary! { "Version" => "1.4" }));
        assert_eq!(lowered.version, "1.7");
    }

    /// A structure tree is reported as present, and its absence as absence.
    #[test]
    fn a_tagged_document_says_so_and_an_untagged_one_says_that() {
        let untagged = read(&document(Dictionary::new()));
        assert_eq!(untagged.tagged, Some(false));

        let tagged = read(&document(dictionary! {
            "StructTreeRoot" => dictionary! { "Type" => "StructTreeRoot" },
        }));
        assert_eq!(tagged.tagged, Some(true));
    }

    /// Embedded files are counted from the name tree's pairs, not its entries.
    #[test]
    fn attachments_are_counted_as_pairs_because_a_name_tree_interleaves_them() {
        let none = read(&document(Dictionary::new()));
        assert_eq!(none.attachments, Some(0));

        let two = read(&document(dictionary! {
            "Names" => dictionary! {
                "EmbeddedFiles" => dictionary! {
                    "Names" => vec![
                        Object::string_literal("a.txt"),
                        Object::Dictionary(dictionary! { "Type" => "Filespec" }),
                        Object::string_literal("b.txt"),
                        Object::Dictionary(dictionary! { "Type" => "Filespec" }),
                    ],
                },
            },
        }));
        // Four array entries, two files. Counting entries would say four.
        assert_eq!(two.attachments, Some(2));
    }

    // ----------------------------------------------------------------- fields

    /// The standard keys come out in the specification's order, custom after.
    #[test]
    fn info_entries_are_ordered_for_a_reader_rather_than_as_the_file_lays_them_out() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        // Deliberately laid out worst-first, which is how the real document
        // this was built against reads: Producer before anything descriptive.
        let info = document.add_object(dictionary! {
            "Producer" => Object::string_literal("Aspose.PDF for .NET 19.1"),
            "Company" => Object::string_literal("ACME Ltd"),
            "Title" => Object::string_literal("A certificate"),
            "CreationDate" => Object::string_literal("D:20260525160353+08'00'"),
        });
        document.trailer.set("Info", info);

        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        let properties = read(&bytes);

        let names: Vec<&str> = properties.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Title", "Producer", "CreationDate", "Company"]);
        assert!(
            !properties.fields[3].standard,
            "Company is the document's own"
        );
        // The date arrives reformatted, which is the one value this rewrites.
        assert_eq!(properties.fields[2].value, "2026-05-25 16:03:53 +08:00");
    }

    // ------------------------------------------------------------- signatures

    /// The one checked fact: a byte range that ends at the file's last byte.
    ///
    /// The numbers are the real document's --- `[0, 705728, 889940, 4340]` over
    /// 894,280 bytes, where the gap is exactly the 184,212-byte `/Contents` hex
    /// string a detached signature leaves. Scaled here only in that the fixture
    /// is smaller; the arithmetic under test is the same.
    #[test]
    fn a_signature_covering_the_file_says_so_and_one_that_does_not_says_that() {
        let bytes = document_signed(dictionary! {
            "Type" => "Sig",
            "Filter" => "Adobe.PPKLite",
            "SubFilter" => "adbe.pkcs7.detached",
            "ByteRange" => vec![0.into(), 100.into(), 300.into(), 40.into()],
            "Contents" => Object::string_literal("junk"),
        });
        // Read against a size the range does reach, and then against one it does
        // not --- the same document, so the only thing that moved is the file
        // length the range is compared with.
        let covering = read_signatures(&parse(&bytes), 340, &mut Limits::default());
        assert!(covering[0].covers_whole_file);
        assert_eq!(covering[0].covered_bytes, 140);

        let short = read_signatures(&parse(&bytes), 900, &mut Limits::default());
        assert!(!short[0].covers_whole_file, "560 bytes lie past the range");
        assert_eq!(
            short[0].covered_bytes, 140,
            "what it covers has not changed"
        );
    }

    /// A range that does not start at zero leaves the file's head unsigned.
    ///
    /// Reaching the last byte is not sufficient on its own, and a check written
    /// only on the end offset passes here --- which is why both ends are read.
    #[test]
    fn a_range_that_skips_the_start_of_the_file_is_not_whole_coverage() {
        let bytes = document_signed(dictionary! {
            "Type" => "Sig",
            "ByteRange" => vec![8.into(), 92.into(), 300.into(), 40.into()],
        });
        let read = read_signatures(&parse(&bytes), 340, &mut Limits::default());
        assert!(
            !read[0].covers_whole_file,
            "the first eight bytes are outside it"
        );
    }

    /// No byte range at all is not zero coverage, and must not read as it.
    #[test]
    fn a_signature_with_no_byte_range_reports_nothing_covered() {
        let bytes = document_signed(dictionary! { "Type" => "Sig" });
        let read = read_signatures(&parse(&bytes), 340, &mut Limits::default());
        assert_eq!(read[0].covered_bytes, 0);
        assert!(!read[0].covers_whole_file);
    }

    /// An odd number of offsets is malformed, and the half pair is dropped.
    #[test]
    fn a_malformed_byte_range_drops_the_offset_with_no_length() {
        let bytes = document_signed(dictionary! {
            "Type" => "Sig",
            "ByteRange" => vec![0.into(), 100.into(), 300.into()],
        });
        let read = read_signatures(&parse(&bytes), 340, &mut Limits::default());
        // 300 is an offset, never a length. Summing every number would give 400.
        assert_eq!(read[0].covered_bytes, 100);
    }

    /// The DocMDP level comes from the entry that is a DocMDP.
    ///
    /// `/Reference` may hold `/FieldMDP` and `/UR` transforms too, and their
    /// `/P` means something else entirely --- so taking the first entry's would
    /// report a signature that locks one form field as certifying the document.
    /// The fixture puts a `/FieldMDP` first, which is the arrangement that makes
    /// the wrong implementation give the wrong answer.
    #[test]
    fn the_certification_level_comes_from_the_docmdp_reference_and_no_other() {
        let bytes = document_signed(dictionary! {
            "Type" => "Sig",
            "Reference" => vec![
                Object::Dictionary(dictionary! {
                    "Type" => "SigRef",
                    "TransformMethod" => "FieldMDP",
                    "TransformParams" => dictionary! { "P" => 3 },
                }),
                Object::Dictionary(dictionary! {
                    "Type" => "SigRef",
                    "TransformMethod" => "DocMDP",
                    "TransformParams" => dictionary! { "P" => 1 },
                }),
            ],
        });
        let read = read_signatures(&parse(&bytes), 340, &mut Limits::default());
        assert_eq!(
            read[0].certification, 1,
            "the DocMDP entry, not the first one"
        );
    }

    /// A level outside 1 to 3 is not repeated back.
    #[test]
    fn a_certification_level_the_specification_does_not_define_is_not_reported() {
        for level in [0, 4, 99] {
            let bytes = document_signed(dictionary! {
                "Type" => "Sig",
                "Reference" => vec![Object::Dictionary(dictionary! {
                    "TransformMethod" => "DocMDP",
                    "TransformParams" => dictionary! { "P" => level },
                })],
            });
            let read = read_signatures(&parse(&bytes), 340, &mut Limits::default());
            assert_eq!(
                read[0].certification, 0,
                "level {level} is not one of the three"
            );
        }
    }

    /// A field with no `/V` is a form waiting for a signature, not an absence.
    #[test]
    fn an_unsigned_signature_field_is_listed_as_a_field() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let field = document.add_object(dictionary! {
            "FT" => "Sig",
            "T" => Object::string_literal("Waiting"),
        });
        let form = document.add_object(dictionary! { "Fields" => vec![field.into()] });
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "AcroForm" => form,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");

        let properties = read(&bytes);
        assert_eq!(properties.signatures.len(), 1);
        assert!(!properties.signatures[0].signed);
        assert_eq!(properties.signatures[0].field, "Waiting");
    }

    /// A form with no signature field at all yields no signatures.
    ///
    /// The control for every test above: a text field must not be read as an
    /// unsigned signature, which is what a walk that skipped the `/FT` check
    /// would do.
    #[test]
    fn a_form_of_ordinary_fields_yields_no_signatures() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let field = document.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("Name"),
        });
        let form = document.add_object(dictionary! { "Fields" => vec![field.into()] });
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "AcroForm" => form,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");

        assert!(read(&bytes).signatures.is_empty());
    }

    /// Claimed strings arrive as the document wrote them.
    #[test]
    fn what_the_signer_says_is_carried_through_unchanged() {
        let bytes = document_signed(dictionary! {
            "Type" => "Sig",
            "Filter" => "Adobe.PPKLite",
            "SubFilter" => "adbe.pkcs7.detached",
            "Name" => Object::string_literal("A. Signer"),
            "Reason" => Object::string_literal("SGS officially issued document"),
            "Location" => Object::string_literal("EUW"),
            "M" => Object::string_literal("D:20260525080507Z"),
        });
        let signature = &read(&bytes).signatures[0];
        assert_eq!(signature.name, "A. Signer");
        assert_eq!(signature.reason, "SGS officially issued document");
        assert_eq!(signature.location, "EUW");
        assert_eq!(signature.when, "2026-05-25 08:05:07 UTC");
        assert_eq!(signature.handler, "Adobe.PPKLite");
        assert_eq!(signature.kind, "adbe.pkcs7.detached");
    }

    /// Nothing this reports can carry a verdict, and the type is what says so.
    ///
    /// The frontend has a test over rendered words; this is the other half, and
    /// it is the stronger one --- adding a `valid: bool` here would be a
    /// compile error rather than a red test, because every field is matched.
    #[test]
    fn no_signature_field_may_carry_a_verdict() {
        let signature = Signature::default();
        let Signature {
            field: _,
            signed: _,
            handler: _,
            kind: _,
            name: _,
            reason: _,
            location: _,
            when: _,
            covers_whole_file: _,
            covered_bytes: _,
            certification: _,
            // A struct of its own, with a guard of its own ---
            // `no_certificate_field_may_carry_a_verdict`.
            certificate: _,
        } = signature;
    }

    // ---------------------------------------------------------------- helpers

    /// Parses fixture bytes the way [`scan`] does.
    fn parse(bytes: &[u8]) -> Document {
        Document::load_mem_with_options(
            bytes,
            LoadOptions {
                max_decompressed_size: Some(MAX_DECODE),
                ..Default::default()
            },
        )
        .expect("the fixture must parse")
    }

    /// A value too long to show is shortened, and the shortening is reported.
    #[test]
    fn a_value_that_would_fill_the_dialog_is_clipped_and_counted() {
        let mut limits = Limits::default();
        let long = "x".repeat(MAX_VALUE_CHARS + 10);
        assert_eq!(clip(long, &mut limits).chars().count(), MAX_VALUE_CHARS);
        assert_eq!(limits.values_clipped, 1);

        // The control: a value at the bound is not clipped and is not counted.
        let mut untouched = Limits::default();
        let exact = "y".repeat(MAX_VALUE_CHARS);
        assert_eq!(clip(exact, &mut untouched).chars().count(), MAX_VALUE_CHARS);
        assert_eq!(untouched.values_clipped, 0);
    }

    /// The whole point of reading the blob: a document can carry a certificate
    /// naming its signer while the `/Name` a reader is shown is empty.
    ///
    /// `incr-signed.pdf` is exactly that shape and it is not contrived --- it is
    /// what pyhanko writes when nobody passes a name, which is the default.
    /// Before this, tpdf showed an empty "Signer says" for a document that says
    /// who signed it in the one place that is cryptographically bound.
    #[test]
    fn a_signature_with_no_typed_name_still_names_its_signer() {
        let Ok(bytes) = std::fs::read("../testdata/incr-signed.pdf") else {
            println!("[SKIP] incr-signed.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 1).expect("a signed fixture must parse");
        let signature = &properties.signatures[0];

        assert_eq!(signature.name, "", "the fixture types no /Name");
        let certificate = signature
            .certificate
            .as_ref()
            .expect("and yet it carries a certificate");
        assert_eq!(certificate.subject_cn, "tpdf spike 0.6 test signer");
    }

    /// Every field of a certificate whose bytes this test chose itself.
    ///
    /// **No value here comes from a fixture, and that is the fix rather than a
    /// convenience.** The first version of this test pinned the serial and the
    /// validity dates of `incr-signed.pdf` --- read out of a file that
    /// `make_incremental_pdf.py` writes with `x509.random_serial_number()` and
    /// `datetime.now()`, so every regeneration changes them. It was green in
    /// both places it runs: locally against months-old fixtures, and on a runner
    /// because CI cannot build the signed fixtures at all and the test skipped.
    /// Regenerating turned it red, which is what a test asserting a random
    /// number was always going to do. See `docs/TRAPS.md`.
    ///
    /// The synthetic blob is deterministic, so a value can be pinned honestly ---
    /// and it is the only arrangement in which a *reversed* serial or a validity
    /// read from one end is visible at all, because both readers in the
    /// differential run this same parser.
    #[test]
    fn every_field_of_a_certificate_whose_bytes_the_test_chose() {
        // Three bytes, all different, so a reversal is not a palindrome.
        let blob = cms_blob(
            "A. Signer",
            "Test Root CA",
            &[0x01, 0x02, 0x03],
            &[0x01, 0x02, 0x03],
        );
        let certificate = parse_certificate(&blob).expect("a parseable blob");

        assert_eq!(certificate.subject, "CN=A. Signer");
        assert_eq!(certificate.subject_cn, "A. Signer");
        assert_eq!(certificate.issuer, "CN=Test Root CA");
        assert_eq!(certificate.issuer_cn, "Test Root CA");
        assert_eq!(
            certificate.serial, "010203",
            "most significant byte first; reversed would read 030201"
        );
        assert_eq!(certificate.from, "2026-01-01 00:00:00 UTC");
        assert_eq!(
            certificate.until, "2030-01-01 00:00:00 UTC",
            "the other end of the validity, which must not be the same one"
        );
        assert!(!certificate.self_issued);
        assert_eq!(certificate.chain, 1);
        assert!(certificate.matched_signer);
    }

    /// All five signed fixtures, so a certificate is read from every one of them
    /// rather than from the one that happened to be written first.
    ///
    /// **What is asserted is deliberately not a value.** These fixtures are
    /// generated and not committed, with a fresh random serial and a `not_before`
    /// of *now* on every run, so pinning either produces a test that fails the
    /// next time somebody follows `BUILD.md`. What is stable is the name the
    /// generator hardcodes, the shape of a serial, and --- the discriminating
    /// part --- that the five serials are five *different* serials, which a
    /// parser returning a cached or constant answer could not manage.
    #[test]
    fn each_signed_fixture_carries_its_own_certificate() {
        let cases = [
            "incr-signed.pdf",
            "incr-certified-1.pdf",
            "incr-certified-2.pdf",
            "incr-certified-3.pdf",
            "incr-certified-3-indirect.pdf",
        ];

        let mut examined = 0;
        let mut serials = std::collections::BTreeSet::new();
        for name in cases {
            let Ok(bytes) = std::fs::read(std::path::Path::new("../testdata").join(name)) else {
                println!("[SKIP] {name}: not generated");
                continue;
            };
            let properties = scan(&bytes, 1).expect("a signed fixture must parse");
            let certificate = properties.signatures[0]
                .certificate
                .as_ref()
                .unwrap_or_else(|| panic!("{name} carries a certificate"));

            assert_eq!(
                certificate.subject_cn, "tpdf spike 0.6 test signer",
                "{name}"
            );
            assert!(certificate.self_issued, "{name}: the generator self-signs");
            assert_eq!(certificate.chain, 1, "{name}: no chain above it");
            assert!(certificate.matched_signer, "{name}");
            assert_eq!(
                certificate.serial.len(),
                40,
                "{name}: a 20-byte serial as hex, {:?}",
                certificate.serial
            );
            assert!(
                certificate
                    .serial
                    .chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()),
                "{name}: uppercase hex, {:?}",
                certificate.serial
            );
            assert!(
                !properties.limits.any(),
                "{name}: nothing was cut, so an absent certificate would be a fact"
            );
            serials.insert(certificate.serial.clone());
            examined += 1;
        }

        // Five SKIP lines and a pass look identical without this.
        assert!(examined > 0, "no signed fixture was read; generate them");
        assert_eq!(
            serials.len(),
            examined,
            "every fixture reported a serial of its own"
        );
    }

    /// A blob that is not DER is *unread*, which is not the same as absent.
    ///
    /// The two want opposite readings --- absent is a fact about the document,
    /// unread is a fact about tpdf --- so the second must reach the reader as a
    /// limit rather than as a quiet `None`.
    #[test]
    fn a_blob_that_is_not_der_is_reported_as_unread_rather_than_absent() {
        let mut limits = Limits::default();
        let mut document = Document::with_version("1.7");
        let sig = dictionary! {
            "Type" => "Sig",
            "Filter" => "Adobe.PPKLite",
            "SubFilter" => "adbe.pkcs7.detached",
            "Contents" => Object::String(vec![0x30, 0x82, 0xFF, 0xFF, 0x01], lopdf::StringFormat::Hexadecimal),
        };
        let id = document.add_object(Object::Dictionary(sig.clone()));
        let _ = id;

        assert!(read_certificate(&document, &sig, &mut limits).is_none());
        assert_eq!(limits.certificates_unread, 1, "and it says so");
        assert!(limits.any(), "so the panel shows the notice");
    }

    /// An all-zero `/Contents` is a reserved placeholder, not a parse failure.
    ///
    /// A signature is written by reserving a span and filling it, so this is
    /// what a half-written one looks like. Counting it as unread would put a
    /// "could not read" notice on a document with nothing wrong with it.
    #[test]
    fn an_untouched_placeholder_is_absent_rather_than_unread() {
        let mut limits = Limits::default();
        let document = Document::with_version("1.7");
        let sig = dictionary! {
            "Type" => "Sig",
            "Contents" => Object::String(vec![0; 512], lopdf::StringFormat::Hexadecimal),
        };

        assert!(read_certificate(&document, &sig, &mut limits).is_none());
        assert_eq!(limits.certificates_unread, 0);
        assert!(!limits.any(), "nothing is wrong with it");
    }

    /// A bound a *valid* blob exceeds refuses it rather than parsing it.
    ///
    /// The pair is the test. An oversized piece of garbage cannot check this ---
    /// refused, and parsed-then-failed, produce the same `None` and the same
    /// increment --- so such a test passes whether the guard is there or not.
    /// The first version of it did exactly that, and a mutation deleting the
    /// guard survived it. The same real blob is offered twice instead, once
    /// under a bound it clears and once under one it does not; only the guard
    /// can make those two differ.
    #[test]
    fn a_bound_a_valid_blob_exceeds_refuses_it_rather_than_parsing_it() {
        let Some(blob) = signature_blob("incr-signed.pdf") else {
            println!("[SKIP] incr-signed.pdf: not generated");
            return;
        };
        let document = Document::with_version("1.7");
        let sig = dictionary! {
            "Contents" => Object::String(blob, lopdf::StringFormat::Hexadecimal),
        };

        // The control: under the real bound the very same blob parses.
        let mut generous = Limits::default();
        assert!(
            read_certificate_bounded(&document, &sig, &mut generous, MAX_SIG_BLOB).is_some(),
            "the blob is a real one and must parse when it is allowed to"
        );
        assert_eq!(generous.certificates_unread, 0);

        let mut tight = Limits::default();
        assert!(
            read_certificate_bounded(&document, &sig, &mut tight, 100).is_none(),
            "and must be refused when it is over the bound"
        );
        assert_eq!(tight.certificates_unread, 1, "and say so");
    }

    /// The `/Contents` blob of a signed fixture, padding and all.
    fn signature_blob(name: &str) -> Option<Vec<u8>> {
        let bytes = std::fs::read(std::path::Path::new("../testdata").join(name)).ok()?;
        let document = Document::load_mem(&bytes).ok()?;
        for object in document.objects.values() {
            let Ok(dict) = object.as_dict() else { continue };
            if !dict.has(b"ByteRange") {
                continue;
            }
            if let Ok(contents) = dict.get(b"Contents").and_then(|o| o.as_str()) {
                return Some(contents.to_vec());
            }
        }
        None
    }

    /// Builds a CMS blob by hand, so a test can vary what no fixture varies.
    ///
    /// Every signed fixture in `testdata` is a **self-signed, single-certificate**
    /// blob, which makes two things true by construction: a certificate's issuer
    /// and subject are the same name, and matching a signer by issuer common name
    /// gives the same answer as matching by encoded issuer and serial. Two
    /// mutations survived on exactly that --- `self_issued: true` for everything,
    /// and CN-only signer matching --- and neither is a variant. They are code no
    /// fixture reaches.
    ///
    /// Nothing here is cryptographically meaningful. The key is eight zero bytes
    /// and the signature is four more, because `parse_certificate` verifies
    /// nothing and must not start to; this is a shape, not a credential.
    fn cms_blob(subject: &str, issuer: &str, cert_serial: &[u8], sid_serial: &[u8]) -> Vec<u8> {
        use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
        use cms::content_info::ContentInfo;
        use cms::signed_data::{
            CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo,
            SignerInfos,
        };
        use der::asn1::{Any, BitString, OctetString, SetOfVec, Utf8StringRef};
        use der::{oid::ObjectIdentifier, Encode};

        let named = |text: &str| -> x509_cert::name::Name {
            let attribute = x509_cert::attr::AttributeTypeAndValue {
                oid: ObjectIdentifier::new_unwrap("2.5.4.3"),
                value: Any::encode_from(&Utf8StringRef::new(text).expect("utf8")).expect("any"),
            };
            let mut set = SetOfVec::new();
            set.insert(attribute).expect("one attribute");
            x509_cert::name::RdnSequence(vec![x509_cert::name::RelativeDistinguishedName(set)])
        };
        let algorithm = x509_cert::spki::AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11"),
            parameters: None,
        };
        let moment = |seconds: u64| -> x509_cert::time::Time {
            x509_cert::time::Time::UtcTime(
                der::asn1::UtcTime::from_unix_duration(std::time::Duration::from_secs(seconds))
                    .expect("a time inside the UTCTime range"),
            )
        };

        let certificate = x509_cert::Certificate {
            tbs_certificate: x509_cert::TbsCertificate {
                version: x509_cert::Version::V3,
                serial_number: x509_cert::serial_number::SerialNumber::new(cert_serial)
                    .expect("a serial"),
                signature: algorithm.clone(),
                issuer: named(issuer),
                validity: x509_cert::time::Validity {
                    not_before: moment(1_767_225_600),
                    not_after: moment(1_893_456_000),
                },
                subject: named(subject),
                subject_public_key_info: x509_cert::spki::SubjectPublicKeyInfoOwned {
                    algorithm: algorithm.clone(),
                    subject_public_key: BitString::from_bytes(&[0u8; 8]).expect("a key"),
                },
                issuer_unique_id: None,
                subject_unique_id: None,
                extensions: None,
            },
            signature_algorithm: algorithm.clone(),
            signature: BitString::from_bytes(&[0u8; 4]).expect("a signature"),
        };

        let signer = SignerInfo {
            version: cms::content_info::CmsVersion::V1,
            sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
                issuer: named(issuer),
                serial_number: x509_cert::serial_number::SerialNumber::new(sid_serial)
                    .expect("a serial"),
            }),
            digest_alg: algorithm.clone(),
            signed_attrs: None,
            signature_algorithm: algorithm,
            signature: OctetString::new(&[0u8; 4][..]).expect("a signature"),
            unsigned_attrs: None,
        };

        let mut certificates = SetOfVec::new();
        certificates
            .insert(CertificateChoices::Certificate(certificate))
            .expect("one certificate");
        let mut signers = SetOfVec::new();
        signers.insert(signer).expect("one signer");

        let signed = SignedData {
            version: cms::content_info::CmsVersion::V1,
            digest_algorithms: SetOfVec::new(),
            encap_content_info: EncapsulatedContentInfo {
                econtent_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1"),
                econtent: None,
            },
            certificates: Some(CertificateSet(certificates)),
            crls: None,
            signer_infos: SignerInfos(signers),
        };

        let info = ContentInfo {
            content_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2"),
            content: Any::encode_from(&signed).expect("any"),
        };
        info.to_der().expect("der")
    }

    /// A certificate somebody else issued is not reported as self-issued.
    ///
    /// Unreachable with any fixture in `testdata`, every one of which is
    /// self-signed --- so before this, `self_issued: true` hardcoded would have
    /// passed the whole suite while making a confident false statement about who
    /// vouched for every signer tpdf will ever show.
    #[test]
    fn a_certificate_somebody_else_issued_is_not_called_self_issued() {
        let blob = cms_blob("A. Signer", "Test Root CA", &[0x2A], &[0x2A]);
        let certificate = parse_certificate(&blob).expect("a parseable blob");

        assert_eq!(certificate.subject_cn, "A. Signer");
        assert_eq!(certificate.issuer_cn, "Test Root CA");
        assert!(!certificate.self_issued, "the issuer is not the subject");

        // The control, so the assertion above is about the comparison and not
        // about the builder: the same shape with one name reports the opposite.
        let same = cms_blob("Test Root CA", "Test Root CA", &[0x2A], &[0x2A]);
        let itself = parse_certificate(&same).expect("a parseable blob");
        assert!(itself.self_issued);
    }

    /// A certificate from the right authority but the wrong serial is not the
    /// signer's, and saying so is the whole of `matched_signer`.
    ///
    /// The decisive shape, and it took some finding: with **one** certificate in
    /// the set there is no ordering to reason about, so correct matching (issuer
    /// bytes *and* serial) and matching on the issuer's common name alone give
    /// visibly different answers. Two authorities sharing a common name is the
    /// real-world case this stands for, and it is the one where a reader would
    /// be shown a name that is not the signer's.
    #[test]
    fn a_certificate_from_the_right_issuer_but_the_wrong_serial_is_not_the_signer() {
        let blob = cms_blob("Decoy", "Test Root CA", &[0x01], &[0x02]);
        let certificate = parse_certificate(&blob).expect("a parseable blob");

        assert_eq!(certificate.chain, 1);
        assert!(
            !certificate.matched_signer,
            "the signature names serial 02 and the certificate is serial 01"
        );
        // Still reported, because a set of one leaves nothing to choose between.
        assert_eq!(certificate.subject_cn, "Decoy");

        // The control: the same builder with the serials agreeing does match.
        let agreeing = cms_blob("Decoy", "Test Root CA", &[0x01], &[0x01]);
        let matched = parse_certificate(&agreeing).expect("a parseable blob");
        assert!(matched.matched_signer);
    }

    /// Two signatures, two signers, and a chain above each of them.
    ///
    /// `incr-two-signers.pdf` exists because four things were untestable while
    /// every signed fixture was one signature carrying one self-issued
    /// certificate: walking `/AcroForm /Fields` past its first entry, picking
    /// the signer out of a `certificates` set with something else in it, a first
    /// signature whose range stops short because a second was appended after it,
    /// and `signature-probe`'s pairing of our list against PDFium's.
    ///
    /// Both leaves are issued by one root, so a reader taking the wrong element
    /// of the set reports **the same name for both signatures** --- which is why
    /// the two subjects being different is the assertion that matters here.
    #[test]
    fn two_signers_are_told_apart_and_neither_is_reported_as_the_authority() {
        let Ok(bytes) = std::fs::read("../testdata/incr-two-signers.pdf") else {
            println!("[SKIP] incr-two-signers.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 2).expect("a signed fixture must parse");
        assert_eq!(properties.signatures.len(), 2, "two signature fields");

        let named: Vec<&str> = properties
            .signatures
            .iter()
            .map(|signature| {
                signature
                    .certificate
                    .as_ref()
                    .map_or("", |certificate| certificate.subject_cn.as_str())
            })
            .collect();
        assert_eq!(
            named,
            ["First Signer", "Second Signer"],
            "in document order"
        );

        for (index, signature) in properties.signatures.iter().enumerate() {
            let certificate = signature
                .certificate
                .as_ref()
                .unwrap_or_else(|| panic!("signature {} carries a certificate", index + 1));
            assert_eq!(
                certificate.chain,
                2,
                "signature {}: the leaf and the root above it",
                index + 1
            );
            assert!(
                certificate.matched_signer,
                "signature {}: found through SignerInfo.sid, not by position",
                index + 1
            );
            assert!(
                !certificate.self_issued,
                "signature {}: a root issued it",
                index + 1
            );
            assert_eq!(certificate.issuer_cn, "tpdf test root CA", "both leaves");
        }

        // The append is the second signature, so the first cannot cover the file
        // and the second must. Nothing else in the corpus has both answers in
        // one document, which is what makes this more than a restatement of
        // `a_signature_covering_the_file_says_so_and_one_that_does_not_says_that`.
        assert!(
            !properties.signatures[0].covers_whole_file,
            "the first was signed before the second was appended"
        );
        assert!(
            properties.signatures[1].covers_whole_file,
            "and the second reaches the last byte"
        );
        assert!(
            properties.signatures[0].covered_bytes < properties.signatures[1].covered_bytes,
            "so the first covers strictly less"
        );
    }

    /// The honesty rule, held by the type rather than by review.
    ///
    /// Adding a field to [`Certificate`] is a compile error here, which is the
    /// moment to ask whether the new field states something the parser checked
    /// or something it merely read. `self_issued` is the only checked one and
    /// its doc comment says why that is not a verdict.
    #[test]
    fn no_certificate_field_may_carry_a_verdict() {
        let Certificate {
            subject: _,
            subject_cn: _,
            issuer: _,
            issuer_cn: _,
            serial: _,
            from: _,
            until: _,
            self_issued: _,
            chain: _,
            matched_signer: _,
        } = Certificate::default();
    }

    /// A `BMPString` name is UTF-16BE, which is what Windows authorities emit.
    ///
    /// Decoded as bytes it comes out as text interleaved with nulls, which reads
    /// as a mangled name rather than as a decoding bug --- so this is the case
    /// that would ship looking merely ugly.
    #[test]
    fn a_utf16_name_is_decoded_rather_than_shown_with_nulls() {
        use der::asn1::{Any, BmpString};

        let name = BmpString::from_utf8("Müller GmbH").expect("encodable");
        let attribute = x509_cert::attr::AttributeTypeAndValue {
            oid: "2.5.4.3".parse().expect("the common-name oid"),
            value: Any::encode_from(&name).expect("any"),
        };

        assert_eq!(attribute_text(&attribute), "Müller GmbH");
    }
}
