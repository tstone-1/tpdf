//! What `--json` prints: the command-line tool's stable output.
//!
//! **Schema 1.** Every document carries `"schema": 1` and `"command"`. A field
//! is added without changing the number; one renamed, removed or changed in
//! meaning moves it. `README.md`'s *Command-line tool* section is the reader's
//! description of every key, and `cli::tests::every_json_key_is_described_in_the_readme`
//! holds the two together; `src-tauri/testdata/cli/*.json` are the committed
//! samples, regenerated with `TPDF_CLI_SAMPLES=write` and compared byte for byte
//! on every `cargo test` --- so a change to any shape here is a change to a
//! committed file, visible in review, and never a silent one.
//!
//! Enumerations are the backend's own serde spellings (`integrity::Verdict`,
//! `trust::Standing`, ...), the same strings the application's frontend reads,
//! so a script and the properties dialog see one vocabulary.

use serde::{Deserialize, Serialize};

use crate::integrity::{Verdict, Why};
use crate::revocation::chain::End as ChainEnd;
use crate::revocation::{Basis, Gap, Reason, Source, Status};
use crate::trust::{Doubt, Standing, Store};

/// The schema number every document carries.
pub const SCHEMA: u32 = 1;

/// One page rendered to PNG. Page numbers are one-based.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rendered {
    pub schema: u32,
    pub command: String,
    pub input: String,
    pub output: String,
    pub page: u32,
    pub dpi: u32,
    pub width_px: u32,
    pub height_px: u32,
}

/// An edit plan, either validated without writing or successfully published.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edited {
    pub schema: u32,
    pub command: String,
    pub input: String,
    pub output: String,
    pub written: bool,
    pub operations: usize,
    pub pages: Vec<crate::render::PageSize>,
    /// Newly added annotations remaining after all operations, including undo.
    pub annotations: usize,
    pub signatures_invalidated: usize,
    pub signatures_unknown: bool,
    /// The font each `replace_text` that named one was set in, in plan order.
    pub fonts: Vec<FontUsed>,
}

/// What a `replace_text` operation's `font` came to on this document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontUsed {
    /// The operation's place in the plan, counted from one.
    pub operation: usize,
    /// The editor's name for the font, with its reason when it is a fallback.
    pub font: String,
}

/// Original annotations. Pages are one-based, unlike the internal scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comments {
    pub schema: u32,
    pub command: String,
    pub input: String,
    pub complete: bool,
    pub comments: Vec<crate::annots::Comment>,
    pub limits: crate::annots::Limits,
}

/// Editable text and the revision callers must return with a replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRuns {
    pub schema: u32,
    pub command: String,
    pub input: String,
    /// One-based page in the input file.
    pub page: u32,
    pub revision: Vec<u8>,
    pub runs: Vec<crate::textedit::Run>,
}

/// Machine-readable command discovery, `help --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Help {
    /// Schema version.
    pub schema: u32,
    /// Always `help`.
    pub command: String,
    /// Application version.
    pub version: String,
    /// Every registered command, in help order.
    pub commands: Vec<Command>,
}

/// One command in discovery output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// Subcommand name.
    pub name: String,
    /// Arguments accepted by the command, as shown in help.
    pub usage: String,
    /// Human-readable description.
    pub summary: String,
}

/// A command that failed before it had a more detailed report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failed {
    /// Schema version.
    pub schema: u32,
    /// The requested command, including an unknown command on a usage error.
    pub command: String,
    /// Machine-readable category, exit status and human-readable explanation.
    pub error: CommandError,
}

/// An error shared by command failures and partially published page operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandError {
    /// `usage`, `refused`, or `failed`.
    pub kind: String,
    /// The process exit status.
    pub exit_code: i32,
    /// Human-readable explanation; scripts should branch on kind/exit_code.
    pub message: String,
}

impl CommandError {
    pub(super) fn from_failure(failure: &super::Failure) -> Self {
        Self {
            kind: match failure.exit {
                super::Exit::Usage => "usage",
                super::Exit::Refused => "refused",
                _ => "failed",
            }
            .into(),
            exit_code: failure.exit.code(),
            message: failure.message.clone(),
        }
    }
}

