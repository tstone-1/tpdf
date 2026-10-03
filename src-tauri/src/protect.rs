//! Setting and removing a document's password.
//!
//! A rewrite puts back the encryption the source had (`save.rs`, `checked` and
//! `rewrite`). This module is the two other answers: write the copy with a new
//! password, or with none.
//!
//! **A new password is AES-256, the PDF 2.0 handler (`V 5`, `R 6`).** One
//! password, used as both the user and the owner password, with nothing
//! restricted: it decides who can open the file, and whoever can open it can do
//! everything with it. Permission bits are not offered because no reader is
//! bound by them, tpdf included.
//!
//! **A password is removed only from a document that needs one to open.** A
//! file that opens without a prompt and carries restrictions is somebody
//! else's statement about their document, and it is not tpdf's to drop on the
//! word of a reader who was never asked for a password.

use std::collections::BTreeMap;
use std::sync::Arc;

use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionState, EncryptionVersion, Permissions};
use serde::{Deserialize, Serialize};

/// The longest password, in UTF-8 bytes. Revision 6 reads no more than this,
/// so a longer one would be cut without anybody being told.
pub const MAX_BYTES: usize = 127;

/// What a rewrite does about the document's password.
#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Protection {
    /// What the source had, put back unchanged.
    #[default]
    Keep,
    /// None, for a document a password opened.
    Remove,
    /// This one, whatever the source had.
    Set(String),
}

/// Never the password: a `Plan` is printed by `{:?}` in refusals and tests.
impl std::fmt::Debug for Protection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Protection::Keep => "Keep",
            Protection::Remove => "Remove",
            Protection::Set(_) => "Set(<password>)",
        })
    }
}

/// Whether `password` can be a document's password.
///
/// # Errors
///
/// The sentence for the reader: it is empty, too long, or holds a character a
/// password may not.
pub fn acceptable(password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err(
            "The password is empty; a document protected with it would open for \
                    anybody."
                .into(),
        );
    }
    if password.len() > MAX_BYTES {
        return Err(format!(
            "The password is too long: a PDF password is at most {MAX_BYTES} bytes, and this \
             one is {}.",
            password.len()
        ));
    }
    if password.chars().any(char::is_control) {
        return Err(
            "The password holds a control character, such as a tab or a line break, \
                    which most readers cannot type back."
                .into(),
        );
    }
    Ok(())
}

/// Refuses a removal the document does not support, before anything is built.
///
/// `had` is whether the source was encrypted and `original` its bytes.
///
/// # Errors
///
/// A removal from a document with no encryption, or from one that opens
/// without a password.
pub fn allowed(protection: &Protection, had: bool, original: &[u8]) -> Result<(), String> {
    match protection {
        Protection::Keep => Ok(()),
        Protection::Set(password) => acceptable(password),
        Protection::Remove if !had => {
            Err("This document has no password, so there is none to remove.".into())
        }
        Protection::Remove => {
            // Asked of the bytes rather than of the password that was given:
            // `lopdf` tries the empty password by itself, so a wrong password
            // on a document that opens without one still arrives decrypted.
            let plain = Document::load_mem_with_options(original, lopdf::LoadOptions::default());
            if plain.is_ok_and(|doc| doc.is_encrypted()) {
                Ok(())
            } else {
                Err(
                    "This document opens without a password. What it restricts was set by \
                     whoever made it, and tpdf removes a password only from a document that \
                     needs one to open."
                        .into(),
                )
            }
        }
    }
}

/// The encryption a rewrite ends with: `source`'s, none, or a new one.
///
/// # Errors
///
/// The key could not be made, or `lopdf` refuses the password.
pub fn resolve(
    protection: &Protection,
    source: Option<EncryptionState>,
) -> Result<Option<EncryptionState>, String> {
    match protection {
        Protection::Keep => Ok(source),
        Protection::Remove => Ok(None),
        Protection::Set(password) => state(password).map(Some),
    }
}

