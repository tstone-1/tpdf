//! Handing a print job to Windows.
//!
//! The counterpart of `print_macos.rs`, and it is worth being precise about which
//! parts correspond and which cannot. `print.rs` decides *which* PDF and is
//! entirely portable; this module decides nothing about the document.
//!
//! **[`read`] is the same guarantee, from the same kind of source.** On macOS that
//! is PDFKit; here it is `Windows.Data.Pdf`, which is the operating system's own
//! PDF stack --- the one Windows uses for Explorer thumbnails and preview, and the
//! one behind Edge's viewer. So it is a genuinely independent third parser:
//! independent of `lopdf`, which wrote the job, and of PDFium, which drew what the
//! reader was looking at. Asking it what it sees before a panel opens turns "the
//! subset saved without an error" into "something else can read the pages we claim
//! to have produced", which is the standard `docs/PLAN.md` §6 sets for a redaction.
//!
//! **[`present`] is *not* the same mechanism, and the difference is not a
//! shortcut.** macOS hands PDF bytes to `NSPrintOperation` and the OS paginates and
//! prints them as vectors. Windows has no in-box "print this PDF" API at any layer
//! --- not Win32, not WinRT --- so every Windows PDF viewer, SumatraPDF included,
//! rasterises each page onto a printer device context itself. That is what happens
//! here: PDFium draws each page in a sandboxed worker started over the job
//! ([`spool`]), and GDI moves the bitmap to the spooler. The consequence to state
//! plainly is that Windows output is **raster at a chosen DPI** where macOS output
//! is vector, so text is not selectable in a print-to-PDF result and very fine
//! hairlines depend on [`PRINT_DPI`].
//!
//! **Drawn in a worker, not by the OS, since 2026-10-20.** `Windows.Data.Pdf` drew
//! the pages here before that, in this process, and took over two minutes a sheet
//! on a dense drawing where PDFium takes about half a minute (`docs/VERIFICATION.md`,
//! *How long Windows takes to rasterise a page for printing*). Sending the printer
//! vectors instead was measured and rejected on the same page: the printer took ten
//! minutes and reported an error. So the page is still a picture, drawn by the
//! engine that drew what the reader saw, inside the boundary every other render
//! holds; this process only reads the job back with `Windows.Data.Pdf` before the
//! dialog ([`read`]) and copies pixels to the printer.
//!
//! **No PDFium in this process.** The worker maps `pdfium.dll`; the app process
//! still never does. Reading the job back with a Microsoft component is the same
//! argument macOS makes for using PDFKit rather than PDFium to read a job back.
//!
//! **What is verified, and what needs paper.** [`read`], [`render_page`] and the
//! band arithmetic are covered by tests below and by the four checks in `print.rs`
//! that now run on both platforms. [`render_page`] is `Windows.Data.Pdf`'s own
//! drawing, kept for the probe, which uses it as an independent eye on what was
//! sent and what came out. `present` opening a real panel and a real printer consuming the
//! result is not covered by anything automatic --- the same honest gap
//! `print_macos.rs` records --- though `examples/print_probe.rs` drives everything up to
//! and including the spooler by opening a printer DC directly.

use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions, PdfPageRotation};
use windows::Graphics::Imaging::BitmapEncoder;
use windows::Storage::Streams::{DataWriter, IOutputStream, InMemoryRandomAccessStream};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteDC, GetDeviceCaps, StretchDIBits, BITMAPINFO, DIB_RGB_COLORS, HDC, HORZRES, LOGPIXELSX,
    LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, SRCCOPY, VERTRES,
};

use crate::print_layout::{Paper, Placement};
use crate::render::PageSize;
use windows::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, PrintDlgW, PD_ALLPAGES, PD_ENABLEPRINTHOOK, PD_NOSELECTION, PD_PAGENUMS,
    PD_RETURNDC, PRINTDLGW,
};

/// The resolution pages are rasterised at, in dots per inch.
///
/// 300 is the conventional document-printing resolution and the point where a
/// raster page stops being visibly a raster page. It is not free: an A4 page at
/// 300 dpi is 2480x3508 pixels, so about 35 MB as 32-bit BGRA, and that buffer
/// exists once per page rather than once per job.
///
/// Deliberately a constant and not the printer's own `LOGPIXELSX`. A modern laser
/// printer reports 600 or 1200, which would quadruple or sixteen-fold the buffer
/// for a difference no one can see on text, and a 1200 dpi A0 sheet is over 2 GB
/// --- a job that would fail on the allocation rather than print badly. The
/// printer's resolution is still read, and used to *scale* what is sent.
pub const PRINT_DPI: f32 = 300.0;

/// The units `PdfPage::Size` answers in: device-independent pixels, 96 to the inch.
///
/// **Not 72, which is what a PDF page is measured in and what this constant said
/// first.** WinRT converts to DIPs before reporting, so an A4 page --- 595x842
/// points by definition --- comes back as 793.33x1122.67. Dividing by 72 therefore
/// asks for a render 96/72 too large in each dimension, and the render obliges: a
/// 200x100 page came out 267x133.
///
/// Worth stating why that mattered more than an arithmetic slip usually does. The
/// error is a uniform 1.33x, so every page still rendered, still had the right
/// aspect ratio, and was still scaled down to fit the sheet by `draw_cut` --- which
/// means the *printed* output would have been very slightly soft and nothing else,
/// on a path whose only honest verification is paper. It was caught by asserting the
/// pixel dimensions at two different resolutions, which is a check on the units
/// rather than on the picture.
const WINRT_DPI: f32 = 96.0;

/// The units a PDF page, and so the worker's page size, is measured in.
const POINTS_PER_INCH: f32 = 72.0;

/// How long the worker may take to draw one band of a page for printing.
///
/// Not [`crate::workers::DEFAULT_DEADLINE`]'s 30 s: PDFium took 33 s for the
/// whole first page of `vector-multi.pdf` at the resolution an A4 sheet asks for
/// (`docs/VERIFICATION.md`), and a slow page is a page to print, with progress
/// shown meanwhile, not a failure. Still a bound, so a worker that hangs ends
/// the job rather than holding it for ever.
const BAND_DEADLINE: std::time::Duration = std::time::Duration::from_secs(600);

/// What the OS parser makes of one page.
///
/// Field-for-field the same shape as `print_macos::PageReading`, so `print.rs` can
/// hold one set of expectations for both platforms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageReading {
    /// The page's rotation in degrees, `/Rotate` and everything it inherits.
    pub rotation: i64,
    /// The page's text, and `None` when it was not asked for.
    ///
    /// **On Windows it is always `None`,** because `Windows.Data.Pdf` has no text
    /// extraction at all --- it renders and reports geometry, and that is the whole
    /// surface. Kept in the struct rather than removed so the two platforms share a
    /// type, and typed as an `Option` for exactly the reason macOS gives: "we did
    /// not extract this" and "this page has no extractable text" are different
    /// facts, and collapsing them to an empty string is the same defect as a leak
    /// scanner reporting *clean* on a carrier it could not decode. A check that
    /// needs text must skip out loud here; see `print.rs`.
    pub text: Option<String>,
}

/// What the OS parser makes of a whole document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub pages: Vec<PageReading>,
}

/// Parses PDF bytes with `Windows.Data.Pdf` and reads the document's *shape*.
///
/// Page count and rotations only, which is everything the pre-print check needs.
///
/// `None` means the OS parser refused the document outright. It is deliberately
/// lenient, as every shipping PDF parser is, so this proves the file is *readable*
/// and not that it is well formed --- the same distinction `docs/TRAPS.md` draws
/// about PDFium rendering a document whose `/Size` is wrong.
#[must_use]
pub fn read(bytes: &[u8]) -> Option<Reading> {
    let document = parse(bytes)?;
    let count = document.PageCount().ok()?;
    let mut pages = Vec::with_capacity(count as usize);
    for index in 0..count {
        // A page the document counted and cannot produce. Reporting a short list
        // would read as a shorter document rather than a broken one --- the same
        // choice `print_macos::read_inner` makes.
        let page = document.GetPage(index).ok()?;
        pages.push(PageReading {
            rotation: rotation_degrees(&page)?,
            text: None,
        });
    }
    Some(Reading { pages })
}

/// A page's rotation in degrees.
///
/// WinRT reports an enum with four values rather than a number, so this is a
/// translation and not a computation. Written as an exhaustive match on purpose:
/// the arms are the only four values the format permits, and a `_ => 0` default
/// would silently report an unrotated page for anything else, which is the
/// direction that loses a quarter turn without saying so.
fn rotation_degrees(page: &PdfPage) -> Option<i64> {
    match page.Rotation().ok()? {
        PdfPageRotation::Normal => Some(0),
        PdfPageRotation::Rotate90 => Some(90),
        PdfPageRotation::Rotate180 => Some(180),
        PdfPageRotation::Rotate270 => Some(270),
        // Not reachable through the four constants above, and not an error either:
        // it would mean a future SDK grew a fifth value. `None` puts it on the
        // "could not read this" path rather than inventing an angle.
        _ => None,
    }
}

/// Loads bytes into a `PdfDocument` through an in-memory stream.
///
/// WinRT has no "load from a byte slice" entry point --- everything goes through
/// `IRandomAccessStream` --- so the bytes are written into an in-memory stream
/// first. That is a copy, and it is the same copy `NSData::with_bytes` makes on the
/// other platform.
fn parse(bytes: &[u8]) -> Option<PdfDocument> {
    let stream = to_stream(bytes)?;
    // `.join()` blocks. That is correct here and not laziness: every caller is
    // either a test or a command already running off the UI thread, and a print
    // panel must not open before the document behind it has been read.
    PdfDocument::LoadFromStreamAsync(&stream).ok()?.join().ok()
}