/// A merge, extract, split, rotation or crop, including partial publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pages {
    /// Schema version.
    pub schema: u32,
    /// The requested operation.
    pub command: String,
    /// Source paths, in command-line order.
    pub inputs: Vec<String>,
    /// True only when all requested files were published.
    pub complete: bool,
    /// Files actually published, in order. Empty on a pre-publication failure.
    pub outputs: Vec<PageOutput>,
    /// Number of source signatures affected by this rewrite.
    pub signatures_invalidated: usize,
    /// Signature enumeration was incomplete in at least one input.
    pub signatures_unknown: bool,
    /// Null on success; otherwise describes the failure after partial publication.
    pub error: Option<CommandError>,
}

/// One completed output file of a page operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageOutput {
    /// Destination as derived from the command line.
    pub path: String,
    /// Number of pages checked in a fresh worker before publication.
    pub pages: usize,
}

/// `tpdf identities --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identities {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"identities"`.
    pub command: String,
    /// Certificates with a key that may sign now.
    pub usable: Vec<Usable>,
    /// Certificates with a key that may not, each with the reason.
    pub not_usable: Vec<NotUsable>,
}

/// A certificate that may sign: `sign_cms::Choice`, under the names this
/// schema uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usable {
    /// SHA-256 of the certificate, lowercase hex: what `--identity` accepts.
    pub id: String,
    /// SHA-1 of the certificate, lowercase hex: the thumbprint Windows shows
    /// for it. Also accepted by `--identity`.
    pub sha1: String,
    /// The subject's common name, or its whole name when it has none. Also
    /// accepted by `--identity`.
    pub subject: String,
    /// The issuer's common name, or its whole name.
    pub issuer: String,
    /// When it stops being valid, `YYYY-MM-DD HH:MM:SS UTC`.
    pub expires: String,
    /// `RSA 3072`, `ECDSA P-256`.
    pub method: String,
}

/// A certificate with a key that may not sign, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotUsable {
    /// SHA-256 of the certificate, lowercase hex.
    pub id: String,
    /// SHA-1 of the certificate, lowercase hex: the thumbprint Windows shows
    /// for it.
    pub sha1: String,
    /// Who it names, as far as it could be read.
    pub subject: String,
    /// Why it is not offered, as a clause: "it has expired".
    pub why: String,
}

/// `tpdf verify --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"verify"`.
    pub command: String,
    /// Whether `--strict` would pass: every document read, each with at least
    /// one signature, every signature `intact`, `trusted` or
    /// `trusted_at_timestamp`, and none whose revocation, or whose chain's,
    /// is `revoked` other than after an attested moment. Present whether or
    /// not `--strict` was given.
    pub strict_passed: bool,
    /// One entry per document, in the order given.
    pub files: Vec<File>,
}

/// One document `verify` was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    /// The path as given on the command line.
    pub path: String,
    /// Why the document's signatures could not be read; `null` when they were.
    pub error: Option<FileError>,
    /// Every signed signature field, in the document's order. Empty fields
    /// waiting for a signature are left out.
    pub signatures: Vec<Signature>,
}

/// Why a document could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileError {
    /// `unreadable` (the file could not be opened or is empty), `refused` (the
    /// worker read it and said it is not a document it can read), `locked`
    /// (encrypted, and no password is taken), or `failed` (tpdf's own
    /// failure: the worker did not answer).
    pub kind: ErrorKind,
    /// The sentence, as stderr prints it.
    pub message: String,
}

/// [`FileError::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The file could not be opened, or is empty.
    Unreadable,
    /// The worker read it and refused it.
    Refused,
    /// Encrypted with a password, which the command line does not take.
    Locked,
    /// tpdf's own failure.
    Failed,
}

