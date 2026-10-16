//! The PNG `tpdf-cli` wrote, decoded by the system's WIC decoder and turned
//! into a bitmap Explorer can take.
//!
//! WIC is what Explorer itself decodes every PNG it shows with, so this adds
//! no decoder to the process. The bytes it is handed were written by tpdf's
//! own encoder from raw tiles, and are checked to be a PNG of no more than
//! [`plan::MAX_PNG`] before WIC sees them.

use windows::core::Result;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use crate::plan::{self, Fit};

/// A decoded page: top-down rows of blue, green, red and alpha bytes.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// `png`, decoded, if it is a PNG no larger than `fit`. The calling thread
/// must have COM initialised, as every thread Explorer calls a handler on has.
pub fn decode(png: &[u8], fit: Fit) -> Result<Pixels> {
    if !plan::is_png(png) {
        return Err(E_FAIL.into());
    }
    // SAFETY: plain WIC calls on objects this function owns; `png` outlives
    // the stream that reads it, which is dropped before this returns.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(png)?;
        let decoder = factory.CreateDecoderFromStream(
            &stream,
            std::ptr::null(),
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0u32, 0u32);
        frame.GetSize(&mut width, &mut height)?;
        if !plan::fits(width, height, fit) {
            return Err(E_FAIL.into());
        }
        let converted = WICConvertBitmapSource(&GUID_WICPixelFormat32bppBGRA, &frame)?;
        let stride = width * 4;
        // At most MAX_SIDE squared times four: 64 MiB, by `plan::fits`.
        let mut bgra = vec![0u8; stride as usize * height as usize];
        converted.CopyPixels(std::ptr::null(), stride, &mut bgra)?;
        Ok(Pixels {
            width,
            height,
            bgra,
        })
    }
}

/// `pixels` as a top-down 32-bit DIB section. The caller owns the handle:
/// Explorer for a thumbnail, the preview pane otherwise.
pub fn bitmap(pixels: &Pixels) -> Result<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: pixels.width as i32,
            // Negative: the first row is the top one, as WIC wrote them.
            biHeight: -(pixels.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: `info` describes a DIB of exactly `pixels.bgra.len()` bytes,
    // which is what is copied into the memory the call returns.
    unsafe {
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if bits.is_null() {
            return Err(E_FAIL.into());
        }
        std::ptr::copy_nonoverlapping(pixels.bgra.as_ptr(), bits.cast::<u8>(), pixels.bgra.len());
        Ok(bitmap)
    }
}
