//! How a Linux process asks the reader for a token's PIN (`pkcs11.rs`) or a
//! certificate file's password (`pkcs12.rs`).
//!
//! macOS and Windows show their own prompt and tpdf never sees what is typed.
//! Linux has no such prompt, so tpdf shows one, and keeps the same two
//! properties it can keep: **the PIN never reaches the webview**, because the
//! application's prompt is a native GTK dialog drawn by this process; and the
//! command-line tool reads it from the terminal with echo off, never from an
//! argument, which other users can read in the process list.

use cryptoki::types::AuthPin;
use zeroize::Zeroizing;

use crate::pkcs11::Asked;

/// The sentence of both prompts.
fn question(asked: Asked<'_>) -> String {
    match asked {
        Asked::Pin(token) => {
            format!("Enter the PIN of the smart card or token \u{201c}{token}\u{201d} to sign.")
        }
        Asked::Password(file) => {
            format!("Enter the password of the certificate file \u{201c}{file}\u{201d}.")
        }
    }
}

/// The prompt as the terminal gets it. A token's label and a file's name are not
/// tpdf's words: a terminal would act on an escape sequence in them, and its
/// answer to one would be read as the PIN being typed.
fn terminal_prompt(asked: Asked<'_>) -> String {
    let prompt = format!("{} {}: ", question(asked), names(asked).1);
    crate::cli::printable(&prompt).into_owned()
}

/// The dialog's title and the terminal prompt's label.
fn names(asked: Asked<'_>) -> (&'static str, &'static str) {
    match asked {
        Asked::Pin(_) => ("Sign with a smart card", "PIN"),
        Asked::Password(_) => ("Certificate file", "Password"),
    }
}

/// Installs the application's prompt: a modal GTK dialog over the main window.
pub fn install_dialog(app: tauri::AppHandle) {
    crate::pkcs11::set_pin_asker(Box::new(move |asked| dialog(&app, asked)));
}

fn dialog(app: &tauri::AppHandle, asked: Asked<'_>) -> Result<Option<AuthPin>, String> {
    let question = question(asked);
    let (title, _) = names(asked);
    let app_for_dialog = app.clone();
    let ask = move || -> Option<Zeroizing<String>> {
        use gtk::prelude::*;
        use tauri::Manager as _;

        let parent = app_for_dialog
            .webview_windows()
            .into_values()
            .next()
            .and_then(|window| window.gtk_window().ok());
        let dialog = gtk::Dialog::with_buttons(
            Some(title),
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
        // Zeroed when dropped. GTK's own copies, the entry's buffer and the
        // `GString` `text` returns, are GTK's to free and are not zeroed.
        let answer = (dialog.run() == gtk::ResponseType::Accept)
            .then(|| Zeroizing::new(entry.text().to_string()));
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
        .map_err(|e| said!("the PIN dialog could not be shown: {e}"))?;
        rx.recv()
            .map_err(|_| said!("the PIN dialog closed without an answer"))?
    };
    Ok(answer.map(|pin| secret(&pin)))
}

/// The PIN as `cryptoki` holds it, copied out of a buffer that is zeroed when
/// the caller drops it.
///
/// `Box::from(&str)` makes a new allocation of exactly the PIN's length. A
/// `String` turned `into()` a `Box<str>` instead may be reallocated to shrink
/// it, which leaves the old bytes behind unzeroed.
fn secret(pin: &str) -> AuthPin {
    AuthPin::new(Box::from(pin))
}

/// Installs the command-line tool's prompt, on the controlling terminal.
pub fn install_terminal() {
    crate::pkcs11::set_pin_asker(Box::new(terminal));
}

/// The longest PIN or password the terminal prompt takes, in bytes.
const MOST: usize = 255;

/// Reads a PIN from `/dev/tty`, one key at a time with echo off, or `None`
/// when the reader presses Ctrl+C or Ctrl+D.
///
/// `/dev/tty` rather than standard input, which may be a document or a pipe;
/// a run with no terminal is refused rather than reading a PIN from somewhere
/// the reader did not type it.
///
/// **Ctrl+C is read as a key, not taken as a signal.** With the terminal's
/// signals on, Ctrl+C ended the process before the settings were put back,
/// and the reader's shell was left with echo off. So signals are turned off
/// with echo, and the key cancels the prompt like the dialog's Cancel. Line
/// editing is off too, so the prompt sees each key and does its own backspace.
/// A signal from elsewhere (`kill`, a closed terminal) can still end the
/// process with the settings changed; `stty sane` restores them.
fn terminal(asked: Asked<'_>) -> Result<Option<AuthPin>, String> {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("{} There is no terminal to ask on ({e}).", question(asked)))?;
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
    quiet.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG);
    quiet.c_cc[libc::VMIN] = 1;
    quiet.c_cc[libc::VTIME] = 0;
    // SAFETY: as above.
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) } != 0 {
        return Err(format!(
            "turning off the terminal's echo: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut writer = &tty;
    let _ = write!(writer, "{}", terminal_prompt(asked));
    let _ = writer.flush();
    let typed = read_keys(&tty);
    // SAFETY: as above; restores what was read before any change.
    unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &saved) };
    let _ = writeln!(writer);
    let Some(typed) = typed? else { return Ok(None) };
    Ok(Some(secret(&typed)))
}

