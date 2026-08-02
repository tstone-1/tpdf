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
//! beside the section it copies, in `worker_shm.rs`. `is_scratch` is here for a
//! different reason again: it is no part of the handover, but it guards the same
//! descriptor numbers between `fork` and `exec`, and the two are right together
//! or wrong together.

#[cfg(target_os = "macos")]
use std::os::fd::{FromRawFd, OwnedFd};

/// Whether a temporary descriptor from the pre-`exec` shuffle is only that.
///
/// Between `fork` and `exec` each mapping is `dup`'d to a scratch number and
/// then `dup2`'d onto the number the child expects, because the source may
/// already *be* one of those numbers. The scratch copy is closed afterwards ---
/// except when it is not a scratch copy at all.
///
/// `dup` returns the **lowest free** descriptor, and the trap is that "lowest
/// free" can be a number the shuffle is about to install on. With the document
/// mapping on fd 3, the tile mapping on fd 5 and a hole at fd 4, `dup(3)`
/// returns **4**, which is [`crate::worker::TILE_FD`]: the tile's own `dup2` then
/// installs the
/// tile there, correctly, and closing the document's "temporary" afterwards
/// closes the tile the child is about to be handed. The child starts with a
/// descriptor that names nothing, on a number the protocol says is a 16 MB
/// mapping, and every later diagnosis points at the mapping rather than at the
/// fork.
///
/// So a temporary is compared against **every** number the shuffle installs, not
/// only against its own target --- and the list it is compared against is the
/// same array that drives the `dup2` calls, so there is no second copy of the
/// target set to fall out of step with them. A temporary that equals a target
/// *is* the installed descriptor by then; there is nothing left to close, and
/// nothing leaks by keeping it.
#[cfg(target_os = "macos")]
pub(crate) fn is_scratch(fd: i32, shuffle: &[(i32, i32)]) -> bool {
    !shuffle.iter().any(|(_, target)| *target == fd)
}

/// A connected pair, one half of which is handed to a pre-spawned worker.
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32);

        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err("no control header".into());
        }
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32);
        std::ptr::copy_nonoverlapping(&raw const fd, libc::CMSG_DATA(cmsg).cast::<i32>(), 1);

        if libc::sendmsg(socket, &raw const msg, 0) < 0 {
            return Err(format!("sendmsg: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// Receives a document mapping's descriptor and its length.
///
/// # Errors
///
/// The socket closing --- which is how a pre-spawned worker learns the parent has
/// gone away without ever giving it a file --- or a message that is not the one
/// this protocol sends.
///
/// # Safety
///
/// The caller must own `socket` and must not be reading it concurrently.
#[cfg(target_os = "macos")]
pub unsafe fn recv_document(socket: i32) -> Result<(OwnedFd, usize), String> {
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
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32);

        let read = libc::recvmsg(socket, &raw mut msg, 0);
        if read < 0 {
            return Err(format!("recvmsg: {}", std::io::Error::last_os_error()));
        }
        if read == 0 {
            return Err("the parent closed the handover socket".into());
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
        Ok((OwnedFd::from_raw_fd(fd), len))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use crate::worker::{DOC_FD, SOCK_FD, TILE_FD};

    /// The layout that provokes it, and it is an ordinary one: the parent's own
    /// mapping files land low, so a hole below the tile's descriptor is exactly
    /// what a process that has opened and closed a file has. With the document
    /// on fd 3, the tile on fd 5 and fd 4 free, `dup` of the document returns 4
    /// --- which is `TILE_FD`, and by the time the cleanup runs it holds the
    /// tile.
    ///
    /// The failure this pins is silent on the parent's side: the child comes up
    /// with a closed descriptor where its tile mapping should be, and says so
    /// as a mapping error rather than as a fork one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_temporary_that_landed_on_another_installed_number_is_not_closed() {
        let shuffle = [(4, DOC_FD), (6, TILE_FD)];
        assert!(
            !super::is_scratch(4, &shuffle),
            "fd 4 is TILE_FD and holds the tile mapping by now"
        );
        // And the one that really is a temporary still goes, or the check has
        // been satisfied by refusing to close anything at all.
        assert!(super::is_scratch(6, &shuffle));
    }

    /// The control: the common layout, where both temporaries land above every
    /// number the shuffle installs on and both must be closed.
    #[cfg(target_os = "macos")]
    #[test]
    fn temporaries_above_every_installed_number_are_all_closed() {
        let shuffle = [(7, DOC_FD), (8, TILE_FD)];
        assert!(super::is_scratch(7, &shuffle));
        assert!(super::is_scratch(8, &shuffle));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_temporary_that_is_already_its_own_target_is_not_closed() {
        // `dup2(n, n)` is a no-op that returns `n`, so the "temporary" and the
        // installed descriptor are the same open file --- closing it would take
        // the mapping with it.
        let shuffle = [(DOC_FD, DOC_FD), (9, TILE_FD)];
        assert!(!super::is_scratch(DOC_FD, &shuffle));
        assert!(super::is_scratch(9, &shuffle));
    }

    /// The pre-spawn shuffle installs on different numbers, and the same trap
    /// reaches it: a tile temporary landing on `SOCK_FD` would close the
    /// handover socket, and a spare that never receives a document is not an
    /// error --- it is a process waiting in `recvmsg` for the rest of its life.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_prespawn_shuffle_protects_the_handover_socket_too() {
        let shuffle = [(SOCK_FD, TILE_FD), (7, SOCK_FD)];
        assert!(!super::is_scratch(SOCK_FD, &shuffle));
        assert!(super::is_scratch(7, &shuffle));
    }
}
