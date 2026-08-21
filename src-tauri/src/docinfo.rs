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
use crate::ber;
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
    /// The field's **fully qualified** name --- its ancestors' `/T` values and
    /// its own, joined with a period, as PDF 32000-1 §12.7.3.2 defines it and as
    /// Acrobat displays it. Empty when nothing in the chain is named.
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
    /// What a timestamp authority attested about when this signature existed.
    ///
    /// `None` is a signature with no RFC 3161 token, which is most of them ---
    /// 1 of 10 signed documents to hand carries one. A token that was present
    /// and would not parse is counted in [`Limits::timestamps_unread`] rather
    /// than reported as absent.
    pub timestamp: Option<Timestamp>,
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
    /// The key usage extension, 2.5.29.15 --- what the *issuer* says this key
    /// may be used for, named in the order RFC 5280 §4.2.1.3 defines the bits.
    ///
    /// `None` is a certificate carrying no such extension, which places no
    /// limit at all; `Some` of an empty list is one that limits it to nothing.
    /// The two are different documents and are kept different here.
    pub key_usage: Option<Vec<String>>,
    /// The extended key usage extension, 2.5.29.37 --- the purposes the issuer
    /// named. Each is given its RFC 5280 name when it is one of the handful
    /// written out in [`purpose_name`], and as dotted digits otherwise, so an
    /// unrecognised purpose is reported rather than dropped.
    ///
    /// `None` and `Some(vec![])` differ for the same reason as above.
    pub extended_usage: Option<Vec<String>>,
    /// Basic constraints, 2.5.29.19: whether the certificate says it may issue
    /// others. `None` when the extension is absent.
    pub authority: Option<bool>,
    /// Extensions present but not decodable.
    ///
    /// Counted rather than swallowed, because a malformed key usage reported as
    /// an absent one reads as *"the issuer placed no limit"* --- which is the
    /// reassuring direction, and is a claim the certificate does not make.
    pub extensions_unread: u32,
}

/// What a timestamp authority said about when a signature existed.
///
/// A signature's `/M` is whatever the signer's own computer clock read: free
/// text in the signature dictionary, written by the machine doing the signing.
/// An RFC 3161 token is a **different party's** statement, minted by a
/// timestamp authority and carried as an unsigned attribute on the signer.
/// That is the only reason this is worth reading separately --- the two answer
/// the same question from different places, and only one of them is anything
/// but the signer's own word.
///
/// **Still not verified.** tpdf does not check the token's own signature, build
/// a chain to a TSA it trusts, or compare the token's message imprint against
/// the signature it claims to cover. So this is a second claim, not a check;
/// `properties.ts` says so where it renders it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timestamp {
    /// `genTime` from the token's `TSTInfo`, formatted as every date here is.
    pub when: String,
    /// The certificate the token itself is signed with --- the authority's.
    ///
    /// Read by the same [`parse_certificate`] the signer's certificate goes
    /// through, because a timestamp token **is** a CMS `SignedData`: the
    /// authority is its signer. No second implementation, which is what stops
    /// the two drifting into disagreeing about what a certificate says.
    pub authority: Option<Certificate>,
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
    /// Timestamp tokens present on a signature and not readable.
    ///
    /// Counted rather than swallowed, for the reason every other entry here is:
    /// a token dropped in silence reads as a signature nobody timestamped,
    /// which is the ordinary case and therefore the reassuring one.
    pub timestamps_unread: usize,
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
            || self.timestamps_unread > 0
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
    /// The XMP metadata packet, when the document carries one.
    ///
    /// `None` is a document with no `/Metadata`, which is a fact about it;
    /// `Some` with [`crate::xmp::Xmp::unread`] set is one whose packet could
    /// not be read, which is a fact about tpdf. The two are never the same row.
    pub xmp: Option<crate::xmp::Xmp>,
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
        xmp: catalog.and_then(|c| read_xmp(&document, c)),
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

/// One text string entry, decoded. Empty when the key is absent or is not a
/// string --- the same shape as [`name_of`], for the same reason.
fn text_of(document: &Document, dict: &Dictionary, key: &[u8]) -> String {
    dict.get(key)
        .ok()
        .and_then(|o| resolve(document, o).as_str().ok())
        .map(decode_text_string)
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
/// The catalog's XMP packet, decoded.
///
/// A `/Metadata` stream is *usually* stored uncompressed --- the specification
/// wants a packet readable by a tool that does not parse PDF at all --- but a
/// filter is legal and Acrobat writes one, so this decompresses when it must.
/// A stream that will not decode is reported as an unread packet rather than as
/// an absent one, which is the same distinction [`crate::xmp::Xmp::unread`]
/// exists for one level down.
fn read_xmp(document: &Document, catalog: &Dictionary) -> Option<crate::xmp::Xmp> {
    let object = catalog.get(b"Metadata").ok()?;
    let stream = resolve(document, object).as_stream().ok()?;
    // `decompressed_content` fails on a stream with no filter, which is the
    // common case here, so falling back to the raw content is the normal path
    // rather than an error path.
    let packet = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    if packet.is_empty() {
        return Some(crate::xmp::Xmp {
            bytes: 0,
            unread: true,
            ..Default::default()
        });
    }
    Some(crate::xmp::scan(&packet))
}

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
    let mut queue: Vec<(&Object, u32, String)> =
        fields.iter().map(|f| (f, 0u32, String::new())).collect();
    // Reversed so the queue pops in document order, which is the order a reader
    // sees the fields in every other application.
    queue.reverse();

    while let Some((entry, depth, prefix)) = queue.pop() {
        if out.len() >= MAX_SIGNATURES {
            limits.signatures_dropped += 1;
            continue;
        }
        let Ok(field) = resolve(document, entry).as_dict() else {
            limits.unreadable += 1;
            continue;
        };

        let name = qualified_name(&prefix, &text_of(document, field, b"T"));

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
                    queue.push((kid, depth + 1, name.clone()));
                }
            } else {
                limits.unreadable += 1;
            }
        }

        if name_of(document, field, b"FT") != "Sig" {
            continue;
        }

        out.push(read_signature(document, field, name, size, limits));
    }

    out
}

/// A field's fully qualified name: its ancestors' partial names and its own,
/// joined with a period --- PDF 32000-1 §12.7.3.2.
///
/// A node with no `/T` contributes nothing and is **not** a level in the name,
/// which is what the specification says and is not merely tidier: a widget
/// annotation merged into its field is such a node, and so is the group a
/// document uses purely to hold kids together. Skipping them is what makes the
/// name Acrobat shows and the name reported here the same string.
fn qualified_name(prefix: &str, partial: &str) -> String {
    match (prefix.is_empty(), partial.is_empty()) {
        (_, true) => prefix.to_string(),
        (true, false) => partial.to_string(),
        (false, false) => format!("{prefix}.{partial}"),
    }
}

