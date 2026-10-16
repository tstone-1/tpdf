//! A second launch on macOS that asks for a place hands its command line to
//! the window already running.
//!
//! On Windows and Linux every second launch goes to the running window through
//! `tauri-plugin-single-instance`. On macOS a double-click does not need it,
//! because Launch Services sends the running app an Apple Event; but running
//! the executable from a terminal or an editor (`tpdf file.pdf --page 12`) is
//! a second process with its own window. An editor that moves tpdf to a page
//! on every build would open a new window each time.
//!
//! So a launch that carries `--page` or `--zoom` (`launch::Place`) first tries
//! to hand its command line to a running window, and starts normally when none
//! answers. **Only such a launch**: a plain one starts a second process as it
//! always has. On Windows the plugin makes a stray process silently take every
//! later launch, which `docs/TRAPS.md` records costing a harness its meaning
//! (*Single-instance turns a stray process into a launch that succeeds and does
//! nothing*), and no harness launch carries a place.
//!
//! The socket is in the user's own temporary folder (`_CS_DARWIN_USER_TEMP_DIR`,
//! which only that user can enter), with permissions for the owner only, and
//! its name is a hash of the executable's path. So the installed app and a
//! build under test never hand over to each other, and another user's process
//! cannot reach either. The message is the plugin's format, the working folder
//! and the arguments, read by the same `launch::openings_from_second_launch`.
//! It is held to 64 KiB and two seconds, and the running window answers `ok`
//! once it has queued the document; without that answer the second launch
//! starts on its own rather than lose the document.
//!
//! A socket left behind by a window that crashed is replaced by the next window
//! that listens: a connection to it is refused, which is how it is told apart
//! from one a running window holds.

use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest message read. A command line of a few paths is far below it.
const MAX_MESSAGE: u64 = 64 * 1024;

/// How long either side waits for the other.
const WAIT: Duration = Duration::from_secs(2);

/// What the running window answers once it has the command line.
const ACK: &[u8; 2] = b"ok";

/// The user's own temporary folder, as the system names it; `None` if it will
/// not say. Not `TMPDIR`, which whoever started tpdf could point at `/tmp`.
pub fn user_temp_dir() -> Option<PathBuf> {
    let mut buffer = vec![0u8; 1024];
    // SAFETY: `confstr` writes at most `buffer.len()` bytes including the NUL
    // and returns the length it needed, which is checked against the buffer.
    let needed = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if needed == 0 || needed > buffer.len() {
        return None;
    }
    buffer.truncate(needed - 1);
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buffer)))
}

/// The socket for the executable at `exe`, in `folder`.
pub fn socket_path(folder: &Path, exe: &Path) -> PathBuf {
    folder.join(format!(
        "tpdf-launch-{:016x}.sock",
        fnv1a(exe.as_os_str().as_bytes())
    ))
}

/// FNV-1a, 64 bits. Spelled out because the name has to be the same in two
/// processes and across builds, which `std`'s hasher does not promise.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The working folder, two NULs, then the arguments joined by NULs.
pub fn encode(cwd: &str, args: &[String]) -> Vec<u8> {
    let mut out = cwd.as_bytes().to_vec();
    out.extend_from_slice(b"\0\0");
    out.extend_from_slice(args.join("\0").as_bytes());
    out
}

/// The working folder and arguments, or `None` for anything that is not a
/// message: not UTF-8, or no separator.
pub fn decode(bytes: &[u8]) -> Option<(String, Vec<String>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (cwd, args) = text.split_once("\0\0")?;
    Some((
        cwd.to_string(),
        args.split('\0').map(str::to_string).collect(),
    ))
}

/// Hands a command line to the window listening on `socket`. True only when
/// that window answered that it has it.
pub fn hand_over(socket: &Path, cwd: &str, args: &[String]) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(WAIT));
    let _ = stream.set_read_timeout(Some(WAIT));
    if stream.write_all(&encode(cwd, args)).is_err() {
        return false;
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut answer = [0u8; 2];
    stream.read_exact(&mut answer).is_ok() && &answer == ACK
}

/// Listens on `socket` on a thread of its own, calling `receive` with each
/// command line and its working folder. Does nothing when another window is
/// already listening there, or the socket cannot be made.
pub fn listen(socket: PathBuf, receive: impl Fn(Vec<String>, String) + Send + 'static) {
    std::thread::spawn(move || {
        let Some(listener) = bind(&socket) else {
            return;
        };
        for stream in listener.incoming().flatten() {
            serve(stream, &receive);
        }
    });
}

/// The listening socket, replacing one left by a window that is gone.
fn bind(socket: &Path) -> Option<UnixListener> {
    let listener = match UnixListener::bind(socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(socket).is_ok() {
                return None;
            }
            let _ = std::fs::remove_file(socket);
            UnixListener::bind(socket).ok()?
        }
        Err(_) => return None,
    };
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).ok()?;
    Some(listener)
}

