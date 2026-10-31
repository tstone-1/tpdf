//! Giving a document to a worker that is already running --- the macOS half.
//!
//! Split out of `worker.rs` when that file had grown to 2,861 lines and four
//! concerns. Nothing changed in the move: `worker.rs` re-exports
//! `recv_document`, so `crate::worker::recv_document` still resolves, which is
//! the path `worker_child.rs` reaches it by.
//!
//! **The document arrives as a mapped descriptor, never a path.** That is what
//! makes the sandbox possible at all: a descriptor has no name to guess and
//! survives a policy that denies opening files.
//!
//! The Windows counterpart is not here, and that is placement rather than
//! omission --- it is a `DuplicateHandle` into the child's own table, which sits
//! beside the section it copies, in `worker_shm.rs`. `remap_fds` is here for a
//! different reason again: it is no part of the handover, but it guards the same
//! descriptor numbers between `fork` and `exec`, and the two are right together
//! or wrong together.

#[cfg(unix)]
use std::os::fd::{FromRawFd, OwnedFd};

/// Installs up to four inherited descriptors without allocating between fork and exec.
///
/// Scratch copies must be above every destination: copying all sources first
/// does not help if installing one destination overwrites a later scratch copy.
/// `dup2` clears close-on-exec on each destination; scratch copies are closed on
/// both success and failure. Only async-signal-safe descriptor calls run here.
///
/// **Then every other descriptor above standard error is marked close-on-exec**
/// ([`cloexec_the_rest`]), so the worker starts with its three standard streams
/// and the ones installed here and nothing else. Rust opens its own descriptors
/// close-on-exec, but the app process also holds descriptors it did not open:
/// GTK's, WebKit's, and on Linux every PKCS#11 module p11-kit loads for signing.
/// One opened without the flag would reach every later worker already open,
/// and neither Landlock, seccomp nor the macOS profile restricts a descriptor
/// that is already open. Marked rather than closed, because the standard
/// library's own pipe for reporting a failed `exec` is among them and has to
/// stay open until the `exec`.
///
/// # Safety
///
/// Call only in a forked child before exec, with owned source descriptors and
/// distinct nonnegative targets. The child must exit if any operation fails.
#[cfg(unix)]
pub(crate) unsafe fn remap_fds(mappings: &[(i32, i32)]) -> std::io::Result<()> {
    let mut copies = [-1; 4];
    if mappings.len() > copies.len() {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    let minimum = mappings
        .iter()
        .map(|&(_, target)| target)
        .max()
        .unwrap_or(2)
        .checked_add(1)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: the caller owns each source and no other child thread runs.
    let result = unsafe {
        (|| {
            for (copy, &(source, _)) in copies.iter_mut().zip(mappings) {
                *copy = libc::fcntl(source, libc::F_DUPFD_CLOEXEC, minimum);
                if *copy < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            for (&copy, &(_, target)) in copies.iter().zip(mappings) {
                if libc::dup2(copy, target) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        })()
    };
    for copy in copies {
        if copy >= 0 {
            // SAFETY: this is a temporary descriptor created above.
            unsafe { libc::close(copy) };
        }
    }
    result?;
    let mut keep = [-1; 4];
    for (slot, &(_, target)) in keep.iter_mut().zip(mappings) {
        *slot = target;
    }
    // SAFETY: as above; only descriptor flags change.
    unsafe { cloexec_the_rest(&keep[..mappings.len()], minimum - 1) };
    Ok(())
}

/// Marks close-on-exec every descriptor above standard error that is not in
/// `keep`, where `highest` is the largest number in `keep` (or 2).
///
/// Between `highest` and the top, Linux does it in one call
/// (`close_range` with `CLOSE_RANGE_CLOEXEC`, kernel 5.11 and up). Elsewhere,
/// and on an older kernel, each number up to the descriptor limit is marked in
/// turn, the limit capped at [`LOOP_CEILING`]: a descriptor the process could
/// hold above that is a process that raised its own limit past it, which tpdf
/// does not do. A number with nothing open answers `EBADF`, which is ignored.
///
/// # Safety
///
/// As [`remap_fds`]: a forked child before `exec`. Only `fcntl`, `getrlimit`
/// and, on Linux, `close_range` run here, none of which allocates.
#[cfg(unix)]
unsafe fn cloexec_the_rest(keep: &[i32], highest: i32) {
    // SAFETY: flag changes on numbers that may or may not be open.
    unsafe {
        for fd in 3..=highest {
            if !keep.contains(&fd) {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        #[cfg(target_os = "linux")]
        {
            const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
            let first = libc::c_uint::try_from(highest + 1).unwrap_or(libc::c_uint::MAX);
            if libc::syscall(
                libc::SYS_close_range,
                first,
                libc::c_uint::MAX,
                CLOSE_RANGE_CLOEXEC,
            ) == 0
            {
                return;
            }
        }
        let mut limit: libc::rlimit = std::mem::zeroed();
        let top = if libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) == 0 {
            limit.rlim_cur.min(LOOP_CEILING)
        } else {
            LOOP_CEILING
        };
        let top = i32::try_from(top).unwrap_or(i32::MAX);
        for fd in highest + 1..top {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
}

/// The most descriptor numbers [`cloexec_the_rest`] walks one at a time.
///
/// macOS starts an app with a limit of 256 and a terminal with a few thousand,
/// and Linux's soft limit is 1,024; this is above all of them and costs a few
/// milliseconds a worker where it is reached.
#[cfg(unix)]
const LOOP_CEILING: libc::rlim_t = 65_536;

/// A connected pair, one half of which is handed to a pre-spawned worker.
#[cfg(unix)]
pub(crate) fn socket_pair() -> Result<(OwnedFd, OwnedFd), String> {
    let mut fds = [0i32; 2];
    // SAFETY: writes exactly two descriptors into a two-element array.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(format!("socketpair: {}", std::io::Error::last_os_error()));
    }

    // Close-on-exec on **both** ends, and this is not hygiene --- without it a
    // pre-spawned worker never dies.
    //
    // A spare blocks in `recvmsg` on this socket, so unlike a document-serving
    // worker it is not reading stdin and cannot notice the parent going away that
    // way. What should end it is the socket reaching EOF when the parent's end
    // closes. But `socketpair` descriptors are not close-on-exec, so every child
    // spawned afterwards inherits a copy and holds the write end open --- and the
    // spare therefore waits forever, reparented to init, on a socket that will
    // never close because a sibling has it.
    //
    // The symptom is a pile of orphaned `--prespawn` processes that outlive every
    // run, which is what the process table showed: eighteen of them, some seconds
    // old. `Drop` does not help here, because `std::process::exit` runs no
    // destructors and every probe and the app itself exit that way.
    //
    // `dup2` clears the flag on the descriptor it creates, so the child still
    // receives a usable socket on `SOCK_FD`.
    for fd in fds {
        // SAFETY: both descriptors were just created by `socketpair`.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            let e = std::io::Error::last_os_error();
            // SAFETY: closing descriptors this function owns and is abandoning.
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(format!(
                "could not set FD_CLOEXEC on a handover socket: {e}"
            ));
        }
    }
    // SAFETY: both are fresh descriptors this process owns.
    unsafe { Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
}

/// Sends a document mapping's descriptor, with its length as the payload.
///
/// The length travels in the ordinary payload rather than in a second message
/// because a descriptor carries no notion of how much of it to map, and two
/// messages could be interleaved by a future caller in a way one cannot.
///
/// A byte of payload is required, not incidental: a `sendmsg` carrying only
/// ancillary data may transfer nothing at all, and the receiver then blocks
/// forever on a message that was never framed.
#[cfg(unix)]
pub(crate) fn send_document(socket: i32, fd: i32, len: usize) -> Result<(), String> {
    let mut payload = (len as u64).to_le_bytes();
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut space = [0u8; 32];
    // SAFETY: the control buffer is sized by CMSG_SPACE for one descriptor, and
    // every pointer is into storage that outlives the call.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr().cast();
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) as _;

        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err("no control header".into());
        }
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as _;
        std::ptr::copy_nonoverlapping(&raw const fd, libc::CMSG_DATA(cmsg).cast::<i32>(), 1);

        if libc::sendmsg(socket, &raw const msg, 0) < 0 {
            return Err(format!("sendmsg: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// Receives a document mapping's descriptor and its length, or `None` when the
/// socket closed first --- which is how a pre-spawned worker learns the parent
/// has gone away without ever giving it a file. That is an ending, not an
/// error: every spare a parent never used ends this way.
///
/// # Errors
///
/// A message that is not the one this protocol sends.
///
/// # Safety
///
/// The caller must own `socket` and must not be reading it concurrently.
#[cfg(unix)]
pub unsafe fn recv_document(socket: i32) -> Result<Option<(OwnedFd, usize)>, String> {
    let mut payload = [0u8; 8];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut space = [0u8; 32];
    // SAFETY: as `send_document`; the control header is only read once `recvmsg`
    // has reported success.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr().cast();
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) as _;

        let read = libc::recvmsg(socket, &raw mut msg, 0);
        if read < 0 {
            return Err(format!("recvmsg: {}", std::io::Error::last_os_error()));
        }
        if read == 0 {
            return Ok(None);
        }
        // Checked rather than assumed: a short read leaves the rest of `payload`
        // zeroed, and a length of zero is a mapping of nothing that would fail
        // much further along with a far worse message.
        if read as usize != payload.len() {
            return Err(format!("the handover payload was {read} bytes, wanted 8"));
        }
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err("no descriptor arrived with the handover".into());
        }
        if (*cmsg).cmsg_level != libc::SOL_SOCKET || (*cmsg).cmsg_type != libc::SCM_RIGHTS {
            return Err("the handover control message was not SCM_RIGHTS".into());
        }
        let mut fd: i32 = -1;
        std::ptr::copy_nonoverlapping(libc::CMSG_DATA(cmsg).cast::<i32>(), &raw mut fd, 1);
        if fd < 0 {
            return Err("the descriptor that arrived is not valid".into());
        }
        let len = usize::try_from(u64::from_le_bytes(payload))
            .map_err(|_| "the handover length does not fit in this address space".to_string())?;
        if len == 0 {
            return Err("the handover length is zero".into());
        }
        Ok(Some((OwnedFd::from_raw_fd(fd), len)))
    }
}

/// What a worker gets for stderr: the parent's, unless that is a terminal.
///
/// **A terminal is not a place for a worker to hold a descriptor to.** A document
/// that takes over a worker could `TIOCSTI` a command line into the reader's shell,
/// read what is typed at a PIN prompt, or write sequences that rewrite the screen, all
/// past the sandbox. So there it gets a pipe, and [`relay_stderr`] prints what comes
/// down it. Anywhere else it is inherited byte for byte, which is what the harnesses
/// that read a worker's stderr capture. The Windows path does not come through here.
#[cfg(unix)]
pub(crate) fn worker_stderr() -> std::process::Stdio {
    use std::io::IsTerminal as _;
    if std::io::stderr().is_terminal() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::inherit()
    }
}

/// Prints what a worker says down the pipe [`worker_stderr`] gave it, if it gave one,
/// until the worker ends. A sandbox denial and a dying worker's last words stay visible.
#[cfg(unix)]
pub(crate) fn relay_stderr(child: &mut std::process::Child) {
    if let Some(said) = child.stderr.take() {
        // A thread that cannot start leaves a closed pipe: the worker's writes fail
        // and nothing else about it changes.
        let _ = std::thread::Builder::new()
            .name("worker-stderr".into())
            .spawn(move || relay(said, &mut std::io::stderr()));
    }
}

/// Copies `from` to `to` with whatever a terminal acts on replaced, by
/// [`crate::cli::printable`], in pieces of a bounded size so that a worker writing
/// no line end holds no memory here.
#[cfg(unix)]
fn relay(mut from: impl std::io::Read, to: &mut impl std::io::Write) {
    let mut piece = [0u8; 8192];
    while let Ok(read) = from.read(&mut piece) {
        if read == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&piece[..read]);
        let _ = to.write_all(crate::cli::printable(&text).as_bytes());
        let _ = to.flush();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    /// Forces low descriptor holes in a forked child, away from the test runner's
    /// descriptor table. Distinct pipe bytes expose aliases; nonblocking reads
    /// fail immediately even if another process inherited a pipe's writer.
    fn check_layout(targets: &[i32], invalid_source: bool) {
        let sources: Vec<OwnedFd> = targets
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut pipe = [-1; 2];
                // SAFETY: writable two-element array, then one initialized byte.
                unsafe {
                    assert_eq!(libc::pipe(pipe.as_mut_ptr()), 0);
                    let read = OwnedFd::from_raw_fd(pipe[0]);
                    let write = OwnedFd::from_raw_fd(pipe[1]);
                    assert_eq!(
                        libc::fcntl(read.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK),
                        0
                    );
                    let byte = b'A' + index as u8;
                    assert_eq!(
                        libc::write(write.as_raw_fd(), (&raw const byte).cast(), 1),
                        1
                    );
                    let high = libc::fcntl(read.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 32);
                    assert!(high >= 32);
                    OwnedFd::from_raw_fd(high)
                }
            })
            .collect();
        // SAFETY: the child uses only descriptor syscalls, remap_fds and _exit;
        // it never returns to the multithreaded test runner or runs destructors.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            // SAFETY: all changes affect only this child. The source copies are
            // above 31, so clearing the low slots cannot close them.
            unsafe {
                for fd in 3..32 {
                    libc::close(fd);
                }
                let mut mappings = [(-1, -1); 4];
                for (index, (source, &target)) in sources.iter().zip(targets).enumerate() {
                    let low = 10 + index as i32;
                    if libc::dup2(source.as_raw_fd(), low) < 0 {
                        libc::_exit(80);
                    }
                    mappings[index] = (low, target);
                }
                if invalid_source {
                    mappings[targets.len() - 1].0 = -1;
                }
                let result = remap_fds(&mappings[..targets.len()]);
                if invalid_source {
                    if result.is_ok() {
                        libc::_exit(81);
                    }
                } else {
                    if result.is_err() {
                        libc::_exit(82);
                    }
                    for (index, &target) in targets.iter().enumerate() {
                        let mut byte = 0u8;
                        if libc::read(target, (&raw mut byte).cast(), 1) != 1
                            || byte != b'A' + index as u8
                        {
                            libc::_exit(90 + index as i32);
                        }
                        if libc::fcntl(target, libc::F_GETFD) & libc::FD_CLOEXEC != 0 {
                            libc::_exit(100);
                        }
                    }
                }
                for fd in 3..32 {
                    if !(10..10 + targets.len() as i32).contains(&fd)
                        && (invalid_source || !targets.contains(&fd))
                        && libc::fcntl(fd, libc::F_GETFD) != -1
                    {
                        libc::_exit(101);
                    }
                }
                libc::_exit(0);
            }
        }
        let mut status = 0;
        loop {
            // SAFETY: this process owns the child and status is writable.
            let waited = unsafe { libc::waitpid(pid, &raw mut status, 0) };
            if waited == pid {
                break;
            }
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR)
            );
        }
        assert_eq!(
            status, 0,
            "descriptor layout {targets:?}: child status {status}"
        );
    }

    #[test]
    fn low_holes_cannot_alias_the_prespawn_socket() {
        check_layout(&[crate::worker::TILE_FD, crate::worker::SOCK_FD], false);
    }

    #[test]
    fn low_holes_preserve_all_document_output_and_input_mappings() {
        use crate::worker::{DOC_FD, IN_FD, OUT_FD, TILE_FD};
        let targets = [DOC_FD, TILE_FD, OUT_FD, IN_FD];
        for count in 2..=4 {
            check_layout(&targets[..count], false);
        }
    }

    #[test]
    fn an_invalid_source_closes_every_temporary_without_installing_anything() {
        check_layout(&[crate::worker::TILE_FD, crate::worker::SOCK_FD], true);
    }

    /// Whether a program started with `pre_exec` (or without, for the control)
    /// finds descriptor `fd` open, asked through `/dev/fd`, which on both macOS
    /// and Linux lists what the asking process has open.
    fn child_sees(fd: i32, shuffle: bool) -> bool {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg(format!("[ -e /dev/fd/{fd} ]"));
        if shuffle {
            // SAFETY: remap_fds runs only async-signal-safe descriptor calls.
            unsafe { command.pre_exec(|| remap_fds(&[])) };
        }
        command.status().expect("sh runs").success()
    }

    /// A descriptor the app holds without close-on-exec does not reach a
    /// worker. The control is the same program started without the shuffle,
    /// which finds it open: a check that could not see an inherited descriptor
    /// would pass either way.
    ///
    /// The descriptor is taken at the lowest free number from 200 up rather
    /// than placed on a fixed one, because the test runner's other threads own
    /// the numbers below and a `dup2` onto one of them would close it.
    #[test]
    fn a_descriptor_opened_without_close_on_exec_does_not_reach_the_child() {
        let mut pipe = [-1; 2];
        // SAFETY: writes two descriptors into a two-element array.
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        // SAFETY: both were just made by `pipe`; `F_DUPFD` leaves the copy
        // without close-on-exec, which is the case under test.
        let (high, ends) = unsafe {
            let high = libc::fcntl(pipe[0], libc::F_DUPFD, 200);
            let ends = [OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])];
            (high, ends)
        };
        assert!(high >= 200, "no descriptor from 200 up");
        // SAFETY: the copy made above, owned from here.
        let high = unsafe { OwnedFd::from_raw_fd(high) };
        // SAFETY: a flag query on a descriptor this test owns.
        let flags = unsafe { libc::fcntl(high.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(
            flags & libc::FD_CLOEXEC,
            0,
            "the copy is close-on-exec already"
        );
        assert!(
            child_sees(high.as_raw_fd(), false),
            "the control cannot see it"
        );
        assert!(
            !child_sees(high.as_raw_fd(), true),
            "a worker would inherit descriptor {}",
            high.as_raw_fd()
        );
        drop(ends);
    }

    /// A worker's words reach the reader, and what a terminal would act on does not.
    #[test]
    fn what_a_worker_says_is_relayed_with_nothing_a_terminal_acts_on() {
        let mut out = Vec::new();
        super::relay(
            &b"[worker] denied\n\x1b]52;c;x\x07\x9b2J\tend\n"[..],
            &mut out,
        );
        let out = String::from_utf8(out).expect("UTF-8");
        assert!(out.starts_with("[worker] denied\n"), "{out:?}");
        assert!(out.ends_with("\tend\n"), "{out:?}");
        assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
    }
}