/// Copies a slice into a fresh WinRT in-memory stream, rewound to the start.
fn to_stream(bytes: &[u8]) -> Option<InMemoryRandomAccessStream> {
    let stream = InMemoryRandomAccessStream::new().ok()?;
    let sink: IOutputStream = stream.cast().ok()?;
    let writer = DataWriter::CreateDataWriter(&sink).ok()?;
    writer.WriteBytes(bytes).ok()?;
    // Without this the bytes sit in the writer and the stream is empty, which
    // presents as a document the parser refuses rather than as a programming
    // mistake.
    writer.StoreAsync().ok()?.join().ok()?;
    writer.FlushAsync().ok()?.join().ok()?;
    // **Load-bearing, and the omission of it is invisible.** A `DataWriter` owns the
    // output stream it was created over and closes it when the last reference goes
    // away --- which, in Rust, is the end of this function. Without the detach the
    // stream handed back is a *closed* stream, and the only symptom is
    // `LoadFromStreamAsync` reporting that it cannot read the document: exactly what
    // a malformed PDF looks like, on a PDF that is fine.
    //
    // It cost a diagnosis. Written inline in a probe the same code worked, because
    // there the writer was still alive when the stream was used; moved into a helper
    // that returns the stream, every document became unreadable. The discriminator
    // was a known-good fixture, which is the general lesson --- the failure was
    // reported against our own hand-rolled test PDF first, and believing that would
    // have sent the fix into the fixture generator.
    writer.DetachStream().ok()?;
    stream.Seek(0).ok()?;
    Some(stream)
}

/// One page rasterised by the OS, as a Windows BMP.
///
/// A BMP rather than the default PNG, and that choice is what keeps this module
/// free of an image decoder: a BMP *is* a DIB with a 14-byte file header in front
/// of it, so [`draw_cut`] hands the bytes after that header straight to
/// `StretchDIBits`. Asking for PNG would mean pulling in a decoder to undo an
/// encode that never needed to happen.
///
/// Public because `examples/print_probe.rs` and the tests below both need to look at a
/// rendered page without a printer involved.
///
/// # Errors
///
/// The page index being out of range, or WinRT refusing to render or encode.
pub fn render_page(bytes: &[u8], index: u32, dpi: f32) -> Result<Vec<u8>, String> {
    let document =
        parse(bytes).ok_or_else(|| said!("the OS parser could not read the print job"))?;
    let page = document
        .GetPage(index)
        .map_err(|e| format!("page {index}: {e}"))?;
    render_page_of(&page, dpi)
}

/// As [`render_page`], for a page already in hand.
fn render_page_of(page: &PdfPage, dpi: f32) -> Result<Vec<u8>, String> {
    let size = page.Size().map_err(|e| format!("page size: {e}"))?;
    let scale = dpi / WINRT_DPI;
    // Rounded away from zero: a 0-pixel dimension is not a small page, it is a
    // render WinRT refuses, and `as u32` on a sub-1.0 value produces exactly that.
    let width = ((size.Width * scale).round() as u32).max(1);
    let height = ((size.Height * scale).round() as u32).max(1);

    let options = PdfPageRenderOptions::new().map_err(|e| format!("render options: {e}"))?;
    options
        .SetBitmapEncoderId(BitmapEncoder::BmpEncoderId().map_err(|e| format!("bmp id: {e}"))?)
        .map_err(|e| format!("selecting the BMP encoder: {e}"))?;
    // Both dimensions, not one. Setting only the width lets WinRT derive the
    // height from the page's own aspect ratio, which is *usually* the same answer
    // and is not the same answer for a page whose `/MediaBox` and `/CropBox`
    // disagree --- and a page that came back a few pixels short would be scaled to
    // fit the sheet by `draw_cut` and print very slightly wrong.
    options
        .SetDestinationWidth(width)
        .map_err(|e| format!("destination width: {e}"))?;
    options
        .SetDestinationHeight(height)
        .map_err(|e| format!("destination height: {e}"))?;

    let stream = InMemoryRandomAccessStream::new().map_err(|e| format!("render stream: {e}"))?;
    page.RenderWithOptionsToStreamAsync(&stream, &options)
        .map_err(|e| format!("render: {e}"))?
        .join()
        .map_err(|e| format!("render: {e}"))?;
    read_stream(&stream)
}

/// Drains a WinRT stream into a `Vec`.
fn read_stream(stream: &InMemoryRandomAccessStream) -> Result<Vec<u8>, String> {
    use windows::Storage::Streams::{DataReader, InputStreamOptions};

    let len = stream.Size().map_err(|e| format!("stream size: {e}"))? as usize;
    stream.Seek(0).map_err(|e| format!("stream seek: {e}"))?;
    let reader =
        DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(|e| e.to_string())?)
            .map_err(|e| format!("stream reader: {e}"))?;
    // Without `ReadAhead` a partial read is legal and this would silently return
    // a truncated image, which `draw_cut` would then reject as a malformed header
    // --- a confusing report of a problem that is not there.
    reader
        .SetInputStreamOptions(InputStreamOptions::ReadAhead)
        .map_err(|e| e.to_string())?;
    let got = reader
        .LoadAsync(len as u32)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|e| e.to_string())? as usize;
    if got != len {
        return Err(format!("stream gave {got} of {len} bytes"));
    }
    let mut out = vec![0u8; len];
    reader
        .ReadBytes(&mut out)
        .map_err(|e| format!("stream read: {e}"))?;
    Ok(out)
}

// ---------------------------------------------------------------------- printing

/// Where the pixel data starts in a BMP, and where its info header does.
///
/// `BITMAPFILEHEADER` is 14 bytes and is the *only* part of a BMP that is not
/// already a DIB. Everything after it --- the info header, any colour masks, the
/// palette --- is exactly what `StretchDIBits` wants, which is why the encoder
/// choice in [`render_page_of`] matters.
const BMP_FILE_HEADER: usize = 14;

/// The smallest DIB header a BMP may carry, `BITMAPINFOHEADER`.
///
/// Later headers --- `BITMAPV4HEADER` at 108 bytes, `BITMAPV5HEADER` at 124 ---
/// extend it, and GDI reads whichever one the `biSize` field announces. So the
/// header is copied by its own declared length rather than by a fixed 40.
const DIB_HEADER_MIN: usize = 40;

/// `biCompression` for an uncompressed image, the one the OS encoder produces.
///
/// Named here rather than imported: the `windows` crate wraps it in a newtype
/// that would have to be unwrapped to compare against a field read out of a byte
/// slice, which buys nothing over the two numbers the format defines.
const BI_RGB: u32 = 0;

/// `biCompression` for an uncompressed image whose channels are given as masks.
///
/// Uncompressed like [`BI_RGB`] --- the name is the format's, not a description
/// --- so the pixel data is the same size. What differs is that three 32-bit
/// masks follow a 40-byte header, in the space a palette would occupy, and GDI
/// reads them through the same pointer as the header itself.
const BI_BITFIELDS: u32 = 3;

/// A parsed BMP, with its header copied somewhere GDI can read it.
///
/// **The header is owned, not borrowed, and that is not a stylistic choice.** A
/// BMP's DIB header begins at byte 14, which is never 4-byte aligned, so taking a
/// `&BITMAPINFO` into the buffer is a misaligned reference --- undefined behaviour
/// on a struct of `u32` fields, however well it happens to work on x86. The first
/// version of this did exactly that and Rust's debug assertions caught it
/// immediately: *"misaligned pointer dereference: address must be a multiple of
/// 0x4"*, aborting the whole test binary with `STATUS_STACK_BUFFER_OVERRUN` rather
/// than failing one test. Backed by a `Vec<u32>` because that allocation is
/// 4-byte aligned by construction, which is the property being bought.
struct Dib<'a> {
    /// Aligned storage for the header, and any colour masks or palette after it.
    header: Vec<u32>,
    bits: &'a [u8],
    width: i32,
    height: i32,
    /// Bits per pixel, as the header declares it.
    ///
    /// GDI reads this back out of `header`, so it is here for [`Raster`] rather
    /// than for [`draw_cut`] --- a pixel comparison needs the stride, and the
    /// stride is a function of this and the width.
    bpp: u16,
    /// Whether the rows run top to bottom, which a BMP says with a negative height.
    ///
    /// [`Self::height`] is the magnitude, because that is what `StretchDIBits`
    /// wants, so the sign has to survive somewhere or it is lost.
    top_down: bool,
}

impl Dib<'_> {
    /// The header, as the type GDI's parameter wants.
    fn info(&self) -> *const BITMAPINFO {
        self.header.as_ptr().cast()
    }
}

/// How many bytes of pixel data `StretchDIBits` reads for a declared image.
///
/// **The one quantity in a BMP that is not in the BMP.** Every other field can be
/// checked against the buffer it came from; this one GDI *computes* from the
/// geometry and then reads that much through a pointer, so a `bits` slice shorter
/// than this is an out-of-bounds read inside the driver rather than an error
/// anybody sees --- and at `offset == bytes.len()` the slice is empty and the
/// pointer handed over is dangling.
///
/// Rows are padded to a 4-byte boundary, which is the part that is easy to omit
/// and impossible to notice: 32-bit rows are aligned already, so a stride
/// computed as `width * bpp / 8` agrees with this on exactly the format the OS
/// encoder produces and disagrees on every other one.
///
/// Saturating rather than wrapping, because both factors come from the file: a
/// declared 2-billion-pixel image at 65,535 bits per pixel overflows the
/// multiplication, and a wrapped product is a *small* number, which is the one
/// answer that would wave the buffer through.
fn pixel_bytes(
    width: i32,
    rows: u32,
    bpp: u16,
    compression: u32,
    size_image: u32,
) -> Result<u64, String> {
    if compression != BI_RGB && compression != BI_BITFIELDS {
        // RLE, embedded JPEG and embedded PNG: the geometry says nothing about
        // the byte count and `biSizeImage` is the only statement of it, which the
        // format requires to be present for exactly this reason. Nothing here
        // produces one --- but guessing a stride for it would produce a bound
        // that is too small, which is worse than refusing.
        if size_image == 0 {
            return Err(format!(
                "BMP compression {compression} declares no image size"
            ));
        }
        return Ok(u64::from(size_image));
    }
    let stride = (u64::from(width.unsigned_abs()) * u64::from(bpp)).div_ceil(32) * 4;
    Ok(stride.saturating_mul(u64::from(rows)))
}

