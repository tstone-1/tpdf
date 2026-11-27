//! Reading the file a signature is made over, and writing the signed copy.
//!
//! **The one writer whose bytes the app process assembled itself**, and that is
//! not a lapse of the rule every other writer here follows --- that the
//! coordinator never holds a document it produced by parsing. Nothing here is
//! produced by parsing: the file is read as bytes, the worker's update section
//! is appended as bytes, and the signature was spliced into it by position
//! (`sign_cms.rs`). The worker parsed; this process only concatenates.
//!
//! **The original is never modified.** Signing writes a new file the reader
//! named, whose first bytes are the original's, byte for byte, followed by one
//! incremental revision --- which is what keeps every earlier signature's range
//! the bytes it was made over. Naming the source as the destination is refused,
//! as a copy is.

use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::{commit, same_file, stage, Refusal};
use crate::fingerprint::Fingerprint;

/// Reads `source` whole, and refuses unless it is the file that was opened.
///
/// Compared by length and SHA-256 over **the bytes read**, not over a second
/// read of the path: what is signed is exactly what is compared.
///
/// # Errors
///
/// The file cannot be read, or its bytes are not the ones `opened_as`
/// recorded --- the document changed on disk since it was opened.
pub fn read_to_sign(source: &Path, opened_as: &Fingerprint) -> Result<Vec<u8>, Refusal> {
    crate::sign_cms::refuse_too_large(opened_as.len)?;
    read_opened(source, opened_as)
}

/// Reads at most one byte beyond the fingerprinted length to detect growth.
pub(crate) fn read_opened(source: &Path, opened_as: &Fingerprint) -> Result<Vec<u8>, Refusal> {
    let unread = |e: std::io::Error| {
        Refusal::from(said!("could not read {path}: {e}", path = source.display()))
    };
    let file = std::fs::File::open(source).map_err(unread)?;
    let bytes = read_bounded(file, opened_as.len).map_err(unread)?;
    if bytes.len() as u64 != opened_as.len
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != opened_as.digest
    {
        return Err(Refusal::changed(said!(
            "the file on disk is not the one that was opened, so it was not signed --- \
             reopen it to sign what is there now"
        )));
    }
    Ok(bytes)
}

fn read_bounded(file: impl std::io::Read, len: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    file.take(len.saturating_add(1)).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn a_growing_source_is_read_only_one_byte_past_its_opening_length() {
    let mut file = std::io::Cursor::new([7; 4096]);
    assert_eq!(read_bounded(&mut file, 10).unwrap().len(), 11);
    assert_eq!(file.position(), 11);
}

pub(crate) fn check_signed_destination(source: &Path, out: &Path) -> Result<(), Refusal> {
    if same_file(source, out) {
        return Err(said!(
            "the signed document is written as a new file --- choose a name other than \
             the original's"
        )
        .into());
    }
    Ok(())
}

/// Writes a signed document to `out`, atomically, and never over `source`.
///
/// `bytes` is the whole file: the original followed by the signed revision.
///
/// # Errors
///
/// `out` is `source`, or the staging file cannot be written or put in place.
/// Nothing is left at `out` on failure.
pub fn write_signed(source: &Path, out: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    use std::io::Write as _;

    check_signed_destination(source, out)?;
    let staged = stage(out, |file| {
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .map_err(|e| Refusal::from(said!("could not write the signed document: {e}")))
    })?;
    commit(&staged, out)?;
    Ok(())
}
