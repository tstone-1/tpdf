//! Can a worker be given its document *after* it has been sandboxed?
//!
//! Today a worker is tied to one document at `exec`: the parent `dup2`s the
//! mapping's descriptor to a fixed number and the child adopts it. That is why a
//! worker cannot be started before the file is chosen, and therefore why the
//! ~6.6 ms floor measured by `prespawn-bench` sits on the critical path to the
//! first page instead of overlapping the ~250 ms shell start.
//!
//! A pre-spawned worker needs the descriptor to arrive later, over a socket, as
//! `SCM_RIGHTS` ancillary data. Whether that is *allowed* is the question this
//! probe exists to answer, and it is not one to reason about: `AGENTS.md` records
//! a sandbox that rendered `ok` with a substituted font, and the standing rule
//! that a sandbox is verified by comparing output, never by checking that a call
//! returned success. Three things have to hold, and each is asserted separately
//! because any one of them failing sinks the design:
//!
//! 1. **The descriptor crosses.** `recvmsg` yields a usable fd after
//!    `sandbox_init` has run with `(deny file-read*)`.
//! 2. **It can be mapped.** `mmap` is not an open, so a policy denying opens
//!    should permit it --- "should" being exactly the word that needs a
//!    measurement.
//! 3. **The bytes are the document's.** Not that a call succeeded: the child
//!    parses what it received and reports the page count, which is compared
//!    against what the parent independently knows. A descriptor that arrived
//!    pointing at the wrong thing, or a mapping of the right length full of
//!    zeroes, returns success at every step and the wrong answer here.
//!
//! The control matters as much as the checks. A run where the child never
//! sandboxed itself would pass all three and prove nothing, so the child reports
//! whether `sandbox_init` succeeded and the parent **fails the run** if it did
//! not --- the same shape as the sliced render that must report having paused.
//!
//! ```text
//! cargo run --release --example fdpass-probe -- testdata/text-heavy.pdf
//! ```

// The body is macOS-only, and a `#![cfg]` at the crate root cannot express that
// for a `[[bin]]`: it removes every item including `main`, and cargo then reports
// "`main` function not found", which reads like a missing entry point rather than
// a deliberately empty target. A module carries the same gate and leaves room for
// an entry point beside it.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("fdpass-probe measures the unix descriptor handover; macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main();
}

#[cfg(target_os = "macos")]
mod imp {

    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use tpdf_lib::progressive::{self, RawDocument};
    use tpdf_lib::worker::Shm;
    use tpdf_lib::worker_child::{apply_sandbox, bind};

    /// Where the parent puts the socket before `exec`, mirroring `DOC_FD`/`TILE_FD`.
    const SOCK_FD: i32 = 5;

