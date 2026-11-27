//! Restore the main window in AppKit points, including its frame before zoom.

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSWindow, NSWindowStyleMask, NSWindowWillCloseNotification,
    NSWindowWillEnterFullScreenNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    ptr::NonNull,
};

#[derive(Deserialize, Serialize)]
struct Placement {
    frame: String,
    maximized: bool,
}

impl Placement {
    fn save(&self, path: &Path) -> io::Result<()> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(file.as_file(), self)?;
        file.persist(path)?;
        Ok(())
    }
}

pub fn install(webview: &tauri::WebviewWindow, path: PathBuf) {
    let (Some(_main), Ok(raw)) = (MainThreadMarker::new(), webview.ns_window()) else {
        return;
    };
    // SAFETY: Tauri owns this NSWindow and setup is on its main thread.
    let window = unsafe { &*raw.cast::<NSWindow>() };
    let saved: Option<Placement> = fs::File::open(&path)
        .ok()
        .and_then(|f| serde_json::from_reader(f).ok());
    if let Some(saved) = &saved {
        window.setFrameFromString(&NSString::from_str(&saved.frame));
        if window.screen().is_none() {
            window.center();
        }
        if saved.maximized && !window.isZoomed() {
            window.zoom(None);
        }
    }

    // The full-screen frame belongs to its Space. Capture the windowed frame
    // before the transition so quitting in that Space restores a usable window.
    for closing in [false, true] {
        let path = path.clone();
        let handler = RcBlock::new(move |note: NonNull<NSNotification>| {
            // SAFETY: both notifications are scoped to this NSWindow on the
            // posting (AppKit main) thread, and the notification lives through the callback.
            let Some(window) = (unsafe { note.as_ref() })
                .object()
                .and_then(|obj| obj.downcast::<NSWindow>().ok())
            else {
                return;
            };
            if closing && window.styleMask().contains(NSWindowStyleMask::FullScreen) {
                return;
            }
            let maximized = window.isZoomed();
            if maximized {
                // AppKit retains the exact pre-zoom frame across an animated zoom.
                if closing {
                    window.orderOut(None);
                }
                window.zoom(None);
            }
            let frame = window.stringWithSavedFrame().to_string();
            if maximized && !closing {
                window.zoom(None);
            }
            if let Err(error) = (Placement { frame, maximized }).save(&path) {
                crate::diag::note(&format!("saving window position: {error}"));
            }
        });
        // SAFETY: the center retains the block for this main window's lifetime.
        unsafe {
            let name = if closing {
                NSWindowWillCloseNotification
            } else {
                NSWindowWillEnterFullScreenNotification
            };
            let _token = NSNotificationCenter::defaultCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(name),
                    Some(window),
                    None,
                    &handler,
                );
        }
    }
}
