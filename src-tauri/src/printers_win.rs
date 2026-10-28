//! The printers Windows knows, their papers, and the settings a print uses.
//!
//! tpdf's print preview is the print dialog on Windows: it lists the printers,
//! offers each one's papers, and lays the job out on the paper chosen
//! (`print_layout.rs`), so the preview shows the sheets that will print. The
//! classic `PrintDlgW` that came before it chose the printer after the preview,
//! and has neither a scale nor pages to a sheet.
//!
//! A printer's settings are its `DEVMODE`, the driver's own record of paper,
//! orientation, copies, sides and colour, followed by however many bytes of
//! private settings the driver keeps. tpdf changes only the public fields and
//! carries the rest through untouched, and **Printer settings...** hands the
//! whole record to the driver's own dialog (`DocumentProperties`), so anything
//! the driver offers --- trays, quality, its own pages-to-a-sheet --- is still
//! reachable. Every change goes back through the driver, which corrects what it
//! cannot do rather than failing later at the spooler.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateDCW, CreateICW, DeleteDC, DEVMODEW, DMCOLOR_COLOR, DMCOLOR_MONOCHROME, DMDUP_HORIZONTAL,
    DMDUP_SIMPLEX, DMDUP_VERTICAL, DM_COLOR, DM_COPIES, DM_DUPLEX, DM_IN_BUFFER, DM_IN_PROMPT,
    DM_ORIENTATION, DM_OUT_BUFFER, DM_PAPERSIZE, HDC,
};
use windows::Win32::Graphics::Printing::{
    ClosePrinter, DocumentPropertiesW, EnumPrintersW, GetDefaultPrinterW, GetPrinterW,
    OpenPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_4W,
    PRINTER_INFO_5W,
};
use windows::Win32::Storage::Xps::{
    DeviceCapabilitiesW, DC_COLORDEVICE, DC_COPIES, DC_DUPLEX, DC_PAPERNAMES, DC_PAPERS,
    PRINTER_DEVICE_CAPABILITIES,
};

use crate::print_layout::{Device, Paper, PaperChoice, TwoSided};

/// `DocumentProperties` succeeded, and the reader pressed OK.
const IDOK: i32 = 1;
/// The reader closed the driver's dialog with Cancel.
const IDCANCEL: i32 = 2;
/// `dmOrientation` for a landscape sheet; portrait is 1.
const LANDSCAPE: i16 = 2;
const PORTRAIT: i16 = 1;
/// How many UTF-16 units `DC_PAPERNAMES` gives each name.
const PAPER_NAME: usize = 64;

/// The names of the printers this account can print to, local and shared.
///
/// # Errors
///
/// The spooler refusing the enumeration.
pub fn list() -> Result<Vec<String>, String> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut returned) = (0u32, 0u32);
    // SAFETY: a size query, with no buffer and two counters that outlive it.
    // It fails with "insufficient buffer" by design, and reports the size.
    let _ = unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut returned) };
    if needed == 0 {
        return Ok(Vec::new());
    }
    // `u64`s, so the records the spooler writes into it are aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: a byte view of that buffer, `needed` bytes long at least.
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), needed as usize)
    };
    // SAFETY: the buffer the size query asked for, and the same two counters.
    unsafe {
        EnumPrintersW(
            flags,
            PCWSTR::null(),
            4,
            Some(bytes),
            &mut needed,
            &mut returned,
        )
    }
    .map_err(|e| said!("the printers could not be listed: {e}"))?;
    // SAFETY: the spooler wrote `returned` records at the start of the buffer.
    let records = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_4W>(), returned as usize)
    };
    // SAFETY: each name points into the same buffer, which is alive here.
    Ok(records
        .iter()
        .filter_map(|record| unsafe { record.pPrinterName.to_string() }.ok())
        .collect())
}

/// The default printer's name, if there is one.
#[must_use]
pub fn default_name() -> Option<String> {
    let mut length = 0u32;
    // SAFETY: a size query; fails by design and reports the length.
    let _ = unsafe { GetDefaultPrinterW(None, &mut length) };
    if length == 0 {
        return None;
    }
    let mut name = vec![0u16; length as usize];
    // SAFETY: a buffer of the length asked for.
    if !unsafe { GetDefaultPrinterW(Some(PWSTR(name.as_mut_ptr())), &mut length) }.as_bool() {
        return None;
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..end])).filter(|name| !name.is_empty())
}

