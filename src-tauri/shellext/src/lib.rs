//! The Explorer handler: a thumbnail provider and a preview handler for
//! `.pdf`, loaded by Explorer's COM surrogate and by `prevhost.exe`. See
//! `Cargo.toml` and `docs/THREAT-MODEL.md` §T6.47.
//!
//! It parses no PDF. Each page is rendered by `tpdf-cli.exe` beside this DLL,
//! in a job object ([`render`]), and comes back as a PNG that the system's
//! decoder reads ([`png`]). Registration is done by the application
//! (`explorer.rs`), so the only exports are the two COM needs to make an
//! object: `DllGetClassObject` and `DllCanUnloadNow`.
//!
//! Only [`plan`] and the class identifiers are compiled on other platforms,
//! so their tests run anywhere; the rest is Windows code.

// MSVC's linker asks for the two COM exports to be PRIVATE (LNK4104), which
// keeps them out of the import library. Nothing links against this DLL; COM
// finds them with `GetProcAddress`, which a PRIVATE export does not change.
#![allow(linker_messages)]

pub mod plan;

/// The thumbnail provider's class, as the registry spells it. `explorer.rs`
/// writes the same string; a test there holds the two equal.
pub const THUMBNAIL_CLSID: &str = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A21}";

/// The preview handler's class, as the registry spells it.
pub const PREVIEW_CLSID: &str = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A22}";

#[cfg(windows)]
mod png;
#[cfg(windows)]
mod preview;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod thumbnail;

#[cfg(windows)]
pub use windows_parts::*;

#[cfg(windows)]
mod windows_parts {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
    use std::sync::Arc;

