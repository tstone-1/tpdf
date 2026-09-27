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
    let bytes = std::fs::read(source)
        .map_err(|e| Refusal::from(format!("could not read {}: {e}", source.display())))?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() as u64 != opened_as.len || digest != opened_as.digest {
        return Err(Refusal::changed(
            "the file on disk is not the one that was opened, so it was not signed --- \
             reopen it to sign what is there now",
        ));
    }
    Ok(bytes)
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

    if same_file(source, out) {
        return Err(
            "the signed document is written as a new file --- choose a name other than \
             the original's"
                .into(),
        );
    }
    let staged = stage(out, |file| {
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .map_err(|e| Refusal::from(format!("could not write the signed document: {e}")))
    })?;
    commit(&staged, out)?;
    Ok(())
}