/// How much GDI reads through the *header* pointer, which is more than the header.
///
/// `BITMAPINFO` is a header followed by whatever the format puts in its trailing
/// array, and GDI reads that array through the same pointer [`Dib::info`] hands
/// it. Two things live there and both are sized by fields in the header itself,
/// so neither is covered by the `biSize` check: the channel masks of a
/// [`BI_BITFIELDS`] image with the original 40-byte header, and the palette of an
/// indexed one.
///
/// Nothing this module renders is either --- the OS encoder produces 32-bit
/// `BI_RGB` --- but a *header* saying it is would walk the aligned copy off its
/// end, which is the same class of read as a short `bits` and is invisible for
/// the same reason.
fn header_bytes(declared: usize, bpp: u16, compression: u32, clr_used: u32) -> u64 {
    let mut needed = declared as u64;
    if compression == BI_BITFIELDS && declared == DIB_HEADER_MIN {
        needed += 12;
    }
    if bpp <= 8 {
        // Zero means "all of them", which is what an image using every entry of
        // its depth is entitled to leave unsaid.
        let entries = if clr_used == 0 {
            1u64 << bpp
        } else {
            u64::from(clr_used)
        };
        needed += entries * 4;
    }
    needed
}

/// Reads just enough of a BMP to hand it to GDI.
///
/// Refuses rather than guesses, and reports which field was wrong. A BMP that
/// cannot be parsed here means WinRT produced something unexpected, and passing a
/// bad header to `StretchDIBits` is an access violation rather than an error.
///
/// The last two checks are about **how much memory GDI will read**, which the
/// header states only indirectly --- see [`pixel_bytes`] and [`header_bytes`].
/// They are the ones that make the sentence above true: a header can be
/// self-consistent in every field and still describe an image larger than the
/// bytes that arrived.
fn parse_bmp(bytes: &[u8]) -> Result<Dib<'_>, String> {
    if bytes.len() < BMP_FILE_HEADER + DIB_HEADER_MIN {
        return Err(format!("a {}-byte BMP is too short", bytes.len()));
    }
    if &bytes[..2] != b"BM" {
        return Err("not a BMP: no BM signature".to_owned());
    }
    let offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    if offset < BMP_FILE_HEADER + DIB_HEADER_MIN || offset > bytes.len() {
        return Err(format!("BMP pixel offset {offset} is out of range"));
    }
    let header = &bytes[BMP_FILE_HEADER..offset];
    let declared = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if declared < DIB_HEADER_MIN || declared > header.len() {
        return Err(format!(
            "BMP declares a {declared}-byte DIB header, with {} available",
            header.len()
        ));
    }
    let width = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let height = i32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    // A negative height means a top-down DIB, which is legal, so only the
    // magnitude is a size. Taken unsigned rather than through `abs`, because
    // `i32::MIN` has no positive counterpart and `abs` panics on it in debug ---
    // a hostile header should be refused here, not abort the process that was
    // about to refuse it. Exactly that one magnitude does not fit back into the
    // `i32` `StretchDIBits` wants, so it is refused rather than clamped.
    let rows = height.unsigned_abs();
    let Ok(source_rows) = i32::try_from(rows) else {
        return Err(format!("BMP reports a {width}x{height} image"));
    };
    if width <= 0 || rows == 0 {
        return Err(format!("BMP reports a {width}x{height} image"));
    }

    let bpp = u16::from_le_bytes([header[14], header[15]]);
    let compression = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    let size_image = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);
    let clr_used = u32::from_le_bytes([header[32], header[33], header[34], header[35]]);

    let reads = header_bytes(declared, bpp, compression, clr_used);
    if reads > header.len() as u64 {
        return Err(format!(
            "BMP needs {reads} bytes of header, masks and palette at {bpp} bpp, with {} available",
            header.len()
        ));
    }

    let bits = &bytes[offset..];
    let needed = pixel_bytes(width, rows, bpp, compression, size_image)?;
    if (bits.len() as u64) < needed {
        return Err(format!(
            "BMP declares a {width}x{height} image at {bpp} bpp, needing {needed} bytes of \
             pixels, with {} available",
            bits.len()
        ));
    }

    // Everything between the declared header and the pixels, which is the colour
    // masks for `BI_BITFIELDS` and the palette for anything indexed. Copied along
    // with the header because GDI reads it through the same pointer, and dropping
    // it would silently lose the channel masks of a 32-bit image.
    let mut aligned = vec![0u32; header.len().div_ceil(4)];
    // SAFETY: the destination is at least as long as the source, rounded up to a
    // whole number of `u32`s, and the two do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(
            header.as_ptr(),
            aligned.as_mut_ptr().cast::<u8>(),
            header.len(),
        );
    }

    Ok(Dib {
        header: aligned,
        bits,
        width,
        // The magnitude, since `StretchDIBits` takes the source rectangle in
        // whole rows and reads a top-down DIB's direction from the header.
        height: source_rows,
        bpp,
        top_down: height < 0,
    })
}

/// A rendered page as pixels a check can index, in the source's own orientation.
///
/// **Built on [`parse_bmp`] rather than beside it.** That function already refuses
/// every malformed header this one would have to, and a second reader of the same
/// bytes is a second thing to drift --- `examples/print_probe.rs` had one, written
/// from the offsets by hand, and it is this type now.
///
/// **`y` counts from the top, always.** A BMP is bottom-up unless its height is
/// negative, and a caller that gets that backwards reads a mark's rectangle at the
/// wrong end of the page --- which for a symmetric fixture produces a plausible
/// number rather than an obvious error. The flip happens once, here, so no check
/// has to know the convention.
pub struct Raster<'a> {
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    stride: usize,
    top_down: bool,
    bits: &'a [u8],
}

impl<'a> Raster<'a> {
    /// The pixels of a BMP produced by [`render_page`].
    ///
    /// # Errors
    ///
    /// Anything [`parse_bmp`] refuses, and a bit depth below 24 --- which is not a
    /// limitation so much as a statement about the caller: `render_page` asks WinRT
    /// for the BMP encoder's default, and an indexed image would need the palette
    /// applied before a pixel comparison meant anything.
    pub fn of(bytes: &'a [u8]) -> Result<Self, String> {
        let dib = parse_bmp(bytes)?;
        let bpp = dib.bpp as usize;
        if !(bpp == 24 || bpp == 32) {
            return Err(format!(
                "a {bpp}-bit render cannot be compared pixel for pixel without its palette"
            ));
        }
        let width = dib.width.unsigned_abs() as usize;
        let height = dib.height.unsigned_abs() as usize;
        let bytes_per_pixel = bpp / 8;
        // Rows are padded to a four-byte boundary. Ignoring that reads the padding
        // as pixels and drifts across the image, which on a mostly-white page gives
        // a small plausible count rather than an obvious error.
        let stride = (width * bytes_per_pixel).div_ceil(4) * 4;
        Ok(Self {
            width,
            height,
            bytes_per_pixel,
            stride,
            top_down: dib.top_down,
            bits: dib.bits,
        })
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// One pixel as `[b, g, r]`, with `y` counting down from the top row.
    ///
    /// Returns white for a coordinate outside the image, so a rectangle that
    /// overhangs the page reads as blank rather than panicking --- the same
    /// decision `mask_columns` makes about a region wider than its page.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        if x >= self.width || y >= self.height {
            return [0xFF, 0xFF, 0xFF];
        }
        let row = if self.top_down {
            y
        } else {
            self.height - 1 - y
        };
        let at = row * self.stride + x * self.bytes_per_pixel;
        match self.bits.get(at..at + 3) {
            Some(px) => [px[0], px[1], px[2]],
            None => [0xFF, 0xFF, 0xFF],
        }
    }

    /// Whether a pixel is anything other than white.
    #[must_use]
    pub fn inked(&self, x: usize, y: usize) -> bool {
        self.pixel(x, y) != [0xFF, 0xFF, 0xFF]
    }
}

/// The part of a placed page that lands in the printable area, and the pixels
/// that draw it.
///
/// A page is drawn at the size its [`Placement`] gives it, which since the
/// print settings moved into tpdf's own preview can be larger than the paper
/// (actual size, or a percentage) as well as smaller. Only the part inside the
/// printable area is rendered: the printer would clip the rest, and rendering
/// an A0 page at 400% to throw nearly all of it away is a buffer of gigabytes.
///
/// **The density is fixed on paper, not on the page.** `scale` is chosen so a
/// pixel is `1 / density` of an inch on the sheet, whatever the page's own
/// size: an A0 page shrunk to A4 renders at a quarter of `density` per inch of
/// A0, and lands at `density` on paper, which is what the old `paper_dpi` did
/// for the one case it knew --- every pixel beyond what the sheet can hold is
/// rendered, paid for and thrown away by the scaler otherwise, and a twelve-page
/// A0 job did not finish inside two minutes before that was fixed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cut {
    /// Render pixels per point of the page.
    scale: f32,
    /// Quarter turns clockwise the page is rendered by: 3 for a page the
    /// layout turned, whose top then lies along the sheet's left edge.
    turns: u8,
    /// The window of the rendered page to draw, in render pixels.
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// Where that window goes, in points from the printable area's corner.
    left: f32,
    top: f32,
    across: f32,
    down: f32,
}