    use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, HRESULT};
    use windows::Win32::Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HINSTANCE, HMODULE,
        S_FALSE, S_OK,
    };
    use windows::Win32::System::Com::{
        IClassFactory, IClassFactory_Impl, IStream, STATFLAG_NONAME, STATSTG,
    };
    use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
    use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

    use crate::plan;

    /// The thumbnail provider's class.
    pub const THUMBNAIL: GUID = GUID::from_u128(0x7D5A3B9E_2C41_4E8F_9A6B_1F0C3D5E7A21);

    /// The preview handler's class.
    pub const PREVIEW: GUID = GUID::from_u128(0x7D5A3B9E_2C41_4E8F_9A6B_1F0C3D5E7A22);

    /// This DLL's module handle, saved when it is loaded.
    static MODULE: AtomicIsize = AtomicIsize::new(0);

    /// Objects alive, and threads of this DLL still running. COM may unload
    /// the DLL only when this and [`LOCKS`] are both zero.
    static OBJECTS: AtomicUsize = AtomicUsize::new(0);

    /// `IClassFactory::LockServer` calls not yet undone.
    static LOCKS: AtomicUsize = AtomicUsize::new(0);

    /// Held by every object, and by every thread that runs code in this DLL
    /// after the call that started it returned, so that the DLL is not
    /// unloaded under it.
    pub(crate) struct Counted;

    impl Counted {
        pub(crate) fn new() -> Self {
            OBJECTS.fetch_add(1, Ordering::SeqCst);
            Counted
        }
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            OBJECTS.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub(crate) fn module() -> HINSTANCE {
        HINSTANCE(MODULE.load(Ordering::SeqCst) as *mut c_void)
    }

    /// `tpdf-cli.exe` in the folder this DLL was loaded from: the install
    /// folder, or `target\<profile>` for a build under test.
    pub(crate) fn cli_path() -> Option<PathBuf> {
        let mut name = vec![0u16; 32 * 1024];
        // SAFETY: the buffer and its length are passed together; the module
        // handle is this DLL's own, saved at load.
        let len = unsafe { GetModuleFileNameW(Some(HMODULE(module().0)), &mut name) } as usize;
        if len == 0 || len >= name.len() {
            return None;
        }
        let dll = PathBuf::from(String::from_utf16(&name[..len]).ok()?);
        Some(dll.parent()?.join("tpdf-cli.exe"))
    }

    /// `body`, with a panic turned into an error. Unwinding across a COM
    /// method would abort the host process, which is Explorer's surrogate or
    /// `prevhost.exe`.
    pub(crate) fn guarded<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
            .unwrap_or_else(|_| Err(E_FAIL.into()))
    }

    /// All of `stream`, if it is no longer than [`plan::MAX_DOCUMENT`]. The
    /// size the stream reports is checked first, and the read loop holds the
    /// same limit whatever it reported.
    pub(crate) fn read_stream(stream: &IStream) -> Result<Arc<[u8]>> {
        let mut stat = STATSTG::default();
        // SAFETY: an out parameter of the right type; with STATFLAG_NONAME no
        // name is allocated that would need freeing.
        if unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.is_ok()
            && stat.cbSize > plan::MAX_DOCUMENT
        {
            return Err(E_FAIL.into());
        }
        let mut document = Vec::new();
        let mut chunk = vec![0u8; 1024 * 1024];
        loop {
            let mut read = 0u32;
            // SAFETY: the buffer and its length are passed together.
            let result = unsafe {
                stream.Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&mut read),
                )
            };
            if result.is_err() {
                return Err(result.into());
            }
            let read = (read as usize).min(chunk.len());
            if read == 0 {
                break;
            }
            if document.len() as u64 + read as u64 > plan::MAX_DOCUMENT {
                return Err(E_FAIL.into());
            }
            document.extend_from_slice(&chunk[..read]);
        }
        if document.is_empty() {
            return Err(E_FAIL.into());
        }
        Ok(document.into())
    }

    /// Which object a factory makes.
    #[derive(Clone, Copy)]
    enum Kind {
        Thumbnail,
        Preview,
    }

    // Not agile, as `preview.rs` explains.
    #[implement(IClassFactory, Agile = false)]
    struct Factory {
        kind: Kind,
        _counted: Counted,
    }

    impl IClassFactory_Impl for Factory_Impl {
        fn CreateInstance(
            &self,
            outer: Ref<windows::core::IUnknown>,
            iid: *const GUID,
            object: *mut *mut c_void,
        ) -> Result<()> {
            if object.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: checked for null above; COM's out parameter.
            unsafe { *object = std::ptr::null_mut() };
            if !outer.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            guarded(|| {
                let made: windows::core::IUnknown = match self.kind {
                    Kind::Thumbnail => crate::thumbnail::Thumbnail::new().into(),
                    Kind::Preview => crate::preview::Preview::new().into(),
                };
                // SAFETY: COM's own pointers, passed through.
                unsafe { made.query(iid, object) }.ok()
            })
        }

        fn LockServer(&self, lock: BOOL) -> Result<()> {
            if lock.as_bool() {
                LOCKS.fetch_add(1, Ordering::SeqCst);
            } else {
                // Never below zero, whatever a caller does.
                let _ =
                    LOCKS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
            }
            Ok(())
        }
    }

    /// Saves the module handle. Nothing else runs under the loader lock.
    ///
    /// # Safety
    ///
    /// Called by the Windows loader only.
    #[no_mangle]
    pub unsafe extern "system" fn DllMain(
        module: HINSTANCE,
        reason: u32,
        _reserved: *mut c_void,
    ) -> BOOL {
        if reason == DLL_PROCESS_ATTACH {
            MODULE.store(module.0 as isize, Ordering::SeqCst);
            // SAFETY: this DLL's own handle; it only stops thread notices.
            let _ = unsafe { DisableThreadLibraryCalls(HMODULE(module.0)) };
        }
        BOOL::from(true)
    }

    /// The class factory for one of this DLL's two classes.
    ///
    /// # Safety
    ///
    /// Called by COM, with valid pointers or null ones, which are refused.
    #[no_mangle]
    pub unsafe extern "system" fn DllGetClassObject(
        class: *const GUID,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> HRESULT {
        if class.is_null() || iid.is_null() || object.is_null() {
            return E_POINTER;
        }
        // SAFETY: checked for null above.
        unsafe { *object = std::ptr::null_mut() };
        // SAFETY: checked for null above.
        let kind = match unsafe { *class } {
            THUMBNAIL => Kind::Thumbnail,
            PREVIEW => Kind::Preview,
            _ => return CLASS_E_CLASSNOTAVAILABLE,
        };
        let factory: IClassFactory = Factory {
            kind,
            _counted: Counted::new(),
        }
        .into();
        // SAFETY: COM's own pointers, passed through.
        unsafe { factory.query(iid, object) }
    }

    /// Whether COM may unload this DLL: no object alive, no thread of it
    /// running, no lock held. The preview pane's window class is
    /// unregistered then, since a class whose procedure is in an unloaded DLL
    /// would crash the next window made of it.
    ///
    /// # Safety
    ///
    /// Called by COM.
    #[no_mangle]
    pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
        if OBJECTS.load(Ordering::SeqCst) == 0 && LOCKS.load(Ordering::SeqCst) == 0 {
            crate::preview::unregister_class();
            S_OK
        } else {
            S_FALSE
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_classes_are_the_strings_the_registry_is_given() {
            let spelled = |guid: GUID| format!("{{{guid:?}}}");
            assert!(spelled(THUMBNAIL).eq_ignore_ascii_case(crate::THUMBNAIL_CLSID));
            assert!(spelled(PREVIEW).eq_ignore_ascii_case(crate::PREVIEW_CLSID));
        }
    }
}