/// Reads one signature field.
fn read_signature(
    document: &Document,
    field: &Dictionary,
    name: String,
    size: u64,
    limits: &mut Limits,
) -> Signature {
    let text = |dict: &Dictionary, key: &[u8]| -> String { text_of(document, dict, key) };
    let mut out = Signature {
        field: name,
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

    // A *document* timestamp is a signature whose `/Contents` is the token
    // itself rather than a CMS carrying one as an attribute --- PDF 2.0
    // §12.8.5, `/SubFilter /ETSI.RFC3161`. Its signer is the authority, so the
    // certificate read above is the TSA's; the timestamp is the whole point of
    // the field rather than an attribute on something else.
    let document_timestamp = out.kind == "ETSI.RFC3161";
    if let Some(blob) =
        signature_contents(document, sig, MAX_SIG_BLOB, &mut limits.timestamps_unread)
    {
        out.timestamp = if document_timestamp {
            match parse_timestamp_token(&blob) {
                Some(timestamp) => Some(timestamp),
                None => {
                    limits.timestamps_unread += 1;
                    None
                }
            }
        } else {
            read_timestamp(&blob, &mut limits.timestamps_unread)
        };
    }
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
    let der_bytes = &signature_contents(document, sig, bound, &mut limits.certificates_unread)?;

    match parse_certificate(der_bytes) {
        Some(certificate) => Some(certificate),
        None => {
            limits.certificates_unread += 1;
            None
        }
    }
}

/// One signature's `/Contents` blob, ended where its structure ends and bounded.
///
/// A signature is written by reserving a fixed span and filling it, so the blob
/// is right-padded with zeros to whatever the writer reserved. Those are not
/// part of the encoding and a decoder is entitled to reject them.
///
/// All zeros is a reserved-but-unwritten placeholder, not a failure --- so that
/// case leaves the counter alone, and a mutation that increments there is
/// caught by `an_untouched_placeholder_is_absent_rather_than_unread`. It has to
/// be answered **before** the walk, which would otherwise read the leading
/// `00 00` as a well-formed empty value.
///
/// [`ber::to_definite_length`] decides where the blob ends by reading its
/// structure, which is what closed the two defects the trailing-zero scan this
/// replaced had: a value ending in a legitimate `0x00` lost it, and a BER blob
/// lost its end-of-contents markers. It also rewrites indefinite lengths, so a
/// signature no parser here could read now reaches one. See the traps of those
/// names.
fn signature_contents(
    document: &Document,
    sig: &Dictionary,
    bound: usize,
    unread: &mut usize,
) -> Option<Vec<u8>> {
    let raw = sig
        .get(b"Contents")
        .ok()
        .and_then(|o| resolve(document, o).as_str().ok())?;
    if raw.iter().all(|byte| *byte == 0) {
        return None;
    }
    let Some(blob) = ber::to_definite_length(raw) else {
        *unread += 1;
        return None;
    };

    if blob.len() > bound {
        *unread += 1;
        return None;
    }
    Some(blob)
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
    let extensions = tbs.extensions.as_deref().unwrap_or(&[]);
    let mut unread = 0u32;
    Some(Certificate {
        subject_cn: common_name(&tbs.subject),
        issuer_cn: common_name(&tbs.issuer),
        self_issued: tbs.subject.to_der().ok() == tbs.issuer.to_der().ok(),
        subject,
        issuer,
        serial: hex_of(tbs.serial_number.as_bytes()),
        from: certificate_date(&tbs.validity.not_before),
        until: certificate_date(&tbs.validity.not_after),
        key_usage: key_usage(extensions, &mut unread),
        extended_usage: extended_usage(extensions, &mut unread),
        authority: authority(extensions, &mut unread),
        chain,
        matched_signer,
        extensions_unread: unread,
    })
}

/// One extension's octets, by OID, and whether it was there at all.
///
/// The OIDs are written out at each call site rather than pulled from
/// `const_oid`'s database, which is the same choice [`subject_key_identifier`]
/// makes and for the same reason: the dependency stays to what is parsed.
fn extension_bytes<'a>(extensions: &'a [x509_cert::ext::Extension], oid: &str) -> Option<&'a [u8]> {
    extensions
        .iter()
        .find(|extension| extension.extn_id.to_string() == oid)
        .map(|extension| extension.extn_value.as_bytes())
}

/// Decodes one extension, counting a present-but-malformed one.
///
/// The count is what stops a malformed extension reading as an absent one. For
/// key usage those are opposite claims --- absent places no limit, malformed
/// places an unknown one --- and absent is the reassuring branch.
fn decode_extension<T>(
    extensions: &[x509_cert::ext::Extension],
    oid: &str,
    unread: &mut u32,
) -> Option<T>
where
    // Not `Decode<'static>`: that bound is satisfiable here only by leaking the
    // bytes, which on attacker-chosen input is a leak an attacker sizes. The
    // three types decoded here own everything they keep, so they decode from
    // any lifetime and the borrow ends with the call.
    T: for<'a> der::Decode<'a>,
{
    let bytes = extension_bytes(extensions, oid)?;
    match T::from_der(bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            *unread += 1;
            None
        }
    }
}

/// What the issuer says this key is for --- 2.5.29.15.
fn key_usage(extensions: &[x509_cert::ext::Extension], unread: &mut u32) -> Option<Vec<String>> {
    use x509_cert::ext::pkix::{KeyUsage, KeyUsages};

    let usage: KeyUsage = decode_extension(extensions, "2.5.29.15", unread)?;
    // Listed in the order RFC 5280 numbers the bits, so two certificates with
    // the same usage read the same way round.
    let named = [
        (KeyUsages::DigitalSignature, "Digital signature"),
        (KeyUsages::NonRepudiation, "Non-repudiation"),
        (KeyUsages::KeyEncipherment, "Key encipherment"),
        (KeyUsages::DataEncipherment, "Data encipherment"),
        (KeyUsages::KeyAgreement, "Key agreement"),
        (KeyUsages::KeyCertSign, "Certificate signing"),
        (KeyUsages::CRLSign, "CRL signing"),
        (KeyUsages::EncipherOnly, "Encipher only"),
        (KeyUsages::DecipherOnly, "Decipher only"),
    ];
    Some(
        named
            .into_iter()
            .filter(|(bit, _)| usage.0.contains(*bit))
            .map(|(_, name)| name.to_string())
            .collect(),
    )
}

/// The purposes the issuer named --- 2.5.29.37.
fn extended_usage(
    extensions: &[x509_cert::ext::Extension],
    unread: &mut u32,
) -> Option<Vec<String>> {
    use x509_cert::ext::pkix::ExtendedKeyUsage;

    let usage: ExtendedKeyUsage = decode_extension(extensions, "2.5.29.37", unread)?;
    Some(
        usage
            .0
            .iter()
            .map(|oid| purpose_name(&oid.to_string()))
            .collect(),
    )
}

/// An extended key usage OID's name, or the OID itself.
///
/// Only the purposes RFC 5280 §4.2.1.12 defines are named, plus the wildcard.
/// Anything else is returned as dotted digits: a purpose nobody here has heard
/// of is a fact about the certificate, and dropping it would be the one outcome
/// that reads as *"the issuer named nothing"*.
fn purpose_name(oid: &str) -> String {
    match oid {
        "2.5.29.37.0" => "Any purpose",
        "1.3.6.1.5.5.7.3.1" => "TLS server",
        "1.3.6.1.5.5.7.3.2" => "TLS client",
        "1.3.6.1.5.5.7.3.3" => "Code signing",
        "1.3.6.1.5.5.7.3.4" => "Email protection",
        "1.3.6.1.5.5.7.3.8" => "Time stamping",
        "1.3.6.1.5.5.7.3.9" => "OCSP signing",
        other => return other.to_string(),
    }
    .to_string()
}

/// Whether the certificate says it may issue others --- 2.5.29.19.
fn authority(extensions: &[x509_cert::ext::Extension], unread: &mut u32) -> Option<bool> {
    use x509_cert::ext::pkix::BasicConstraints;

    let constraints: BasicConstraints = decode_extension(extensions, "2.5.29.19", unread)?;
    Some(constraints.ca)
}

/// The RFC 3161 token a signer carries, and what it says.
///
/// The token lives in `SignerInfo.unsignedAttrs` under
/// 1.2.840.113549.1.9.16.2.14, which is where it has to live: it is minted
/// *after* the signature exists, so it cannot be inside what the signature
/// covers.
///
/// `None` is a signature with no token. `Some` with an empty [`Timestamp::when`]
/// cannot happen --- a token whose `genTime` will not read is reported through
/// `unread` and returns nothing, because a timestamp with no time is not a
/// weaker claim, it is no claim.
fn read_timestamp(blob: &[u8], unread: &mut usize) -> Option<Timestamp> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;
    use der::{Decode, Encode};

    let info = ContentInfo::from_der(blob).ok()?;
    let signed: SignedData = info.content.decode_as().ok()?;
    let signer = signed.signer_infos.0.as_slice().first()?;
    let attribute = signer
        .unsigned_attrs
        .as_ref()?
        .iter()
        // 1.2.840.113549.1.9.16.2.14, id-aa-timeStampToken, written out for the
        // reason every other OID in this module is.
        .find(|attribute| attribute.oid.to_string() == "1.2.840.113549.1.9.16.2.14")?;

    // The attribute's value is a SET OF, and a timestamp attribute carries
    // exactly one. Taking the first of several would be a guess; there is
    // nothing to choose between them, so several is refused.
    let [value] = attribute.values.as_slice() else {
        *unread += 1;
        return None;
    };
    let Ok(token) = value.to_der() else {
        *unread += 1;
        return None;
    };
    match parse_timestamp_token(&token) {
        Some(timestamp) => Some(timestamp),
        None => {
            *unread += 1;
            None
        }
    }
}