/// One signature, as the properties dialog reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// True for an `ETSI.RFC3161` document timestamp, including an archive
    /// timestamp; false for a document signature, even one carrying a timestamp.
    pub document_timestamp: bool,
    /// The field's fully qualified name.
    pub field: String,
    /// Whom the signing certificate names (its common name, or its whole
    /// name), or empty when no certificate could be read.
    pub signer: String,
    /// Who issued that certificate, likewise.
    pub issuer: String,
    /// `/M`, the signing time the signer's own machine claimed. Not checked.
    pub claimed_time: String,
    /// Whether the signed range reaches the file's last byte.
    pub covers_whole_file: bool,
    /// How many bytes were written after the signed range ends.
    pub appended_bytes: u64,
    /// Whether the signature still covers the bytes it was made over.
    pub integrity: IntegrityReport,
    /// Whether the OS trust store vouches for the signer; `null` unless
    /// `integrity.verdict` is `intact` or `weak`.
    pub trust: Option<TrustReport>,
    /// The RFC 3161 timestamp the signature carries, or that a document
    /// timestamp is; `null` for a signature with none. Added to schema 1 on
    /// 2026-09-28, which the schema's own rule permits: a new key.
    pub timestamp: Option<TimestampReport>,
    /// What the document's own revocation data says about the signer's
    /// certificate (a document timestamp's: its authority's); `null` exactly
    /// when `trust` is. Added to schema 1 on 2026-09-28, a new key.
    pub revocation: Option<RevocationReport>,
    /// What the same data says about every certificate from the signer's up
    /// to its root; `null` exactly when `revocation` is. Added to schema 1 on
    /// 2026-09-28, a new key.
    pub revocation_chain: Option<ChainReport>,
}

/// `docinfo::Timestamp`, with the sentences the application shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimestampReport {
    /// The time the token states, `YYYY-MM-DD HH:MM:SS UTC`. **Attested only
    /// when [`TimestampReport::attested`] is true.**
    pub time: String,
    /// The authority, from its certificate (common name, or whole name), or
    /// empty when the token carries none.
    pub authority: String,
    /// Whether the time is attested: the token checks out and covers what it
    /// is attached to (`integrity.verdict` is `intact` or `weak`).
    pub attested: bool,
    /// Whether the token is sound and covers this signature, or for a
    /// document timestamp the signed bytes; `sentence` is the properties
    /// dialog's Timestamped row, word for word.
    pub integrity: IntegrityReport,
    /// Whether the OS trust store vouches for the authority, for timestamping;
    /// `null` unless `attested`. `sentence` is the dialog's Timestamp
    /// authority row.
    pub trust: Option<TrustReport>,
    /// What the document's revocation data says about the authority's
    /// certificate, at the time the token states; `null` exactly when `trust`
    /// is. Added 2026-09-28.
    pub revocation: Option<RevocationReport>,
    /// The same, for every certificate from the authority's up to its root;
    /// `null` exactly when `revocation` is. Added 2026-09-28.
    pub revocation_chain: Option<ChainReport>,
}

/// `integrity::Integrity`, with the sentence the application shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    /// `intact`, `weak`, `altered`, `broken` or `unchecked`.
    pub verdict: Verdict,
    /// Why nothing was concluded, for `unchecked`; `null` otherwise.
    pub why: Option<Why>,
    /// `SHA-256`, or empty.
    pub digest: String,
    /// `RSA`, `RSA-PSS`, `ECDSA P-256`, or empty.
    pub method: String,
    /// The properties dialog's Integrity row, word for word.
    pub sentence: String,
}

/// `trust::Trust`, with the sentence the application shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustReport {
    /// `trusted`, `expired`, `not_yet_valid`, `untrusted` or `unchecked`.
    pub standing: Standing,
    /// Why not trusted, for `untrusted` and `unchecked`; `null` otherwise.
    pub why: Option<Doubt>,
    /// `mac` or `windows`: whose store answered; `null` when none did.
    pub store: Option<Store>,
    /// The moment the chain was judged at, `YYYY-MM-DD HH:MM:SS UTC`, when it
    /// was the time a trusted timestamp attests; empty when it was judged
    /// now. Added 2026-09-28.
    pub attested_at: String,
    /// The properties dialog's Trust row, word for word.
    pub sentence: String,
}

