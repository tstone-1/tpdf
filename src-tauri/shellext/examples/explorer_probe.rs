//! What Explorer gets from tpdf's handlers, asked the way Explorer asks, on
//! Windows only. `scripts/explorer_check.py` runs it and reads what it prints.
//!
//!     explorer_probe thumbs <file>...
//!     explorer_probe preview <file.pdf>
//!     explorer_probe modules
//!
//! `thumbs` asks Windows' own thumbnail cache for each file with
//! `WTS_FORCEEXTRACTION`, so the registered provider runs in the COM surrogate
//! (`dllhost.exe`) as it does for an Explorer window, and nothing is answered
//! from an earlier run. One line per file:
//! `thumb\t<file>\tok|none\t<w>x<h>\t<distinct colours>\t<ms>`.
//!
//! `preview` hosts the registered preview handler in `prevhost.exe` inside a
//! window of this process, as the preview pane does, and reads the screen:
//! `preview\tpage1\t<share of near-white pixels>` after the first page, and
//! `preview\tpage2\t<share>\t<share of pixels that changed>` after Page Down.
//!
//! `modules` lists, for every `dllhost.exe`, `prevhost.exe` and `explorer.exe`
//! this user can read, whether it has `tpdf_shell.dll` and `pdfium.dll` mapped:
//! `modules\t<name>\t<pid>\ttpdf_shell=<bool>\tpdfium=<bool>`. `thumbs` and
//! `preview` print the same lines while their handler is still loaded.
//!
//! It reads the screen, so `preview` needs a desktop: run it in the logged-on
//! session, not over ssh.

#[cfg(not(windows))]
fn main() {
    eprintln!("explorer_probe runs on Windows only");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // SAFETY: once, on this thread, before any COM call.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let code = match args.first().map(String::as_str) {
        Some("thumbs") => probe::thumbs(&args[1..]),
        Some("preview") if args.len() == 2 => probe::preview(&args[1]),
        Some("modules") => {
            probe::modules();
            0
        }
        _ => {
            eprintln!("usage: explorer_probe thumbs <file>... | preview <file.pdf> | modules");
            2
        }
    };
    std::process::exit(code);
}

#[cfg(windows)]
mod probe {
    use std::time::{Duration, Instant};
    use windows::core::{Interface, GUID, HSTRING};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, GetObjectW, ReleaseDC, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, SRCCOPY,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_NEXT;
    use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
    use windows::Win32::UI::Shell::{
        IPreviewHandler, ISharedBitmap, IShellItem, IThumbnailCache, LocalThumbnailCache,
        SHCreateItemFromParsingName, SHCreateStreamOnFileEx, WTS_EXTRACT, WTS_FORCEEXTRACTION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, PeekMessageW,
        PostMessageW, RegisterClassW, SetForegroundWindow, ShowWindow, TranslateMessage, MSG,
        PM_REMOVE, SW_SHOW, WM_KEYDOWN, WNDCLASSW, WS_CLIPCHILDREN, WS_EX_TOPMOST, WS_POPUP,
        WS_VISIBLE,
    };

    const STGM_READ: u32 = 0;

    /// The preview handler's class, `tpdf_shell::PREVIEW_CLSID`.
    fn preview_class() -> GUID {
        let text = tpdf_shell::PREVIEW_CLSID.trim_matches(['{', '}']);
        GUID::try_from(text).expect("the preview class id parses")
    }

