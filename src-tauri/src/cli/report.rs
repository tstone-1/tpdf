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