/// `revocation::Revocation`, with the sentence the application shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevocationReport {
    /// `good`, `revoked`, `unknown`, `none` (the document carries nothing
    /// about the certificate: **not checked**) or `unchecked`.
    pub standing: Status,
    /// Why nothing was concluded, for `unchecked`; `null` otherwise.
    pub why: Option<Gap>,
    /// `ocsp` or `crl`: which data answered, for `good`, `revoked` and
    /// `unknown`; `null` otherwise.
    pub source: Option<Source>,
    /// When that data was issued, or empty.
    pub issued: String,
    /// When its issuer promised the next, or empty.
    pub next: String,
    /// When the certificate was revoked, for `revoked`; empty otherwise.
    pub revoked: String,
    /// The reason a revocation states, or `null`.
    pub reason: Option<Reason>,
    /// Whose clock `moment` is: `attested` (a trusted timestamp's),
    /// `stated` (a token's own, for its authority), `claimed` (the signer's
    /// `/M`) or `now`.
    pub basis: Basis,
    /// The moment judged, `YYYY-MM-DD HH:MM:SS UTC`.
    pub moment: String,
    /// `revoked` after an attested moment, which does not undo the signature.
    pub after_moment: bool,
    /// The properties dialog's Revocation row, word for word.
    pub sentence: String,
}

/// `revocation::chain::Chain`, with the sentence the application shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainReport {
    /// The chain's answer: `revoked` when any certificate on it is (named by
    /// `decided_by`), `good` when every one is, otherwise the most telling of
    /// `unknown`, `unchecked`, `none` and a revocation after the moment.
    pub standing: Status,
    /// `revoked` after an attested moment, which does not undo the signature.
    pub after_moment: bool,
    /// The index in `certificates` of the certificate that decides
    /// `standing`, or `null` when every one is `good` or the bound decided it.
    pub decided_by: Option<usize>,
    /// Certificates on the chain past the ones tpdf judges, not judged.
    pub dropped: usize,
    /// How the chain ended: `root`, `no_issuer` (the next certificate up is
    /// not in the document) or `loop`.
    pub end: ChainEnd,
    /// Every certificate judged, the leaf first; roots and certificates that
    /// need no check are not listed.
    pub certificates: Vec<ChainCertificate>,
    /// The properties dialog's Chain revocation row, word for word.
    pub sentence: String,
}

/// One certificate on a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainCertificate {
    /// Whom it names (its common name, or its whole name).
    pub subject: String,
    /// Its serial number, uppercase hex.
    pub serial: String,
    /// What the document's data says about it, as `revocation` above.
    pub revocation: RevocationReport,
}

/// Where a visible signature was drawn. Every rectangle is `[x, y, w, h]` in
/// points from the top-left corner of the page as it is displayed: the space
/// `sign --rect` is given in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    /// The page, counted from 1.
    pub page: u32,
    /// The signature's rectangle, as `--rect` gave it.
    pub rect: [f64; 4],
    /// The image, or `null` when none was drawn.
    pub image: Option<[f64; 4]>,
    /// The type size of the lines, in points; 0 when no line was drawn.
    pub font_size: f64,
    /// The lines, top to bottom.
    pub lines: Vec<DrawnLine>,
}

/// One line of a visible signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawnLine {
    /// The words drawn.
    pub text: String,
    /// The box no ink of the line leaves: the words' width with a small
    /// bearing each side, and the font's whole height above and below.
    pub rect: [f64; 4],
    /// Where the line's baseline is, measured down from the top of the page.
    pub baseline: f64,
}

/// `tpdf sign --json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signed {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"sign"`.
    pub command: String,
    /// The document signed, as given.
    pub input: String,
    /// The signed copy, as given.
    pub output: String,
    /// The new signature's field.
    pub field: String,
    /// The certificate it was signed with.
    pub identity: Usable,
    /// Whether it has an appearance on a page.
    pub visible: bool,
    /// Where that appearance was drawn; `null` for an invisible signature.
    pub appearance: Option<Appearance>,
    /// Every signature in the written file, as a worker read it back after
    /// writing --- the new one included, named by [`Signed::field`].
    pub signatures: Vec<Signature>,
    /// The signing panel's closing sentence, word for word.
    pub summary: String,
}

/// `tpdf info --json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"info"`.
    pub command: String,
    /// One entry per document, in the order given.
    pub files: Vec<Described>,
}