/// Reads one RFC 3161 `TimeStampToken`.
///
/// The token is itself a CMS `SignedData`, so its **signer is the authority**
/// and [`parse_certificate`] reads it with no second implementation. Its
/// encapsulated content is a `TSTInfo`, which is where the attested time lives.
///
/// Public because a document timestamp --- a signature field whose `/SubFilter`
/// is `ETSI.RFC3161` --- carries the token directly in `/Contents` rather than
/// as an attribute on something else.
pub fn parse_timestamp_token(token: &[u8]) -> Option<Timestamp> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;
    use der::Decode;

    let info = ContentInfo::from_der(token).ok()?;
    let signed: SignedData = info.content.decode_as().ok()?;
    // 1.2.840.113549.1.9.16.1.4, id-ct-TSTInfo. Checked rather than assumed:
    // a CMS carrying something else is not a timestamp, and reading its content
    // as a TSTInfo would produce a time out of whatever bytes happened to sit
    // in the fifth position.
    if signed.encap_content_info.econtent_type.to_string() != "1.2.840.113549.1.9.16.1.4" {
        return None;
    }
    let content = signed.encap_content_info.econtent.as_ref()?;
    let wrapped = content.decode_as::<der::asn1::OctetString>().ok()?;
    let when = read_gen_time(wrapped.as_bytes())?;

    Some(Timestamp {
        when,
        authority: parse_certificate(token),
    })
}

