//! The preview handler. Explorer runs it in `prevhost.exe` and gives it a
//! window in the preview pane; it shows one page at a time, fitted to the
//! pane, and Page Down, Page Up, the arrow keys, Home and the wheel move
//! between pages.
//!
//! **The pane never waits on the renderer.** `DoPreview` makes the window and
//! returns; each page is rendered on a thread of its own, which leaves its
//! result in a mailbox and posts [`WM_RENDERED`]. A request carries a
//! generation number, and the window takes only the newest one, so a page
//! that comes back after the reader moved on is dropped. A document that runs
//! out its time leaves the pane saying so, and `prevhost.exe` answering.
//!
//! The handler does not know how many pages a document has. Asking for a page
//! past the end is refused by `tpdf-cli`, and the pane keeps the page it shows.
//!
//! Everything but the render thread runs on the one thread `prevhost.exe`
//! calls the handler on, so the pane's state is a `RefCell`. No borrow of it
//! is held across a call that sends a message to the window
//! (`CreateWindowExW`, `SetWindowPos`, `SetParent`, `DestroyWindow`), because
//! the window procedure borrows it too.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{implement, w, Interface, Ref, Result, BOOL, GUID, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_CLASS_ALREADY_EXISTS, E_FAIL, E_INVALIDARG, E_NOTIMPL, E_POINTER,
    HWND, LPARAM, LRESULT, RECT, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, FillRect, GetStockObject, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    DEFAULT_GUI_FONT, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBITMAP, PAINTSTRUCT, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, IStream, COINIT_MULTITHREADED};
use windows::Win32::System::Ole::{
    IObjectWithSite, IObjectWithSite_Impl, IOleWindow, IOleWindow_Impl,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SetFocus, VIRTUAL_KEY, VK_DOWN, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{IPreviewHandler, IPreviewHandlerFrame, IPreviewHandler_Impl};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassExW, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, UnregisterClassW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, DLGC_WANTALLKEYS,
    GWLP_USERDATA, IDC_ARROW, MSG, SWP_NOACTIVATE, SWP_NOZORDER, WINDOW_EX_STYLE, WM_APP,
    WM_ERASEBKGND, WM_GETDLGCODE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_NCCREATE,
    WM_NCDESTROY, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

use crate::plan::{self, Fit, Step};
use crate::png::{self, Pixels};
use crate::{render, Counted};

/// Posted by a render thread when its result is in the pane's mailbox.
const WM_RENDERED: u32 = WM_APP + 1;

/// The timer that turns a run of `WM_SIZE` into one render, 200 ms after
/// the last, so dragging the pane's edge does not start a `tpdf-cli` a pixel.
const RESIZED: usize = 1;

const CLASS: PCWSTR = w!("tpdfPreviewPane");

/// Whether [`CLASS`] is registered by this load of the DLL.
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// What a render thread got.
enum Got {
    Page(Pixels),
    /// `tpdf-cli` refused: on page 1 the document, past it the end.
    Refused,
    Failed,
}

struct Outcome {
    generation: u64,
    page: u32,
    got: Got,
}

/// A page on screen. The bitmap is freed with it.
struct Shown {
    bitmap: HBITMAP,
    width: i32,
    height: i32,
}

impl Drop for Shown {
    fn drop(&mut self) {
        // SAFETY: a bitmap this value made and owns; it is not selected into
        // any DC outside `paint`, which deselects it before returning.
        let _ = unsafe { DeleteObject(self.bitmap.into()) };
    }
}

#[derive(Default)]
struct Pane {
    /// The window, or 0 before `DoPreview` and after it is destroyed.
    window: isize,
    parent: isize,
    rect: RECT,
    document: Option<Arc<[u8]>>,
    /// The page on screen, and the page last asked for. They differ while a
    /// render is out, and keys move from the second, so pressing Page Down
    /// three times asks for three pages on.
    page: u32,
    wanted: u32,
    shown: Option<Shown>,
    /// Nothing could be shown: the document was refused or failed.
    failed: bool,
    generation: u64,
    /// The box and page of the newest request, so that a resize that ends at
    /// the size already asked for starts nothing.
    asked: Option<(Fit, u32)>,
    mailbox: Arc<Mutex<Option<Outcome>>>,
    /// Set when the window procedure took its reference to the pane, so that
    /// a `CreateWindowExW` that failed before then can give it back.
    taken: bool,
}

// `Agile = false`: `#[implement]` otherwise adds `IAgileObject` and the
// free-threaded marshaler, and COM then calls the object on whatever thread
// the call arrives on. In `prevhost.exe` that was a thread of the
// multithreaded apartment, which runs no message loop, so the preview's window
// got the messages sent to it and never the ones posted: no `WM_PAINT`, no
// timer, no finished render (measured 2026-10-16). Not agile, COM keeps every
// call in the apartment the `Apartment` registration asks for.
#[implement(
    IInitializeWithStream,
    IPreviewHandler,
    IObjectWithSite,
    IOleWindow,
    Agile = false
)]
pub struct Preview {
    pane: Rc<RefCell<Pane>>,
    site: RefCell<Option<windows::core::IUnknown>>,
    _counted: Counted,
}