/// One document `info` was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Described {
    /// The path as given on the command line.
    pub path: String,
    /// Why the document could not be described; `null` when it was. `locked`
    /// is a document that needs a password it was not given, and is reported
    /// rather than failed: `info` still exits 0 for it.
    pub error: Option<FileError>,
    /// What the document says about itself; `null` exactly when `error` is not.
    pub document: Option<Document>,
}

/// What the properties dialog shows, as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// `1.7`: the header's, or the catalog's `/Version` where that is later.
    pub version: String,
    /// The file's length.
    pub bytes: u64,
    /// How many pages, as PDFium counts them.
    pub pages: u32,
    /// Each distinct displayed page size, in the order it first occurs.
    pub page_sizes: Vec<PageSize>,
    /// One, plus one per incremental update.
    pub revisions: usize,
    /// The `/Info` dictionary, the keys PDF defines first in their order, then
    /// the document's own.
    pub metadata: Vec<crate::docinfo::Field>,
    /// `/Lang`, or empty.
    pub language: String,
    /// `null` for a document that is not encrypted.
    pub encryption: Option<crate::docinfo::Encryption>,
    /// Whether it has a structure tree; `null` when that could not be asked.
    pub tagged: Option<bool>,
    /// What its XMP metadata claims; `null` when it carries none.
    pub conformance: Option<Conformance>,
    /// Embedded files; `null` when they could not be counted.
    pub attachments: Option<usize>,
    /// Its interactive form.
    pub form: Form,
    /// Every signed signature field, in the document's order: the objects
    /// `verify` prints.
    pub signatures: Vec<Signature>,
    /// Signature fields waiting for a signature.
    pub unsigned_signature_fields: usize,
    /// What could not be read, so nothing above is silently partial.
    pub limits: crate::docinfo::Limits,
}

/// Pages of one displayed size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageSize {
    /// Width as displayed, points, to two decimals.
    pub width_pt: f64,
    /// Height as displayed, points, to two decimals.
    pub height_pt: f64,
    /// How many pages have it.
    pub count: usize,
}

/// A conformance claim. Claimed, never checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conformance {
    /// `PDF/A-3B`, `PDF/UA-1`, sorted.
    pub claimed: Vec<String>,
    /// The XMP packet was there and could not all be read.
    pub unread: bool,
}

/// The document's interactive form, as the application's form filling reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    /// Whether tpdf could read the form; `false` with `why` when it refused.
    pub readable: bool,
    /// Fields with at least one widget on a page. `0` when there is no form.
    pub fields: usize,
    /// Widgets on pages; a field may have several.
    pub widgets: usize,
    /// Whether the form is XFA, which tpdf does not read.
    pub xfa: bool,
    /// Why the form could not be read; `null` when it could.
    pub why: Option<String>,
}

/// `tpdf text --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"text"`.
    pub command: String,
    /// The document, as given.
    pub path: String,
    /// The pages asked for, in document order.
    pub pages: Vec<PageText>,
}

/// One page's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageText {
    /// The page, counted from 1.
    pub page: u32,
    /// How the order was decided.
    pub order: Order,
    /// Whether the document says what its characters mean.
    pub encoding: Encoding,
    /// The page's lines in reading order, joined by `\n`.
    pub text: String,
}

/// [`PageText::order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// The document's own tags, which cover every visible character.
    Tagged,
    /// Recovered from where the characters sit, as the viewer recovers it.
    Geometric,
    /// The page has no characters to order.
    None,
}

/// [`PageText::encoding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// Every font on the page states what its characters mean.
    Stated,
    /// Some font does not, and PDFium guessed: some of the text may be noise.
    Guessed,
    /// The fonts could not all be examined.
    Unknown,
}

/// `tpdf fields --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fields {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"fields"`.
    pub command: String,
    /// The document, as given.
    pub path: String,
    /// Every field with a widget on a page, in the order its first widget is
    /// met: page by page, and within a page in the order of `/Annots`.
    pub fields: Vec<Field>,
}