/// Reads one message, hands it on, and answers.
fn serve(mut stream: UnixStream, receive: &impl Fn(Vec<String>, String)) {
    let _ = stream.set_read_timeout(Some(WAIT));
    let _ = stream.set_write_timeout(Some(WAIT));
    let mut bytes = Vec::new();
    if (&mut stream)
        .take(MAX_MESSAGE + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return;
    }
    if bytes.len() as u64 > MAX_MESSAGE {
        return;
    }
    let Some((cwd, args)) = decode(&bytes) else {
        return;
    };
    receive(args, cwd);
    let _ = stream.write_all(ACK);
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, hand_over, listen, socket_path, user_temp_dir, MAX_MESSAGE};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::Duration;

    /// A socket path of its own per test, short enough for `sun_path`.
    fn socket(name: &str) -> PathBuf {
        let path = user_temp_dir()
            .expect("temp dir")
            .join(format!("tpdf-ho-{}-{name}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_folder_is_the_users_own() {
        let folder = user_temp_dir().expect("temp dir");
        assert!(folder.is_dir());
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(&folder).expect("metadata");
        assert_eq!(meta.permissions().mode() & 0o077, 0, "{folder:?}");
        // SAFETY: `getuid` has no preconditions.
        assert_eq!(meta.uid(), unsafe { libc::getuid() });
    }

    #[test]
    fn two_executables_have_two_sockets_and_one_has_the_same_one_each_time() {
        let folder = Path::new("/x");
        let installed = socket_path(
            folder,
            Path::new("/Applications/tpdf.app/Contents/MacOS/tpdf"),
        );
        let built = socket_path(folder, Path::new("/src/tpdf/target/debug/tpdf"));
        assert_ne!(installed, built);
        assert_eq!(
            installed,
            socket_path(
                folder,
                Path::new("/Applications/tpdf.app/Contents/MacOS/tpdf")
            )
        );
        // The FNV-1a offset basis for no bytes: the name does not depend on a
        // hasher that could change between builds.
        assert_eq!(
            socket_path(folder, Path::new("")),
            folder.join("tpdf-launch-cbf29ce484222325.sock")
        );
    }

    #[test]
    fn a_message_reads_back_as_it_was_written() {
        let written = args(&["tpdf", "my report.pdf", "--page", "3"]);
        assert_eq!(
            decode(&encode("/work", &written)),
            Some(("/work".to_string(), written))
        );
        assert_eq!(decode(b"no separator"), None);
        assert_eq!(decode(&[0xff, 0, 0, b'a']), None);
    }

    #[test]
    fn a_command_line_reaches_the_running_window_and_is_answered() {
        let path = socket("reach");
        let (sent, got) = mpsc::channel();
        listen(path.clone(), move |args, cwd| {
            let _ = sent.send((args, cwd));
        });
        // The listener binds on its own thread.
        for _ in 0..200 {
            if path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let line = args(&["tpdf", "a.pdf", "--page", "2"]);
        assert!(hand_over(&path, "/work", &line));
        assert_eq!(
            got.recv_timeout(Duration::from_secs(2)).expect("received"),
            (line, "/work".into())
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("socket")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn with_no_window_listening_the_launch_starts_on_its_own() {
        let path = socket("none");
        assert!(!hand_over(&path, "/work", &args(&["tpdf", "a.pdf"])));
        // A socket left by a window that is gone: refused, so not handed over.
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("bind");
        drop(stale);
        assert!(!hand_over(&path, "/work", &args(&["tpdf", "a.pdf"])));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_stale_socket_is_replaced_and_a_live_one_is_left_alone() {
        let path = socket("stale");
        drop(std::os::unix::net::UnixListener::bind(&path).expect("bind"));
        let (sent, got) = mpsc::channel();
        listen(path.clone(), move |args, _| {
            let _ = sent.send(args);
        });
        let line = args(&["tpdf", "b.pdf", "--zoom", "150"]);
        let mut answered = false;
        for _ in 0..200 {
            if hand_over(&path, "/", &line) {
                answered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(answered, "the stale socket was not replaced");
        assert_eq!(
            got.recv_timeout(Duration::from_secs(2)).expect("first"),
            line
        );
        // A second window finds the first one listening and stays out of it.
        let (second, never) = mpsc::channel::<Vec<String>>();
        listen(path.clone(), move |args, _| {
            let _ = second.send(args);
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(hand_over(&path, "/", &line));
        assert_eq!(
            got.recv_timeout(Duration::from_secs(2))
                .expect("still the first"),
            line
        );
        assert!(never.try_recv().is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_message_past_the_limit_is_not_answered() {
        let path = socket("large");
        let (sent, got) = mpsc::channel();
        listen(path.clone(), move |args, _| {
            let _ = sent.send(args);
        });
        for _ in 0..200 {
            if path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let long = "a".repeat(MAX_MESSAGE as usize);
        assert!(!hand_over(&path, "/work", &args(&["tpdf", &long])));
        assert!(got.recv_timeout(Duration::from_millis(200)).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
