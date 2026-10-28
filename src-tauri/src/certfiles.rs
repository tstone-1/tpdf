//! The certificate files a reader has added for signing on Linux.
//!
//! A certificate file (`pkcs12.rs`) is not in any store the system keeps, so
//! tpdf remembers the ones a reader adds: each file's path, its signer's
//! certificate and its chain, which are public, so the chooser can list it
//! without asking for a password. **Never the key or the password**: signing
//! opens the file again and asks again.
//!
//! The list is `certificate-files.json` in tpdf's configuration folder,
//! `$XDG_CONFIG_HOME/com.timostein.tpdf` or `~/.config/com.timostein.tpdf`,
//! which is the folder Tauri gives the application on Linux.

use std::path::{Path, PathBuf};

use crate::pkcs12::{self, Refusal};

/// One added file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Remembered {
    /// Where the file is.
    pub path: PathBuf,
    /// The signer's certificate, DER, as hex.
    certificate: String,
    /// The file's other certificates, DER, as hex.
    chain: Vec<String>,
}

impl Remembered {
    /// The signer's certificate, DER.
    #[must_use]
    pub fn certificate(&self) -> Vec<u8> {
        unhex(&self.certificate)
    }

    /// The file's other certificates, DER.
    #[must_use]
    pub fn chain(&self) -> Vec<Vec<u8>> {
        self.chain.iter().map(|c| unhex(c)).collect()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .filter_map(|i| u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

/// Where the list is, or `None` with no home folder to put it in.
#[must_use]
pub fn store() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(
        base.join("com.timostein.tpdf")
            .join("certificate-files.json"),
    )
}

/// The files in the list at `store`, for the chooser. A missing list is an
/// empty one, since nothing was added; one that cannot be read or is damaged
/// lists nothing and says why in the diagnostics log.
#[must_use]
pub fn read(store: &Path) -> Vec<Remembered> {
    load(store).unwrap_or_else(|why| {
        crate::diag::note(&format!("[WARN] {why}"));
        Vec::new()
    })
}

/// The list at `store`: empty when there is none, an error when there is one
/// that cannot be read or is not a list.
///
/// Kept apart from [`read`] for [`add`], which writes the list back: until
/// 2026-10-15 a damaged list read as empty there too, and adding a file then
/// replaced it with a list of that one file, losing every earlier entry
/// without a word.
fn load(store: &Path) -> Result<Vec<Remembered>, String> {
    let bytes = match std::fs::read(store) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(said!(
                "The list of certificate files {path} could not be read: {e}.",
                path = store.display()
            ))
        }
    };
    serde_json::from_slice(&bytes).map_err(|e| {
        said!(
            "The list of certificate files {path} is damaged ({e}).",
            path = store.display()
        )
    })
}

/// Opens the file at `path` with `password`, and, if it can sign, adds it to
/// the list at `store`, replacing an earlier entry for the same path. The key
/// is dropped before this returns.
///
/// # Errors
///
/// The file's [`Refusal`] as a sentence; a list already there that cannot be
/// read or is damaged, which is left exactly as it is; or the list not being
/// written.
pub fn add(store: &Path, path: &Path, password: &str) -> Result<Remembered, String> {
    let mut list = load(store)
        .map_err(|why| said!("{why} Nothing was added; move it aside to start a new list."))?;
    let bytes =
        std::fs::read(path).map_err(|e| said!("The certificate file could not be read: {e}."))?;
    let opened = pkcs12::open(&bytes, password).map_err(|why| why.sentence())?;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let entry = Remembered {
        path: path.clone(),
        certificate: hex(&opened.certificate),
        chain: opened.chain.iter().map(|c| hex(c)).collect(),
    };
    drop(opened);
    list.retain(|other| other.path != path);
    list.push(entry.clone());
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| said!("The list of certificate files could not be saved: {e}."))?;
    }
    let text = serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?;
    let part = store.with_extension("json.part");
    std::fs::write(&part, text)
        .and_then(|()| std::fs::rename(&part, store))
        .map_err(|e| said!("The list of certificate files could not be saved: {e}."))?;
    Ok(entry)
}

/// Opens `entry`'s file for one signature, with `password`, and checks it is
/// still the file that was added: a file replaced since by one with another
/// certificate is refused rather than signing as somebody else.
///
/// # Errors
///
/// The file gone, its [`Refusal`], or a different certificate.
pub fn reopen(entry: &Remembered, password: &str) -> Result<pkcs12::Opened, String> {
    let bytes = std::fs::read(&entry.path).map_err(|e| {
        said!(
            "The certificate file {path} could not be read: {e}.",
            path = entry.path.display()
        )
    })?;
    let opened = pkcs12::open(&bytes, password).map_err(|why: Refusal| why.sentence())?;
    if opened.certificate != entry.certificate() {
        return Err(said!(
            "The certificate file {path} now holds a different certificate from the one added; add it again.",
            path = entry.path.display()
        ));
    }
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_comes_back_as_the_bytes_it_was() {
        let bytes = [0x00, 0x7f, 0x80, 0xff, 0x30];
        assert_eq!(unhex(&hex(&bytes)), bytes);
    }

    #[test]
    fn a_missing_list_is_empty() {
        let missing =
            std::env::temp_dir().join(format!("tpdf-no-list-{}.json", std::process::id()));
        assert!(read(&missing).is_empty());
    }

    /// A damaged list refuses an addition and stays byte for byte as it was.
    /// The certificate file does not exist, which does not matter: the list is
    /// read first, so the refusal names the list and not the file.
    #[test]
    fn a_damaged_list_is_left_as_it_is_and_nothing_is_added() {
        let dir = std::env::temp_dir().join(format!("tpdf-damaged-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let store = dir.join("certificate-files.json");
        let damaged = b"[{\"path\": \"/a.p12\", \"certif";
        std::fs::write(&store, damaged).expect("written");
        let err = add(&store, &dir.join("missing.p12"), "x").expect_err("refused");
        assert!(err.contains("is damaged"), "{err}");
        assert_eq!(std::fs::read(&store).expect("still there"), damaged);
        assert!(read(&store).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_store_is_in_the_applications_config_folder() {
        let found = store().expect("a home folder");
        assert!(
            found.ends_with("com.timostein.tpdf/certificate-files.json"),
            "{}",
            found.display()
        );
    }
}