impl Preview {
    pub fn new() -> Self {
        Preview {
            pane: Rc::new(RefCell::new(Pane {
                page: 1,
                wanted: 1,
                ..Default::default()
            })),
            site: RefCell::new(None),
            _counted: Counted::new(),
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        destroy(&self.pane);
    }
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// Destroys the window if there is one, and forgets the page and the
/// document. A result still out is dropped by the generation moving on.
fn destroy(pane: &Rc<RefCell<Pane>>) {
    let window = {
        let Ok(mut p) = pane.try_borrow_mut() else {
            return;
        };
        p.generation += 1;
        p.asked = None;
        p.shown = None;
        p.document = None;
        std::mem::take(&mut p.window)
    };
    if window != 0 {
        // SAFETY: a window this pane made; no borrow is held, since its
        // procedure runs during the call.
        let _ = unsafe { DestroyWindow(hwnd(window)) };
    }
}

fn register_class() -> Result<()> {
    if REGISTERED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(procedure),
        hInstance: crate::module(),
        // SAFETY: a system cursor, which needs no module.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // SAFETY: the structure is complete and its procedure lives in this DLL,
    // which stays loaded while the class is registered (`DllCanUnloadNow`).
    if unsafe { RegisterClassExW(&class) } == 0
        // SAFETY: read straight after the call that set it.
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err(windows::core::Error::from_thread());
    }
    REGISTERED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Unregisters the window class, when COM is about to unload the DLL and no
/// window of it can exist.
pub(crate) fn unregister_class() {
    if REGISTERED.swap(false, Ordering::SeqCst) {
        // SAFETY: the class this DLL registered, under its own module.
        let _ = unsafe { UnregisterClassW(CLASS, Some(crate::module())) };
    }
}

/// Starts a render of `page` fitted to the window, unless that exact one was
/// the last asked for.
fn request(pane: &Rc<RefCell<Pane>>, page: u32) {
    let mut p = pane.borrow_mut();
    if p.window == 0 {
        return;
    }
    let Some(document) = p.document.clone() else {
        return;
    };
    let mut client = RECT::default();
    // SAFETY: the pane's own live window, and an out parameter.
    if unsafe { GetClientRect(hwnd(p.window), &mut client) }.is_err() {
        return;
    }
    let Some(fit) = plan::preview_fit(client.right - client.left, client.bottom - client.top)
    else {
        return;
    };
    if p.asked == Some((fit, page)) {
        return;
    }
    p.asked = Some((fit, page));
    p.wanted = page;
    p.generation += 1;
    let generation = p.generation;
    let window = p.window;
    let mailbox = Arc::clone(&p.mailbox);
    drop(p);

    // Taken here, before the thread exists, so the DLL cannot be unloaded
    // between the two.
    let counted = Counted::new();
    std::thread::spawn(move || {
        let _counted = counted;
        let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rendered(document, fit, page)
        }))
        .unwrap_or(Got::Failed);
        if let Ok(mut slot) = mailbox.lock() {
            // A newer result already waiting is not replaced by an older one.
            if slot
                .as_ref()
                .is_none_or(|waiting| waiting.generation < generation)
            {
                *slot = Some(Outcome {
                    generation,
                    page,
                    got,
                });
            }
        }
        // SAFETY: posting to a window that may be gone is harmless: the call
        // fails, and the result waits in the mailbox until the pane is dropped.
        let _ = unsafe { PostMessageW(Some(hwnd(window)), WM_RENDERED, WPARAM(0), LPARAM(0)) };
    });
}