/// One field, as `fill` addresses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// The fully qualified name --- its ancestors' `/T` and its own, joined by
    /// periods: the key `fill` takes.
    pub name: String,
    /// What kind of answer it takes.
    pub kind: FieldKind,
    /// Its current answer, in the form `fill` takes one: a string for text, a
    /// boolean for a checkbox, an export value or `null` for a radio group or a
    /// single choice, an array of export values for a multiple-selection list;
    /// `null` for `other`.
    pub value: serde_json::Value,
    /// A radio group's states or a choice's options, in the document's order;
    /// empty for the other kinds.
    pub options: Vec<FieldOption>,
    /// A list that allows several selections.
    pub multiple: bool,
    /// A dropdown that also takes text of its own (`/Ff` Edit).
    pub custom_text: bool,
    /// A text field that takes several lines.
    pub multiline: bool,
    /// `/MaxLen`, in characters; `null` when there is none.
    pub max_length: Option<usize>,
    /// The pages its widgets are on, counted from 1, each once.
    pub pages: Vec<u32>,
    /// How many widgets it has.
    pub widgets: usize,
    /// Whether `fill` can answer it.
    pub editable: bool,
    /// Why not, when it cannot; `null` when it can.
    pub not_editable: Option<NotEditable>,
    /// The sentence for `not_editable`, as the application shows it; `null`
    /// when it is editable.
    pub why: Option<String>,
}

/// [`Field::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// A text field.
    Text,
    /// A checkbox.
    Checkbox,
    /// A group of radio buttons.
    Radio,
    /// A dropdown.
    ChoiceCombo,
    /// A list box.
    ChoiceList,
    /// Anything else: a signature field, a push button. Never editable.
    Other,
}

/// One option of a choice, or one state of a radio group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldOption {
    /// What `fill` takes and the document stores.
    pub export: String,
    /// What is shown. A radio state's label is its export value.
    pub label: String,
    /// Whether it is chosen now. The one place two options sharing an export
    /// value can be told apart, since `value` names the export value only.
    pub selected: bool,
}

/// [`Field::not_editable`]: the reasons the application's form filling gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotEditable {
    /// `/Ff` ReadOnly.
    ReadOnly,
    /// A password text field.
    Password,
    /// A file-select text field.
    FileSelect,
    /// A comb text field.
    Comb,
    /// A rich-text text field.
    RichText,
    /// Its widget is hidden.
    Hidden,
    /// A kind tpdf does not fill: a signature field, a push button.
    Unsupported,
    /// Anything else, such as radio buttons whose appearances do not say which
    /// state is which; `why` says what.
    Other,
}

/// `tpdf fill --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filled {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"fill"`.
    pub command: String,
    /// The document filled, as given. It is never written.
    pub input: String,
    /// The filled copy, as given.
    pub output: String,
    /// Whether the filled copy was written and kept.
    pub written: bool,
    /// Why not, every reason at once: the answers `fill` refused, or those
    /// that did not read back as given. Empty when `written`.
    pub problems: Vec<Problem>,
    /// Every field answered, as a worker read it back from the written file;
    /// empty unless `written`.
    pub fields: Vec<FilledField>,
}

/// One answer `fill` refused, or one that did not read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// The name as the answers give it.
    pub field: String,
    /// Which rule.
    pub problem: ProblemKind,
    /// The sentence, as stderr prints it.
    pub why: String,
}

/// [`Problem::problem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// No field has this name.
    Unknown,
    /// Several fields have this name, or several options this export value.
    Ambiguous,
    /// The field cannot be answered; `why` says which reason `fields` gives.
    NotEditable,
    /// The answer is the wrong JSON type for the field.
    Type,
    /// It names no option of the field, or more than the field takes.
    Option,
    /// Characters Helvetica cannot draw: tpdf writes each answer's appearance
    /// in Helvetica with WinAnsiEncoding, Western European text.
    Characters,
    /// Longer than the field's `max_length`, or than 16 KB.
    Length,
    /// A line break in a field that takes one line.
    Line,
    /// It does not fit visibly in the field.
    Layout,
    /// It was written, and the written file does not say it.
    ReadBack,
}