/// A printer and the settings this print uses on it.
pub struct Printer {
    name: String,
    port: String,
    /// The `DEVMODE` and the driver's private bytes after it, in `u64`s so
    /// the record is aligned.
    mode: Vec<u64>,
}

// SAFETY: a name, a port and a byte record; no handle is held between calls.
unsafe impl Send for Printer {}

/// A printer opened for one call, closed when dropped.
struct Opened(PRINTER_HANDLE);

impl Opened {
    fn new(name: &HSTRING) -> Result<Self, String> {
        let mut handle = PRINTER_HANDLE::default();
        // SAFETY: a wide name and a handle slot that outlive the call.
        unsafe { OpenPrinterW(name, &mut handle, None) }
            .map_err(|e| said!("the printer {name} could not be opened: {e}"))?;
        Ok(Self(handle))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: the handle `OpenPrinterW` gave, closed once.
        let _ = unsafe { ClosePrinter(self.0) };
    }
}

impl Printer {
    /// Opens `name` with the driver's current settings.
    ///
    /// # Errors
    ///
    /// The printer cannot be opened, or its driver gives no settings.
    pub fn open(name: &str) -> Result<Self, String> {
        let wide = HSTRING::from(name);
        let opened = Opened::new(&wide)?;
        // SAFETY: a size query on a live handle.
        let size = unsafe { DocumentPropertiesW(None, opened.0, &wide, None, None, 0) };
        let size = usize::try_from(size)
            .ok()
            .filter(|&size| size >= std::mem::size_of::<DEVMODEW>())
            .ok_or_else(|| said!("the driver of {name} gives no settings"))?;
        let mut mode = vec![0u64; size.div_ceil(8)];
        // SAFETY: a buffer of the size the driver asked for.
        let got = unsafe {
            DocumentPropertiesW(
                None,
                opened.0,
                &wide,
                Some(mode.as_mut_ptr().cast()),
                None,
                DM_OUT_BUFFER.0,
            )
        };
        if got != IDOK {
            return Err(said!("the driver of {name} gives no settings ({got})"));
        }
        let port = port_of(&opened).unwrap_or_default();
        Ok(Self {
            name: name.to_owned(),
            port,
            mode,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn mode(&self) -> &DEVMODEW {
        // SAFETY: the buffer holds a whole `DEVMODEW` (checked in `open`) and
        // is aligned for it.
        unsafe { &*self.mode.as_ptr().cast::<DEVMODEW>() }
    }

    fn mode_mut(&mut self) -> &mut DEVMODEW {
        // SAFETY: as `mode`.
        unsafe { &mut *self.mode.as_mut_ptr().cast::<DEVMODEW>() }
    }

    /// Sets the public fields the preview controls, and has the driver merge
    /// them into its own settings. `None` leaves a field as the driver has it.
    ///
    /// # Errors
    ///
    /// The driver refusing the settings.
    pub fn choose(
        &mut self,
        paper: Option<i16>,
        landscape: Option<bool>,
        copies: u16,
        two_sided: Option<TwoSided>,
        colour: Option<bool>,
    ) -> Result<(), String> {
        let offered = self.paper_ids();
        let mode = self.mode_mut();
        // SAFETY: the printer arm of the union is the one a printer's record uses.
        let fields = unsafe { &mut mode.Anonymous1.Anonymous1 };
        if let Some(paper) = paper.filter(|paper| offered.contains(paper)) {
            fields.dmPaperSize = paper;
            mode.dmFields |= DM_PAPERSIZE;
        }
        if let Some(landscape) = landscape {
            fields.dmOrientation = if landscape { LANDSCAPE } else { PORTRAIT };
            mode.dmFields |= DM_ORIENTATION;
        }
        fields.dmCopies = i16::try_from(copies.max(1)).unwrap_or(i16::MAX);
        mode.dmFields |= DM_COPIES;
        if let Some(sides) = two_sided {
            mode.dmDuplex = match sides {
                TwoSided::Off => DMDUP_SIMPLEX,
                TwoSided::Long => DMDUP_VERTICAL,
                TwoSided::Short => DMDUP_HORIZONTAL,
            };
            mode.dmFields |= DM_DUPLEX;
        }
        if let Some(colour) = colour {
            mode.dmColor = if colour {
                DMCOLOR_COLOR
            } else {
                DMCOLOR_MONOCHROME
            };
            mode.dmFields |= DM_COLOR;
        }
        self.merge(None, false).map(|_| ())
    }

    /// Opens the driver's own settings dialog over `owner`. `false` when the
    /// reader cancelled it, which leaves the settings as they were.
    ///
    /// # Errors
    ///
    /// The driver failing to show its dialog.
    pub fn prompt(&mut self, owner: Option<HWND>) -> Result<bool, String> {
        self.merge(owner, true)
    }

    /// Hands the record to the driver, with its dialog or without, and keeps
    /// what it answers.
    fn merge(&mut self, owner: Option<HWND>, prompt: bool) -> Result<bool, String> {
        let wide = HSTRING::from(self.name.as_str());
        let opened = Opened::new(&wide)?;
        let before = self.mode.clone();
        let mut after = vec![0u64; before.len()];
        let mode = if prompt {
            DM_IN_BUFFER.0 | DM_IN_PROMPT.0 | DM_OUT_BUFFER.0
        } else {
            DM_IN_BUFFER.0 | DM_OUT_BUFFER.0
        };
        // SAFETY: two buffers of the driver's size, and a live handle.
        let answer = unsafe {
            DocumentPropertiesW(
                owner,
                opened.0,
                &wide,
                Some(after.as_mut_ptr().cast()),
                Some(before.as_ptr().cast()),
                mode,
            )
        };
        match answer {
            IDOK => {
                self.mode = after;
                Ok(true)
            }
            IDCANCEL if prompt => Ok(false),
            other => Err(said!(
                "the driver of {name} refused the settings ({other})",
                name = self.name
            )),
        }
    }

    /// What the printer offers and what is chosen now.
    #[must_use]
    pub fn describe(&self) -> Device {
        let mode = self.mode();
        // SAFETY: the printer arm of the union, as in `choose`.
        let fields = unsafe { mode.Anonymous1.Anonymous1 };
        let names = self.paper_names();
        let papers = self
            .paper_ids()
            .into_iter()
            .zip(names)
            .map(|(id, name)| PaperChoice { id, name })
            .collect();
        let sides = match mode.dmDuplex {
            DMDUP_VERTICAL => TwoSided::Long,
            DMDUP_HORIZONTAL => TwoSided::Short,
            _ => TwoSided::Off,
        };
        let two_sided = (self.capability(DC_DUPLEX) == 1).then_some(sides);
        let colour =
            (self.capability(DC_COLORDEVICE) == 1).then_some(mode.dmColor != DMCOLOR_MONOCHROME);
        Device {
            printer: self.name.clone(),
            papers,
            paper: fields.dmPaperSize,
            landscape: fields.dmOrientation == LANDSCAPE,
            copies: u16::try_from(fields.dmCopies).unwrap_or(1).max(1),
            most_copies: u16::try_from(self.capability(DC_COPIES))
                .unwrap_or(1)
                .max(1),
            two_sided,
            colour,
        }
    }

    /// The paper as these settings give it, measured by the driver.
    ///
    /// # Errors
    ///
    /// The driver giving no information context for the settings.
    pub fn paper(&self) -> Result<Paper, String> {
        let dc = self.context(false)?;
        let paper = crate::print_win::paper_of(dc);
        // SAFETY: the context just created, released once.
        let _ = unsafe { DeleteDC(dc) };
        Ok(paper)
    }

    /// A device context to print on with these settings. The caller deletes it.
    ///
    /// # Errors
    ///
    /// The driver refusing to create one.
    pub fn dc(&self) -> Result<HDC, String> {
        self.context(true)
    }

    fn context(&self, print: bool) -> Result<HDC, String> {
        let driver = HSTRING::from("WINSPOOL");
        let device = HSTRING::from(self.name.as_str());
        let mode: *const DEVMODEW = self.mode.as_ptr().cast();
        // SAFETY: two wide strings and the settings record, all alive here.
        let dc = unsafe {
            if print {
                CreateDCW(&driver, &device, PCWSTR::null(), Some(mode))
            } else {
                CreateICW(&driver, &device, PCWSTR::null(), Some(mode))
            }
        };
        if dc.is_invalid() {
            return Err(said!(
                "the printer {name} could not be prepared: {e}",
                name = self.name,
                e = std::io::Error::last_os_error()
            ));
        }
        Ok(dc)
    }

    /// A capability that is a single number; -1 when the driver does not say.
    fn capability(&self, which: PRINTER_DEVICE_CAPABILITIES) -> i32 {
        let (name, port) = (HSTRING::from(self.name.as_str()), self.port_name());
        // SAFETY: two wide strings, no output buffer, and the settings record.
        unsafe { DeviceCapabilitiesW(&name, &port, which, None, Some(self.mode.as_ptr().cast())) }
    }

    fn port_name(&self) -> HSTRING {
        HSTRING::from(self.port.as_str())
    }

    /// The driver's numbers for the papers it offers.
    fn paper_ids(&self) -> Vec<i16> {
        let count = usize::try_from(self.capability(DC_PAPERS)).unwrap_or(0);
        let mut ids = vec![0u16; count];
        if count > 0 {
            self.fill(DC_PAPERS, ids.as_mut_ptr());
        }
        ids.into_iter().map(|id| id as i16).collect()
    }

    /// The names of those papers, in the same order.
    fn paper_names(&self) -> Vec<String> {
        let count = usize::try_from(self.capability(DC_PAPERNAMES)).unwrap_or(0);
        let mut names = vec![0u16; count * PAPER_NAME];
        if count > 0 {
            self.fill(DC_PAPERNAMES, names.as_mut_ptr());
        }
        names
            .chunks(PAPER_NAME)
            .map(|name| {
                let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                String::from_utf16_lossy(&name[..end])
            })
            .collect()
    }

    /// Asks for a list capability into `out`, sized by the count asked first.
    fn fill(&self, which: PRINTER_DEVICE_CAPABILITIES, out: *mut u16) {
        let (name, port) = (HSTRING::from(self.name.as_str()), self.port_name());
        // SAFETY: `out` holds as many entries as the same capability counted.
        let _ = unsafe {
            DeviceCapabilitiesW(
                &name,
                &port,
                which,
                Some(PWSTR(out)),
                Some(self.mode.as_ptr().cast()),
            )
        };
    }
}

/// The port a printer prints to, which `DeviceCapabilities` wants beside its name.
fn port_of(opened: &Opened) -> Option<String> {
    let mut needed = 0u32;
    // SAFETY: a size query on a live handle.
    let _ = unsafe { GetPrinterW(opened.0, 5, None, &mut needed) };
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: a byte view of that buffer, `needed` bytes long at least.
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), needed as usize)
    };
    // SAFETY: the buffer the size query asked for.
    unsafe { GetPrinterW(opened.0, 5, Some(bytes), &mut needed) }.ok()?;
    // SAFETY: the spooler wrote one record at the start, with its strings after it.
    let record = unsafe { &*buffer.as_ptr().cast::<PRINTER_INFO_5W>() };
    // SAFETY: the port name points into the same live buffer.
    unsafe { record.pPortName.to_string() }.ok()
}

