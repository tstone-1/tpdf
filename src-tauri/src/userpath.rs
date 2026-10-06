//! The user's `PATH` on Windows: adding the folder `tpdf-cli.exe` is in, and
//! taking it out again.
//!
//! **Written here and not in the installer's script.** An NSIS string is cut
//! at a fixed length, and a `PATH` read cut and written back loses every
//! entry past the cut --- the reader's, not ours. The registry call below
//! reads the value at whatever length it has, and writes nothing unless the
//! read succeeded and the list changed.
//!
//! The value is `HKCU\Environment\Path`: the user's own, which needs no
//! administrator and is what a per-user install can change. Its type is kept
//! (`REG_EXPAND_SZ` entries such as `%USERPROFILE%\bin` stay unexpanded), and
//! every other entry is kept byte for byte and in order.
//!
//! **A reading cannot write.** The value is read through a handle opened for
//! `KEY_QUERY_VALUE` alone and written through a second one opened for
//! `KEY_SET_VALUE`, so [`stored`] --- which the window asks whenever it comes
//! to the front, to grey a command with nothing to do --- holds nothing a
//! write could go through.

/// A folder as `PATH` compares them: case ignored, quotes and a trailing
/// separator dropped.
fn key(entry: &str) -> String {
    entry
        .trim()
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// `value` with `dir` as its last entry, or `None` when it is already one.
#[must_use]
pub fn with(value: &str, dir: &str) -> Option<String> {
    if value.split(';').any(|entry| key(entry) == key(dir)) {
        return None;
    }
    let kept = value.trim_end_matches(';');
    Some(if kept.is_empty() {
        dir.to_string()
    } else {
        format!("{kept};{dir}")
    })
}

/// `value` without any entry that is `dir`, or `None` when it has none. The
/// other entries are kept as they were written.
#[must_use]
pub fn without(value: &str, dir: &str) -> Option<String> {
    if !value.split(';').any(|entry| key(entry) == key(dir)) {
        return None;
    }
    Some(
        value
            .split(';')
            .filter(|entry| key(entry) != key(dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// What a change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The value was written.
    Changed,
    /// It already was as asked, and nothing was written.
    Unchanged,
}

/// Adds `dir` to the user's `PATH`, or with `add` false takes it out.
///
/// # Errors
///
/// The value could not be read or written, or is not text.
#[cfg(windows)]
pub fn apply(dir: &str, add: bool) -> Result<Outcome, String> {
    let (value, kind) = windows::read()?;
    let changed = if add {
        with(&value, dir)
    } else {
        without(&value, dir)
    };
    let Some(changed) = changed else {
        return Ok(Outcome::Unchanged);
    };
    windows::write(&changed, kind)?;
    windows::announce();
    Ok(Outcome::Changed)
}

/// Whether `dir` is on the user's `PATH` as stored, whatever a terminal that
/// was opened earlier still holds. Reads only: see the module's last paragraph.
///
/// # Errors
///
/// The value could not be read, or is not text.
#[cfg(windows)]
pub fn stored(dir: &str) -> Result<bool, String> {
    Ok(with(&windows::read()?.0, dir).is_none())
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
        REG_SZ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from `RegCreateKeyExW` and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// The key the user's `PATH` is a value of.
    const ENVIRONMENT: &str = "Environment";

    /// `subkey` of the user's hive, to be asked and never written: the handle
    /// carries `KEY_QUERY_VALUE` and nothing else, and a key that is not there
    /// is not made. `None` when it is not there.
    fn reading(subkey: &str) -> Result<Option<Key>, String> {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: every pointer is to a live local; the key name is
        // NUL-terminated.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(subkey).as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &raw mut key,
            )
        };
        match status {
            ERROR_SUCCESS => Ok(Some(Key(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            _ => Err(format!("your PATH could not be opened (error {status})")),
        }
    }

    /// `subkey` of the user's hive, to be written; made when it is not there.
    fn writing(subkey: &str) -> Result<Key, String> {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: every pointer is to a live local; the key name is
        // NUL-terminated.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(subkey).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &raw mut key,
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Key(key))
        } else {
            Err(format!("your PATH could not be opened (error {status})"))
        }
    }

    /// The value and its type. A value that is not there reads as empty, to be
    /// created as `REG_EXPAND_SZ`, which is what Windows itself writes.
    pub fn read() -> Result<(String, u32), String> {
        let Some(key) = reading(ENVIRONMENT)? else {
            return Ok((String::new(), REG_EXPAND_SZ));
        };
        let name = wide("Path");
        let (mut kind, mut bytes) = (0u32, 0u32);
        // SAFETY: a size query; the data pointer is null and `bytes` is live.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &raw mut kind,
                std::ptr::null_mut(),
                &raw mut bytes,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok((String::new(), REG_EXPAND_SZ));
        }
        if status != ERROR_SUCCESS {
            return Err(format!("your PATH could not be read (error {status})"));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Err("your PATH is not stored as text, so tpdf leaves it alone".into());
        }
        let mut data = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        let mut held = u32::try_from(data.len() * 2).map_err(|_| "your PATH is too long")?;
        // SAFETY: `data` is `held` bytes long and both outlive the call.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &raw mut kind,
                data.as_mut_ptr().cast(),
                &raw mut held,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(format!("your PATH could not be read (error {status})"));
        }
        data.truncate((held as usize) / 2);
        while data.last() == Some(&0) {
            data.pop();
        }
        // Not lossy: a value that is not UTF-16 is refused, never rewritten.
        let value = String::from_utf16(&data)
            .map_err(|_| "your PATH holds text tpdf cannot read, so tpdf leaves it alone")?;
        Ok((value, kind))
    }

    pub fn write(value: &str, kind: u32) -> Result<(), String> {
        set(&writing(ENVIRONMENT)?, "Path", value, kind)
    }

    /// Sets one text value through `key`.
    fn set(key: &Key, name: &str, value: &str, kind: u32) -> Result<(), String> {
        let data = wide(value);
        let bytes = u32::try_from(data.len() * 2).map_err(|_| "your PATH is too long")?;
        // SAFETY: `data` is `bytes` bytes long, NUL included, and outlives the
        // call.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                kind,
                data.as_ptr().cast(),
                bytes,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("your PATH could not be written (error {status})"))
        }
    }

    /// Tells running programs the environment changed, so a terminal opened
    /// afterwards sees it without signing out. Best effort: a window that does
    /// not answer within the bound is skipped.
    pub fn announce() {
        let what = wide("Environment");
        // SAFETY: `what` is NUL-terminated and outlives the call; the result
        // pointer may be null.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                what.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                2000,
                std::ptr::null_mut(),
            );
        }
    }

    /// What the tests below need and nothing else does: when a key was last
    /// written, a scratch key of their own to write, and a write attempted
    /// through the reading handle.
    #[cfg(test)]
    pub mod probe {
        use super::{reading, set, wide, writing, ENVIRONMENT};
        use windows_sys::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
        use windows_sys::Win32::System::Registry::{
            RegDeleteKeyW, RegQueryInfoKeyW, HKEY_CURRENT_USER, REG_SZ,
        };

        /// The key the user's `PATH` is in.
        pub const PATH_KEY: &str = ENVIRONMENT;

        /// When `subkey` was last written, in the registry's own clock. A
        /// `RegSetValueExW` that changes a value moves it; one that stores the
        /// bytes already there was measured not to.
        pub fn written(subkey: &str) -> u64 {
            let key = reading(subkey).expect("opened").expect("the key exists");
            let mut at = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            // SAFETY: every out-pointer but the last is null, which the call
            // allows; `at` is live.
            let status = unsafe {
                RegQueryInfoKeyW(
                    key.0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut at,
                )
            };
            assert_eq!(status, ERROR_SUCCESS, "the key's times could not be read");
            (u64::from(at.dwHighDateTime) << 32) | u64::from(at.dwLowDateTime)
        }

        /// Writes one value into `subkey`, making the key.
        pub fn write(subkey: &str, value: &str) {
            let key = writing(subkey).expect("scratch key");
            set(&key, "value", value, REG_SZ).expect("written");
        }

        /// The same write, attempted through the handle a reading uses.
        pub fn write_through_reading(subkey: &str, value: &str) -> Result<(), String> {
            let key = reading(subkey).expect("opened").expect("the key exists");
            set(&key, "value", value, REG_SZ)
        }

        /// Removes a scratch key.
        pub fn remove(subkey: &str) {
            // SAFETY: the name is NUL-terminated and outlives the call.
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(subkey).as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\a\AppData\Local\tpdf";

    #[test]
    fn a_folder_is_added_last_and_once() {
        assert_eq!(with("", DIR).as_deref(), Some(DIR));
        assert_eq!(
            with(r"C:\one;%USERPROFILE%\bin", DIR),
            Some(format!(r"C:\one;%USERPROFILE%\bin;{DIR}"))
        );
        // A trailing separator does not become an empty entry.
        assert_eq!(with(r"C:\one;", DIR), Some(format!(r"C:\one;{DIR}")));
        // Already there, however it is written: nothing to write.
        for there in [
            DIR.to_string(),
            format!(r"C:\one;{DIR};C:\two"),
            format!(r"C:\one;{}\", DIR.to_uppercase()),
            format!("C:\\one;\"{DIR}\""),
            format!(r"C:\one; {DIR} "),
        ] {
            assert_eq!(with(&there, DIR), None, "{there}");
        }
        // A folder that only starts the same is another folder.
        let longer = format!(r"{DIR}-old");
        assert_eq!(with(&longer, DIR), Some(format!("{longer};{DIR}")));
    }

    #[test]
    fn a_folder_is_removed_and_every_other_entry_is_kept_as_written() {
        assert_eq!(without(r"C:\one;C:\two", DIR), None);
        assert_eq!(without("", DIR), None);
        assert_eq!(without(DIR, DIR).as_deref(), Some(""));
        assert_eq!(
            without(&format!(r"C:\One;{DIR};%USERPROFILE%\bin"), DIR).as_deref(),
            Some(r"C:\One;%USERPROFILE%\bin")
        );
        // Every spelling of it goes, and twice is twice.
        assert_eq!(
            without(&format!("{}\\;C:\\one;\"{DIR}\"", DIR.to_uppercase()), DIR).as_deref(),
            Some(r"C:\one")
        );
        // A folder that only starts the same stays.
        let longer = format!(r"{DIR}-old");
        assert_eq!(
            without(&format!("{longer};{DIR}"), DIR).as_deref(),
            Some(longer.as_str())
        );
    }

    /// A scratch key of this test's own, away from the user's environment.
    #[cfg(windows)]
    fn scratch(name: &str) -> String {
        let key = format!(r"Software\tpdf-test-{}-{name}", std::process::id());
        super::windows::probe::remove(&key);
        super::windows::probe::write(&key, "first");
        // The registry's clock moves in steps of up to 16 ms, so a write made
        // straight after is not told from the one that made the key.
        std::thread::sleep(std::time::Duration::from_millis(100));
        key
    }

    #[cfg(windows)]
    #[test]
    fn asking_whether_a_folder_is_stored_writes_nothing() {
        use super::windows::probe::{remove, write, written, PATH_KEY};
        // The control: the instrument sees a write that changes the value.
        // It does not see one that stores the bytes already there --- measured
        // on Windows 11, 2026-10-06, where that left the key's time alone ---
        // which is why the test below, of the handle, is the one that rules a
        // write out and this one only shows that nothing changed.
        let key = scratch("control");
        let before = written(&key);
        write(&key, "second");
        assert_ne!(written(&key), before, "a write did not move the key's time");
        remove(&key);

        // The user's real `PATH`, asked about and never written by a test.
        let before = written(PATH_KEY);
        let (value, kind) = super::windows::read().expect("the PATH is readable");
        for dir in [DIR, r"C:\Windows", ""] {
            assert_eq!(
                stored(dir).expect("asked"),
                with(&value, dir).is_none(),
                "{dir}"
            );
        }
        assert_eq!(written(PATH_KEY), before, "asking wrote the key");
        assert_eq!(
            super::windows::read().expect("still readable"),
            (value, kind)
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_handle_a_reading_holds_cannot_write() {
        use super::windows::probe::{remove, write_through_reading, written};
        let key = scratch("handle");
        let before = written(&key);
        let refused = write_through_reading(&key, "second").expect_err("a reading handle wrote");
        // 5 is ERROR_ACCESS_DENIED.
        assert!(refused.contains("(error 5)"), "{refused}");
        assert_eq!(written(&key), before);
        remove(&key);
    }
}