/// One answered field, read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilledField {
    /// Its fully qualified name.
    pub name: String,
    /// As [`Field::kind`].
    pub kind: FieldKind,
    /// As [`Field::value`], read from the written file.
    pub value: serde_json::Value,
}

/// `tpdf redact --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redacted {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"redact"`.
    pub command: String,
    /// The document, as given. It is never written.
    pub input: String,
    /// The redacted copy, as given; `null` for a dry run given no `-o`.
    pub output: Option<String>,
    /// `--dry-run`: nothing was or would have been written by this run.
    pub dry_run: bool,
    /// Whether the redacted copy was written and kept.
    pub written: bool,
    /// `true` only when every check proved the written copy clean; `false`
    /// with every reason in [`Redacted::reasons`]; `null` when nothing was
    /// written.
    pub verified: Option<bool>,
    /// Why the copy could not be proved clean, one sentence each --- or, for a
    /// dry run, what the removal will not be able to take. Empty when
    /// `verified` is `true`.
    pub reasons: Vec<String>,
    /// The sentence the application shows after a redaction, word for word;
    /// `null` unless written.
    pub summary: Option<String>,
    /// How many regions were marked.
    pub regions: usize,
    /// How many distinct removals they take: text-showing operations, text
    /// inside forms, and image draws.
    pub removals: usize,
    /// Signatures the input carried, every one of which the rewrite
    /// invalidates; nonzero only with `--invalidate-signatures`.
    pub signatures_invalidated: usize,
    /// Each `--text` and `--pattern`, in the order given, with how many times
    /// it matched on the pages searched.
    pub searches: Vec<Search>,
    /// Every page that has a match or a region, in page order.
    pub pages: Vec<RedactedPage>,
}

/// One `--text` or `--pattern`, and what it found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    /// Which option gave it.
    pub kind: SearchKind,
    /// The query, as given.
    pub query: String,
    /// How many times it matched.
    pub matches: usize,
}

/// `tpdf search --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Searched {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// Always `"search"`.
    pub command: String,
    /// Every `--text`, then every `--pattern`; a [`Hit::query`] indexes this.
    pub queries: Vec<Query>,
    /// The documents, in the order given.
    pub files: Vec<SearchedFile>,
}

/// One `--text` or `--pattern` of a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Which option gave it.
    pub kind: SearchKind,
    /// The query, as given.
    pub query: String,
}

/// One document `search` was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchedFile {
    /// The path as given on the command line.
    pub path: String,
    /// Why the document could not be searched; `null` when it was.
    pub error: Option<FileError>,
    /// How many pages were searched.
    pub pages_searched: u32,
    /// The pages, counted from 1, that held no text at all: a scan has
    /// nothing to search, which is not the same as holding no match.
    pub pages_without_text: Vec<u32>,
    /// Every match, by page and then by where on the page it starts.
    pub matches: Vec<Hit>,
}

/// One match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// The page it starts on, counted from 1.
    pub page: u32,
    /// The page it ends on, when a phrase runs over a page break; else `null`.
    pub end_page: Option<u32>,
    /// Which of [`Searched::queries`] found it, counted from 0.
    pub query: usize,
    /// The text before it, whitespace collapsed.
    pub before: String,
    /// The matched text, as the page spells it.
    pub hit: String,
    /// The text after it, whitespace collapsed.
    pub after: String,
}

/// [`Search::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    /// `--text`: the viewer's find, literally.
    Text,
    /// `--pattern`: the viewer's find with *Regular expression* on.
    Pattern,
}

/// One page of a redaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedPage {
    /// Counted from 1.
    pub page: u32,
    /// Each match that starts on this page, as the page spells it.
    pub hits: Vec<String>,
    /// Regions marked on it: one per run of a match on a line, plus each
    /// `--regions` rectangle.
    pub regions: usize,
    /// Text-showing operations in the page's content that go.
    pub text_removals: usize,
    /// Text-showing operations inside forms the page draws that go.
    pub form_text_removals: usize,
    /// Image draws that go.
    pub image_removals: usize,
    /// What the removed operations draw, one string per region that took any
    /// --- often more than the match, because a whole operation goes.
    pub taking: Vec<String>,
    /// What the removal cannot take, one sentence each.
    pub left: Vec<String>,
}
