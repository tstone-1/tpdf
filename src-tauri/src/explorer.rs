//! *Show PDFs in Explorer with tpdf*: registering the thumbnail provider and
//! the preview handler in `shellext/` for `.pdf`, and taking them out again.
//! Windows only, and off until the reader turns it on (`docs/PLAN.md` 7.8,
//! `docs/THREAT-MODEL.md` §T6.47).
//!
//! **Everything is the current user's.** The keys go under
//! `HKCU\Software\Classes`, which Windows merges over the machine's classes,
//! and under the user's `PreviewHandlers` list, so no administrator is asked.
//! A handler the machine registered (Edge's preview handler, on Windows 11) is
//! masked while tpdf's is on and shows again when it is turned off. A handler
//! another program registered for this user is replaced only when the reader
//! says so, and its class id is kept in tpdf's own class key, so turning tpdf
//! off puts it back and deleting that key leaves nothing of tpdf's behind.
//!
//! The logic is written against [`Registry`], so the tests run it on every
//! platform against a map; [`windows::Hkcu`] is the one implementation that
//! touches the real registry.

use std::path::{Path, PathBuf};

/// The DLL's file name, beside `tpdf.exe` and `tpdf-cli.exe`.
pub const DLL: &str = "tpdf_shell.dll";
/// `tpdf_shell::THUMBNAIL_CLSID`; a test holds the two equal.
pub const THUMBNAIL_CLSID: &str = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A21}";
/// `tpdf_shell::PREVIEW_CLSID`; a test holds the two equal.
pub const PREVIEW_CLSID: &str = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A22}";
/// `prevhost.exe`'s application id: a preview handler names it to be loaded
/// there rather than in Explorer.
const PREVHOST_APPID: &str = "{6d2b5079-2f0b-48dd-ab7f-97cec514d30b}";
/// Where a preview handler is listed for the user.
const PREVIEW_LIST: &str = r"Software\Microsoft\Windows\CurrentVersion\PreviewHandlers";
/// The value in tpdf's class key holding the class id it replaced.
const PREVIOUS: &str = "tpdf_previous";
/// `.pdf`'s shell extensions, of which tpdf takes two.
const SHELLEX: &str = r"Software\Classes\.pdf\ShellEx";

/// The two places Explorer looks for a `.pdf` handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Thumbnail,
    Preview,
}

impl Slot {
    pub const ALL: [Slot; 2] = [Slot::Thumbnail, Slot::Preview];

    /// The shell's interface id that names the slot under `ShellEx`.
    fn id(self) -> &'static str {
        match self {
            Slot::Thumbnail => "{e357fccd-a995-4576-b01f-234630154e96}",
            Slot::Preview => "{8895b1c6-b41f-4c1c-a562-0d564250836f}",
        }
    }

    fn ours(self) -> &'static str {
        match self {
            Slot::Thumbnail => THUMBNAIL_CLSID,
            Slot::Preview => PREVIEW_CLSID,
        }
    }

    fn key(self) -> String {
        format!(r"{SHELLEX}\{}", self.id())
    }

    /// The class's own name, as Explorer's settings would show it.
    fn title(self) -> &'static str {
        match self {
            Slot::Thumbnail => "tpdf thumbnails",
            Slot::Preview => "tpdf preview",
        }
    }
}

fn class_key(clsid: &str) -> String {
    format!(r"Software\Classes\CLSID\{clsid}")
}

/// Which registry a value is read from. Writes go to the user's only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    User,
    Machine,
}