/// COM on the render thread, for WIC, undone when the thread ends.
struct ComScope(bool);

impl ComScope {
    fn new() -> Self {
        // SAFETY: once per thread, undone in `drop` only if it succeeded.
        ComScope(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs the successful `CoInitializeEx` above.
            unsafe { CoUninitialize() };
        }
    }
}

/// The render thread's work: `tpdf-cli`, then WIC.
fn rendered(document: Arc<[u8]>, fit: Fit, page: u32) -> Got {
    let _com = ComScope::new();
    let Some(cli) = crate::cli_path() else {
        return Got::Failed;
    };
    match render::page(&cli, document, fit, page, plan::PREVIEW_TIME) {
        Ok(png) => png::decode(&png, fit).map_or(Got::Failed, Got::Page),
        Err(render::Failure::Refused) => Got::Refused,
        Err(render::Failure::TimedOut | render::Failure::Failed) => Got::Failed,
    }
}

/// Takes the mailbox's result, if it is the newest request's, onto the screen.
fn arrived(pane: &Rc<RefCell<Pane>>) {
    let mut p = pane.borrow_mut();
    let outcome = p.mailbox.lock().ok().and_then(|mut slot| slot.take());
    let Some(outcome) = outcome else {
        return;
    };
    if outcome.generation != p.generation {
        return;
    }
    match outcome.got {
        Got::Page(pixels) => match png::bitmap(&pixels) {
            Ok(bitmap) => {
                p.shown = Some(Shown {
                    bitmap,
                    width: pixels.width as i32,
                    height: pixels.height as i32,
                });
                p.page = outcome.page;
                p.wanted = outcome.page;
                p.failed = false;
            }
            Err(_) => fail(&mut p),
        },
        Got::Refused | Got::Failed => fail(&mut p),
    }
    let window = p.window;
    drop(p);
    // SAFETY: the pane's own window; invalidating sends no message.
    let _ = unsafe { InvalidateRect(Some(hwnd(window)), None, false) };
}

/// A page did not come back: past the end, or broken. With a page on screen
/// that page stays and so does its number; with none, the pane says so.
fn fail(p: &mut Pane) {
    if p.shown.is_some() {
        p.wanted = p.page;
        p.asked = None;
    } else {
        p.failed = true;
    }
}

fn step(pane: &Rc<RefCell<Pane>>, step: Step) {
    let from = pane.borrow().wanted;
    if let Some(page) = plan::stepped(from, step) {
        request(pane, page);
    }
}

fn key_step(key: u16) -> Option<Step> {
    let key = VIRTUAL_KEY(key);
    if key == VK_NEXT || key == VK_DOWN || key == VK_RIGHT {
        Some(Step::Next)
    } else if key == VK_PRIOR || key == VK_UP || key == VK_LEFT {
        Some(Step::Previous)
    } else if key == VK_HOME {
        Some(Step::First)
    } else {
        None
    }
}

fn paint(pane: &Rc<RefCell<Pane>>, window: HWND) {
    let p = pane.borrow();
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: the usual paint sequence on the pane's own window. Every GDI
    // object made here is deleted here, and the bitmap is deselected from
    // the memory DC before that DC is deleted.
    unsafe {
        let dc = BeginPaint(window, &mut paint);
        let mut client = RECT::default();
        let _ = GetClientRect(window, &mut client);
        let background = CreateSolidBrush(COLORREF(0x00A0_A0A0));
        FillRect(dc, &client, background);
        let _ = DeleteObject(background.into());

        if let Some(shown) = &p.shown {
            let (x, y) = plan::placed(client.right, client.bottom, shown.width, shown.height);
            let memory = CreateCompatibleDC(Some(dc));
            let before = SelectObject(memory, shown.bitmap.into());
            let _ = BitBlt(
                dc,
                x,
                y,
                shown.width,
                shown.height,
                Some(memory),
                0,
                0,
                SRCCOPY,
            );
            SelectObject(memory, before);
            let _ = DeleteDC(memory);
        }

        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0));
        SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
        let (text, mut rect) = if p.shown.is_some() {
            let mut label = client;
            label.top = label.bottom - plan::LABEL;
            (format!("Page {}", p.page), label)
        } else if p.failed {
            ("tpdf could not show this document.".to_string(), client)
        } else {
            ("Loading...".to_string(), client)
        };
        let mut text: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
        let _ = EndPaint(window, &paint);
    }
}