/// Keys read one at a time until Enter, edited with backspace; `None` for
/// Ctrl+C, or Ctrl+D on an empty line.
///
/// Into a fixed buffer that is zeroed on drop, rather than a `String` that
/// reallocates as it grows and leaves each earlier copy behind.
fn read_keys(mut source: impl std::io::Read) -> Result<Option<Zeroizing<String>>, String> {
    let mut held = Zeroizing::new([0u8; MOST]);
    let mut len = 0;
    loop {
        let mut key = [0u8; 1];
        match source.read(&mut key) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("reading the PIN: {e}")),
        }
        match key[0] {
            b'\r' | b'\n' => break,
            0x03 => return Ok(None),
            0x04 if len == 0 => return Ok(None),
            0x7f | 0x08 => {
                // A whole character: its continuation bytes, then its first.
                while len > 0 && held[len - 1] & 0xc0 == 0x80 {
                    len -= 1;
                }
                len = len.saturating_sub(1);
            }
            byte => {
                if len == MOST {
                    return Err(format!("a PIN or password is at most {MOST} characters"));
                }
                held[len] = byte;
                len += 1;
            }
        }
    }
    let text = std::str::from_utf8(&held[..len])
        .map_err(|_| "the PIN was not text the terminal could pass on".to_string())?;
    Ok(Some(Zeroizing::new(text.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(bytes: &[u8]) -> Result<Option<String>, String> {
        read_keys(bytes).map(|typed| typed.map(|t| t.to_string()))
    }

    #[test]
    fn a_label_or_file_name_cannot_send_the_terminal_anything() {
        for asked in [
            Asked::Pin("card\u{1b}[6n\u{9b}2J"),
            Asked::Password("a\u{1b}]0;x\u{7}.p12"),
        ] {
            let prompt = terminal_prompt(asked);
            assert!(!prompt.chars().any(char::is_control), "{prompt:?}");
            assert!(prompt.ends_with(": "), "{prompt:?}");
        }
    }

    #[test]
    fn the_terminal_prompt_ends_at_enter_and_edits_with_backspace() {
        assert_eq!(keys(b"1234\r"), Ok(Some("1234".into())));
        assert_eq!(keys(b"12x\x7f34\n"), Ok(Some("1234".into())));
        assert_eq!(keys(b"\x7f\x7f12\x08\r"), Ok(Some("1".into())));
        assert_eq!(keys(b"p\xc3\xa4ss\r"), Ok(Some("p\u{e4}ss".into())));
        assert_eq!(keys(b"p\xc3\xa4\x7f\r"), Ok(Some("p".into())));
    }

    #[test]
    fn ctrl_c_cancels_the_terminal_prompt() {
        assert_eq!(keys(b"12\x03"), Ok(None));
        assert_eq!(keys(b"\x04"), Ok(None));
        assert_eq!(keys(b"1\x04\r"), Ok(Some("1\u{4}".into())));
    }

    #[test]
    fn the_terminal_prompt_refuses_more_than_it_holds() {
        let long = [b'7'; MOST + 1];
        assert!(keys(&long).is_err());
        let mut full = vec![b'7'; MOST];
        full.push(b'\r');
        assert_eq!(keys(&full).map(|t| t.map(|t| t.len())), Ok(Some(MOST)));
    }
}