/// An AES-256 state for `password`, with a fresh file key.
fn state(password: &str) -> Result<EncryptionState, String> {
    acceptable(password)?;
    let mut key = [0u8; 32];
    getrandom::fill(&mut key)
        .map_err(|e| format!("tpdf could not get random bytes for the encryption key: {e}"))?;
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: password,
        user_password: password,
        permissions: Permissions::all(),
    })
    .map_err(|e| format!("tpdf could not use this password: {e}"))
}

/// Completes what `Document::encrypt` wrote. Call it after every `encrypt`.
///
/// **`lopdf` leaves the key length out of each crypt filter, and CoreGraphics
/// then decrypts nothing.** Measured 2026-10-03 with PDFKit on a rewrite that
/// kept the source's own encryption: the password is accepted, every page
/// reads as empty, and the log says *unsupported crypt filter key length*.
/// `qpdf` and PDFium both assume the length from the method, which is why the
/// rewrite passed its checks for five weeks. The lengths written here are the
/// only ones each method has.
///
/// A new password needs two things more, both of which `qpdf --check` warns
/// about: a file identifier when the source had none, and a header that says
/// PDF 1.7, where a reader starts looking for the revision 6 handler.
///
/// A document that is not encrypted is left alone.
///
/// # Errors
///
/// The encryption dictionary cannot be read back, or no random bytes could be
/// had for the identifier.
pub fn finish(doc: &mut Document, protection: &Protection) -> Result<(), String> {
    let Ok(at) = doc
        .trailer
        .get(b"Encrypt")
        .and_then(lopdf::Object::as_reference)
    else {
        return Ok(());
    };
    let dictionary = doc
        .get_dictionary_mut(at)
        .map_err(|e| format!("tpdf could not complete the encryption it wrote: {e}"))?;
    let version = dictionary.get(b"V").and_then(lopdf::Object::as_i64);
    if version.is_ok_and(|v| v == 5) && !dictionary.has(b"Length") {
        dictionary.set("Length", 256);
    }
    if let Ok(filters) = dictionary
        .get_mut(b"CF")
        .and_then(lopdf::Object::as_dict_mut)
    {
        for (_, filter) in filters.iter_mut() {
            let Ok(filter) = filter.as_dict_mut() else {
                continue;
            };
            // In bytes, as every writer states it: 16 for AES-128, 32 for
            // AES-256. An RC4 filter's length is the document's own and is
            // not guessed.
            let bytes = match filter.get(b"CFM").and_then(lopdf::Object::as_name) {
                Ok(b"AESV2") => 16,
                Ok(b"AESV3") => 32,
                _ => continue,
            };
            if !filter.has(b"Length") {
                filter.set("Length", bytes);
            }
        }
    }
    if !matches!(protection, Protection::Set(_)) {
        return Ok(());
    }
    if doc.version.as_str() < "1.7" {
        doc.version = "1.7".into();
    }
    let identified = doc
        .trailer
        .get(b"ID")
        .and_then(lopdf::Object::as_array)
        .is_ok_and(|both| both.len() == 2 && both.iter().all(|one| one.as_str().is_ok()));
    if !identified {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id)
            .map_err(|e| format!("tpdf could not get random bytes for the file identifier: {e}"))?;
        let one = || lopdf::Object::String(id.to_vec(), lopdf::StringFormat::Hexadecimal);
        doc.trailer.set("ID", vec![one(), one()]);
    }
    Ok(())
}

