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
use crate::trust::{Doubt, Standing, Store};

/// The schema number every document carries.
pub const SCHEMA: u32 = 1;

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
    /// one signature, every signature `intact` and `trusted`. Present whether
    /// or not `--strict` was given.
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
    /// The properties dialog's Trust row, word for word.
    pub sentence: String,
}

/// `tpdf sign --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