/// The few registry operations this needs. A name of `""` is a key's default
/// value. Every write is to the current user's hive.
pub trait Registry {
    /// A string value, or `None` when the key or the value is not there.
    ///
    /// # Errors
    ///
    /// The registry refused the read.
    fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, String>;
    /// Writes a string value, making the key when it is not there.
    ///
    /// # Errors
    ///
    /// The registry refused the write.
    fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String>;
    /// Removes one value; one that is not there is not an error.
    ///
    /// # Errors
    ///
    /// The registry refused.
    fn remove_value(&mut self, key: &str, name: &str) -> Result<(), String>;
    /// Removes a key and everything under it; one that is not there is not an
    /// error.
    ///
    /// # Errors
    ///
    /// The registry refused.
    fn remove_tree(&mut self, key: &str) -> Result<(), String>;
    /// Removes a key that holds no value and no key.
    ///
    /// # Errors
    ///
    /// The registry refused.
    fn remove_if_empty(&mut self, key: &str) -> Result<(), String>;
}

/// Who answers Explorer for a slot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Owner {
    Nobody,
    Tpdf,
    Other { name: String },
}

/// Both slots, for the window's question and for `tpdf explorer`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Handlers {
    pub thumbnail: Owner,
    pub preview: Owner,
}

/// A value that is there and not empty.
fn present(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// The class id Explorer uses for `slot`: the user's, else the machine's,
/// which is how Windows merges the two.
fn current(registry: &dyn Registry, slot: Slot) -> Result<Option<String>, String> {
    Ok(match present(registry.get(Hive::User, &slot.key(), "")?) {
        Some(clsid) => Some(clsid),
        None => present(registry.get(Hive::Machine, &slot.key(), "")?),
    })
}

/// What a class calls itself, else its id.
fn name_of(registry: &dyn Registry, clsid: &str) -> Result<String, String> {
    for hive in [Hive::User, Hive::Machine] {
        for value in ["DisplayName", ""] {
            if let Some(name) = present(registry.get(hive, &class_key(clsid), value)?) {
                // A display name can be an indirect string (`@file.dll,-101`),
                // which says nothing to a reader.
                if !name.starts_with('@') {
                    return Ok(name);
                }
            }
        }
    }
    Ok(clsid.to_string())
}

/// Who answers for `slot` now.
///
/// # Errors
///
/// The registry refused a read.
pub fn owner(registry: &dyn Registry, slot: Slot) -> Result<Owner, String> {
    Ok(match current(registry, slot)? {
        None => Owner::Nobody,
        Some(clsid) if clsid.eq_ignore_ascii_case(slot.ours()) => Owner::Tpdf,
        Some(clsid) => Owner::Other {
            name: name_of(registry, &clsid)?,
        },
    })
}

/// Both slots.
///
/// # Errors
///
/// The registry refused a read.
pub fn state(registry: &dyn Registry) -> Result<Handlers, String> {
    Ok(Handlers {
        thumbnail: owner(registry, Slot::Thumbnail)?,
        preview: owner(registry, Slot::Preview)?,
    })
}

/// Writes tpdf's class for `slot`, served by `dll`.
fn write_class(registry: &mut dyn Registry, slot: Slot, dll: &str) -> Result<(), String> {
    let key = class_key(slot.ours());
    registry.set(&key, "", slot.title())?;
    if slot == Slot::Preview {
        registry.set(&key, "DisplayName", slot.title())?;
        registry.set(&key, "AppID", PREVHOST_APPID)?;
    }
    let server = format!(r"{key}\InprocServer32");
    registry.set(&server, "", dll)?;
    registry.set(&server, "ThreadingModel", "Apartment")
}

/// Turns tpdf's handlers on, served by `dll`. A slot nobody had, or tpdf
/// already had, is taken; one another program has is taken only with
/// `replace`. Returns the sentence for the reader.
///
/// # Errors
///
/// The registry refused, partway: what was written stays, and turning it off
/// removes it.
pub fn enable(registry: &mut dyn Registry, dll: &str, replace: bool) -> Result<String, String> {
    let mut taken = Vec::new();
    let mut left = Vec::new();
    for slot in Slot::ALL {
        match owner(registry, slot)? {
            Owner::Other { name } if !replace => {
                left.push((slot, name));
                continue;
            }
            Owner::Other { .. } => {
                // The user's own value, or none when the machine's answered:
                // that one shows again by itself when tpdf's is gone.
                let previous =
                    present(registry.get(Hive::User, &slot.key(), "")?).unwrap_or_default();
                registry.remove_tree(&class_key(slot.ours()))?;
                write_class(registry, slot, dll)?;
                registry.set(&class_key(slot.ours()), PREVIOUS, &previous)?;
            }
            Owner::Nobody => {
                registry.remove_tree(&class_key(slot.ours()))?;
                write_class(registry, slot, dll)?;
            }
            // Written again, so a copy of tpdf that moved serves from where it is now.
            Owner::Tpdf => write_class(registry, slot, dll)?,
        }
        registry.set(&slot.key(), "", slot.ours())?;
        taken.push(slot);
    }
    if taken.contains(&Slot::Preview) {
        registry.set(PREVIEW_LIST, PREVIEW_CLSID, Slot::Preview.title())?;
    }
    Ok(said_enabled(&taken, &left))
}

fn said_enabled(taken: &[Slot], left: &[(Slot, String)]) -> String {
    let what = |slot: &Slot| match slot {
        Slot::Thumbnail => "thumbnails",
        Slot::Preview => "the preview pane",
    };
    let mut sentence = match taken {
        [] => "Nothing changed.".to_string(),
        [one] => format!("Explorer now shows PDF {} with tpdf.", what(one)),
        _ => "Explorer now shows PDF thumbnails and the preview pane with tpdf.".to_string(),
    };
    for (slot, name) in left {
        let is = match slot {
            Slot::Thumbnail => "PDF thumbnails are still",
            Slot::Preview => "The preview pane still is",
        };
        sentence.push_str(&format!(
            " {is} {name}'s; turning it on again can replace it."
        ));
    }
    sentence
}

/// Turns tpdf's handlers off, puts back what it replaced for this user, and
/// removes every key it wrote. Returns the sentence for the reader.
///
/// # Errors
///
/// The registry refused, partway; running it again finishes.
pub fn disable(registry: &mut dyn Registry) -> Result<String, String> {
    for slot in Slot::ALL {
        let ours = present(registry.get(Hive::User, &slot.key(), "")?)
            .is_some_and(|clsid| clsid.eq_ignore_ascii_case(slot.ours()));
        if ours {
            let previous = present(registry.get(Hive::User, &class_key(slot.ours()), PREVIOUS)?);
            match previous {
                Some(previous) => registry.set(&slot.key(), "", &previous)?,
                None => registry.remove_tree(&slot.key())?,
            }
        }
        registry.remove_tree(&class_key(slot.ours()))?;
    }
    registry.remove_value(PREVIEW_LIST, PREVIEW_CLSID)?;
    registry.remove_if_empty(PREVIEW_LIST)?;
    registry.remove_if_empty(SHELLEX)?;
    let mut sentence = "Explorer no longer uses tpdf for PDFs.".to_string();
    for slot in Slot::ALL {
        if let Owner::Other { name } = owner(registry, slot)? {
            let what = match slot {
                Slot::Thumbnail => "PDF thumbnails are",
                Slot::Preview => "The preview pane is",
            };
            sentence.push_str(&format!(" {what} {name}'s again."));
        }
    }
    Ok(sentence)
}

/// What `tpdf explorer` says with no option.
#[must_use]
pub fn described(state: &Handlers) -> String {
    let part = |what: &str, owner: &Owner| match owner {
        Owner::Tpdf => format!("{what} with tpdf"),
        Owner::Nobody => format!("{what} with nothing"),
        Owner::Other { name } => format!("{what} with {name}"),
    };
    format!(
        "Explorer shows {}, and {}.",
        part("PDF thumbnails", &state.thumbnail),
        part("the preview pane", &state.preview)
    )
}

/// The DLL beside `executable`, refused when it is not there.
///
/// # Errors
///
/// The sentence naming where it was looked for.
pub fn dll_beside(executable: &Path) -> Result<PathBuf, String> {
    let dll = executable
        .parent()
        .ok_or("where tpdf runs from has no folder")?
        .join(DLL);
    if dll.is_file() {
        Ok(dll)
    } else {
        Err(format!(
            "{} is not there, so Explorer has nothing of tpdf's to load; reinstall tpdf",
            dll.display()
        ))
    }
}

/// The real registry: reads from either hive, writes to the user's, and tells
/// Explorer the associations changed after each write.
#[cfg(windows)]
pub mod windows {
    use super::{Hive, Registry};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegDeleteTreeW, RegGetValueW,
        RegOpenKeyExW, RegQueryInfoKeyW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The current user's hive, and the machine's for reading.
    pub struct Hkcu;

    impl Registry for Hkcu {
        fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, String> {
            let root = match hive {
                Hive::User => HKEY_CURRENT_USER,
                Hive::Machine => HKEY_LOCAL_MACHINE,
            };
            let (key_w, name_w) = (wide(key), wide(name));
            let mut size: u32 = 0;
            // SAFETY: both names are NUL-terminated; a null buffer asks for the size.
            let status = unsafe {
                RegGetValueW(
                    root,
                    key_w.as_ptr(),
                    name_w.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut size,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if status != ERROR_SUCCESS {
                return Err(format!(
                    "the registry could not read {key} (error {status})"
                ));
            }
            let mut buffer = vec![0u16; (size as usize).div_ceil(2).max(1)];
            let mut bytes = (buffer.len() * 2) as u32;
            // SAFETY: `buffer` holds `bytes` bytes and outlives the call.
            let status = unsafe {
                RegGetValueW(
                    root,
                    key_w.as_ptr(),
                    name_w.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &raw mut bytes,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    let text = &buffer[..(bytes as usize / 2)];
                    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
                    Ok(Some(String::from_utf16_lossy(&text[..end])))
                }
                ERROR_FILE_NOT_FOUND => Ok(None),
                _ => Err(format!(
                    "the registry could not read {key} (error {status})"
                )),
            }
        }

        fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
            let data = wide(value);
            // SAFETY: the names are NUL-terminated and `data` holds the bytes
            // given, its terminator included, as `REG_SZ` requires.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    wide(key).as_ptr(),
                    wide(name).as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!(
                    "the registry could not write {key} (error {status})"
                ))
            }
        }

        fn remove_value(&mut self, key: &str, name: &str) -> Result<(), String> {
            // SAFETY: both names are NUL-terminated.
            let status = unsafe {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr())
            };
            match status {
                ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
                _ => Err(format!(
                    "the registry could not remove from {key} (error {status})"
                )),
            }
        }

        fn remove_tree(&mut self, key: &str) -> Result<(), String> {
            let name = wide(key);
            // SAFETY: the name is NUL-terminated. `RegDeleteTreeW` empties the
            // key; `RegDeleteKeyW` then removes the key itself.
            let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, name.as_ptr()) };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                return Err(format!(
                    "the registry could not remove {key} (error {status})"
                ));
            }
            // SAFETY: as above.
            let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, name.as_ptr()) };
            match status {
                ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
                _ => Err(format!(
                    "the registry could not remove {key} (error {status})"
                )),
            }
        }

        fn remove_if_empty(&mut self, key: &str) -> Result<(), String> {
            let name = wide(key);
            let mut handle: HKEY = std::ptr::null_mut();
            // SAFETY: the name is NUL-terminated; `handle` is a live local.
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    name.as_ptr(),
                    0,
                    KEY_READ,
                    &raw mut handle,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            if status != ERROR_SUCCESS {
                return Err(format!(
                    "the registry could not open {key} (error {status})"
                ));
            }
            let (mut keys, mut values) = (0u32, 0u32);
            // SAFETY: `handle` was opened above; the two counts are live
            // locals and every other output may be null.
            let status = unsafe {
                RegQueryInfoKeyW(
                    handle,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    &raw mut keys,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut values,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            // SAFETY: closed once.
            unsafe { RegCloseKey(handle) };
            if status != ERROR_SUCCESS {
                return Err(format!(
                    "the registry could not read {key} (error {status})"
                ));
            }
            if keys == 0 && values == 0 {
                // SAFETY: the name is NUL-terminated.
                unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, name.as_ptr()) };
            }
            Ok(())
        }
    }

    /// Tells Explorer the file associations changed, so an open window picks
    /// the handlers up without signing out.
    pub fn announce() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        // SAFETY: both item pointers may be null for this event.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
    }
}