/// What the window does with one message, or `None` for the default.
fn handle(pane: &Rc<RefCell<Pane>>, window: HWND, message: u32, wparam: WPARAM) -> Option<LRESULT> {
    match message {
        // Everything is painted in WM_PAINT, so the background is never
        // erased first and the page does not flicker.
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_PAINT => {
            paint(pane, window);
            Some(LRESULT(0))
        }
        WM_SIZE => {
            // SAFETY: a timer on the pane's own window; setting it again
            // restarts it, which is the debounce.
            unsafe { SetTimer(Some(window), RESIZED, 200, None) };
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == RESIZED => {
            // SAFETY: the timer set above.
            let _ = unsafe { KillTimer(Some(window), RESIZED) };
            let page = pane.borrow().wanted;
            request(pane, page);
            Some(LRESULT(0))
        }
        WM_RENDERED => {
            arrived(pane);
            Some(LRESULT(0))
        }
        WM_KEYDOWN => key_step(wparam.0 as u16).map(|wanted| {
            step(pane, wanted);
            LRESULT(0)
        }),
        WM_MOUSEWHEEL => {
            // The high word is the distance turned, positive away from the reader.
            let delta = (wparam.0 >> 16) as u16 as i16;
            step(
                pane,
                if delta > 0 {
                    Step::Previous
                } else {
                    Step::Next
                },
            );
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            // SAFETY: the pane's own window, on its own thread.
            let _ = unsafe { SetFocus(Some(window)) };
            Some(LRESULT(0))
        }
        WM_GETDLGCODE => Some(LRESULT(DLGC_WANTALLKEYS as isize)),
        _ => None,
    }
}

/// The window procedure. The window holds one reference to the pane, put in
/// its user data at `WM_NCCREATE` and given back at `WM_NCDESTROY`.
unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY, for the whole function: the user data is either 0 or the
    // pointer `Rc::into_raw` gave `DoPreview`, which only `WM_NCDESTROY`
    // turns back into an `Rc`, once.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let raw = create.lpCreateParams as *const RefCell<Pane>;
            SetWindowLongPtrW(window, GWLP_USERDATA, raw as isize);
            if !raw.is_null() {
                Rc::increment_strong_count(raw);
                let pane = Rc::from_raw(raw);
                if let Ok(mut p) = pane.try_borrow_mut() {
                    p.taken = true;
                };
            }
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let raw = GetWindowLongPtrW(window, GWLP_USERDATA) as *const RefCell<Pane>;
        if raw.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            let pane = Rc::from_raw(raw);
            if let Ok(mut p) = pane.try_borrow_mut() {
                p.window = 0;
            };
            drop(pane);
            return DefWindowProcW(window, message, wparam, lparam);
        }
        Rc::increment_strong_count(raw);
        let pane = Rc::from_raw(raw);
        let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle(&pane, window, message, wparam)
        }))
        .ok()
        .flatten();
        drop(pane);
        handled.unwrap_or_else(|| DefWindowProcW(window, message, wparam, lparam))
    }
}

