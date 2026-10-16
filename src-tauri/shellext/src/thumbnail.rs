//! The thumbnail provider. Explorer runs it in its COM surrogate
//! (`dllhost.exe`), hands it the file as a stream, and asks for a bitmap of a
//! given size; the first page comes from `tpdf-cli`, never from code here.

use std::cell::RefCell;
use std::sync::Arc;

use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_UNEXPECTED};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_RGB, WTS_ALPHATYPE,
};

use crate::{plan, png, render, Counted};

// Not agile, as `preview.rs` explains.
#[implement(IInitializeWithStream, IThumbnailProvider, Agile = false)]
pub struct Thumbnail {
    document: RefCell<Option<Arc<[u8]>>>,
    _counted: Counted,
}

impl Thumbnail {
    pub fn new() -> Self {
        Thumbnail {
            document: RefCell::new(None),
            _counted: Counted::new(),
        }
    }
}

impl IInitializeWithStream_Impl for Thumbnail_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> Result<()> {
        let stream = stream.ok()?;
        // Read now rather than at `GetThumbnail`: the stream is Explorer's,
        // and a handler is initialised once.
        let document = crate::read_stream(stream)?;
        *self.document.borrow_mut() = Some(document);
        Ok(())
    }
}

impl IThumbnailProvider_Impl for Thumbnail_Impl {
    fn GetThumbnail(&self, cx: u32, bitmap: *mut HBITMAP, alpha: *mut WTS_ALPHATYPE) -> Result<()> {
        if bitmap.is_null() || alpha.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let fit = plan::thumbnail_fit(cx).ok_or(windows::core::Error::from(E_INVALIDARG))?;
        let document = self
            .document
            .borrow()
            .clone()
            .ok_or(windows::core::Error::from(E_UNEXPECTED))?;
        let made = crate::guarded(|| {
            let cli = crate::cli_path().ok_or(windows::core::Error::from(E_FAIL))?;
            let png = render::page(&cli, document, fit, 1, plan::THUMBNAIL_TIME)
                .map_err(|_| windows::core::Error::from(E_FAIL))?;
            png::bitmap(&png::decode(&png, fit)?)
        })?;
        // SAFETY: both pointers were checked for null above and are Explorer's
        // out parameters; the bitmap is now Explorer's to free.
        unsafe {
            *bitmap = made;
            // Opaque: the page is drawn on white.
            *alpha = WTSAT_RGB;
        }
        Ok(())
    }
}