/// What of a page of `size`, placed as `placed`, shows in a printable area of
/// `area` points, at `density` pixels per inch of paper. `None` when nothing
/// of it does.
fn cut(placed: &Placement, size: PageSize, area: (f32, f32), density: f32) -> Option<Cut> {
    // A turned page is rendered turned, so its width on paper is its height.
    let across = if placed.turned {
        size.height_pt
    } else {
        size.width_pt
    };
    if !(across > 0.0 && placed.width > 0.0 && placed.height > 0.0 && density > 0.0) {
        return None;
    }
    let per_point = density / POINTS_PER_INCH;
    let scale = placed.width / across * per_point;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (full_w, full_h) = (
        (placed.width * per_point).round().max(1.0) as u32,
        (placed.height * per_point).round().max(1.0) as u32,
    );
    let (left, right) = (placed.x.max(0.0), (placed.x + placed.width).min(area.0));
    let (top, bottom) = (placed.y.max(0.0), (placed.y + placed.height).min(area.1));
    if right <= left || bottom <= top {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let span = |from: f32, to: f32, origin: f32, full: u32| {
        let first = (((from - origin) * per_point).floor().max(0.0) as u32).min(full - 1);
        let last = (((to - origin) * per_point).ceil().max(0.0) as u32).clamp(first + 1, full);
        (first, last - first)
    };
    let (x, width) = span(left, right, placed.x, full_w);
    let (y, height) = span(top, bottom, placed.y, full_h);
    #[allow(clippy::cast_precision_loss)]
    Some(Cut {
        scale,
        turns: if placed.turned { 3 } else { 0 },
        x,
        y,
        width,
        height,
        left: placed.x + x as f32 / per_point,
        top: placed.y + y as f32 / per_point,
        across: width as f32 / per_point,
        down: height as f32 / per_point,
    })
}

/// Draws a rendered window of a page onto a device context where [`cut`]
/// placed it.
///
/// **Points become device units through the DC's own resolution, and a pixel
/// of the BMP is never taken for a device unit.** The first version of this
/// module did exactly that: a DIB rendered at 300 dpi placed onto a 600 dpi
/// printer DC unit-for-unit came out at half physical size, and on a page small
/// enough that the fit never engaged there was nothing to correct it. It
/// survived a passing `print-probe` run whose oracle was "less ink than was
/// sent", and was found by measuring the *extent* of ink on the sheet instead.
fn draw_cut(dc: HDC, bmp: &[u8], cut: &Cut, device_dpi: (i32, i32)) -> Result<(), String> {
    let dib = parse_bmp(bmp)?;
    #[allow(clippy::cast_precision_loss)]
    let (per_x, per_y) = (
        device_dpi.0.max(1) as f32 / POINTS_PER_INCH,
        device_dpi.1.max(1) as f32 / POINTS_PER_INCH,
    );
    #[allow(clippy::cast_possible_truncation)]
    let (x, y) = (
        (cut.left * per_x).round() as i32,
        (cut.top * per_y).round() as i32,
    );
    #[allow(clippy::cast_possible_truncation)]
    let (w, h) = (
        ((cut.left + cut.across) * per_x).round() as i32 - x,
        ((cut.top + cut.down) * per_y).round() as i32 - y,
    );
    if w <= 0 || h <= 0 {
        return Err(format!(
            "a {}x{} window maps to {w}x{h} device units",
            dib.width, dib.height
        ));
    }
    // SAFETY: a live DC from the caller, and a header and pixel slice both
    // validated by `parse_bmp` to describe the same image.
    let rows = unsafe {
        StretchDIBits(
            dc,
            x,
            y,
            w,
            h,
            0,
            0,
            dib.width,
            dib.height,
            Some(dib.bits.as_ptr().cast()),
            dib.info(),
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    if rows == 0 {
        return Err(format!(
            "StretchDIBits drew nothing: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// The printable extent of a device context, in its own device units.
fn sheet_size(dc: HDC) -> (i32, i32) {
    // SAFETY: a live DC; both indices are documented constants.
    unsafe {
        (
            GetDeviceCaps(Some(dc), HORZRES),
            GetDeviceCaps(Some(dc), VERTRES),
        )
    }
}

/// The device context's resolution, used to scale a page onto it.
fn dc_dpi(dc: HDC) -> (i32, i32) {
    // SAFETY: as above.
    unsafe {
        (
            GetDeviceCaps(Some(dc), LOGPIXELSX),
            GetDeviceCaps(Some(dc), LOGPIXELSY),
        )
    }
}

/// The paper a device context prints on, in points: the whole sheet and the
/// part of it the printer can reach. `print_layout` lays a job out on this,
/// and the preview shows the same numbers.
///
/// A device with no physical size --- a display, or a driver that does not
/// answer --- is taken to be all printable area.
#[must_use]
pub fn paper_of(dc: HDC) -> Paper {
    let (dpi_x, dpi_y) = dc_dpi(dc);
    let (area_w, area_h) = sheet_size(dc);
    // SAFETY: a live DC; the four indices are documented constants.
    let (sheet_w, sheet_h, left, top) = unsafe {
        (
            GetDeviceCaps(Some(dc), PHYSICALWIDTH),
            GetDeviceCaps(Some(dc), PHYSICALHEIGHT),
            GetDeviceCaps(Some(dc), PHYSICALOFFSETX),
            GetDeviceCaps(Some(dc), PHYSICALOFFSETY),
        )
    };
    let (sheet_w, sheet_h, left, top) = if sheet_w > 0 && sheet_h > 0 {
        (sheet_w, sheet_h, left, top)
    } else {
        (area_w, area_h, 0, 0)
    };
    paper_in_points(
        (sheet_w, sheet_h),
        (left, top),
        (area_w, area_h),
        (dpi_x, dpi_y),
    )
}

/// [`paper_of`]'s arithmetic, on numbers a test can give it.
fn paper_in_points(
    sheet: (i32, i32),
    offset: (i32, i32),
    area: (i32, i32),
    dpi: (i32, i32),
) -> Paper {
    #[allow(clippy::cast_precision_loss)]
    let (x, y) = (
        POINTS_PER_INCH / dpi.0.max(1) as f32,
        POINTS_PER_INCH / dpi.1.max(1) as f32,
    );
    #[allow(clippy::cast_precision_loss)]
    Paper {
        width: sheet.0 as f32 * x,
        height: sheet.1 as f32 * y,
        left: offset.0 as f32 * x,
        top: offset.1 as f32 * y,
        printable_width: area.0 as f32 * x,
        printable_height: area.1 as f32 * y,
    }
}

/// The rows of a page drawn in one request, so that each fits the worker's tile
/// mapping (`worker::TILE_CAPACITY`) and a tile's `u16` height.
///
/// Full-width bands rather than square tiles, because PDFium walks a page's
/// whole content for every request whatever its size (`examples/tile_bench.rs`):
/// the fewer the requests, the fewer the walks. An A4 sheet at 300 dpi is three.
///
/// # Errors
///
/// A row wider than the mapping holds, which no page fitted to a sheet is.
fn band_rows(width: u32) -> Result<u32, String> {
    let row = u64::from(width) * 4;
    let rows = (crate::worker::TILE_CAPACITY as u64 / row.max(1)).min(u64::from(u16::MAX));
    if width == 0 || width > u32::from(u16::MAX) || rows == 0 {
        return Err(said!("a page {width} pixels wide is too wide to print"));
    }
    Ok(u32::try_from(rows).unwrap_or(u32::from(u16::MAX)))
}

/// A BMP header for a top-down 32-bit image of `width` by `height` pixels.
///
/// Built here so the worker's pixels reach GDI through [`parse_bmp`] and
/// [`draw_cut`], the checks every page already passed when the OS drew it,
/// rather than through a second, unchecked route to `StretchDIBits`.
fn bmp_header(width: u32, height: u32) -> Result<Vec<u8>, String> {
    let pixels = u64::from(width) * u64::from(height) * 4;
    let total = u32::try_from(pixels + (BMP_FILE_HEADER + DIB_HEADER_MIN) as u64)
        .map_err(|_| said!("a {width}x{height} page is too large to print"))?;
    let (w, h) = (
        i32::try_from(width).map_err(|_| said!("page too wide"))?,
        i32::try_from(height).map_err(|_| said!("page too tall"))?,
    );
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&((BMP_FILE_HEADER + DIB_HEADER_MIN) as u32).to_le_bytes());
    out.extend_from_slice(&(DIB_HEADER_MIN as u32).to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    // Negative: rows run top to bottom, the order the bands arrive in.
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&BI_RGB.to_le_bytes());
    out.extend_from_slice(&u32::try_from(pixels).unwrap_or(0).to_le_bytes());
    // Resolution, colours used, colours important: unused by GDI here.
    out.extend_from_slice(&[0; 16]);
    Ok(out)
}

/// Appends the worker's RGBA pixels to `bmp` as BGRA, the order a 32-bit DIB
/// stores. Swapped colours would pass every ink count the probe makes, so the
/// order has a test of its own.
fn append_bgra(bmp: &mut Vec<u8>, rgba: &[u8]) {
    for pixel in rgba.chunks_exact(4) {
        bmp.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
}

/// A sandboxed worker holding the print job, which draws its pages.
struct Painter {
    session: crate::save_outside::Session,
    /// Each page's size in points, after its `/Rotate`.
    sizes: Vec<crate::render::PageSize>,
}

impl Painter {
    /// Starts a worker over `bytes` and asks it for every page's size.
    fn start(bytes: &[u8], library_dir: &std::path::Path) -> Result<Self, String> {
        use crate::worker_proto::{Reply, Request};
        let mut session =
            crate::save::InWorker::at(library_dir.to_path_buf()).session_over(bytes)?;
        let reply = session
            .ask(Request::Open {
                lazy_geometry: false,
            })
            .map_err(|e| {
                said!(
                    "the worker could not open the print job: {why}",
                    why = e.message()
                )
            })?;
        match reply {
            Reply::Open {
                pages, page_count, ..
            } if pages.len() == page_count => Ok(Self {
                session,
                sizes: pages,
            }),
            Reply::Open { .. } => Err(said!("the worker returned incomplete page sizes")),
            other => Err(format!(
                "the worker answered the print job's sizes with {}",
                other.described()
            )),
        }
    }

    /// The window of page `index` that `cut` names, as a top-down 32-bit BMP
    /// for [`draw_cut`].
    fn paint(&mut self, index: u32, cut: &Cut, marks: bool) -> Result<Vec<u8>, String> {
        let (width, height) = (cut.width, cut.height);
        let rows = band_rows(width)?;
        let mut bmp = bmp_header(width, height)?;
        let mut y = 0;
        while y < height {
            // Between bands too: a dense page takes seconds a band.
            stopped()?;
            let band = rows.min(height - y);
            let rgba = self
                .session
                .tile_within(
                    crate::render::TileRequest {
                        annotations: marks,
                        rid: 0,
                        doc: 0,
                        page: index,
                        scale: cut.scale,
                        turns: cut.turns,
                        invert: false,
                        x: i32::try_from(cut.x).map_err(|_| said!("page too wide"))?,
                        y: i32::try_from(cut.y + y).map_err(|_| said!("page too tall"))?,
                        width: u16::try_from(width).map_err(|_| said!("page too wide"))?,
                        height: u16::try_from(band).map_err(|_| said!("band too tall"))?,
                        format: crate::render::TileFormat::Raw,
                        crop: None,
                    },
                    BAND_DEADLINE,
                )
                .map_err(|e| format!("page {}: {}", index + 1, e.message()))?;
            append_bgra(&mut bmp, &rgba);
            y += band;
        }
        Ok(bmp)
    }
}

/// Renders every page of a job onto an open printer device context.
///
/// Split out from [`present`] so that the whole pipeline --- parse, rasterise,
/// spool --- can be exercised against a printer DC opened directly, with no dialog
/// and no user. That is what `examples/print_probe.rs` does, and it is the only way any
/// of this is verifiable without paper.
///
/// The pages are drawn by PDFium in a worker started over `bytes`, from the
/// library in `library_dir`; see the module note for why not by the OS.
///
/// `sheets` names which of the job's pages to send, zero-based and in the order
/// they should print --- `crate::print::sheets` builds it from whatever range the
/// panel came back with. It is a list rather than a pair because the caller has
/// already validated it: a bad range is refused before a document is opened on
/// the spooler, rather than half-printed and then abandoned.
///
/// `beat`, when given, hears each sheet handed over, and stops its clock while
/// tpdf reads the job and rasterises a page. Every caller runs it under
/// `print::watched`, because a stuck queue blocks a call here forever rather
/// than failing it.
///
/// # Errors
///
/// The worker refusing the job, a sheet outside it, a page failing to render,
/// or GDI refusing a page or the document.
pub fn spool(
    dc: HDC,
    bytes: &[u8],
    title: &str,
    output: Option<&str>,
    sheets: &[u32],
    beat: Option<&crate::print::Beat<'_>>,
    library_dir: &std::path::Path,
) -> Result<u32, String> {
    spool_planned(
        dc,
        bytes,
        title,
        output,
        true,
        beat,
        library_dir,
        |sizes, paper| {
            // `print::sheets` has already validated these against a count --- but
            // a *different* count, from the OS parser's read of the same bytes a
            // moment earlier. Two parses are two chances to disagree, and asking
            // again costs one comparison per sheet of a job about to be drawn.
            let count = u32::try_from(sizes.len()).unwrap_or(u32::MAX);
            if let Some(index) = sheets.iter().find(|&&index| index >= count) {
                return Err(said!("sheet {index} is not in this job, which has {count}"));
            }
            crate::print_layout::lay_out(sizes, sheets, paper, crate::print_layout::Layout::PLAIN)
        },
    )
}

/// Prints a job laid out as the preview's settings say: `pages` of it, zero
/// based and in order, as `layout` places them, with the comments and other
/// marks only when `marks` is set. `output` names a file to print to, as for
/// [`spool`].
///
/// # Errors
///
/// As [`spool`], and anything [`crate::print_layout::lay_out`] refuses.
#[allow(clippy::too_many_arguments)]
pub fn spool_laid_out(
    dc: HDC,
    bytes: &[u8],
    title: &str,
    output: Option<&str>,
    pages: &[u32],
    layout: crate::print_layout::Layout,
    marks: bool,
    beat: Option<&crate::print::Beat<'_>>,
    library_dir: &std::path::Path,
) -> Result<u32, String> {
    spool_planned(
        dc,
        bytes,
        title,
        output,
        marks,
        beat,
        library_dir,
        |sizes, paper| crate::print_layout::lay_out(sizes, pages, paper, layout),
    )
}

/// Set by [`stop`], read before every sheet and every band of one.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a print that was stopped fails with; [`print_on`] reads it as a
/// stop rather than a failure.
pub const STOPPED: &str = "the print was stopped";

/// Stops the print that is being drawn, if one is. The sheets the spooler
/// has are discarded with the job (`OpenDocument`); a sheet already on the
/// printer is not called back.
pub fn stop() {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// [`STOPPED`] once [`stop`] has been called.
fn stopped() -> Result<(), String> {
    if STOP.load(std::sync::atomic::Ordering::SeqCst) {
        Err(STOPPED.to_owned())
    } else {
        Ok(())
    }
}

/// Draws the sheets `plan` lays out, from the job's page sizes and the paper
/// the DC prints on, and hands them to the spooler.
///
/// The plan is made before the document is opened on the spooler, so a layout
/// that is refused leaves nothing half-printed.
#[allow(clippy::too_many_arguments)]
fn spool_planned(
    dc: HDC,
    bytes: &[u8],
    title: &str,
    output: Option<&str>,
    marks: bool,
    beat: Option<&crate::print::Beat<'_>>,
    library_dir: &std::path::Path,
    plan: impl FnOnce(&[PageSize], &Paper) -> Result<Vec<Vec<Placement>>, String>,
) -> Result<u32, String> {
    use windows::Win32::Storage::Xps::{EndPage, StartDocW, StartPage, DOCINFOW};

    // A Stop pressed for an earlier print does not stop this one.
    STOP.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut painter = ours(beat, || Painter::start(bytes, library_dir))?;
    let paper = paper_of(dc);
    let sheets = plan(&painter.sizes, &paper)?;

    // Both `HSTRING`s are bound to locals rather than built inline, because
    // `DOCINFOW` holds borrowed pointers and a temporary would be dropped at the
    // end of the statement that created it --- leaving the struct pointing at
    // freed memory for the `StartDocW` below.
    let title_w = HSTRING::from(title);
    let output_w = output.map(HSTRING::from);
    let info = DOCINFOW {
        cbSize: i32::try_from(std::mem::size_of::<DOCINFOW>())
            .map_err(|_| "DOCINFOW does not fit an i32")?,
        lpszDocName: PCWSTR(title_w.as_ptr()),
        // Naming an output file makes the spooler write there instead of asking,
        // which is what lets a probe drive "Microsoft Print to PDF" with no save
        // dialog. Null for a real print, where the driver decides.
        lpszOutput: output_w
            .as_ref()
            .map_or(PCWSTR::null(), |o| PCWSTR(o.as_ptr())),
        ..Default::default()
    };

    // SAFETY: a live printer DC and a fully initialised DOCINFOW whose strings
    // outlive the call.
    if unsafe { StartDocW(dc, &info) } <= 0 {
        return Err(format!(
            "StartDoc failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // From here every early return leaves through this guard, which abandons
    // the job. Declared after the two `HSTRING`s, so it is dropped before them.
    let open = OpenDocument { dc };

    let (dpi_x, dpi_y) = dc_dpi(dc);
    // Render at the printer's own resolution where that is *lower* than the
    // constant, so a 150 dpi device is not handed four times the pixels it can
    // use --- and never higher, for the buffer reason `PRINT_DPI` gives.
    #[allow(clippy::cast_precision_loss)]
    let density = PRINT_DPI.min(dpi_x.min(dpi_y).max(1) as f32);
    let area = (paper.printable_width, paper.printable_height);

    for (number, sheet) in sheets.iter().enumerate() {
        let number = number + 1;
        stopped()?;
        // SAFETY: a live DC inside an open document.
        if unsafe { StartPage(dc) } <= 0 {
            return Err(format!(
                "StartPage failed on sheet {number}: {}",
                std::io::Error::last_os_error()
            ));
        }
        for placed in sheet {
            let size = painter.sizes[placed.page as usize];
            let Some(cut) = cut(placed, size, area, density) else {
                continue;
            };
            let bmp = ours(beat, || painter.paint(placed.page, &cut, marks))?;
            draw_cut(dc, &bmp, &cut, (dpi_x, dpi_y))?;
        }
        // SAFETY: as above, closing the page this opened.
        if unsafe { EndPage(dc) } <= 0 {
            return Err(format!(
                "EndPage failed on sheet {number}: {}",
                std::io::Error::last_os_error()
            ));
        }
        if let Some(beat) = beat {
            beat.sheet();
        }
    }

    open.end()?;
    // What was spooled, not what the job holds: a reader who asked for two sheets
    // of forty should see two here, and the caller compares this against the
    // request rather than against the document.
    Ok(u32::try_from(sheets.len()).unwrap_or(u32::MAX))
}

/// Runs tpdf's own part of a spool with the watchdog's clock stopped, when
/// there is one (`print::Beat::ours`).
fn ours<R>(beat: Option<&crate::print::Beat<'_>>, work: impl FnOnce() -> R) -> R {
    match beat {
        Some(beat) => beat.ours(work),
        None => work(),
    }
}

/// A print document between `StartDocW` and whichever call closes it.
///
/// **Dropping it abandons the job.** [`spool`] can fail in several places after
/// the document is open --- a sheet outside the job, a page that will not render,
/// GDI refusing a page --- and each of them used to return with the document
/// still open on the spooler: no `EndDoc`, no `AbortDoc`, and a caller that then
/// deletes the device context under it. What a driver does with a job left that
/// way is its own business, and printing the pages that did arrive is one of
/// the answers. `AbortDoc` is the call for it: it stops the job and discards
/// what was spooled.
///
/// A guard rather than a call on each error path, because the next error path
/// added to `spool` would otherwise have to remember.
struct OpenDocument {
    dc: HDC,
}

impl OpenDocument {
    /// Closes the document so that it prints.
    ///
    /// # Errors
    ///
    /// GDI refusing `EndDoc`. The guard is dropped on that path like any other,
    /// so a document that would not end is abandoned rather than left open.
    fn end(self) -> Result<(), String> {
        // SAFETY: closing the document `spool` opened on this live DC.
        if unsafe { windows::Win32::Storage::Xps::EndDoc(self.dc) } <= 0 {
            return Err(format!(
                "EndDoc failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Ended, so there is nothing left to abandon.
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for OpenDocument {
    fn drop(&mut self) {
        // SAFETY: the DC is the caller's and outlives `spool`, the only place
        // this is built. The result is ignored: this runs while a failure is
        // already being reported, and a job that will not abort leaves nothing
        // further to try.
        let _ = unsafe { windows::Win32::Storage::Xps::AbortDoc(self.dc) };
    }
}

/// Tells the reader why a print they asked for did not happen, in a modal
/// message over `owner`: after the dialog closed, nothing else would. The
/// Windows half of `print_linux::tell`.
pub fn tell(owner: Option<HWND>, why: &str) {
    message(
        owner,
        &format!("The document was not printed.\n\n{why}"),
        true,
    );
}

/// A modal message box over `owner`, an error or a warning. It blocks the
/// calling thread, which is never the window's: printing runs on its own.
fn message(owner: Option<HWND>, text: &str, error: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONWARNING, MB_OK,
    };
    let text = HSTRING::from(text);
    let caption = HSTRING::from("tpdf");
    let icon = if error { MB_ICONERROR } else { MB_ICONWARNING };
    // SAFETY: two wide strings that outlive the call; a null owner is allowed.
    let _ = unsafe { MessageBoxW(owner, &text, &caption, MB_OK | icon) };
}

/// A print hook that does nothing, installed so Windows shows the classic dialog.
///
/// Windows 11 answers `PrintDlgW` with an owner window by showing its newer print
/// dialog, whose preview pane reads *This app doesn't support print preview* for
/// every program that prints this way: it can only show pages an app hands it
/// through the WinRT print API, and `spool` draws on a device context instead. A
/// reader takes that sentence as a defect of tpdf. The newer dialog cannot host a
/// hook, so with one installed Windows shows the classic dialog, which has no
/// preview pane and offers the same printer, page range, copies and printer
/// properties. Measured 2026-10-17 on Windows 11 25H2 and 26H2 with `PrintDlgW`
/// from a script: no owner gives the classic dialog, an owner gives the newer
/// one, an owner and a hook give the classic one again.
///
/// Returning zero lets the dialog handle every message itself.
unsafe extern "system" fn classic_dialog(_: HWND, _: u32, _: WPARAM, _: LPARAM) -> usize {
    0
}

/// Prints a job on a DC the preview's printer settings made, laid out as the
/// preview showed it: `pages` of it, placed as `layout` says, which `sheets`
/// sheets of paper hold, with the marks when `marks` is set. Deletes the DC
/// whatever the outcome.
///
/// `progress` hears `(sent, of)` before the first sheet and after each, as in
/// [`present`], and a stuck queue is reported the same way. A print stopped
/// with [`stop`] answers `false`, as a Cancel does.
///
/// # Errors
///
/// No page to print, or the job failing to spool.
#[allow(clippy::too_many_arguments)]
pub fn print_on(
    dc: HDC,
    bytes: &[u8],
    title: &str,
    owner: Option<HWND>,
    progress: &dyn Fn(u32, u32),
    library_dir: &std::path::Path,
    pages: &[u32],
    layout: crate::print_layout::Layout,
    sheets: usize,
    marks: bool,
) -> Result<bool, String> {
    let result = if pages.is_empty() {
        Err(said!("this print job has no pages in it"))
    } else {
        let of = u32::try_from(sheets).unwrap_or(u32::MAX);
        let shown = owner.map(|h| h.0 as isize);
        progress(0, of);
        crate::print::watched(
            crate::print::STALL,
            |sent| progress(sent, of),
            |beat| {
                spool_laid_out(
                    dc,
                    bytes,
                    title,
                    None,
                    pages,
                    layout,
                    marks,
                    Some(beat),
                    library_dir,
                )
            },
            move |sent| {
                let owner = shown.map(|h| HWND(h as *mut std::ffi::c_void));
                message(owner, &crate::print::stalled_message(sent), false);
            },
        )
    };
    // SAFETY: the DC the caller handed over to be printed on and freed here.
    let _ = unsafe { DeleteDC(dc) };
    match result {
        Err(e) if e == STOPPED => Ok(false),
        other => other.map(|_| true),
    }
}

/// Opens the system print dialog for these bytes and prints what it returns.
///
/// Returns whether a job was sent. `false` is Cancel, and it is distinguished from
/// a failure here rather than folded into it: `PrintDlgW` returns zero for both,
/// and `CommDlgExtendedError` is the only thing that separates them. macOS cannot
/// make that distinction at all (`runOperation` answers one boolean), so this is
/// the one respect in which the Windows path reports more than the macOS one.
///
/// **The Pages field is offered, and until 2026-08-23 it was not.** `nMinPage`
/// and `nMaxPage` both defaulted to zero, and Win32 disables the Pages radio and
/// its two edit controls whenever those are equal --- so a Windows reader could
/// print the whole document or nothing, while a macOS reader typing "2 to 4" into
/// `NSPrintPanel` got two to four. That is a capability one platform had and the
/// other did not, and the parity is worth more than the ten lines it costs.
///
/// It is also why the range is honoured rather than the field simply enabled: the
/// comment on `PD_NOSELECTION` below states the rule --- offering a control and
/// then ignoring it is worse than not offering it --- and enabling this one
/// without reading `PD_PAGENUMS` back would break exactly that rule on the
/// noisier control.
///
/// `progress` hears `(sent, of)` once the reader has pressed Print, with 0 sent,
/// and again after each sheet: rasterising a dense page takes minutes, and
/// Windows shows nothing while it does (`print::Beat::ours`). Not on Cancel.
///
/// # Errors
///
/// The dialog failing for a reason other than Cancel, a range naming sheets the
/// job does not have, or the job failing to spool.
pub fn present(
    bytes: &[u8],
    title: &str,
    owner: Option<HWND>,
    progress: &dyn Fn(u32, u32),
    library_dir: &std::path::Path,
) -> Result<bool, String> {
    // Parsed here only to bound the dialog, and it is a second parse of bytes
    // `spool` parses again below. Worth stating rather than hiding: the count has
    // to be known *before* the panel opens, and `spool` has to own its own
    // document because `examples/print_probe.rs` calls it with no panel at all.
    // The job is already in memory and the parse is the OS reader's, so this buys
    // the range field for one extra read of a document we are about to print.
    let count = parse(bytes)
        .and_then(|document| document.PageCount().ok())
        .unwrap_or(0);
    // `WORD` fields, so a job longer than 65,535 sheets can only offer a range
    // over its first 65,535. Nothing else clamps, and this one does because the
    // alternative is not offering the field at all on a document where a reader
    // most wants it.
    //
    // **A count of zero has to give equal bounds, not `1` and `0`.** That is the
    // parser having refused the job, and `nMinPage > nMaxPage` is not a struct
    // Win32 accepts --- the dialog would fail with a `CDERR`, which reads as a
    // broken print system rather than as the unreadable document `spool` is about
    // to report accurately.
    let (first, last) = match u16::try_from(count).unwrap_or(u16::MAX) {
        0 => (0, 0),
        last => (1, last),
    };
    let mut dialog = PRINTDLGW {
        lStructSize: u32::try_from(std::mem::size_of::<PRINTDLGW>())
            .map_err(|_| "PRINTDLGW does not fit a u32")?,
        hwndOwner: owner.unwrap_or_default(),
        // `PD_RETURNDC` is the whole reason this call is here: it hands back a DC
        // for the printer the reader chose, which is what `spool` needs.
        // `PD_NOSELECTION` because there is no selection to print --- offering the
        // radio button and then ignoring it would be worse than not offering it.
        // `PD_ENABLEPRINTHOOK` for the dialog it selects, not for the hook: see
        // [`classic_dialog`].
        Flags: PD_RETURNDC | PD_ALLPAGES | PD_NOSELECTION | PD_ENABLEPRINTHOOK,
        lpfnPrintHook: Some(classic_dialog),
        nCopies: 1,
        // Equal bounds disable the field, which is the right answer for a
        // one-page job and for a document the OS parser could not count --- there
        // is no range to type over one sheet, and none to validate against zero.
        nMinPage: first,
        nMaxPage: last,
        nFromPage: first,
        nToPage: last,
        ..Default::default()
    };

    // SAFETY: a fully initialised PRINTDLGW whose `lStructSize` describes it.
    let chosen = unsafe { PrintDlgW(&mut dialog) };
    if !chosen.as_bool() {
        // SAFETY: no arguments; reads the thread's last common-dialog error.
        let why = unsafe { CommDlgExtendedError() };
        if why.0 == 0 {
            // Cancel. Not an error: putting a failure in front of someone who
            // pressed Cancel is the mistake `print_macos.rs` avoids the other way
            // round, by refusing to report at all.
            return Ok(false);
        }
        return Err(said!(
            "the print dialog failed: CDERR {code:#06x}",
            code = why.0
        ));
    }

    // What the reader typed, or nothing if they left the field alone --- the
    // dialog sets `PD_PAGENUMS` only when the Pages radio is the one selected, so
    // an untouched panel and a disabled field are the same answer here and both
    // mean every sheet.
    let range = if dialog.Flags.0 & PD_PAGENUMS.0 == 0 {
        None
    } else {
        Some((u32::from(dialog.nFromPage), u32::from(dialog.nToPage)))
    };

    let dc = HDC(dialog.hDC.0);
    // The DC is owned from here, so the refusal has to be carried rather than
    // returned: `PD_RETURNDC` hands over a device context the caller must free on
    // every path, and an early `?` here would leak one for every mistyped range.
    let result = crate::print::sheets(range, count).and_then(|sheets| {
        // Only reachable with no range at all, since a validated range names at
        // least one sheet --- so this is a job with nothing in it, and a message
        // about a range would be about something the reader never typed.
        if sheets.is_empty() {
            return Err(said!("this print job has no pages in it"));
        }
        // A stuck queue blocks `spool` inside GDI and reports nothing, so the
        // reader is told from another thread instead (`print::STALL`). The
        // owner crosses as an integer, as it crossed into this thread.
        let shown = owner.map(|h| h.0 as isize);
        let of = u32::try_from(sheets.len()).unwrap_or(u32::MAX);
        progress(0, of);
        crate::print::watched(
            crate::print::STALL,
            |sent| progress(sent, of),
            |beat| spool(dc, bytes, title, None, &sheets, Some(beat), library_dir),
            move |sent| {
                let owner = shown.map(|h| HWND(h as *mut std::ffi::c_void));
                message(owner, &crate::print::stalled_message(sent), false);
            },
        )
    });
    // SAFETY: the DC `PD_RETURNDC` handed over; the caller owns and must free it,
    // on every path including the failing one.
    let _ = unsafe { DeleteDC(dc) };
    match result {
        Err(e) if e == STOPPED => Ok(false),
        other => other.map(|_| true),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_bgra, band_rows, bmp_header, cut, header_bytes, paper_in_points, parse_bmp,
        pixel_bytes, read, render_page, Raster, BI_BITFIELDS, BI_RGB,
    };
    use crate::print_layout::{lay_out, Layout, Paper, Placement, Scale};

    /// The worker's red reaches GDI as red: RGBA in, BGRA out.
    #[test]
    fn worker_pixels_are_reordered_for_the_dib() {
        let mut bmp = bmp_header(2, 1).expect("a header");
        append_bgra(&mut bmp, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let raster = Raster::of(&bmp).expect("pixels");
        // `Raster::pixel` answers `[b, g, r]`.
        assert_eq!(raster.pixel(0, 0), [0, 0, 255], "red");
        assert_eq!(raster.pixel(1, 0), [255, 0, 0], "blue");
    }
    use crate::render::PageSize;
    use crate::worker::TILE_CAPACITY;

    /// A band is as many whole rows as the worker's mapping holds, and no more.
    #[test]
    fn a_band_fills_the_tile_mapping_without_overflowing_it() {
        // An A4 page at 300 dpi, the common case: three bands.
        let rows = band_rows(2480).expect("an A4 row fits");
        assert!(rows as usize * 2480 * 4 <= TILE_CAPACITY);
        assert!((rows as usize + 1) * 2480 * 4 > TILE_CAPACITY);
        assert_eq!(3508_u32.div_ceil(rows), 3);
        // A narrow page is bounded by the tile's `u16` height, not the mapping.
        assert_eq!(band_rows(1).expect("one pixel wide"), u32::from(u16::MAX));
        // Nothing to draw, and wider than a tile can say.
        assert!(band_rows(0).is_err());
        assert!(band_rows(u32::from(u16::MAX) + 1).is_err());
    }

    /// The worker's pixels reach GDI through `parse_bmp`, top row first.
    #[test]
    fn a_painted_page_is_a_top_down_bmp_that_parse_bmp_accepts() {
        let mut bmp = bmp_header(2, 2).expect("a header");
        // BGRA, top row first: red, green / blue, black.
        for pixel in [
            [0, 0, 255, 255],
            [0, 255, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 255],
        ] {
            bmp.extend_from_slice(&pixel);
        }
        let dib = parse_bmp(&bmp).expect("parse_bmp accepts what bmp_header writes");
        assert_eq!((dib.width, dib.height, dib.bpp), (2, 2, 32));
        assert!(dib.top_down);
        let raster = Raster::of(&bmp).expect("pixels");
        assert_eq!(raster.pixel(0, 0), [0, 0, 255]);
        assert_eq!(raster.pixel(1, 0), [0, 255, 0]);
        assert_eq!(raster.pixel(0, 1), [255, 0, 0]);
        // A header with its pixels missing is refused, not read past.
        let short = bmp_header(2, 2).expect("a header");
        assert!(parse_bmp(&short).is_err());
    }

    /// A Brother's A4 sheet at 600 dpi: 4960x7016 device units of paper, of
    /// which 4757x6812 are printable, from 101 and 102 units in.
    fn brother() -> Paper {
        paper_in_points((4960, 7016), (101, 102), (4757, 6812), (600, 600))
    }

    /// The device's units become points through its own resolution.
    #[test]
    fn the_paper_is_measured_in_points() {
        let paper = brother();
        assert!((paper.width - 595.2).abs() < 0.01, "{paper:?}");
        assert!(
            (paper.printable_width - 4757.0 * 0.12).abs() < 0.01,
            "{paper:?}"
        );
        assert!((paper.left - 12.12).abs() < 0.01, "{paper:?}");
        assert!((paper.top - 12.24).abs() < 0.01, "{paper:?}");
    }

    /// A page shrunk onto the sheet renders at the density asked for on
    /// paper, not on the page: an A0 page at 300 dpi of A4 renders at about
    /// a quarter of that per inch of A0.
    #[test]
    fn a_shrunk_page_renders_at_the_density_of_the_paper() {
        let paper = brother();
        let area = (paper.printable_width, paper.printable_height);
        let a0 = PageSize {
            width_pt: 2384.0,
            height_pt: 3370.0,
        };
        let sheets = lay_out(&[a0], &[0], &paper, Layout::PLAIN).expect("laid out");
        let placed = sheets[0][0];
        let cut = cut(&placed, a0, area, 300.0).expect("on the paper");
        // 300 pixels per inch of the placed width.
        let want = (placed.width / 72.0 * 300.0).round();
        #[allow(clippy::cast_precision_loss)]
        let got = cut.width as f32;
        assert!((got - want).abs() <= 1.0, "{got} against {want}");
        assert!(cut.scale < 300.0 / 72.0 / 3.0, "{cut:?}");
        assert_eq!((cut.x, cut.y), (0, 0));
    }

    /// A page larger than the paper at actual size renders only the part the
    /// printer can put on it, and that part lands where it belongs.
    #[test]
    fn a_page_past_the_paper_renders_only_what_lands_on_it() {
        let paper = brother();
        let area = (paper.printable_width, paper.printable_height);
        let a3 = PageSize {
            width_pt: 842.0,
            height_pt: 1191.0,
        };
        let actual = Layout {
            scale: Scale::Actual,
            ..Layout::PLAIN
        };
        let placed = lay_out(&[a3], &[0], &paper, actual).expect("laid out")[0][0];
        let cut = cut(&placed, a3, area, 300.0).expect("on the paper");
        // At most the printable area at 300 dpi, with a pixel either side.
        let most = (area.0 / 72.0 * 300.0).ceil() + 1.0;
        #[allow(clippy::cast_precision_loss)]
        let wide = cut.width as f32;
        assert!(wide <= most, "{wide} wider than {most}");
        assert!(cut.x > 0, "the left of the page is off the paper: {cut:?}");
        // The window starts at or just left of the printable area's edge.
        assert!(cut.left <= 0.0 && cut.left > -0.5, "{cut:?}");
        // A page wholly off the paper has nothing to draw.
        let away = Placement {
            x: area.0 + 10.0,
            ..placed
        };
        assert_eq!(super::cut(&away, a3, area, 300.0), None);
    }

    /// A turned page is rendered a quarter turn anticlockwise, at the density
    /// of the paper across what is now its width: the page's own height.
    #[test]
    fn a_turned_page_renders_turned_at_the_density_of_the_paper() {
        let paper = brother();
        let area = (paper.printable_width, paper.printable_height);
        let landscape = PageSize {
            width_pt: 842.0,
            height_pt: 595.0,
        };
        let turning = Layout {
            rotate: true,
            scale: Scale::Fit,
            ..Layout::PLAIN
        };
        let placed = lay_out(&[landscape], &[0], &paper, turning).expect("laid out")[0][0];
        assert!(placed.turned, "{placed:?}");
        let cut = cut(&placed, landscape, area, 300.0).expect("on the paper");
        assert_eq!(cut.turns, 3);
        let want = placed.width / landscape.height_pt * 300.0 / 72.0;
        assert!((cut.scale - want).abs() < 1e-4, "{cut:?} against {want}");
        // Taller than wide, as the turned page lands.
        assert!(cut.height > cut.width, "{cut:?}");
        let flat = Placement {
            turned: false,
            ..placed
        };
        assert_eq!(
            super::cut(&flat, landscape, area, 300.0).map(|c| c.turns),
            Some(0)
        );
    }

    /// A minimal one-page PDF, assembled here rather than by `lopdf`.
    ///
    /// Hand-written on purpose. Every check below is about what an *independent*
    /// parser sees, and generating the input with the library this repository also
    /// writes jobs with would put our own serialiser on both ends --- the trap
    /// `docs/TRAPS.md` records as a writer and its own reader agreeing about a
    /// document that is wrong.
    fn one_page(rotate: i64) -> Vec<u8> {
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Rotate {rotate} >>"),
        ];
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", index + 1));
        }
        let start = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for offset in &offsets {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            objects.len() + 1
        ));
        pdf.into_bytes()
    }

    #[test]
    fn the_os_parser_reads_a_document_this_repository_did_not_write() {
        let reading = read(&one_page(0)).expect("the OS parser refused a valid one-page PDF");
        assert_eq!(reading.pages.len(), 1, "{reading:?}");
        assert_eq!(reading.pages[0].rotation, 0, "{reading:?}");
    }

    #[test]
    fn every_rotation_survives_the_enum_round_trip() {
        // The control for `rotation_degrees`. Mapping all four arms to one value,
        // or defaulting an unknown to zero, passes a check that only ever looks at
        // an unrotated page --- so all four are asserted and each must differ.
        for want in [0, 90, 180, 270] {
            let reading = read(&one_page(want)).unwrap_or_else(|| panic!("refused /Rotate {want}"));
            assert_eq!(reading.pages[0].rotation, want, "/Rotate {want}");
        }
    }

    #[test]
    fn a_document_the_os_parser_refuses_is_reported_as_refused() {
        // Not decoration: `read` returning an empty `Reading` for garbage would
        // let `present_job` open a panel on a document nothing can print, which is
        // the exact failure the readback exists to stop.
        assert!(read(b"this is not a PDF at all").is_none());
        assert!(read(&[]).is_none());
    }

    #[test]
    fn a_rendered_page_is_a_dib_of_the_size_the_dpi_asks_for() {
        // The fixture's `/MediaBox` is 200x100 *points*, so at 72 dpi it is
        // 200x100 pixels and at 144 it is 400x200. Both are asserted because a
        // renderer that ignored `dpi` would satisfy either one on its own --- and
        // because a single row cannot distinguish a wrong scale from a wrong unit,
        // which is exactly the defect this pair caught: with `WINRT_DPI` set to 72
        // both rows come out 1.33x too large, and with it right both are exact.
        for (dpi, want) in [(72.0, (200, 100)), (144.0, (400, 200))] {
            let bmp = render_page(&one_page(0), 0, dpi).expect("render");
            let dib = parse_bmp(&bmp).expect("the OS renderer produced an unparseable BMP");
            assert_eq!((dib.width, dib.height), want, "at {dpi} dpi");
        }
    }

    #[test]
    fn a_page_index_past_the_end_is_an_error_and_not_a_blank_page() {
        assert!(render_page(&one_page(0), 7, 72.0).is_err());
    }

    #[test]
    fn a_bmp_that_is_not_one_is_refused_rather_than_handed_to_gdi() {
        // Every arm here would be an access violation inside `StretchDIBits` if
        // `parse_bmp` waved it through, so a returned `Err` is the whole value.
        assert!(parse_bmp(b"").is_err());
        assert!(parse_bmp(b"BM").is_err());
        let mut truncated = render_page(&one_page(0), 0, 72.0).expect("render");
        truncated.truncate(20);
        assert!(parse_bmp(&truncated).is_err());
        let mut wrong_magic = render_page(&one_page(0), 0, 72.0).expect("render");
        wrong_magic[0] = b'X';
        assert!(parse_bmp(&wrong_magic).is_err());
    }

    #[test]
    fn a_bmp_with_fewer_pixels_than_it_declares_is_refused() {
        // Only the pixel data is cut here, so every other check in `parse_bmp`
        // passes and this is the one that has to fire --- which is the point: a
        // header can be self-consistent in every field it states and still
        // describe an image larger than the bytes that arrived. `StretchDIBits`
        // would read the declared amount regardless, past the end of the buffer,
        // and at exactly the pixel offset the pointer it is handed is dangling.
        let full = render_page(&one_page(0), 0, 72.0).expect("render");
        let offset = u32::from_le_bytes([full[10], full[11], full[12], full[13]]) as usize;
        assert!(
            parse_bmp(&full).is_ok(),
            "the control: the whole image still parses"
        );
        // One byte short as well as the two obvious truncations, because a bound
        // that is off by a row would let the first two through.
        for short in [full.len() - 1, offset + 1, offset] {
            let mut cut = full.clone();
            cut.truncate(short);
            assert!(
                parse_bmp(&cut).is_err(),
                "{short} bytes of {} were accepted",
                full.len()
            );
        }
    }

    #[test]
    fn a_row_is_padded_to_four_bytes_which_the_rendered_fixture_cannot_show() {
        // Three 24-bit pixels are nine bytes and a row of them is twelve: BMP
        // rows are padded to a 4-byte boundary. Asserted directly because the
        // fixture above is 32 bpp, where every row is aligned already -- so the
        // one format this module ever sees cannot tell a padded stride from an
        // unpadded one, and an unpadded one is too *small*, which is the
        // direction that lets GDI read past the buffer.
        assert_eq!(pixel_bytes(3, 2, 24, BI_RGB, 0), Ok(24));
        // The control, and it is the same number by a different route: at 32 bpp
        // the padding is already there, so the arithmetic must not add any.
        assert_eq!(pixel_bytes(3, 2, 32, BI_RGB, 0), Ok(24));
    }

    #[test]
    fn a_palette_and_a_mask_block_are_counted_as_header_gdi_reads() {
        // Both live in `BITMAPINFO`'s trailing array and both are read through
        // the pointer the header is handed on, so neither is covered by the
        // `biSize` check. An 8-bit image that names no palette size is entitled
        // to all 256 entries.
        assert_eq!(header_bytes(40, 8, BI_RGB, 0), 40 + 256 * 4);
        assert_eq!(header_bytes(40, 8, BI_RGB, 5), 40 + 5 * 4);
        assert_eq!(header_bytes(40, 32, BI_BITFIELDS, 0), 40 + 12);
        // The two controls: the masks are inside a later header rather than
        // after it, and the format the OS encoder produces needs nothing extra.
        assert_eq!(header_bytes(124, 32, BI_BITFIELDS, 0), 124);
        assert_eq!(header_bytes(40, 32, BI_RGB, 0), 40);
    }

    /// A BMP built byte by byte, so a test can decide its row order.
    ///
    /// Hand-assembled for the reason `one_page` gives about itself: every check
    /// below is about how [`Raster`] reads bytes it did not write, and generating
    /// the input with the same code that reads it would put one implementation on
    /// both ends. `rows` is given **top row first**, whatever `top_down` says, so
    /// the expected image is the same sentence in both directions and the test
    /// cannot inherit the convention it is checking.
    fn bmp(rows: &[Vec<[u8; 3]>], top_down: bool) -> Vec<u8> {
        let height = rows.len();
        let width = rows[0].len();
        let stride = (width * 3).div_ceil(4) * 4;
        let mut pixels = vec![0u8; stride * height];
        for (index, row) in rows.iter().enumerate() {
            // A bottom-up BMP stores the last row first, so the top row of the
            // image is the last row of the buffer.
            let at = if top_down { index } else { height - 1 - index };
            for (x, px) in row.iter().enumerate() {
                let start = at * stride + x * 3;
                pixels[start..start + 3].copy_from_slice(px);
            }
        }

        let offset = 14 + 40;
        let mut out = Vec::with_capacity(offset + pixels.len());
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&u32::try_from(offset + pixels.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&i32::try_from(width).unwrap().to_le_bytes());
        let signed = i32::try_from(height).unwrap();
        out.extend_from_slice(&if top_down { -signed } else { signed }.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(pixels.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&pixels);
        out
    }

    /// Three rows that are distinguishable from each other and from their reverse.
    ///
    /// The middle row is not the mirror of anything, so an image read upside down
    /// disagrees with this in the first pixel a test looks at. A symmetric fixture
    /// is the one that cannot tell a flip from an identity.
    fn three_rows() -> Vec<Vec<[u8; 3]>> {
        vec![
            vec![[1, 1, 1], [2, 2, 2], [3, 3, 3]],
            vec![[4, 4, 4], [5, 5, 5], [6, 6, 6]],
            vec![[7, 7, 7], [8, 8, 8], [9, 9, 9]],
        ]
    }

    #[test]
    fn the_top_row_is_the_top_row_whichever_way_the_bmp_stores_it() {
        let rows = three_rows();
        let up = bmp(&rows, false);
        let down = bmp(&rows, true);
        // The two files really are different, or this compares one thing with
        // itself and holds however the flip is written.
        assert_ne!(up, down, "the control: the two encodings differ on disk");

        let up = Raster::of(&up).expect("bottom-up");
        let down = Raster::of(&down).expect("top-down");
        for (y, row) in rows.iter().enumerate() {
            for (x, px) in row.iter().enumerate() {
                assert_eq!(up.pixel(x, y), *px, "bottom-up at {x},{y}");
                assert_eq!(down.pixel(x, y), *px, "top-down at {x},{y}");
            }
        }
    }

    #[test]
    fn a_row_is_read_past_the_padding_that_follows_it() {
        // Three 24-bit pixels are 9 bytes and a row is padded to 12, so a reader
        // that walks 9 bytes per row drifts three bytes further into the image on
        // every row --- which on a real page produces a plausible small count
        // rather than an obvious error. Row 2 is where a three-byte drift first
        // shows, because row 1 is only one padding away.
        let rows = three_rows();
        let image = bmp(&rows, true);
        let raster = Raster::of(&image).expect("top-down");
        assert_eq!(raster.pixel(0, 2), [7, 7, 7]);
        assert_eq!(raster.pixel(2, 2), [9, 9, 9]);
    }

    #[test]
    fn a_pixel_outside_the_image_reads_white_rather_than_panicking() {
        let image = bmp(&three_rows(), true);
        let raster = Raster::of(&image).expect("top-down");
        assert_eq!(raster.width(), 3);
        assert_eq!(raster.height(), 3);
        // A rectangle that overhangs the page is a question a check may legally
        // ask, and answering it with a panic would make the check's failure a
        // crash rather than a reading.
        assert_eq!(raster.pixel(3, 0), [0xFF, 0xFF, 0xFF]);
        assert_eq!(raster.pixel(0, 3), [0xFF, 0xFF, 0xFF]);
        assert!(!raster.inked(3, 3));
        // And the control, or "outside is white" is satisfied by everything being
        // white.
        assert!(raster.inked(0, 0));
    }

    /// A **valid** 8-bit BMP: 40-byte header, a full 256-entry palette, one row.
    ///
    /// Valid on purpose, and that is the whole point of the helper. The first
    /// version of the test below took a 24-bit image and wrote `8` into its depth
    /// field, which leaves the palette `parse_bmp` then requires missing --- so
    /// that function refused it and [`Raster`]'s own depth check never ran. The
    /// mutation deleting that check SURVIVED, and the test's own comment had
    /// excused it in advance: *"either refusal is correct here"*. A guard whose
    /// neighbour refuses the same input cannot be tested by it.
    fn indexed_bmp() -> Vec<u8> {
        let (width, height) = (4usize, 1usize);
        let offset = 14 + 40 + 256 * 4;
        let mut out = Vec::with_capacity(offset + width * height);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(
            &u32::try_from(offset + width * height)
                .unwrap()
                .to_le_bytes(),
        );
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&i32::try_from(width).unwrap().to_le_bytes());
        out.extend_from_slice(&(-i32::try_from(height).unwrap()).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(width * height).unwrap().to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        // The palette, and the reason an index is not a colour: entry 1 is black
        // and entry 2 is white, so two pixels one apart in index are as far apart
        // in colour as this image goes.
        for entry in 0..256u32 {
            let value = u8::try_from(entry).unwrap_or(0);
            out.extend_from_slice(&[value, value, value, 0]);
        }
        // One padded row of four indices.
        out.extend_from_slice(&[1, 2, 3, 4]);
        out
    }

    #[test]
    fn an_indexed_bmp_is_refused_rather_than_compared_without_its_palette() {
        let indexed = indexed_bmp();
        // **The control, and it is what makes the assertion below about `Raster`.**
        // If `parse_bmp` refused this too, the refusal underneath would satisfy the
        // test whatever `Raster` did --- which is exactly how the first version of
        // this test let its mutation survive.
        assert!(
            parse_bmp(&indexed).is_ok(),
            "the control: a well-formed indexed BMP parses, so the depth check is \
             the only thing left to refuse it"
        );
        assert!(
            Raster::of(&indexed).is_err(),
            "an 8-bit image must not be compared pixel for pixel: its bytes are \
             palette indices, and two colours with adjacent indices would read as \
             nearly the same pixel"
        );
    }

    #[test]
    fn a_render_reads_back_as_pixels_of_the_size_it_was_asked_for() {
        // The synthetic images above prove the reading; this proves the type is
        // pointed at the right thing, on bytes WinRT actually produced. Without it
        // every test here could pass against an encoder this never meets.
        let bmp = render_page(&one_page(0), 0, 72.0).expect("render");
        let raster = Raster::of(&bmp).expect("a rendered page is readable");
        assert_eq!((raster.width(), raster.height()), (200, 100));

        // **`one_page` carries no `/Contents` at all**, so the honest expectation
        // is that every pixel is white --- and the first draft of this asserted the
        // opposite, requiring ink no correct render of a blank page could produce.
        // What this does prove is that the whole declared buffer is readable and
        // holds a plausible colour rather than uninitialised memory; what it cannot
        // prove is that ink would be *found*, which is what the synthetic images
        // above are for.
        let inked = (0..raster.height())
            .flat_map(|y| (0..raster.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| raster.inked(x, y))
            .count();
        assert_eq!(inked, 0, "a page with no /Contents renders blank");
    }
}