fn place(window: isize, rect: RECT) {
    if window != 0 {
        // SAFETY: the pane's own window; no borrow is held during the call,
        // which sends WM_SIZE.
        let _ = unsafe {
            SetWindowPos(
                hwnd(window),
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }
}

impl IInitializeWithStream_Impl for Preview_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> Result<()> {
        let document = crate::read_stream(stream.ok()?)?;
        let mut p = self.pane.borrow_mut();
        p.document = Some(document);
        p.page = 1;
        p.wanted = 1;
        Ok(())
    }
}

impl IPreviewHandler_Impl for Preview_Impl {
    fn SetWindow(&self, parent: HWND, rect: *const RECT) -> Result<()> {
        if rect.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let (window, rect) = {
            let mut p = self.pane.borrow_mut();
            p.parent = parent.0 as isize;
            // SAFETY: checked for null above; prevhost's own rectangle.
            p.rect = unsafe { *rect };
            (p.window, p.rect)
        };
        if window != 0 {
            // SAFETY: the pane's own window moved under the new parent; no
            // borrow is held.
            let _ = unsafe { SetParent(hwnd(window), Some(parent)) };
            place(window, rect);
        }
        Ok(())
    }

    fn SetRect(&self, rect: *const RECT) -> Result<()> {
        if rect.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let (window, rect) = {
            let mut p = self.pane.borrow_mut();
            // SAFETY: checked for null above.
            p.rect = unsafe { *rect };
            (p.window, p.rect)
        };
        place(window, rect);
        Ok(())
    }

    fn DoPreview(&self) -> Result<()> {
        crate::guarded(|| {
            let (window, parent, rect, page) = {
                let mut p = self.pane.borrow_mut();
                if p.document.is_none() || p.parent == 0 {
                    return Err(E_FAIL.into());
                }
                p.taken = false;
                (p.window, p.parent, p.rect, p.wanted)
            };
            if window == 0 {
                register_class()?;
                let raw = Rc::into_raw(Rc::clone(&self.pane));
                // SAFETY: a child of prevhost's window, of the class this DLL
                // registered; the pane pointer is the window's to give back.
                let made = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        CLASS,
                        PCWSTR::null(),
                        WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        Some(hwnd(parent)),
                        None,
                        Some(crate::module()),
                        Some(raw.cast::<c_void>()),
                    )
                };
                match made {
                    Ok(made) => self.pane.borrow_mut().window = made.0 as isize,
                    Err(error) => {
                        if !self.pane.borrow().taken {
                            // SAFETY: the window never took it, so this is
                            // the only owner of the reference made above.
                            drop(unsafe { Rc::from_raw(raw) });
                        }
                        return Err(error);
                    }
                }
            }
            request(&self.pane, page);
            Ok(())
        })
    }

    fn Unload(&self) -> Result<()> {
        destroy(&self.pane);
        Ok(())
    }

    fn SetFocus(&self) -> Result<()> {
        let window = self.pane.borrow().window;
        if window != 0 {
            // SAFETY: the pane's own window, on its own thread.
            let _ = unsafe { SetFocus(Some(hwnd(window))) };
        }
        Ok(())
    }

    fn QueryFocus(&self) -> Result<HWND> {
        // SAFETY: no arguments.
        Ok(unsafe { GetFocus() })
    }

    fn TranslateAccelerator(&self, message: *const MSG) -> Result<()> {
        if message.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let frame = self
            .site
            .borrow()
            .as_ref()
            .and_then(|site| site.cast::<IPreviewHandlerFrame>().ok());
        let Some(frame) = frame else {
            return Err(S_FALSE.into());
        };
        // Called through the table rather than the wrapper, which reads
        // S_FALSE ("not mine") as success and would swallow every key.
        // SAFETY: a live interface and prevhost's own message.
        let result =
            unsafe { (Interface::vtable(&frame).TranslateAccelerator)(frame.as_raw(), message) };
        if result == S_OK {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }
}

impl IObjectWithSite_Impl for Preview_Impl {
    fn SetSite(&self, site: Ref<windows::core::IUnknown>) -> Result<()> {
        *self.site.borrow_mut() = site.cloned();
        Ok(())
    }

    fn GetSite(&self, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        if object.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: checked for null above.
        unsafe { *object = std::ptr::null_mut() };
        match self.site.borrow().as_ref() {
            // SAFETY: COM's own pointers, passed through.
            Some(site) => unsafe { site.query(iid, object) }.ok(),
            None => Err(E_FAIL.into()),
        }
    }
}

impl IOleWindow_Impl for Preview_Impl {
    fn GetWindow(&self) -> Result<HWND> {
        match self.pane.borrow().window {
            0 => Err(E_FAIL.into()),
            window => Ok(hwnd(window)),
        }
    }

    fn ContextSensitiveHelp(&self, _enter: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}