/// `genTime` out of a `TSTInfo`, formatted as every date in this module is.
///
/// RFC 3161 §2.4.2 puts it fifth:
///
/// ```text
/// TSTInfo ::= SEQUENCE {
///    version         INTEGER { v1(1) },
///    policy          TSAPolicyId,
///    messageImprint  MessageImprint,
///    serialNumber    INTEGER,
///    genTime         GeneralizedTime,
///    ...             -- five optional fields this does not read
/// }
/// ```
///
/// The four ahead of it are skipped as opaque values rather than modelled,
/// which is deliberate: modelling `MessageImprint` and the five optional
/// trailing fields would be a page of types to reach one string, and every one
/// of them is a place to be wrong about a document nobody here has seen. The
/// fields are skipped **by position**, so a `TSTInfo` that is malformed enough
/// to shift them reads as no time rather than as the wrong time --- the parse of
/// the fifth value as a `GeneralizedTime` is what enforces that.
fn read_gen_time(tst_info: &[u8]) -> Option<String> {
    use der::{Decode, Tagged as _};

    let mut outer = der::SliceReader::new(tst_info).ok()?;
    let sequence = der::asn1::AnyRef::decode(&mut outer).ok()?;
    // Checked rather than trusted: reading the contents of something that is
    // not a SEQUENCE would walk whatever bytes follow and could still land a
    // plausible `GeneralizedTime`.
    if sequence.tag() != der::Tag::Sequence {
        return None;
    }
    let mut inner = der::SliceReader::new(sequence.value()).ok()?;
    for _ in 0..4 {
        der::asn1::AnyRef::decode(&mut inner).ok()?;
    }
    let at = der::asn1::GeneralizedTime::decode(&mut inner)
        .ok()?
        .to_date_time();
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minutes(),
        at.seconds()
    ))
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

        let names: Vec<&str> = cases.iter().map(|case| case.0).collect();
        if none_generated(&names) {
            println!("[SKIP] no signed fixture is generated here (BUILD.md, Test fixtures)");
            return;
        }
        // Five SKIP lines and a pass look identical, and this is the check that
        // separates them --- see the same guard in `crate::print`.
        assert_eq!(
            examined,
            cases.len(),
            "every signed fixture that exists must be read --- generate testdata/ (BUILD.md, Test fixtures)"
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
            timestamp: _,
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
    /// Whether *none* of a set of fixtures is present, which is a different
    /// state from one of them missing and needs a different answer.
    ///
    /// A hosted runner has no signed fixture at all: they need pyhanko, and
    /// `scripts/ci_fixtures.py` records in its own docstring which families a
    /// runner deliberately does not get. So `assert!(examined > 0)` beneath a
    /// loop over them is red on every machine that is not a development
    /// checkout --- measured: hiding `testdata/incr-*.pdf` reddens three tests
    /// here. It is the same shape as the `corpora` gate that demanded on a
    /// runner exactly what the repository had already written down as absent.
    ///
    /// The state that guard is *for* is the other one: some present and one
    /// missing, where every read is a `[SKIP]` and the run looks like a pass.
    /// That is now asserted directly --- every named fixture must be examined,
    /// not merely one of them --- which is stronger than the count it replaces
    /// and says nothing at all when the family is absent.
    fn none_generated(names: &[&str]) -> bool {
        !names
            .iter()
            .any(|name| std::path::Path::new("../testdata").join(name).exists())
    }

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

        if none_generated(&cases) {
            println!("[SKIP] no signed fixture is generated here (BUILD.md, Test fixtures)");
            return;
        }
        // Five SKIP lines and a pass look identical without this.
        assert_eq!(
            examined,
            cases.len(),
            "every signed fixture that exists must be read, not merely one"
        );
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
        // A well-formed value that is not a CMS `ContentInfo`. It has to be
        // well formed: `ber::to_definite_length` refuses a blob it cannot walk
        // and counts that itself, so a *malformed* blob reaches the same
        // counter without ever reaching the arm under test --- which is what
        // made a mutation of that arm survive when the walk was put in front of
        // it. One input per mechanism, and the other one is below.
        let sig = dictionary! {
            "Type" => "Sig",
            "Filter" => "Adobe.PPKLite",
            "SubFilter" => "adbe.pkcs7.detached",
            "Contents" => Object::String(vec![0x30, 0x03, 0x02, 0x01, 0x41], lopdf::StringFormat::Hexadecimal),
        };
        let id = document.add_object(Object::Dictionary(sig.clone()));
        let _ = id;

        assert!(read_certificate(&document, &sig, &mut limits).is_none());
        assert_eq!(limits.certificates_unread, 1, "and it says so");
        assert!(limits.any(), "so the panel shows the notice");
    }

    /// The other mechanism: a blob whose *structure* cannot be walked at all.
    ///
    /// `30 82 FF FF 01` is a SEQUENCE claiming 65,535 bytes with one byte
    /// present. Nothing here can parse it and nothing should try; the point is
    /// that it is counted rather than reported as a signature with no
    /// certificate, the same as the parse failure above and by a different
    /// route.
    #[test]
    fn a_blob_whose_structure_will_not_walk_is_reported_as_unread() {
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
        cms_blob_with(subject, issuer, cert_serial, sid_serial, Vec::new())
    }

    /// One extension, built from a value that knows its own encoding.
    ///
    /// The OID is passed rather than taken from `AssociatedOid`, so a test can
    /// put a value under the *wrong* OID --- which is how the malformed case is
    /// built without hand-writing DER.
    fn extension(oid: &str, critical: bool, der_bytes: Vec<u8>) -> x509_cert::ext::Extension {
        use der::asn1::OctetString;

        x509_cert::ext::Extension {
            extn_id: oid.parse().expect("an oid"),
            critical,
            extn_value: OctetString::new(der_bytes).expect("octets"),
        }
    }

    /// As [`cms_blob`], with the certificate carrying the extensions given.
    ///
    /// An empty list means no `extensions` member at all, which is the
    /// distinction the reading turns on: a certificate with no key usage places
    /// no limit, and one with an empty key usage limits it to nothing.
    fn cms_blob_with(
        subject: &str,
        issuer: &str,
        cert_serial: &[u8],
        sid_serial: &[u8],
        extensions: Vec<x509_cert::ext::Extension>,
    ) -> Vec<u8> {
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
                extensions: (!extensions.is_empty()).then_some(extensions),
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

    /// A signature field two levels down the `/AcroForm` field tree is found.
    ///
    /// `/Fields` is a tree, not a list: an entry may be a node whose `/Kids` hold
    /// fields, and producers that group fields write it that way. Every other
    /// signature fixture puts its field directly in `/Fields`, so the recursion
    /// in [`read_signatures`] was reached by nothing at all.
    ///
    /// **PDFium does not walk that tree**, established by control rather than
    /// inferred --- the same document with the leaf flat instead of nested, same
    /// signature dictionary byte for byte, gives `FPDF_GetSignatureCount` 1 and 0
    /// respectively, and `qpdf --check` passes both. So this is one of the very
    /// few places where the differential cannot corroborate us and the reading
    /// has to stand on the specification; `signature-probe --mode nested`
    /// asserts the disagreement so it cannot expire quietly.
    #[test]
    fn a_signature_field_under_kids_is_found_rather_than_walked_past() {
        let Ok(bytes) = std::fs::read("../testdata/signed-nested-field.pdf") else {
            println!("[SKIP] signed-nested-field.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 1).expect("the fixture must parse");

        assert_eq!(
            properties.signatures.len(),
            1,
            "one signature, two levels down"
        );
        let signature = &properties.signatures[0];
        assert!(signature.signed);
        assert_eq!(
            signature.field, "top.group.Signature1",
            "the two groups it hangs under are levels of its name, not scenery"
        );
        assert_eq!(signature.kind, "adbe.pkcs7.detached");
        assert!(
            signature
                .certificate
                .as_ref()
                .is_some_and(|certificate| !certificate.subject_cn.is_empty()),
            "and its certificate is read like any other"
        );
        assert_eq!(properties.limits.unreadable, 0, "nothing was walked past");
    }

    /// Builds a document whose `/AcroForm /Fields` is the tree described --- one
    /// chain per entry, each element a node's `/T` and the last of them the
    /// signature field itself --- and returns the names reported for it. `None`
    /// is a node carrying no `/T` at all.
    fn field_names(chains: &[&[Option<&str>]]) -> Vec<String> {
        let mut document = Document::with_version("1.7");
        let mut roots: Vec<Object> = Vec::new();
        for chain in chains {
            let sig = document.add_object(Object::Dictionary(dictionary! {
                "Type" => "Sig",
                "Filter" => "Adobe.PPKLite",
                "SubFilter" => "adbe.pkcs7.detached",
            }));
            let (leaf, above) = chain.split_last().expect("a chain ends in a field");
            let mut node = dictionary! {
                "FT" => "Sig",
                "V" => Object::Reference(sig),
            };
            if let Some(name) = leaf {
                node.set("T", Object::string_literal(*name));
            }
            let mut id = document.add_object(Object::Dictionary(node));
            for name in above.iter().rev() {
                let mut parent = dictionary! {
                    "Kids" => vec![Object::Reference(id)],
                };
                if let Some(name) = name {
                    parent.set("T", Object::string_literal(*name));
                }
                id = document.add_object(Object::Dictionary(parent));
            }
            roots.push(Object::Reference(id));
        }
        let form = document.add_object(Object::Dictionary(dictionary! {
            "Fields" => roots,
        }));
        let pages = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![]),
            "Count" => 0,
        }));
        let catalog = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
            "AcroForm" => Object::Reference(form),
        }));
        document.trailer.set("Root", Object::Reference(catalog));
        let mut out = Vec::new();
        document.save_to(&mut out).expect("a writable document");
        scan(&out, 0)
            .expect("the document must parse")
            .signatures
            .iter()
            .map(|signature| signature.field.clone())
            .collect()
    }

    /// Two fields may share a partial name, and the tree is what tells them
    /// apart.
    ///
    /// This is the whole reason a qualified name exists rather than being
    /// tidiness: `/T` is unique only among *siblings*, so a form that groups its
    /// fields is free to have a `Signature1` under each group, and a reader
    /// showing the leaf's own name shows one string for two fields. Every
    /// fixture here happens to have unique leaf names, so nothing else can tell
    /// the two readings apart.
    #[test]
    fn two_fields_with_the_same_leaf_name_are_told_apart_by_the_groups_above_them() {
        let names = field_names(&[
            &[Some("payer"), Some("Signature1")],
            &[Some("payee"), Some("Signature1")],
        ]);
        assert_eq!(names, vec!["payer.Signature1", "payee.Signature1"]);
    }

    /// A node with no `/T` is not a level of the name.
    ///
    /// PDF 32000-1 §12.7.3.2 says so, and it is not a detail: a widget
    /// annotation merged into its own field is exactly such a node, and so is a
    /// group written only to hold kids together. Joining unconditionally would
    /// put an empty component in the middle of every name that has one ---
    /// `top..Signature1` --- which is a string no other reader shows.
    #[test]
    fn a_node_with_no_name_of_its_own_is_not_a_level_of_the_name() {
        assert_eq!(
            field_names(&[&[Some("top"), None, Some("Signature1")]]),
            vec!["top.Signature1"],
            "the unnamed group in the middle contributes nothing"
        );
        assert_eq!(
            field_names(&[&[Some("top"), Some("group"), None]]),
            vec!["top.group"],
            "and neither does an unnamed leaf, which is then named by its parents"
        );
        assert_eq!(
            field_names(&[&[None, None]]),
            vec![""],
            "a chain naming nothing is reported as having no name, not as a dot"
        );
    }

    /// The field tree is walked to eight levels and no further.
    ///
    /// A `/Kids` chain is attacker-shaped: a document is free to make it a
    /// thousand deep, or to point a node at itself. The bound is what stops that,
    /// and until now nothing exercised it --- so this asserts both halves, which
    /// is what makes it a bound rather than a refusal. Seven levels are walked;
    /// nine are not, and the refusal is *counted* rather than silent, because a
    /// signature dropped without a word is indistinguishable from a document that
    /// has none.
    #[test]
    fn a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted() {
        // `depth` counts the nodes above the signature field.
        let scan_at = |depth: usize| -> Properties {
            let mut document = Document::with_version("1.7");
            let sig = document.add_object(Object::Dictionary(dictionary! {
                "Type" => "Sig",
                "Filter" => "Adobe.PPKLite",
                "SubFilter" => "adbe.pkcs7.detached",
            }));
            let mut node = document.add_object(Object::Dictionary(dictionary! {
                "FT" => "Sig",
                "T" => Object::string_literal("Signature1"),
                "V" => Object::Reference(sig),
            }));
            for _ in 0..depth {
                node = document.add_object(Object::Dictionary(dictionary! {
                    "Kids" => vec![Object::Reference(node)],
                }));
            }
            let form = document.add_object(Object::Dictionary(dictionary! {
                "Fields" => vec![Object::Reference(node)],
            }));
            let pages = document.add_object(Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => Object::Array(vec![]),
                "Count" => 0,
            }));
            let catalog = document.add_object(Object::Dictionary(dictionary! {
                "Type" => "Catalog",
                "Pages" => Object::Reference(pages),
                "AcroForm" => Object::Reference(form),
            }));
            document.trailer.set("Root", Object::Reference(catalog));
            let mut out = Vec::new();
            document.save_to(&mut out).expect("a writable document");
            scan(&out, 0).expect("the document must parse")
        };

        let shallow = scan_at(7);
        assert_eq!(
            shallow.signatures.len(),
            1,
            "seven levels are within the bound"
        );
        assert_eq!(shallow.limits.unreadable, 0, "and nothing was refused");

        let deep = scan_at(9);
        assert!(deep.signatures.is_empty(), "nine levels are past it");
        assert!(
            deep.limits.unreadable > 0,
            "and the refusal is counted, so the reader is told rather than \
             shown a document that looks unsigned"
        );
        assert!(deep.limits.any(), "which puts the notice on the dialog");
    }

    /// The honesty rule, held by the type rather than by review.
    ///
    /// Adding a field to [`Certificate`] is a compile error here, which is the
    /// moment to ask whether the new field states something the parser checked
    /// or something it merely read. `self_issued` is the only checked one and
    /// its doc comment says why that is not a verdict.
    ///
    /// The three usage fields are the case the question is worth asking about,
    /// because they are one short step from one: *the issuer says this key is
    /// for signing* is read out of the certificate, and *therefore this
    /// signature is sound* is a verdict nothing here is entitled to. What the
    /// extension constrains is the **key**, and only a chain built to a trusted
    /// issuer makes that constraint mean anything --- so these say what the
    /// certificate says, and the dialog labels them that way.
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
            key_usage: _,
            extended_usage: _,
            authority: _,
            extensions_unread: _,
        } = Certificate::default();
    }

    /// What a real certificate says it is for, against `openssl x509 -text`.
    ///
    /// The oracle shares no code with the parser --- for `incr-signed.pdf` it
    /// reads *"X509v3 Key Usage: critical / Digital Signature, Non Repudiation"*
    /// and *"X509v3 Basic Constraints: critical / CA:FALSE"*, and reports no
    /// extended key usage at all. Both of those are the *encoded* form: the bit
    /// order and the CA default are the two things a hand-rolled reading gets
    /// wrong, and neither is visible in a certificate that merely parses.
    #[test]
    fn the_usage_a_real_certificate_states_is_the_usage_openssl_reads() {
        let Ok(bytes) = std::fs::read("../testdata/incr-signed.pdf") else {
            println!("[SKIP] incr-signed.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 1).expect("the fixture must parse");
        let certificate = properties.signatures[0]
            .certificate
            .as_ref()
            .expect("the fixture is signed");

        assert_eq!(
            certificate.key_usage.as_deref(),
            Some(
                &[
                    "Digital signature".to_string(),
                    "Non-repudiation".to_string()
                ][..]
            ),
            "the two bits openssl names, in the order RFC 5280 numbers them"
        );
        assert_eq!(
            certificate.extended_usage, None,
            "the certificate carries no extended key usage, which is not the \
             same as carrying an empty one"
        );
        assert_eq!(certificate.authority, Some(false), "CA:FALSE, stated");
        assert_eq!(certificate.extensions_unread, 0);
    }

    /// What the authority attested, against the instant the fixture pinned.
    ///
    /// `genTime` is fixed by the generator rather than taken from the clock, so
    /// this asserts the **instant** and not merely its shape --- the distinction
    /// a serial and a date pinned out of generated bytes got wrong here once
    /// already. The authority's name is likewise the generator's own string.
    #[test]
    fn the_time_a_timestamp_authority_attested_is_read() {
        let Ok(bytes) = std::fs::read("../testdata/incr-timestamped.pdf") else {
            println!("[SKIP] incr-timestamped.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 2).expect("the fixture must parse");
        let signature = &properties.signatures[0];
        let timestamp = signature
            .timestamp
            .as_ref()
            .expect("the fixture's signature carries a token");

        assert_eq!(timestamp.when, "2026-08-21 12:00:00 UTC");
        assert_eq!(
            timestamp
                .authority
                .as_ref()
                .map(|certificate| certificate.subject_cn.as_str()),
            Some("tpdf dummy timestamp authority"),
            "the token's own signer is the authority, read by the same parser \
             the signer's certificate goes through"
        );
        assert_eq!(properties.limits.timestamps_unread, 0);
    }

    /// The attested time and the signer's own clock are two different answers.
    ///
    /// This is the whole reason a timestamp is worth reading separately. `/M`
    /// is written by the machine doing the signing and nothing checks it; the
    /// token is a third party's statement. In the fixture they differ by hours,
    /// because the generator pins one and the signing clock supplies the other
    /// --- so a reader that showed one number for both would be caught here.
    #[test]
    fn the_attested_time_is_not_the_clock_the_signer_wrote() {
        let Ok(bytes) = std::fs::read("../testdata/incr-timestamped.pdf") else {
            println!("[SKIP] incr-timestamped.pdf: not generated");
            return;
        };
        let signature = &scan(&bytes, 2).expect("must parse").signatures[0];

        assert!(!signature.when.is_empty(), "the signer's clock is reported");
        let attested = &signature.timestamp.as_ref().expect("a token").when;
        assert_ne!(
            &signature.when, attested,
            "two sources, two answers -- and the readout must not collapse them"
        );
    }

    /// A signature with no token reports none, and counts nothing.
    ///
    /// The control for every assertion above: most signatures carry no
    /// timestamp --- 1 of 10 signed documents to hand does --- so a reader that
    /// invented one would fail this and a reader that found none would fail
    /// only the tests above.
    #[test]
    fn a_signature_with_no_token_reports_no_timestamp_and_no_failure() {
        let Ok(bytes) = std::fs::read("../testdata/incr-signed.pdf") else {
            println!("[SKIP] incr-signed.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 1).expect("must parse");

        assert!(properties.signatures[0].timestamp.is_none());
        assert_eq!(
            properties.limits.timestamps_unread, 0,
            "absent is not the same as unreadable, and only one of them is a \
             failure worth telling the reader about"
        );
    }

    /// An ordinary signature blob is a CMS and is not a timestamp token.
    ///
    /// The check that makes that true is one comparison ---
    /// `eContentType == id-ct-TSTInfo` --- and without it this would read the
    /// encapsulated content of *any* CMS as a `TSTInfo` and hand back whatever
    /// a `GeneralizedTime` could be made of the fifth value in it. A time
    /// invented out of an ordinary signature is the worst outcome this module
    /// has, because it is a plausible number attributed to an authority.
    #[test]
    fn an_ordinary_signature_blob_is_not_read_as_a_timestamp_token() {
        let Ok(bytes) = std::fs::read("../testdata/incr-signed.pdf") else {
            println!("[SKIP] incr-signed.pdf: not generated");
            return;
        };
        let document = Document::load_mem(&bytes).expect("must parse");
        let mut limits = Limits::default();
        let field = document
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .find(|dict| dict.has(b"ByteRange"))
            .expect("the fixture is signed");
        // `signature_contents`, not the test helper of a similar name one
        // module down -- the collision cost a compile error, which is the
        // outcome to want from two functions with nearly the same name.
        let blob = signature_contents(
            &document,
            field,
            MAX_SIG_BLOB,
            &mut limits.certificates_unread,
        )
        .expect("a blob");

        // It is a well-formed CMS -- the certificate reader gets a certificate
        // out of it -- so this is not refused for being unparseable.
        assert!(
            parse_certificate(&blob).is_some(),
            "the control: these bytes are a CMS this module reads"
        );
        assert!(
            parse_timestamp_token(&blob).is_none(),
            "and they are still not a timestamp token"
        );
    }

    /// A real token relabelled as something else is not read as a timestamp.
    ///
    /// The fixture above cannot test the `eContentType` check and a mutation
    /// deleting it **survived**, correctly: an `adbe.pkcs7.detached` signature
    /// is *detached*, so it carries no encapsulated content at all and the step
    /// after the check refuses it anyway. Two guards, one outcome, and the
    /// weaker one doing the work.
    ///
    /// What discriminates is a CMS that **has** content the check must reject:
    /// the real token with its content type changed and its `TSTInfo` left
    /// exactly where it was. Reading it would be reading a time out of
    /// something that does not claim to be a timestamp.
    #[test]
    fn a_token_relabelled_as_something_else_is_not_read_as_a_timestamp() {
        let Ok(bytes) = std::fs::read("../testdata/incr-timestamped.pdf") else {
            println!("[SKIP] incr-timestamped.pdf: not generated");
            return;
        };
        let token = timestamp_token_of(&timestamped_blob(&bytes));

        // The control: as it stands it reads, so the refusal below is about the
        // one thing that changed.
        assert!(
            parse_timestamp_token(&token).is_some(),
            "the token itself reads"
        );
        assert!(
            parse_timestamp_token(&relabelled(&token)).is_none(),
            "and the same bytes under another content type do not"
        );
    }

    /// The timestamp token out of a signature blob's unsigned attributes.
    fn timestamp_token_of(blob: &[u8]) -> Vec<u8> {
        use cms::content_info::ContentInfo;
        use cms::signed_data::SignedData;
        use der::{Decode, Encode};

        let info = ContentInfo::from_der(blob).expect("a CMS");
        let signed: SignedData = info.content.decode_as().expect("signed data");
        let signer = signed.signer_infos.0.get(0).expect("a signer");
        let attribute = signer
            .unsigned_attrs
            .as_ref()
            .expect("unsigned attrs")
            .iter()
            .find(|attribute| attribute.oid.to_string() == "1.2.840.113549.1.9.16.2.14")
            .expect("a timestamp attribute");
        attribute.values.as_slice()[0]
            .to_der()
            .expect("the token's bytes")
    }

    /// A token whose encapsulated content type says it is something else.
    ///
    /// `id-data` rather than `id-ct-TSTInfo`. Nothing else about the token
    /// changes, which is what makes it a control over one comparison.
    fn relabelled(token: &[u8]) -> Vec<u8> {
        use cms::content_info::ContentInfo;
        use cms::signed_data::SignedData;
        use der::{Decode, Encode};

        let info = ContentInfo::from_der(token).expect("a CMS");
        let mut signed: SignedData = info.content.decode_as().expect("signed data");
        signed.encap_content_info.econtent_type = "1.2.840.113549.1.7.1".parse().expect("id-data");
        let content = der::Any::encode_from(&signed).expect("re-encodable");
        ContentInfo {
            content_type: info.content_type,
            content,
        }
        .to_der()
        .expect("a CMS")
    }

    /// A timestamp attribute carrying several values is refused.
    ///
    /// The attribute's value is a `SET OF`, and RFC 3161 puts exactly one token
    /// in it. Several is not a richer document, it is one nothing can choose
    /// between --- and picking the first would present a guess as an
    /// authority's statement. Refusing is the only honest answer, and it is
    /// **counted**, because a refusal in silence reads as no timestamp at all.
    ///
    /// Built by hand rather than taken from a file, because no producer emits
    /// this: the fixture is the signed one with a second copy of the token
    /// spliced into the same attribute.
    #[test]
    fn a_timestamp_attribute_carrying_more_than_one_value_is_refused() {
        let Ok(bytes) = std::fs::read("../testdata/incr-timestamped.pdf") else {
            println!("[SKIP] incr-timestamped.pdf: not generated");
            return;
        };
        let blob = timestamped_blob(&bytes);
        let mut unread = 0;

        // The control first: as it stands, one value, and it reads.
        assert!(
            read_timestamp(&blob, &mut unread).is_some(),
            "the unaltered blob carries a readable token"
        );
        assert_eq!(unread, 0);

        let doubled = with_a_second_timestamp_value(&blob);
        let mut unread = 0;
        assert!(
            read_timestamp(&doubled, &mut unread).is_none(),
            "two values leave nothing to choose between"
        );
        assert_eq!(unread, 1, "and the refusal is counted, not silent");
    }

    /// A token that will not parse is counted, not read as an absent one.
    #[test]
    fn a_token_that_will_not_parse_is_counted_rather_than_read_as_absent() {
        let Ok(bytes) = std::fs::read("../testdata/incr-timestamped.pdf") else {
            println!("[SKIP] incr-timestamped.pdf: not generated");
            return;
        };
        let blob = timestamped_blob(&bytes);
        let broken = with_broken_timestamp_attribute(&blob);
        let mut unread = 0;

        assert!(read_timestamp(&broken, &mut unread).is_none());
        assert_eq!(
            unread, 1,
            "a token nobody could read is a failure worth reporting; a \
             signature nobody timestamped is not"
        );
    }

    /// The `/Contents` blob of the timestamped fixture.
    fn timestamped_blob(bytes: &[u8]) -> Vec<u8> {
        let document = Document::load_mem(bytes).expect("the fixture must parse");
        let field = document
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .find(|dict| dict.has(b"ByteRange"))
            .expect("the fixture is signed");
        let mut unread = 0;
        signature_contents(&document, field, MAX_SIG_BLOB, &mut unread).expect("a blob")
    }

    /// The same blob with a second value on the timestamp attribute.
    ///
    /// The second value is **not** a copy of the token, because it cannot be: a
    /// `SET OF` forbids duplicate members and `SetOfVec::insert` refuses one.
    /// So the attribute carries the real token and something else, which is
    /// still the shape the guard exists for --- more than one value, nothing to
    /// choose between them.
    ///
    /// Rebuilt through the CMS types rather than spliced at the byte level, so
    /// the result is a structurally valid signature differing from the input in
    /// exactly one thing.
    fn with_a_second_timestamp_value(blob: &[u8]) -> Vec<u8> {
        rebuilt_with(blob, |values| {
            // An empty SET, whose encoding is `31 00`. The tag matters: a
            // `SET OF` is ordered by encoded bytes, so a value beginning below
            // 0x30 sorts **ahead** of the token, and a mutation taking the
            // first value would then get the rubbish and fail anyway --- which
            // is what an INTEGER did here, and the mutation survived. 0x31
            // sorts after 0x30, so the token stays first and "take the first"
            // and "refuse several" give different answers.
            let other = der::Any::new(der::Tag::Set, []).expect("an empty set");
            values.insert(other).expect("a second, different value");
        })
    }

    /// The same blob with the token replaced by something that is not one.
    fn with_broken_timestamp_attribute(blob: &[u8]) -> Vec<u8> {
        rebuilt_with(blob, |values| {
            let rubbish =
                der::Any::encode_from(&der::asn1::Uint::new(&[7]).expect("a small integer"))
                    .expect("encodable");
            *values = Default::default();
            values.insert(rubbish).expect("one value");
        })
    }

    /// Rebuilds a signature blob with its timestamp attribute's values edited.
    fn rebuilt_with(blob: &[u8], edit: impl FnOnce(&mut der::asn1::SetOfVec<der::Any>)) -> Vec<u8> {
        use cms::content_info::ContentInfo;
        use cms::signed_data::{SignedData, SignerInfos};
        use der::{Decode, Encode};

        let info = ContentInfo::from_der(blob).expect("a CMS");
        let mut signed: SignedData = info.content.decode_as().expect("signed data");

        // `SetOfVec` offers no mutable access at all, so both sets are rebuilt
        // from clones. The fixture carries one signer, which is what makes
        // rebuilding rather than editing straightforward here.
        let mut signer = signed.signer_infos.0.get(0).cloned().expect("a signer");
        let attributes = signer
            .unsigned_attrs
            .as_ref()
            .expect("unsigned attrs")
            .clone();
        let mut edit = Some(edit);
        let mut rebuilt = der::asn1::SetOfVec::new();
        for attribute in attributes.iter() {
            let mut attribute = attribute.clone();
            if attribute.oid.to_string() == "1.2.840.113549.1.9.16.2.14" {
                if let Some(edit) = edit.take() {
                    edit(&mut attribute.values);
                }
            }
            rebuilt.insert(attribute).expect("one attribute");
        }
        assert!(
            edit.is_none(),
            "the fixture must carry a timestamp attribute for this to alter one"
        );
        signer.unsigned_attrs = Some(rebuilt);

        let mut infos = der::asn1::SetOfVec::new();
        infos.insert(signer).expect("one signer");
        signed.signer_infos = SignerInfos(infos);

        let content = der::Any::encode_from(&signed).expect("re-encodable");
        ContentInfo {
            content_type: info.content_type,
            content,
        }
        .to_der()
        .expect("a CMS")
    }

    /// A `TSTInfo` whose fields are shifted reads as no time, not a wrong one.
    ///
    /// The four fields ahead of `genTime` are skipped by position rather than
    /// modelled, which is a deliberate trade --- and this is what bounds it. A
    /// structure short of five fields, or one whose fifth is something else,
    /// has to yield nothing: a time read out of the wrong field would be
    /// presented as an authority's statement, which is worse than silence.
    #[test]
    fn a_tst_info_that_is_not_shaped_like_one_yields_no_time() {
        // Written as bytes rather than through a DER writer, because the point
        // of each fixture is its *shape* and the bytes say it in one line.
        // `02 01 vv` is an INTEGER; `30 LL` a SEQUENCE of that length.
        let four_integers = &[
            0x30, 0x0C, // SEQUENCE, 12 bytes
            0x02, 0x01, 0x01, // version
            0x02, 0x01, 0x02, // policy, standing in
            0x02, 0x01, 0x03, // messageImprint, standing in
            0x02, 0x01,
            0x04, // serialNumber
                  // and nothing where genTime belongs
        ];
        assert!(
            read_gen_time(four_integers).is_none(),
            "four fields and no fifth is not a time"
        );

        let fifth_is_not_a_time = &[
            0x30, 0x0F, // SEQUENCE, 15 bytes
            0x02, 0x01, 0x01, 0x02, 0x01, 0x02, 0x02, 0x01, 0x03, 0x02, 0x01, 0x04, 0x02, 0x01,
            0x05, // an INTEGER where the GeneralizedTime belongs
        ];
        assert!(
            read_gen_time(fifth_is_not_a_time).is_none(),
            "a fifth field that is not a GeneralizedTime is not a time"
        );

        // Not a SEQUENCE at all. The discriminating shape is an OCTET STRING
        // wrapping a body that IS well formed: an INTEGER holding rubbish is
        // refused with or without the tag check --- the four-value walk fails on
        // it either way --- so a mutation deleting the check survived that
        // version of this assertion. Same contents, different tag, and only the
        // check can tell them apart.
        let body: &[u8] = &[
            0x02, 0x01, 0x01, 0x02, 0x01, 0x02, 0x02, 0x01, 0x03, 0x02, 0x01, 0x04, 0x18, 0x0F,
            b'2', b'0', b'2', b'6', b'0', b'8', b'2', b'1', b'1', b'2', b'0', b'0', b'0', b'0',
            b'Z',
        ];
        let mut octet_string = vec![0x04, 0x1D];
        octet_string.extend_from_slice(body);
        assert!(
            read_gen_time(&octet_string).is_none(),
            "an OCTET STRING holding a well-formed body is still not a TSTInfo"
        );
        assert!(
            read_gen_time(&[0x02, 0x01, 0x09]).is_none(),
            "and neither is an INTEGER"
        );

        // And the control, so the three refusals above are not three ways of
        // saying that this function never answers: the same shape with a real
        // GeneralizedTime fifth **does** read.
        let well_formed = &[
            0x30, 0x1D, // SEQUENCE, 29 bytes: 12 of integers, 2 of header, 15 of time
            0x02, 0x01, 0x01, 0x02, 0x01, 0x02, 0x02, 0x01, 0x03, 0x02, 0x01, 0x04, 0x18,
            0x0F, // GeneralizedTime, 15 bytes
            b'2', b'0', b'2', b'6', b'0', b'8', b'2', b'1', b'1', b'2', b'0', b'0', b'0', b'0',
            b'Z',
        ];
        assert_eq!(
            read_gen_time(well_formed).as_deref(),
            Some("2026-08-21 12:00:00 UTC")
        );
    }

    /// A document whose catalog points at the XMP packet given.
    fn with_metadata(packet: &[u8], compress: bool) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let mut stream = lopdf::Stream::new(
            dictionary! {
                "Type" => "Metadata",
                "Subtype" => "XML",
            },
            packet.to_vec(),
        );
        if compress {
            stream.compress().expect("a compressible stream");
        }
        let metadata = document.add_object(Object::Stream(stream));
        let pages = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![]),
            "Count" => 0,
        }));
        let catalog = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
            "Metadata" => Object::Reference(metadata),
        }));
        document.trailer.set("Root", Object::Reference(catalog));
        let mut out = Vec::new();
        document.save_to(&mut out).expect("a writable document");
        out
    }

    /// A PDF/A claim in the packet reaches the readout.
    ///
    /// The end-to-end half of `xmp.rs`'s own tests, which take bytes: this is
    /// what proves the catalog is consulted, the stream is fetched and the
    /// packet handed over. All three are things a unit test over the parser is
    /// structurally unable to check.
    #[test]
    fn a_conformance_claim_in_the_metadata_stream_reaches_the_readout() {
        let packet = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>3</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

        let plain = scan(&with_metadata(packet, false), 0).expect("must parse");
        let xmp = plain.xmp.as_ref().expect("the catalog names a packet");
        assert_eq!(xmp.conformance, vec!["PDF/A-3B"]);
        assert!(!xmp.unread);

        // And the same packet behind a filter. The specification wants a packet
        // readable by a tool that does not parse PDF, so uncompressed is the
        // common case -- which is exactly why the compressed one is the path
        // that would ship unexercised.
        let squeezed = scan(&with_metadata(packet, true), 0).expect("must parse");
        assert_eq!(
            squeezed.xmp.as_ref().map(|x| x.conformance.clone()),
            Some(vec!["PDF/A-3B".to_string()]),
            "a filtered stream states the same thing as an unfiltered one"
        );
    }

    /// No `/Metadata` is `None`, which is not the same as an unread packet.
    #[test]
    fn a_document_with_no_metadata_stream_reports_no_packet_at_all() {
        let properties = scan(&with_no_metadata(), 0).expect("must parse");
        assert!(
            properties.xmp.is_none(),
            "a document that carries no packet is not a document whose packet \
             could not be read"
        );
    }

    /// A document with a catalog and no `/Metadata`.
    fn with_no_metadata() -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let pages = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![]),
            "Count" => 0,
        }));
        let catalog = document.add_object(Object::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
        }));
        document.trailer.set("Root", Object::Reference(catalog));
        let mut out = Vec::new();
        document.save_to(&mut out).expect("a writable document");
        out
    }

    /// The one fixture carrying a real packet is read, and claims nothing.
    ///
    /// A file written by the hostile-document generator rather than by these
    /// tests, so it exercises the byte layout a real producer emits ---
    /// `<?xpacket?>` wrappers included. It states a `dc:title` and no
    /// conformance, which is what most documents with XMP look like, so this is
    /// the control against a reader that finds a claim in anything.
    #[test]
    fn the_fixture_with_a_real_packet_is_read_and_claims_nothing() {
        let Ok(bytes) = std::fs::read("../testdata/hostile-metadata.pdf") else {
            println!("[SKIP] hostile-metadata.pdf: not generated");
            return;
        };
        let properties = scan(&bytes, 1).expect("the fixture must parse");
        let xmp = properties.xmp.as_ref().expect("it carries a packet");

        assert!(xmp.conformance.is_empty(), "it claims no standard");
        assert!(!xmp.unread, "and all of it was read");
        assert!(xmp.bytes > 100, "its size is reported: {}", xmp.bytes);
    }

    /// Every key usage bit, on its own, against the name RFC 5280 gives it.
    ///
    /// Written because a mutation swapping the first two rows of the table
    /// **survived**, and it was right to: the fixture certificate sets both
    /// bits, so a permutation of two selected entries produces the same list.
    /// That is a fixture on which the right table and a wrong one agree, and no
    /// assertion over it could have said so --- the discrimination has to come
    /// from a certificate setting **one** bit, which no real one does.
    ///
    /// Enumerating the domain rather than sampling it: nine bits is the whole
    /// of it, so every mapping is checked rather than the two that happen to
    /// appear in the corpus.
    #[test]
    fn each_key_usage_bit_is_named_by_the_name_rfc_5280_gives_it() {
        use der::Encode as _;
        use x509_cert::ext::pkix::{KeyUsage, KeyUsages};

        let bits = [
            (KeyUsages::DigitalSignature, "Digital signature"),
            (KeyUsages::NonRepudiation, "Non-repudiation"),
            (KeyUsages::KeyEncipherment, "Key encipherment"),
            (KeyUsages::DataEncipherment, "Data encipherment"),
            (KeyUsages::KeyAgreement, "Key agreement"),
            (KeyUsages::KeyCertSign, "Certificate signing"),
            (KeyUsages::CRLSign, "CRL signing"),
            (KeyUsages::EncipherOnly, "Encipher only"),
            (KeyUsages::DecipherOnly, "Decipher only"),
        ];

        for (bit, expected) in bits {
            let blob = cms_blob_with(
                "Signer",
                "Signer",
                &[1],
                &[1],
                vec![extension(
                    "2.5.29.15",
                    true,
                    KeyUsage(bit.into()).to_der().expect("der"),
                )],
            );
            let certificate = parse_certificate(&blob).expect("a certificate");
            assert_eq!(
                certificate.key_usage.as_deref(),
                Some(&[expected.to_string()][..]),
                "the bit for {expected} must produce that name and no other"
            );
        }

        // And the half the loop above structurally cannot check: **order**.
        // Every assertion in it is over a one-element list, so a table whose
        // rows are correct pairs in the wrong sequence satisfies all nine and
        // then lists a real certificate's usage in an order no other reader
        // shows. A certificate setting every bit is the only input that can
        // see it.
        //
        // The table below is a second copy of the one in `key_usage`, which is
        // what makes this able to fail at all --- comparing a table against
        // itself is the failure this repository has recorded from the other
        // direction. What it does not cover is a *tenth* row added to the
        // production table, which nothing here sets and so nothing here sees;
        // RFC 5280 closes the domain at nine, so that is a bound rather than a
        // gap.
        let all = bits.iter().fold(
            Default::default(),
            |set: der::flagset::FlagSet<KeyUsages>, (bit, _)| set | *bit,
        );
        let blob = cms_blob_with(
            "Signer",
            "Signer",
            &[1],
            &[1],
            vec![extension(
                "2.5.29.15",
                true,
                KeyUsage(all).to_der().expect("der"),
            )],
        );
        let named: Vec<String> = bits.iter().map(|(_, name)| name.to_string()).collect();
        assert_eq!(
            parse_certificate(&blob)
                .expect("a certificate")
                .key_usage
                .as_deref(),
            Some(named.as_slice())
        );
    }

    /// A certificate with the extension and one without make different claims.
    ///
    /// Absent places **no** limit on the key; empty limits it to **nothing**.
    /// Collapsing them onto one value --- an empty list, or a `None` --- makes
    /// the dialog say the same thing about two certificates that say opposite
    /// things, and the direction it would fall is the reassuring one.
    #[test]
    fn a_certificate_stating_no_usage_and_one_stating_none_are_told_apart() {
        use der::Encode as _;
        use x509_cert::ext::pkix::KeyUsage;

        let read = |blob: Vec<u8>| parse_certificate(&blob).expect("a certificate");

        let silent = read(cms_blob("Signer", "Signer", &[1], &[1]));
        assert_eq!(silent.key_usage, None, "no extension: no limit stated");
        assert_eq!(silent.extended_usage, None);
        assert_eq!(silent.authority, None, "and no basic constraints either");

        let empty = read(cms_blob_with(
            "Signer",
            "Signer",
            &[1],
            &[1],
            vec![extension(
                "2.5.29.15",
                true,
                KeyUsage(Default::default()).to_der().expect("der"),
            )],
        ));
        assert_eq!(
            empty.key_usage,
            Some(Vec::new()),
            "an extension naming nothing limits the key to nothing"
        );
        assert_eq!(empty.extensions_unread, 0, "and it read perfectly well");
    }

    /// A purpose nobody here has heard of is reported, not dropped.
    ///
    /// Adobe's own signing purposes are outside RFC 5280, and so is every
    /// enterprise arc; a reader shown *"Email protection"* for a certificate
    /// that also names something else has been told the issuer named one
    /// purpose. Dotted digits are ugly and true.
    #[test]
    fn an_extended_usage_this_module_cannot_name_is_shown_as_its_oid() {
        use der::Encode as _;
        use x509_cert::ext::pkix::ExtendedKeyUsage;

        let purposes = ExtendedKeyUsage(vec![
            "1.3.6.1.5.5.7.3.4".parse().expect("email protection"),
            "1.2.840.113583.1.1.5".parse().expect("an adobe arc"),
        ]);
        let blob = cms_blob_with(
            "Signer",
            "Signer",
            &[1],
            &[1],
            vec![extension(
                "2.5.29.37",
                false,
                purposes.to_der().expect("der"),
            )],
        );

        let certificate = parse_certificate(&blob).expect("a certificate");
        assert_eq!(
            certificate.extended_usage.as_deref(),
            Some(
                &[
                    "Email protection".to_string(),
                    "1.2.840.113583.1.1.5".to_string()
                ][..]
            )
        );
    }

    /// A certificate saying it may issue others says so.
    ///
    /// Rare on a signer and worth surfacing when it happens, because a
    /// certificate that is both the signer and an authority is one nobody
    /// vouched for. It is still not a verdict --- see the exhaustive-match test.
    #[test]
    fn a_certificate_claiming_to_be_an_authority_is_reported_as_one() {
        use der::Encode as _;
        use x509_cert::ext::pkix::BasicConstraints;

        let blob = cms_blob_with(
            "Signer",
            "Signer",
            &[1],
            &[1],
            vec![extension(
                "2.5.29.19",
                true,
                BasicConstraints {
                    ca: true,
                    path_len_constraint: None,
                }
                .to_der()
                .expect("der"),
            )],
        );

        assert_eq!(
            parse_certificate(&blob).expect("a certificate").authority,
            Some(true)
        );
    }

    /// An extension that will not decode is counted, not read as absent.
    ///
    /// This is the failure with a reassuring shape: a malformed key usage
    /// reported as `None` reads as *"the issuer placed no limit"*, which is a
    /// claim the certificate does not make. The fixture puts basic constraints
    /// under the key usage OID, so the bytes are real DER of the wrong type ---
    /// which is what a producer bug actually looks like, and is not something a
    /// length check would catch.
    #[test]
    fn an_extension_that_will_not_decode_is_counted_rather_than_read_as_absent() {
        use der::Encode as _;
        use x509_cert::ext::pkix::BasicConstraints;

        let wrong_type = BasicConstraints {
            ca: true,
            path_len_constraint: None,
        }
        .to_der()
        .expect("der");
        let blob = cms_blob_with(
            "Signer",
            "Signer",
            &[1],
            &[1],
            vec![extension("2.5.29.15", true, wrong_type)],
        );

        let certificate = parse_certificate(&blob).expect("the certificate still parses");
        assert_eq!(certificate.key_usage, None, "nothing could be read");
        assert_eq!(
            certificate.extensions_unread, 1,
            "and the reader is told, which is the whole difference between \
             this and a certificate that states no usage"
        );
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

    /// The pair that makes a check of [`ber::to_definite_length`] discriminate.
    ///
    /// `incr-ber.pdf` is `incr-signed.pdf` with every constructed value in its
    /// signature blob rewritten in BER's indefinite form and **nothing else**
    /// changed --- the file is the same length, byte for byte identical outside
    /// the `/Contents` span. So two blobs that come out equal can only have
    /// done so by the length form being normalised away.
    #[test]
    fn a_ber_blob_and_its_der_twin_arrive_identical() {
        let (Ok(der), Ok(ber)) = (
            std::fs::read("../testdata/incr-signed.pdf"),
            std::fs::read("../testdata/incr-ber.pdf"),
        ) else {
            println!("[SKIP] incr-signed.pdf / incr-ber.pdf: not generated");
            return;
        };
        assert_ne!(der, ber, "the fixtures must differ, or this tests nothing");
        assert_eq!(
            timestamped_blob(&der),
            timestamped_blob(&ber),
            "the same signature written two ways must read as the same bytes"
        );
    }

    /// The same document, read the whole way through rather than as bytes.
    #[test]
    fn a_ber_signature_names_the_same_signer_as_its_der_twin() {
        let read = |name: &str| -> Option<Properties> {
            let bytes = std::fs::read(std::path::Path::new("../testdata").join(name)).ok()?;
            scan(&bytes, 1).ok()
        };
        let (Some(der), Some(ber)) = (read("incr-signed.pdf"), read("incr-ber.pdf")) else {
            println!("[SKIP] incr-signed.pdf / incr-ber.pdf: not generated");
            return;
        };
        let named = |properties: &Properties| {
            properties
                .signatures
                .first()
                .and_then(|signature| signature.certificate.as_ref())
                .map(|certificate| certificate.subject_cn.clone())
        };
        assert!(named(&der).is_some(), "the DER twin must name a signer");
        assert_eq!(named(&der), named(&ber));
        assert_eq!(der.limits.certificates_unread, 0);
        assert_eq!(ber.limits.certificates_unread, 0);
    }

    /// A blob that is already DER must survive the walk untouched, because it
    /// now goes through it on the way to every parser here.
    ///
    /// The old trailing-zero scan is the oracle, and it is a fair one *for
    /// these fixtures*: every one ends in a non-zero byte, which is the case
    /// the scan gets right. It is not a fair one in general, which is why it
    /// was replaced.
    #[test]
    fn a_der_signature_blob_is_returned_byte_for_byte() {
        let names = [
            "incr-signed.pdf",
            "incr-timestamped.pdf",
            "incr-two-signers.pdf",
        ];
        let mut examined = 0;
        let mut files = 0;
        for name in names {
            let Ok(bytes) = std::fs::read(std::path::Path::new("../testdata").join(name)) else {
                continue;
            };
            files += 1;
            let document = Document::load_mem(&bytes).expect("the fixture must parse");
            for field in document
                .objects
                .values()
                .filter_map(|object| object.as_dict().ok())
                .filter(|dict| dict.has(b"ByteRange"))
            {
                let raw = field
                    .get(b"Contents")
                    .ok()
                    .and_then(|object| resolve(&document, object).as_str().ok())
                    .expect("a signed field carries its blob");
                let last = raw
                    .iter()
                    .rposition(|byte| *byte != 0)
                    .expect("not a placeholder");
                let mut unread = 0;
                let read = signature_contents(&document, field, MAX_SIG_BLOB, &mut unread)
                    .expect("a walkable blob");
                assert_eq!(read, raw[..=last], "{name}: DER must come back unchanged");
                assert_eq!(unread, 0);
                examined += 1;
            }
        }
        if none_generated(&names) {
            println!("[SKIP] no signed fixture is generated here (BUILD.md, Test fixtures)");
            return;
        }
        assert_eq!(
            files, names.len(),
            "every signed fixture that exists must be read --- generate testdata/ (BUILD.md, Test fixtures)"
        );
        assert!(
            examined > files,
            "incr-two-signers.pdf carries two signatures"
        );
    }
}