    pub fn thumbs(files: &[String]) -> i32 {
        // SAFETY: COM is initialised on this thread; the class is Windows' own.
        let cache: IThumbnailCache =
            match unsafe { CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER) } {
                Ok(cache) => cache,
                Err(e) => {
                    eprintln!("the thumbnail cache did not start: {e}");
                    return 1;
                }
            };
        for file in files {
            let started = Instant::now();
            let answer = thumbnail(&cache, file);
            let ms = started.elapsed().as_millis();
            match answer {
                Ok((w, h, colours)) => println!("thumb\t{file}\tok\t{w}x{h}\t{colours}\t{ms}"),
                Err(why) => println!("thumb\t{file}\tnone\t{why}\t0\t{ms}"),
            }
        }
        modules();
        0
    }

    fn thumbnail(cache: &IThumbnailCache, file: &str) -> Result<(i32, i32, usize), String> {
        // SAFETY: the path is a live HSTRING; COM is initialised.
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from(file), None) }
            .map_err(|e| format!("{:#010x}", e.code().0))?;
        let mut shared: Option<ISharedBitmap> = None;
        // SAFETY: `shared` outlives the call; the other outputs are not asked for.
        unsafe {
            cache.GetThumbnail(
                &item,
                256,
                WTS_EXTRACT | WTS_FORCEEXTRACTION,
                Some(&mut shared),
                None,
                None,
            )
        }
        .map_err(|e| format!("{:#010x}", e.code().0))?;
        let shared = shared.ok_or("no bitmap")?;
        // SAFETY: the cache owns the bitmap for as long as `shared` lives.
        let bitmap = unsafe { shared.GetSharedBitmap() }.map_err(|e| e.to_string())?;
        let (w, h, pixels) = read_bitmap(bitmap)?;
        Ok((w, h, distinct(&pixels)))
    }

    /// The bitmap's pixels as 32-bit BGRA, top row first.
    fn read_bitmap(bitmap: HBITMAP) -> Result<(i32, i32, Vec<u8>), String> {
        let mut info = BITMAP::default();
        // SAFETY: `info` is the size given.
        let got = unsafe {
            GetObjectW(
                bitmap.into(),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&raw mut info).cast()),
            )
        };
        if got == 0 {
            return Err("the bitmap could not be read".into());
        }
        let (w, h) = (info.bmWidth, info.bmHeight.abs());
        // SAFETY: a screen DC, released below.
        let screen = unsafe { GetDC(None) };
        let pixels = dib(screen, bitmap, w, h);
        // SAFETY: released once.
        unsafe { ReleaseDC(None, screen) };
        Ok((w, h, pixels?))
    }

    fn dib(dc: HDC, bitmap: HBITMAP, w: i32, h: i32) -> Result<Vec<u8>, String> {
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        // SAFETY: `pixels` holds `h` rows of `w` 32-bit pixels, as `header` says.
        let rows = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut header,
                DIB_RGB_COLORS,
            )
        };
        if rows == 0 {
            return Err("the bitmap's pixels could not be read".into());
        }
        Ok(pixels)
    }

    /// How many colours a picture has: a generic icon or an empty bitmap has
    /// few, a rendered page many.
    fn distinct(pixels: &[u8]) -> usize {
        let mut seen = std::collections::HashSet::new();
        for p in pixels.chunks_exact(4) {
            seen.insert((p[0], p[1], p[2]));
        }
        seen.len()
    }

    extern "system" fn host_proc(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        // SAFETY: the default procedure for this window.
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    fn pump(for_how_long: Duration) {
        let until = Instant::now() + for_how_long;
        while Instant::now() < until {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live local.
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                // SAFETY: a message this thread received.
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The window's client area as read from the screen, BGRA.
    fn screen_of(rect: RECT) -> Vec<u8> {
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        // SAFETY: GDI objects made and released here, in order.
        unsafe {
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(memory, bitmap.into());
            let _ = BitBlt(
                memory,
                0,
                0,
                w,
                h,
                Some(screen),
                rect.left,
                rect.top,
                SRCCOPY,
            );
            SelectObject(memory, old);
            let pixels = dib(memory, bitmap, w, h).unwrap_or_default();
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            pixels
        }
    }

    /// Writes what was read off the screen as a BMP into `TPDF_PROBE_SHOTS`,
    /// when that names a folder, so a failed check can be looked at.
    fn save_bmp(pixels: &[u8], rect: RECT, name: &str) {
        let Some(folder) = std::env::var_os("TPDF_PROBE_SHOTS") else {
            return;
        };
        let (w, h) = (
            (rect.right - rect.left) as u32,
            (rect.bottom - rect.top) as u32,
        );
        let mut file = Vec::with_capacity(54 + pixels.len());
        let size = 54 + pixels.len() as u32;
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&size.to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&w.to_le_bytes());
        // Negative: the rows are top first, as `dib` read them.
        file.extend_from_slice(&(-(h as i32)).to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&[0; 24]);
        file.extend_from_slice(pixels);
        let _ = std::fs::write(
            std::path::Path::new(&folder).join(format!("preview-{name}.bmp")),
            file,
        );
    }

    fn white_share(pixels: &[u8]) -> f64 {
        let count = pixels.chunks_exact(4).count().max(1);
        let white = pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240)
            .count();
        white as f64 / count as f64
    }

    fn changed_share(a: &[u8], b: &[u8]) -> f64 {
        let count = a.len().min(b.len()) / 4;
        let changed = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(x, y)| x[..3] != y[..3])
            .count();
        changed as f64 / count.max(1) as f64
    }

    pub fn preview(file: &str) -> i32 {
        match preview_inner(file) {
            Ok(()) => 0,
            Err(why) => {
                println!("preview\tfailed\t{why}");
                1
            }
        }
    }

    fn preview_inner(file: &str) -> Result<(), String> {
        let class = HSTRING::from("tpdfExplorerProbe");
        let rect = RECT {
            left: 40,
            top: 40,
            right: 40 + 600,
            bottom: 40 + 700,
        };
        // SAFETY: the class and window are made once and live for the process.
        let host = unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(host_proc),
                // Black, so a pane the handler never painted reads as black
                // and not as whatever was on the screen before.
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                    windows::Win32::Graphics::Gdi::GetStockObject(
                        windows::Win32::Graphics::Gdi::BLACK_BRUSH,
                    )
                    .0,
                ),
                lpszClassName: windows::core::PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WS_EX_TOPMOST,
                &class,
                &HSTRING::from("tpdf preview probe"),
                WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                None,
                None,
            )
        }
        .map_err(|e| format!("the host window: {e}"))?;
        // SAFETY: a window this thread made.
        unsafe {
            // Twice: a process's first `ShowWindow` takes the show state its
            // parent asked for, and the check runs under a hidden PowerShell,
            // so the first call hides the window. Measured: with one call the
            // screen read back was the desktop.
            let _ = ShowWindow(host, SW_SHOW);
            let _ = ShowWindow(host, SW_SHOW);
            let _ = SetForegroundWindow(host);
        }
        pump(Duration::from_millis(300));
        // SAFETY: COM is initialised; the class is registered by `tpdf explorer --add`.
        let init: IInitializeWithStream =
            unsafe { CoCreateInstance(&preview_class(), None, CLSCTX_LOCAL_SERVER) }
                .map_err(|e| format!("the preview handler did not start: {e}"))?;
        // SAFETY: the path is a live HSTRING.
        let stream =
            unsafe { SHCreateStreamOnFileEx(&HSTRING::from(file), STGM_READ, 0, false, None) }
                .map_err(|e| format!("the document: {e}"))?;
        // SAFETY: the stream is live for the call.
        unsafe { init.Initialize(&stream, STGM_READ) }.map_err(|e| format!("Initialize: {e}"))?;
        let handler: IPreviewHandler = init.cast().map_err(|e| e.to_string())?;
        let client = RECT {
            left: 0,
            top: 0,
            right: rect.right - rect.left,
            bottom: rect.bottom - rect.top,
        };
        // SAFETY: the host window and the rectangle are live.
        unsafe { handler.SetWindow(host, &client) }.map_err(|e| format!("SetWindow: {e}"))?;
        let asked = Instant::now();
        // SAFETY: as above.
        unsafe { handler.DoPreview() }.map_err(|e| format!("DoPreview: {e}"))?;
        println!("preview\tdo_preview_ms\t{}", asked.elapsed().as_millis());
        pump(Duration::from_secs(6));
        // SAFETY: windows of this thread and the handler's child, read only.
        unsafe {
            let child = FindWindowExW(Some(host), None, None, None).unwrap_or_default();
            let mut at = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(child, &mut at);
            println!(
                "preview\twindows\thost_visible={}\tchild={}\tchild_visible={}\tchild_rect={},{},{},{}",
                windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(host).as_bool(),
                !child.is_invalid(),
                windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(child).as_bool(),
                at.left, at.top, at.right, at.bottom
            );
        }
        let first = screen_of(rect);
        println!("preview\tpage1\t{:.3}", white_share(&first));
        save_bmp(&first, rect, "page1");
        modules();
        // SAFETY: a child window of ours, made by the handler in prevhost.
        let child = unsafe { FindWindowExW(Some(host), None, None, None) }.unwrap_or_default();
        if child.is_invalid() {
            println!("preview\tpage2\t0\t0\tno child window");
        } else {
            // SAFETY: posting a key to the handler's window.
            unsafe {
                let _ = PostMessageW(
                    Some(child),
                    WM_KEYDOWN,
                    WPARAM(VK_NEXT.0 as usize),
                    LPARAM(0),
                );
            }
            pump(Duration::from_secs(6));
            let second = screen_of(rect);
            save_bmp(&second, rect, "page2");
            println!(
                "preview\tpage2\t{:.3}\t{:.3}",
                white_share(&second),
                changed_share(&first, &second)
            );
        }
        // SAFETY: as above.
        let _ = unsafe { handler.Unload() };
        Ok(())
    }

    fn name_of(raw: &[u16]) -> String {
        let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..end])
    }

    pub fn modules() {
        // SAFETY: a snapshot of the process table, closed below.
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            println!("modules\tunreadable\t0\ttpdf_shell=false\tpdfium=false");
            return;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: `entry` has its size set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            let name = name_of(&entry.szExeFile).to_ascii_lowercase();
            if ["dllhost.exe", "prevhost.exe", "explorer.exe"].contains(&name.as_str()) {
                let (shell, pdfium) = mapped(entry.th32ProcessID);
                println!(
                    "modules\t{name}\t{}\ttpdf_shell={shell}\tpdfium={pdfium}",
                    entry.th32ProcessID
                );
            }
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        // SAFETY: closed once.
        let _ = unsafe { CloseHandle(snapshot) };
    }

    fn mapped(pid: u32) -> (bool, bool) {
        // SAFETY: a module snapshot of one process; a process this user cannot
        // read gives an error, read as nothing mapped.
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) }) else {
            return (false, false);
        };
        let mut entry = MODULEENTRY32W {
            dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let (mut shell, mut pdfium) = (false, false);
        // SAFETY: `entry` has its size set.
        let mut more = unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            let name = name_of(&entry.szModule).to_ascii_lowercase();
            shell |= name == "tpdf_shell.dll";
            pdfium |= name == "pdfium.dll";
            // SAFETY: as above.
            more = unsafe { Module32NextW(snapshot, &mut entry) }.is_ok();
        }
        // SAFETY: closed once.
        let _ = unsafe { CloseHandle(snapshot) };
        (shell, pdfium)
    }
}