/// Reads the bytes a rewrite produced and refuses any that are not protected
/// the way `protection` asked.
///
/// `pages` is how many the document was written with.
///
/// # Errors
///
/// A copy that should need its password and opens without it, one that the
/// password does not open, or one that should be plain and is not.
pub fn written_as_asked(protection: &Protection, bytes: &[u8], pages: usize) -> Result<(), String> {
    let load = |password: Option<&str>| {
        Document::load_mem_with_options(
            bytes,
            lopdf::LoadOptions {
                password: password.map(str::to_string),
                ..Default::default()
            },
        )
        .map_err(|e| format!("tpdf could not read back the copy it built: {e}"))
    };
    match protection {
        Protection::Keep => Ok(()),
        Protection::Remove => {
            let plain = load(None)?;
            if plain.is_encrypted() || plain.was_encrypted() || plain.get_pages().len() != pages {
                return Err(
                    "The copy tpdf built is still encrypted, so it was not written.".into(),
                );
            }
            Ok(())
        }
        Protection::Set(password) => {
            // `lopdf` parses no objects for a document it cannot authenticate
            // and still answers `Ok`, so the lock is read from `is_encrypted`.
            if !load(None)?.is_encrypted() {
                return Err(
                    "The copy tpdf built opens without the password, so it was not written.".into(),
                );
            }
            // A password that does not open it is an `Err` from the load.
            let opens = load(Some(password))
                .is_ok_and(|doc| !doc.is_encrypted() && doc.get_pages().len() == pages);
            if !opens {
                return Err(
                    "The copy tpdf built does not open with the password, so it was not written."
                        .into(),
                );
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Object, Stream};

    /// One page with a content stream and an info string.
    fn plain() -> Document {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let content = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => content,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        doc
    }

    fn bytes_of(mut doc: Document) -> Vec<u8> {
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn protected(password: &str) -> Vec<u8> {
        let mut doc = plain();
        doc.encrypt(&state(password).unwrap()).unwrap();
        finish(&mut doc, &Protection::Set(password.into())).unwrap();
        bytes_of(doc)
    }

    #[test]
    fn a_new_password_states_its_key_length_and_identifies_the_file() {
        let mut old = plain();
        old.version = "1.4".into();
        old.encrypt(&state("s3cret").unwrap()).unwrap();
        finish(&mut old, &Protection::Set("s3cret".into())).unwrap();
        assert_eq!(old.version, "1.7");
        let id = old.trailer.get(b"ID").unwrap().as_array().unwrap().clone();
        assert_eq!(id.len(), 2);
        let encrypt = old.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
        let written = old.get_dictionary(encrypt).unwrap();
        assert_eq!(written.get(b"Length").unwrap().as_i64().unwrap(), 256);
        let filter = written
            .get(b"CF")
            .and_then(Object::as_dict)
            .and_then(|all| all.get(b"StdCF"))
            .and_then(Object::as_dict)
            .unwrap();
        assert_eq!(filter.get(b"Length").unwrap().as_i64().unwrap(), 32);

        // An identifier the file has is the file's, and is kept.
        let mut named = plain();
        let was = vec![
            Object::string_literal("first-identifier"),
            Object::string_literal("second-identifier"),
        ];
        named.trailer.set("ID", was.clone());
        named.encrypt(&state("s3cret").unwrap()).unwrap();
        finish(&mut named, &Protection::Set("s3cret".into())).unwrap();
        assert_eq!(named.trailer.get(b"ID").unwrap().as_array().unwrap(), &was);

        // Encryption that was kept gets its filter length and nothing else:
        // the header and the missing identifier are the source's.
        let mut same = plain();
        same.version = "1.4".into();
        same.encrypt(&state("s3cret").unwrap()).unwrap();
        finish(&mut same, &Protection::Keep).unwrap();
        let encrypt = same
            .trailer
            .get(b"Encrypt")
            .unwrap()
            .as_reference()
            .unwrap();
        let filter = same
            .get_dictionary(encrypt)
            .unwrap()
            .get(b"CF")
            .and_then(Object::as_dict)
            .and_then(|all| all.get(b"StdCF"))
            .and_then(Object::as_dict)
            .unwrap();
        assert_eq!(filter.get(b"Length").unwrap().as_i64().unwrap(), 32);
        assert_eq!(same.version, "1.4");
        assert!(same.trailer.get(b"ID").is_err());

        // And a document that is not encrypted is not touched at all.
        let mut kept = plain();
        kept.version = "1.4".into();
        finish(&mut kept, &Protection::Remove).unwrap();
        finish(&mut kept, &Protection::Keep).unwrap();
        assert_eq!(kept.version, "1.4");
        assert!(kept.trailer.get(b"ID").is_err());
    }

    #[test]
    fn a_new_password_locks_the_copy_and_opens_it() {
        let bytes = protected("s3cret-ä");
        assert_eq!(
            written_as_asked(&Protection::Set("s3cret-ä".into()), &bytes, 1),
            Ok(())
        );
        let wrong = written_as_asked(&Protection::Set("other".into()), &bytes, 1).unwrap_err();
        assert!(wrong.contains("does not open with the password"), "{wrong}");
    }

    #[test]
    fn a_copy_written_in_the_clear_is_not_one_with_a_password() {
        let why =
            written_as_asked(&Protection::Set("s3cret".into()), &bytes_of(plain()), 1).unwrap_err();
        assert!(why.contains("opens without the password"), "{why}");
    }

    #[test]
    fn a_copy_that_lost_a_page_is_refused() {
        let bytes = protected("s3cret");
        assert!(written_as_asked(&Protection::Set("s3cret".into()), &bytes, 2).is_err());
        assert!(written_as_asked(&Protection::Remove, &bytes_of(plain()), 2).is_err());
    }

    #[test]
    fn a_removal_is_checked_on_the_bytes() {
        assert_eq!(
            written_as_asked(&Protection::Remove, &bytes_of(plain()), 1),
            Ok(())
        );
        let why = written_as_asked(&Protection::Remove, &protected("s3cret"), 1).unwrap_err();
        assert!(why.contains("still encrypted"), "{why}");
    }

    #[test]
    fn a_removal_needs_a_document_that_a_password_opens() {
        let none = allowed(&Protection::Remove, false, &bytes_of(plain())).unwrap_err();
        assert!(none.contains("has no password"), "{none}");
        assert_eq!(
            allowed(&Protection::Remove, true, &protected("s3cret")),
            Ok(())
        );
        // An empty user password: every reader opens it unasked.
        let mut open = plain();
        let key = [7u8; 32];
        let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
        let restricted = EncryptionState::try_from(EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
            file_encryption_key: &key,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::PRINTABLE,
        })
        .unwrap();
        open.encrypt(&restricted).unwrap();
        let why = allowed(&Protection::Remove, true, &bytes_of(open)).unwrap_err();
        assert!(why.contains("opens without a password"), "{why}");
    }

    #[test]
    fn a_password_nobody_could_type_back_is_refused() {
        assert!(acceptable("").unwrap_err().contains("empty"));
        assert!(acceptable(&"x".repeat(MAX_BYTES)).is_ok());
        assert!(acceptable(&"x".repeat(MAX_BYTES + 1))
            .unwrap_err()
            .contains("too long"));
        assert!(acceptable("a\tb").unwrap_err().contains("control"));
        assert_eq!(
            allowed(&Protection::Set(String::new()), false, &[]),
            acceptable("")
        );
    }

    #[test]
    fn the_password_is_not_in_the_debug_form() {
        let shown = format!("{:?}", Protection::Set("s3cret".into()));
        assert!(!shown.contains("s3cret"), "{shown}");
    }

    #[test]
    fn two_copies_get_two_keys() {
        let a = state("s3cret").unwrap();
        let b = state("s3cret").unwrap();
        assert_ne!(a.file_encryption_key(), b.file_encryption_key());
    }

    #[test]
    fn keep_and_remove_resolve_without_a_key() {
        assert!(resolve(&Protection::Keep, None).unwrap().is_none());
        assert!(resolve(&Protection::Keep, Some(state("a").unwrap()))
            .unwrap()
            .is_some());
        assert!(resolve(&Protection::Remove, Some(state("a").unwrap()))
            .unwrap()
            .is_none());
    }
}