#[cfg(test)]
mod tests {
    use super::{default_name, list, Printer};

    /// Every printer this machine has is opened, described and measured, and
    /// a paper and an orientation chosen on it are what it then reports.
    ///
    /// A machine with no printer has nothing to check, and says so; the
    /// Windows runner has "Microsoft Print to PDF".
    #[test]
    fn each_printer_describes_its_papers_and_keeps_a_choice() {
        let names = list().expect("the printers are listed");
        if names.is_empty() {
            eprintln!("[SKIP] this machine has no printer");
            return;
        }
        if let Some(default) = default_name() {
            assert!(names.contains(&default), "{default:?} not in {names:?}");
        }
        for name in &names {
            let Ok(mut printer) = Printer::open(name) else {
                // A shared printer whose server is off is listed and cannot
                // be opened; that is the network's state, not a defect.
                eprintln!("[SKIP] {name} could not be opened");
                continue;
            };
            let device = printer.describe();
            assert!(!device.papers.is_empty(), "{name} offers no paper");
            assert!(
                device.papers.iter().any(|p| p.id == device.paper),
                "{name}'s paper {} is not among those it offers: {:?}",
                device.paper,
                device.papers
            );
            let paper = printer.paper().expect("measured");
            assert!(
                paper.printable_width > 0.0 && paper.printable_width <= paper.width,
                "{name}: {paper:?}"
            );
            let turned = !device.landscape;
            printer
                .choose(None, Some(turned), 1, None, None)
                .expect("the driver takes an orientation");
            assert_eq!(printer.describe().landscape, turned, "{name}");
            let across = printer.paper().expect("measured again");
            assert_eq!(
                across.width > across.height,
                paper.width <= paper.height,
                "{name}: turning the paper turns its measurements: {paper:?} then {across:?}"
            );
            eprintln!(
                "{name}: {} papers, {} chosen, {:.0}x{:.0} pt, copies to {}, two-sided {:?}, colour {:?}",
                device.papers.len(),
                device.paper,
                paper.width,
                paper.height,
                device.most_copies,
                device.two_sided,
                device.colour
            );
        }
    }
}
