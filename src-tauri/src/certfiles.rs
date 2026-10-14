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

/// The files in the list at `store`. A missing or unreadable list is an
/// empty one: nothing was added, or nothing that can be read was.
#[must_use]
pub fn read(store: &Path) -> Vec<Remembered> {
    std::fs::read(store)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Opens the file at `path` with `password`, and, if it can sign, adds it to
/// the list at `store`, replacing an earlier entry for the same path. The key
/// is dropped before this returns.
///
/// # Errors
///
/// The file's [`Refusal`] as a sentence, or the list not being written.
pub fn add(store: &Path, path: &Path, password: &str) -> Result<Remembered, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("The certificate file could not be read: {e}."))?;
    let opened = pkcs12::open(&bytes, password).map_err(|why| why.sentence())?;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let entry = Remembered {
        path: path.clone(),
        certificate: hex(&opened.certificate),
        chain: opened.chain.iter().map(|c| hex(c)).collect(),
    };
    drop(opened);
    let mut list = read(store);
    list.retain(|other| other.path != path);
    list.push(entry.clone());
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("The list of certificate files could not be saved: {e}."))?;
    }
    let text = serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?;
    let part = store.with_extension("json.part");
    std::fs::write(&part, text)
        .and_then(|()| std::fs::rename(&part, store))
        .map_err(|e| format!("The list of certificate files could not be saved: {e}."))?;
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
        format!(
            "The certificate file {} could not be read: {e}.",
            entry.path.display()
        )
    })?;
    let opened = pkcs12::open(&bytes, password).map_err(|why: Refusal| why.sentence())?;
    if opened.certificate != entry.certificate() {
        return Err(format!(
            "The certificate file {} now holds a different certificate from the one added; add it again.",
            entry.path.display()
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
