//! How a Linux process asks the reader for a token's PIN (`pkcs11.rs`).
//!
//! macOS and Windows show their own prompt and tpdf never sees what is typed.
//! Linux has no such prompt, so tpdf shows one, and keeps the same two
//! properties it can keep: **the PIN never reaches the webview**, because the
//! application's prompt is a native GTK dialog drawn by this process; and the
//! command-line tool reads it from the terminal with echo off, never from an
//! argument, which other users can read in the process list.

use cryptoki::types::AuthPin;

/// The words of both prompts.
fn question(token: &str) -> String {
    format!("Enter the PIN of the smart card or token \u{201c}{token}\u{201d} to sign.")
}

/// Installs the application's prompt: a modal GTK dialog over the main window.
pub fn install_dialog(app: tauri::AppHandle) {
    crate::pkcs11::set_pin_asker(Box::new(move |token| dialog(&app, token)));
}

fn dialog(app: &tauri::AppHandle, token: &str) -> Result<Option<AuthPin>, String> {
    let question = question(token);
    let app_for_dialog = app.clone();
    let ask = move || -> Option<String> {
        use gtk::prelude::*;
        use tauri::Manager as _;

        let parent = app_for_dialog
            .webview_windows()
            .into_values()
            .next()
            .and_then(|window| window.gtk_window().ok());
        let dialog = gtk::Dialog::with_buttons(
            Some("Sign with a smart card"),
            parent.as_ref(),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Sign", gtk::ResponseType::Accept),
            ],
        );
        dialog.set_default_response(gtk::ResponseType::Accept);
        let area = dialog.content_area();
        area.set_spacing(12);
        area.set_margin_top(12);
        area.set_margin_bottom(12);
        area.set_margin_start(12);
        area.set_margin_end(12);
        let label = gtk::Label::new(Some(&question));
        label.set_line_wrap(true);
        let entry = gtk::Entry::new();
        entry.set_visibility(false);
        entry.set_input_purpose(gtk::InputPurpose::Pin);
        entry.set_activates_default(true);
        area.add(&label);
        area.add(&entry);
        dialog.show_all();
        let answer = (dialog.run() == gtk::ResponseType::Accept).then(|| entry.text().to_string());
        entry.set_text("");
        // SAFETY: the dialog is ours and not used after this.
        unsafe { dialog.destroy() };
        answer
    };

    // `run` spins a nested main loop, so it must be on GTK's thread. Signing
    // runs on Tauri's blocking pool, so this normally hands it over and waits.
    let answer = if std::thread::current().name() == Some("main") {
        ask()
    } else {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(ask());
        })
        .map_err(|e| format!("the PIN dialog could not be shown: {e}"))?;
        rx.recv()
            .map_err(|_| "the PIN dialog closed without an answer".to_string())?
    };
    Ok(answer.map(|pin| AuthPin::new(pin.into())))
}

/// Installs the command-line tool's prompt, on the controlling terminal.
pub fn install_terminal() {
    crate::pkcs11::set_pin_asker(Box::new(|token| terminal(token).map(Some)));
}

/// Reads one line from `/dev/tty` with echo off.
///
/// `/dev/tty` rather than standard input, which may be a document or a pipe;
/// a run with no terminal is refused rather than reading a PIN from somewhere
/// the reader did not type it.
fn terminal(token: &str) -> Result<AuthPin, String> {
    use std::io::{BufRead, Write};
    use std::os::fd::AsRawFd;

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| {
            format!("the token {token:?} needs its PIN, and there is no terminal to ask on ({e})")
        })?;
    let fd = tty.as_raw_fd();
    // SAFETY: `termios` is plain data, filled by `tcgetattr` before it is read.
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid pointer.
    if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
        return Err(format!(
            "reading the terminal's settings: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut quiet = saved;
    quiet.c_lflag &= !libc::ECHO;
    quiet.c_lflag |= libc::ICANON;
    // SAFETY: as above.
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) } != 0 {
        return Err(format!(
            "turning off the terminal's echo: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut writer = &tty;
    let _ = write!(writer, "{} PIN: ", question(token));
    let _ = writer.flush();
    let mut line = String::new();
    let read = std::io::BufReader::new(&tty).read_line(&mut line);
    // SAFETY: as above; restores what was read before any change.
    unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &saved) };
    let _ = writeln!(writer);
    read.map_err(|e| format!("reading the PIN: {e}"))?;
    let pin = line.trim_end_matches(['\r', '\n']).to_string();
    line.clear();
    Ok(AuthPin::new(pin.into()))
}
