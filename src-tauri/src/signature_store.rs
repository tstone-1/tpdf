//! Remembered signature pixels protected by the current OS user account.
//! macOS uses Keychain; Windows stores only DPAPI ciphertext in app-local data.

use crate::signature::Image;
use std::path::Path;

const MAX_BYTES: usize = 512 * 256 * 4 + 8;
static ACCESS: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    Load,
    Save { image: Image },
    Forget,
}

fn encode(image: &Image) -> Result<Vec<u8>, String> {
    if !image.valid() {
        return Err("invalid signature pixels".into());
    }
    let mut bytes = image.width.to_le_bytes().to_vec();
    bytes.extend(image.height.to_le_bytes());
    bytes.extend(&image.rgba);
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || bytes.len() > MAX_BYTES {
        return Err("invalid saved signature size".into());
    }
    let image = Image {
        width: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        height: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        rgba: bytes[8..].to_vec(),
    };
    if !image.valid() {
        return Err("invalid saved signature pixels".into());
    }
    Ok(image)
}

/// Operate on a store selected by the application identifier, never by the webview.
///
/// # Errors
/// Invalid pixels, inaccessible protected storage, or damaged stored data.
pub fn perform(service: &str, path: &Path, action: Action) -> Result<Option<Image>, String> {
    let _guard = ACCESS.lock();
    match action {
        Action::Load => platform::load(service, path)?
            .map(|bytes| decode(&bytes))
            .transpose(),
        Action::Save { image } => {
            platform::save(service, path, &encode(&image)?)?;
            Ok(None)
        }
        Action::Forget => {
            platform::forget(service, path)?;
            Ok(None)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    const ACCOUNT: &str = "remembered-signature";
    pub fn load(service: &str, _path: &Path) -> Result<Option<Vec<u8>>, String> {
        match get_generic_password(service, ACCOUNT) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(error) => Err(format!(
                "Keychain could not read the saved signature: {error}"
            )),
        }
    }
    pub fn save(service: &str, _path: &Path, bytes: &[u8]) -> Result<(), String> {
        set_generic_password(service, ACCOUNT, bytes)
            .map_err(|e| format!("Keychain could not save the signature: {e}"))
    }
    pub fn forget(service: &str, _path: &Path) -> Result<(), String> {
        match delete_generic_password(service, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(format!("Keychain could not remove the signature: {error}")),
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::io::{Read, Write};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    };

    fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // Inputs live across the call. DPAPI allocates output with LocalAlloc;
        // copy before freeing it, and never enable the machine-wide key flag.
        unsafe {
            let ok = if encrypt {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            };
            if ok == 0 {
                return Err(format!(
                    "Windows could not protect or unlock the signature: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let result = if output.pbData.is_null() || output.cbData as usize > MAX_BYTES + 4096 {
                Err("invalid protected signature size".into())
            } else {
                Ok(std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec())
            };
            LocalFree(output.pbData.cast());
            result
        }
    }
    pub fn load(_service: &str, path: &Path) -> Result<Option<Vec<u8>>, String> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 4097) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES + 4096 {
            return Err("saved signature exceeds its size limit".into());
        }
        protect(&bytes, false).map(Some)
    }
    pub fn save(_service: &str, path: &Path, bytes: &[u8]) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let encrypted = protect(bytes, true)?;
        std::fs::create_dir_all(path.parent().ok_or("missing signature directory")?)
            .map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&encrypted)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Both paths are terminated UTF-16 and remain live through the call.
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    pub fn forget(_service: &str, path: &Path) -> Result<(), String> {
        for stored in [path.to_path_buf(), path.with_extension("tmp")] {
            match std::fs::remove_file(stored) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::*;
    pub fn load(_: &str, _: &Path) -> Result<Option<Vec<u8>>, String> {
        Err("protected signature storage is unavailable on this platform".into())
    }
    pub fn save(_: &str, _: &Path, _: &[u8]) -> Result<(), String> {
        Err("protected signature storage is unavailable on this platform".into())
    }
    pub fn forget(_: &str, _: &Path) -> Result<(), String> {
        Err("protected signature storage is unavailable on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_signature_codec_is_bounded_and_requires_visible_pixels() {
        let image = Image {
            width: 1,
            height: 1,
            rgba: vec![0, 20, 30, 255],
        };
        assert_eq!(decode(&encode(&image).unwrap()).unwrap(), image);
        assert!(decode(&[0; 8]).is_err());
        assert!(decode(&vec![1; MAX_BYTES + 1]).is_err());
        let mut hidden = image;
        hidden.rgba[3] = 0;
        assert!(encode(&hidden).is_err());
    }
    #[test]
    #[ignore = "uses native protected storage; run explicitly on an unlocked desktop"]
    fn native_store_roundtrips_maximum_pixels_and_forgets() {
        let service = format!(
            "com.timostein.tpdf.storage-probe.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let room = std::env::temp_dir().join(&service);
        let path = room.join("signature.bin");
        let result = (|| -> Result<(), String> {
            if perform(&service, &path, Action::Load)?.is_some() {
                return Err("probe store already exists".into());
            }
            let image = Image {
                width: 512,
                height: 256,
                rgba: vec![127; 512 * 256 * 4],
            };
            #[cfg(windows)]
            let plain = encode(&image)?;
            perform(
                &service,
                &path,
                Action::Save {
                    image: image.clone(),
                },
            )?;
            assert_eq!(perform(&service, &path, Action::Load)?, Some(image));
            #[cfg(windows)]
            assert_ne!(std::fs::read(&path).unwrap(), plain);
            perform(&service, &path, Action::Forget)?;
            assert_eq!(perform(&service, &path, Action::Load)?, None);
            Ok(())
        })();
        let cleanup = perform(&service, &path, Action::Forget);
        let _ = std::fs::remove_dir(&room);
        result.unwrap();
        cleanup.unwrap();
    }
}