    /// The child half, re-exec'd into by the parent.
    const CHILD_ARGV: &str = "--fdpass-child";

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == CHILD_ARGV) {
            child(&args);
        }

        let Some(document) = args.get(1).map(PathBuf::from) else {
            eprintln!("usage: fdpass-probe <file.pdf>");
            std::process::exit(2);
        };

        match parent(&document) {
            Ok(passed) if passed => {
                println!(
                "\nfd passing survives the sandbox --- a worker can be started before its document"
            );
            }
            Ok(_) => {
                eprintln!("\n[FAIL] fd passing does not survive the sandbox");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("\n[FAIL] {e}");
                std::process::exit(1);
            }
        }
    }

    /// Spawns a sandboxed child, sends it a descriptor, and checks what came back.
    fn parent(document: &Path) -> Result<bool, String> {
        // What the child's answer is compared against, parsed here, in a process
        // that opened the file the ordinary way. An oracle the child produces would
        // only prove the child agrees with itself.
        let expected = page_count_here(document)?;
        println!(
            "{} has {expected} pages, read in this process",
            document.display()
        );

        let doc = Shm::map_file(document)?;
        let (ours, theirs) = socketpair()?;

        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let mut command = Command::new(exe);
        command
            .arg(CHILD_ARGV)
            .arg("--lib")
            .arg(library_dir())
            .arg("--doc-len")
            .arg(doc.len().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let sock_fd = theirs.as_raw_fd();
        // SAFETY: only dup/dup2/close run between fork and exec, all async-signal-safe.
        // Duplicated first because the source may already sit on the target number.
        unsafe {
            command.pre_exec(move || {
                let s = libc::dup(sock_fd);
                if s < 0 || libc::dup2(s, SOCK_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if s != SOCK_FD {
                    libc::close(s);
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        drop(theirs);
        let stdout = BufReader::new(child.stdout.take().ok_or("child has no stdout")?);

        // Sent only after the child says it is sandboxed, so the descriptor cannot
        // arrive during the window before the policy is in force -- which would make
        // the whole run a measurement of an unsandboxed process.
        let mut lines = stdout.lines();
        let ready = lines
            .next()
            .transpose()
            .map_err(|e| format!("reading the child: {e}"))?
            .ok_or("the child said nothing")?;
        let sandboxed = ready.starts_with("sandboxed");
        report(
            "the child sandboxed itself before receiving anything",
            sandboxed,
            &ready,
        );
        // Read as a positive assertion, never as "the string is absent": a child that
        // failed before printing anything would satisfy a `!contains("false")` test.
        let contained = ready.contains("denied-file-read=true");
        report(
            "and the policy is really in force (it cannot read /etc/hosts)",
            contained,
            &ready,
        );
        if !sandboxed || !contained {
            // Killed rather than left. A child that is never given a descriptor
            // blocks in `recvmsg` forever, and it inherited this process's stderr --
            // so anything capturing our output waits on a pipe that child still
            // holds, and a run that should fail in milliseconds hangs instead. Found
            // exactly that way while proving this control can go red.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }

        send_fd(ours.as_raw_fd(), doc.raw_fd())?;

        let answer = lines
            .next()
            .transpose()
            .map_err(|e| format!("reading the child: {e}"))?
            .ok_or("the child sent no answer")?;
        let _ = child.wait();

        // Parsed rather than pattern-matched loosely: "pages=12" and "error: ..."
        // must not both satisfy a check for "12".
        let got = answer
            .strip_prefix("pages=")
            .and_then(|n| n.trim().parse::<u32>().ok());
        let crossed = got.is_some();
        report("the descriptor crossed and mapped", crossed, &answer);
        let correct = got == Some(expected);
        report(
            "the bytes are the document's",
            correct,
            &format!("child says {got:?}, this process says {expected}"),
        );

        Ok(sandboxed && crossed && correct)
    }

    /// Opens the document normally, to have something to compare the child against.
    fn page_count_here(document: &Path) -> Result<u32, String> {
        let pdfium = bind(&library_dir())?;
        let bindings = progressive::bindings_of(pdfium);
        let doc = RawDocument::open(bindings, document)?;
        Ok(doc.page_count())
    }

    /// The sandboxed half: adopt the socket, sandbox, then wait to be given a file.
    fn child(args: &[String]) -> ! {
        let code = match serve(args) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("[fdpass-child] {e}");
                1
            }
        };
        let _ = std::io::stdout().flush();
        std::process::exit(code);
    }

    fn serve(args: &[String]) -> Result<(), String> {
        let library_dir = value_after(args, "--lib").ok_or("--lib is missing")?;
        let doc_len: usize = value_after(args, "--doc-len")
            .ok_or("--doc-len is missing")?
            .parse()
            .map_err(|_| "--doc-len is not a number".to_string())?;

        // Before the sandbox, exactly as a real worker does: binding maps libpdfium,
        // which a policy denying file reads forbids.
        let pdfium = bind(Path::new(&library_dir))?;
        let bindings = progressive::bindings_of(pdfium);

        apply_sandbox(tpdf_lib::worker::SANDBOX_PROFILE)?;

        // The control, and without it this probe proves nothing. Every check below
        // passes identically whether the policy is in force or `apply_sandbox` did
        // nothing at all -- an fd crosses a socket either way. So the child proves
        // its own containment by attempting something the profile forbids: opening a
        // file it was never given. That must fail, and if it succeeds the run is void
        // rather than green.
        //
        // `/etc/hosts` because it is readable by anyone unsandboxed, so a refusal is
        // the policy and not a permissions accident.
        let denied = std::fs::read("/etc/hosts").is_err();

        let mut out = std::io::stdout();
        writeln!(out, "sandboxed denied-file-read={denied}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;

        // The whole question: a descriptor arriving after the policy is in force.
        let fd = recv_fd(SOCK_FD)?;
        // SAFETY: the descriptor was just received and is owned here; the length is
        // the parent's, for a mapping the parent made and still holds.
        let shm = unsafe { Shm::from_fd(fd.as_raw_fd(), doc_len, false) }?;
        // SAFETY: forgotten below, so the bytes outlive every use of them.
        let bytes: &'static [u8] = unsafe { shm.as_static() };
        std::mem::forget(shm);
        std::mem::forget(fd);

        let document = RawDocument::open_bytes(bindings, bytes)?;
        writeln!(out, "pages={}", document.page_count()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// A connected pair, one half of which is handed to the child.
    fn socketpair() -> Result<(OwnedFd, OwnedFd), String> {
        let mut fds = [0i32; 2];
        // SAFETY: writes two descriptors into a two-element array.
        let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
        if rc != 0 {
            return Err(format!("socketpair: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: both are fresh descriptors this process owns.
        unsafe { Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
    }

    /// Sends one descriptor as `SCM_RIGHTS` ancillary data.
    ///
    /// A byte of ordinary payload goes with it: a `sendmsg` carrying only ancillary
    /// data is permitted to send nothing at all on some systems, and then the
    /// receiver blocks forever on a message that was never framed.
    fn send_fd(socket: i32, fd: i32) -> Result<(), String> {
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        let mut space = [0u8; 32];
        // SAFETY: the control buffer is sized by CMSG_SPACE for one descriptor, and
        // every pointer below is into storage that outlives the call.
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

    /// Receives one descriptor sent as `SCM_RIGHTS`.
    fn recv_fd(socket: i32) -> Result<OwnedFd, String> {
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        let mut space = [0u8; 32];
        // SAFETY: as `send_fd`; the control buffer is sized for one descriptor and
        // the header is only read after `recvmsg` reports success.
        unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &raw mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = space.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32);

            if libc::recvmsg(socket, &raw mut msg, 0) < 0 {
                return Err(format!("recvmsg: {}", std::io::Error::last_os_error()));
            }
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            if cmsg.is_null() {
                return Err("no descriptor arrived".into());
            }
            if (*cmsg).cmsg_level != libc::SOL_SOCKET || (*cmsg).cmsg_type != libc::SCM_RIGHTS {
                return Err("the control message was not SCM_RIGHTS".into());
            }
            let mut fd: i32 = -1;
            std::ptr::copy_nonoverlapping(libc::CMSG_DATA(cmsg).cast::<i32>(), &raw mut fd, 1);
            if fd < 0 {
                return Err("the descriptor that arrived is not valid".into());
            }
            Ok(OwnedFd::from_raw_fd(fd))
        }
    }

    fn report(name: &str, ok: bool, detail: &str) {
        println!("[{}] {name:<48} {detail}", if ok { "OK  " } else { "FAIL" });
    }

    fn value_after(args: &[String], name: &str) -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    fn library_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            .join("vendor/pdfium/lib")
    }
}