/// Turns the integration on or off for this copy of tpdf, from the window or
/// the command line. `None` reads the state only.
///
/// # Errors
///
/// The sentence to show: not Windows, the DLL missing, or the registry refused.
pub fn run(change: Option<(bool, bool)>) -> Result<(String, Handlers), String> {
    #[cfg(windows)]
    {
        let mut registry = windows::Hkcu;
        let said = match change {
            None => None,
            Some((true, replace)) => {
                let executable =
                    std::env::current_exe().map_err(|e| format!("where tpdf runs from: {e}"))?;
                let dll = dll_beside(&executable)?;
                Some(enable(&mut registry, &dll.to_string_lossy(), replace)?)
            }
            Some((false, _)) => Some(disable(&mut registry)?),
        };
        if said.is_some() {
            windows::announce();
        }
        let state = state(&registry)?;
        Ok((said.unwrap_or_else(|| described(&state)), state))
    }
    #[cfg(target_os = "macos")]
    {
        let _ = change;
        Err(
            "Explorer is the Windows file manager; on macOS, Finder and Quick Look show PDFs \
             themselves"
                .into(),
        )
    }
    #[cfg(target_os = "linux")]
    {
        let _ = change;
        Err(
            "Explorer is the Windows file manager; on Linux, the file manager's own \
             thumbnailer shows PDFs"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Two hives as maps from (key, value name) to text. Keys compare without
    /// case, as the registry's do.
    #[derive(Default, Clone, PartialEq, Debug)]
    struct Map {
        user: BTreeMap<(String, String), String>,
        machine: BTreeMap<(String, String), String>,
        /// Keys that exist with nothing in them.
        empty_user_keys: std::collections::BTreeSet<String>,
    }

    fn fold(key: &str) -> String {
        key.to_ascii_lowercase()
    }

    impl Registry for Map {
        fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, String> {
            let map = match hive {
                Hive::User => &self.user,
                Hive::Machine => &self.machine,
            };
            Ok(map.get(&(fold(key), fold(name))).cloned())
        }
        fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
            self.empty_user_keys.remove(&fold(key));
            self.user.insert((fold(key), fold(name)), value.into());
            Ok(())
        }
        fn remove_value(&mut self, key: &str, name: &str) -> Result<(), String> {
            if self.user.remove(&(fold(key), fold(name))).is_some()
                && !self.user.keys().any(|(k, _)| *k == fold(key))
            {
                self.empty_user_keys.insert(fold(key));
            }
            Ok(())
        }
        fn remove_tree(&mut self, key: &str) -> Result<(), String> {
            let key = fold(key);
            let under = |k: &String| *k == key || k.starts_with(&format!("{key}\\"));
            let existed =
                self.user.keys().any(|(k, _)| under(k)) || self.empty_user_keys.iter().any(under);
            self.user.retain(|(k, _), _| !under(k));
            self.empty_user_keys.retain(|k| !under(k));
            // The registry keeps the parent of a removed key, empty or not.
            if let Some((parent, _)) = key.rsplit_once('\\') {
                let prefix = format!("{parent}\\");
                let holds = self
                    .user
                    .keys()
                    .any(|(k, _)| *k == parent || k.starts_with(&prefix))
                    || self.empty_user_keys.iter().any(|k| k.starts_with(&prefix));
                if existed && !holds {
                    self.empty_user_keys.insert(parent.to_string());
                }
            }
            Ok(())
        }
        fn remove_if_empty(&mut self, key: &str) -> Result<(), String> {
            let key = fold(key);
            let prefix = format!("{key}\\");
            let holds = self
                .user
                .keys()
                .any(|(k, _)| *k == key || k.starts_with(&prefix))
                || self.empty_user_keys.iter().any(|k| k.starts_with(&prefix));
            if !holds {
                self.empty_user_keys.remove(&key);
            }
            Ok(())
        }
    }

    const EDGE: &str = "{3A84F9C2-6164-485C-A7D9-4B27F8AC009E}";
    const DLL_AT: &str = r"C:\Users\reader\AppData\Local\tpdf\tpdf_shell.dll";

    /// TEENYWEENY as measured on 2026-10-16: Edge's preview handler for the
    /// machine, no thumbnail provider, and the installer's `.pdf` keys for the
    /// user.
    fn teenyweeny() -> Map {
        let mut map = Map::default();
        map.machine.insert(
            (fold(&Slot::Preview.key()), String::new()),
            EDGE.to_string(),
        );
        map.machine.insert(
            (fold(&class_key(EDGE)), fold("DisplayName")),
            "PDF Preview Handler".into(),
        );
        map.user.insert(
            (fold(r"Software\Classes\.pdf"), fold("PDF document_backup")),
            String::new(),
        );
        // Neighbours every user's hive has, so that removing tpdf's keys does
        // not empty their parents.
        map.user.insert(
            (
                fold(r"Software\Classes\CLSID\{00000000-1111-2222-3333-444444444444}"),
                String::new(),
            ),
            "Another program".into(),
        );
        map.user.insert(
            (
                fold(r"Software\Microsoft\Windows\CurrentVersion\Explorer"),
                fold("ShellState"),
            ),
            "1".into(),
        );
        map
    }

    #[test]
    fn a_free_slot_is_taken_and_one_the_machine_owns_is_left_unless_replaced() {
        let mut map = teenyweeny();
        let said = enable(&mut map, DLL_AT, false).unwrap();
        assert_eq!(
            state(&map).unwrap(),
            Handlers {
                thumbnail: Owner::Tpdf,
                preview: Owner::Other {
                    name: "PDF Preview Handler".into()
                }
            }
        );
        assert!(
            said.contains("thumbnails with tpdf") && said.contains("PDF Preview Handler"),
            "{said}"
        );
        assert_eq!(
            map.get(Hive::User, &class_key(THUMBNAIL_CLSID).add_server(), "")
                .unwrap()
                .as_deref(),
            Some(DLL_AT)
        );
        assert!(map
            .get(Hive::User, PREVIEW_LIST, PREVIEW_CLSID)
            .unwrap()
            .is_none());

        let said = enable(&mut map, DLL_AT, true).unwrap();
        assert_eq!(state(&map).unwrap().preview, Owner::Tpdf, "{said}");
        let preview = class_key(PREVIEW_CLSID);
        assert_eq!(
            map.get(Hive::User, &preview, "AppID").unwrap().as_deref(),
            Some(PREVHOST_APPID)
        );
        assert_eq!(
            map.get(Hive::User, PREVIEW_LIST, PREVIEW_CLSID)
                .unwrap()
                .as_deref(),
            Some("tpdf preview")
        );
        // The machine's handler is masked, not written down: it comes back by itself.
        assert_eq!(
            map.get(Hive::User, &preview, PREVIOUS).unwrap().as_deref(),
            Some("")
        );
    }

    #[test]
    fn turning_it_off_leaves_the_users_hive_as_it_was_before() {
        let before = teenyweeny();
        let mut map = before.clone();
        enable(&mut map, DLL_AT, true).unwrap();
        let said = disable(&mut map).unwrap();
        assert_eq!(map.user, before.user, "{said}");
        assert!(map.empty_user_keys.is_empty(), "{:?}", map.empty_user_keys);
        assert!(said.contains("PDF Preview Handler's again"), "{said}");
        assert_eq!(
            state(&map).unwrap(),
            Handlers {
                thumbnail: Owner::Nobody,
                preview: Owner::Other {
                    name: "PDF Preview Handler".into()
                }
            }
        );
    }

    #[test]
    fn a_handler_the_user_had_is_put_back_after_being_replaced() {
        let other = "{11111111-2222-3333-4444-555555555555}";
        let mut map = teenyweeny();
        map.user
            .insert((fold(&Slot::Thumbnail.key()), String::new()), other.into());
        map.user.insert(
            (fold(&class_key(other)), String::new()),
            "Other PDF thumbnails".into(),
        );
        let before = map.clone();
        let said = enable(&mut map, DLL_AT, false).unwrap();
        assert!(said.starts_with("Nothing changed."), "{said}");
        assert_eq!(map.user, before.user, "nothing written without consent");
        enable(&mut map, DLL_AT, true).unwrap();
        assert_eq!(state(&map).unwrap().thumbnail, Owner::Tpdf);
        disable(&mut map).unwrap();
        assert_eq!(map.user, before.user);
        assert_eq!(
            state(&map).unwrap().thumbnail,
            Owner::Other {
                name: "Other PDF thumbnails".into()
            }
        );
    }

    #[test]
    fn turning_it_on_twice_keeps_what_the_first_time_replaced() {
        let other = "{11111111-2222-3333-4444-555555555555}";
        let mut map = teenyweeny();
        map.user
            .insert((fold(&Slot::Thumbnail.key()), String::new()), other.into());
        let before = map.clone();
        enable(&mut map, DLL_AT, true).unwrap();
        enable(&mut map, r"D:\moved\tpdf_shell.dll", true).unwrap();
        assert_eq!(
            map.get(Hive::User, &class_key(THUMBNAIL_CLSID).add_server(), "")
                .unwrap()
                .as_deref(),
            Some(r"D:\moved\tpdf_shell.dll")
        );
        disable(&mut map).unwrap();
        assert_eq!(map.user, before.user);
    }

    #[test]
    fn turning_it_off_when_it_was_never_on_changes_nothing() {
        let mut map = teenyweeny();
        let before = map.clone();
        disable(&mut map).unwrap();
        assert_eq!(map, before);
    }

    #[test]
    fn an_indirect_display_name_falls_back_to_the_class_name_then_the_id() {
        let mut map = teenyweeny();
        map.machine.insert(
            (fold(&class_key(EDGE)), fold("DisplayName")),
            "@pdf.dll,-101".into(),
        );
        map.machine
            .insert((fold(&class_key(EDGE)), String::new()), "Edge PDF".into());
        assert_eq!(
            state(&map).unwrap().preview,
            Owner::Other {
                name: "Edge PDF".into()
            }
        );
        map.machine.remove(&(fold(&class_key(EDGE)), String::new()));
        assert_eq!(
            state(&map).unwrap().preview,
            Owner::Other { name: EDGE.into() }
        );
    }

    #[test]
    fn the_class_ids_are_the_dlls() {
        assert_eq!(THUMBNAIL_CLSID, tpdf_shell::THUMBNAIL_CLSID);
        assert_eq!(PREVIEW_CLSID, tpdf_shell::PREVIEW_CLSID);
    }

    trait Server {
        fn add_server(self) -> String;
    }
    impl Server for String {
        fn add_server(self) -> String {
            format!(r"{self}\InprocServer32")
        }
    }
}
