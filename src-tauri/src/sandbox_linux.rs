//! The worker's boundary on Linux: what `sandbox_init` is on macOS and the
//! low-integrity token in a job object is on Windows (`sandbox_win.rs`).
//!
//! It grants what macOS's [`crate::worker::SANDBOX_PROFILE`] grants and nothing
//! more. That profile allows everything, then denies the network, every file
//! write and every file read except metadata and the system's font folders.
//! Here:
//!
//! - **Landlock** handles every file-system right the kernel knows and grants
//!   only reading beneath the folders PDFium's Linux font list scans
//!   ([`FONT_DIRS`]). Metadata is not a Landlock right, so `stat` stays
//!   allowed, as `file-read-metadata` does on macOS. Executing a file is a
//!   right too, so no program can be started.
//! - **Landlock's network rights** (kernel ABI 4 and up) deny TCP bind and
//!   connect, and its **scopes** (ABI 6 and up) deny connecting to abstract Unix
//!   sockets made outside the sandbox, which is where an X11 server or a D-Bus
//!   bus listens, and signalling processes outside it.
//! - **seccomp** refuses `socket` and `socketpair`, and the three io_uring
//!   calls, because an io_uring request makes a socket (`IORING_OP_SOCKET`,
//!   kernel 5.19 and up) without the `socket` call that seccomp sees. So no
//!   socket of any family can be made: a worker's channels to the app are
//!   inherited. It also refuses `ptrace` and reading or writing another
//!   process's memory, and on x86-64 every call by its x32 number, which would
//!   otherwise name `socket` by a number the list does not hold.
//!
//! **What is required, and what is taken where the kernel has it.** The file
//! system half is required: a kernel without Landlock, or one that does not
//! enforce the ruleset, gets a refusal, as a refused profile does on macOS. The
//! network and scope halves are taken where the kernel has them; seccomp covers
//! sockets on every kernel regardless. The worker also refuses to drop its
//! authority while it has more than one thread, because Landlock confines only
//! the thread that asks and its later children, and a thread started earlier
//! would keep the authority.

use landlock::{
    Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, ABI,
};

/// The folders PDFium's Linux font list scans (`CFX_LinuxFontInfo`), which a
/// worker may read beneath. A folder this machine lacks is skipped.
pub const FONT_DIRS: [&str; 4] = [
    "/usr/share/fonts",
    "/usr/share/X11/fonts/Type1",
    "/usr/share/X11/fonts/TTF",
    "/usr/local/share/fonts",
];

/// The newest kernel interface this code knows. The `landlock` crate takes
/// what the running kernel has of it.
const ABI_KNOWN: ABI = ABI::V6;

/// Drops this process's authority, or says why it could not.
///
/// # Errors
///
/// More than one thread, a kernel without Landlock or one that did not enforce
/// the file-system ruleset, or a seccomp filter the kernel refused.
pub fn apply() -> Result<(), String> {
    apply_reading(&[])
}

/// [`apply`], with read access beneath `extra` as well as [`FONT_DIRS`].
///
/// For the OCR worker, whose engine reads its language data after the boundary
/// (`ocr_tesseract.rs`). The parser worker passes nothing.
///
/// # Errors
///
/// As [`apply`], and a folder in `extra` that cannot be opened: a folder the
/// caller named and the kernel cannot find is a mistake, where a missing font
/// folder is only a machine without those fonts.
pub fn apply_reading(extra: &[&std::path::Path]) -> Result<(), String> {
    let threads = thread_count()?;
    if threads != 1 {
        return Err(format!(
            "{threads} threads at the boundary; Landlock would confine only this one"
        ));
    }
    landlock(extra)?;
    seccomp()
}

/// The thread count the kernel reports for this process.
fn thread_count() -> Result<usize, String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("could not read /proc/self/status: {e}"))?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|count| count.trim().parse().ok())
        .ok_or_else(|| "/proc/self/status names no thread count".into())
}

fn landlock(extra: &[&std::path::Path]) -> Result<(), String> {
    let failed = |e: landlock::RulesetError| format!("Landlock: {e}");
    let ruleset = Ruleset::default()
        // The first version's file-system rights are required; a kernel that
        // cannot handle them is a refusal rather than an open sandbox.
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .map_err(failed)?
        // Whatever newer rights this kernel has, on top.
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(ABI_KNOWN))
        .map_err(failed)?
        .handle_access(AccessNet::from_all(ABI_KNOWN))
        .map_err(failed)?
        .scope(Scope::from_all(ABI_KNOWN))
        .map_err(failed)?;
    let mut created = ruleset.create().map_err(failed)?;
    for dir in FONT_DIRS {
        // A folder this machine does not have is not an error: there is
        // nothing beneath it to read.
        let Ok(fd) = PathFd::new(dir) else { continue };
        created = created
            .add_rule(PathBeneath::new(fd, AccessFs::from_read(ABI_KNOWN)))
            .map_err(failed)?;
    }
    for dir in extra {
        let fd = PathFd::new(dir).map_err(|e| format!("Landlock: {}: {e}", dir.display()))?;
        created = created
            .add_rule(PathBeneath::new(fd, AccessFs::from_read(ABI_KNOWN)))
            .map_err(failed)?;
    }
    let status = created.restrict_self().map_err(failed)?;
    match status.ruleset {
        RulesetStatus::FullyEnforced | RulesetStatus::PartiallyEnforced => Ok(()),
        RulesetStatus::NotEnforced => Err("Landlock did not enforce the ruleset".into()),
    }
}

/// The calls a worker is refused. Each answers `EPERM`.
///
/// The io_uring three, because one request on a ring makes a socket and
/// another connects it, and seccomp sees neither: it filters calls, and a ring
/// is worked by the kernel. Refusing `io_uring_setup` is enough on its own,
/// since a worker inherits no ring; the other two are refused so that nothing
/// rests on that.
const DENIED: [libc::c_long; 8] = [
    libc::SYS_socket,
    libc::SYS_socketpair,
    libc::SYS_ptrace,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
];

/// The bit an x86-64 kernel built with x32 support reads as "the x32 number of
/// this call". `socket` is 41 and also `41 | X32_BIT`, under the same
/// architecture word, so a list of numbers has to refuse the bit as well.
#[cfg(target_arch = "x86_64")]
const X32_BIT: u32 = 0x4000_0000;

fn seccomp() -> Result<(), String> {
    let program = program()?;
    // Every thread, though `apply` has made sure there is only one.
    seccompiler::apply_filter_all_threads(&program).map_err(|e| format!("seccomp: {e}"))
}

/// The filter, written out rather than built by `seccompiler`'s rule compiler,
/// because that compiler matches whole numbers only and has no way to refuse
/// the x32 bit (`seccompiler` 0.5.0, `backend/bpf.rs`).
///
/// In order: a call from another architecture ends the process, as
/// `seccompiler` does; on x86-64 an x32 number is refused; each call in
/// [`DENIED`] is refused; everything else is allowed. `EPERM` rather than
/// killing the process, so a refusal arrives as an error the parser reports,
/// the way a denied file read does.
fn program() -> Result<seccompiler::BpfProgram, String> {
    // `linux/filter.h`: load a word of `seccomp_data`, jump if equal or if at
    // least, return.
    const LOAD: u16 = 0x20;
    const IF_EQUAL: u16 = 0x15;
    #[cfg(target_arch = "x86_64")]
    const IF_AT_LEAST: u16 = 0x35;
    const RETURN: u16 = 0x06;
    // `struct seccomp_data`: the call number, then the architecture word.
    const NUMBER: u32 = 0;
    const ARCHITECTURE: u32 = 4;
    // `linux/audit.h`.
    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 62 | 0x8000_0000 | 0x4000_0000;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 183 | 0x8000_0000 | 0x4000_0000;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return Err("seccomp: no filter for this architecture".into());

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let op = |code, k, jt, jf| seccompiler::sock_filter { code, jt, jf, k };
        let count = u8::try_from(DENIED.len()).map_err(|_| "seccomp: too many calls")?;
        let mut out = vec![
            op(LOAD, ARCHITECTURE, 0, 0),
            op(IF_EQUAL, AUDIT_ARCH, 1, 0),
            op(RETURN, libc::SECCOMP_RET_KILL_PROCESS, 0, 0),
            op(LOAD, NUMBER, 0, 0),
        ];
        // Each jump counts the instructions it skips to land on the refusal,
        // which comes after the list and the allow.
        #[cfg(target_arch = "x86_64")]
        out.push(op(IF_AT_LEAST, X32_BIT, count + 1, 0));
        for (at, call) in DENIED.iter().enumerate() {
            let number = u32::try_from(*call).map_err(|_| "seccomp: a negative call number")?;
            // `at` is below `count`, which fits in a byte.
            out.push(op(IF_EQUAL, number, count - at as u8, 0));
        }
        out.push(op(RETURN, libc::SECCOMP_RET_ALLOW, 0, 0));
        out.push(op(
            RETURN,
            libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32 & libc::SECCOMP_RET_DATA),
            0,
            0,
        ));
        Ok(out)
    }
}

/// The Landlock interface version this kernel offers, or 0 for none.
///
/// Asked without making a ruleset, so the app can say what its workers will
/// get before any of them asks.
#[must_use]
pub fn kernel_abi() -> i64 {
    // SAFETY: the documented version query: no attributes, no size, and
    // `LANDLOCK_CREATE_RULESET_VERSION`. It makes nothing.
    let version = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            1u32,
        )
    };
    version.max(0)
}

/// What a worker's boundary is on a kernel with Landlock version `abi`, as one
/// line for the diagnostics log.
///
/// The file-system half is required; the halves named here are the ones taken
/// where the kernel has them. Ubuntu 24.04's own kernel (6.8) is version 4:
/// TCP is ruled, the scopes are not.
#[must_use]
pub fn describe(abi: i64) -> String {
    let mut line = format!("worker boundary: Landlock version {abi}");
    if abi < 1 {
        line.push_str("; no Landlock, so no worker can start");
        return line;
    }
    line.push_str(if abi >= 4 {
        "; TCP bind and connect refused"
    } else {
        "; no network rule (version 4), sockets refused by seccomp alone"
    });
    line.push_str(if abi >= 6 {
        "; abstract sockets and signals outside the sandbox refused"
    } else {
        "; no scopes (version 6), so signals to other processes are not refused"
    });
    if abi < 3 {
        line.push_str("; truncating a file by its path is not refused (version 3)");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter, run by hand until an instruction returns. A check of the
    /// program itself, beside the forked child in `worker_child.rs`, which sees
    /// what the kernel does with it but cannot make an x32 call succeed on a
    /// kernel without x32.
    fn verdict(program: &[seccompiler::sock_filter], arch: u32, number: u32) -> u32 {
        let mut at = 0;
        let mut word = 0;
        loop {
            let op = &program[at];
            match op.code {
                0x20 => word = if op.k == 4 { arch } else { number },
                0x06 => return op.k,
                0x15 | 0x35 => {
                    let taken = if op.code == 0x15 {
                        word == op.k
                    } else {
                        word >= op.k
                    };
                    at += usize::from(if taken { op.jt } else { op.jf });
                }
                other => panic!("an instruction the filter does not use: {other:#x}"),
            }
            at += 1;
        }
    }

    const REFUSED: u32 = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;

    #[test]
    fn the_filter_refuses_each_listed_call_and_allows_the_rest() {
        let program = program().expect("a filter for this architecture");
        let arch = program[1].k;
        for call in DENIED {
            assert_eq!(verdict(&program, arch, call as u32), REFUSED, "call {call}");
        }
        for call in [libc::SYS_read, libc::SYS_openat, libc::SYS_mmap] {
            assert_eq!(
                verdict(&program, arch, call as u32),
                libc::SECCOMP_RET_ALLOW,
                "call {call}"
            );
        }
        assert_eq!(
            verdict(&program, arch ^ 1, libc::SYS_read as u32),
            libc::SECCOMP_RET_KILL_PROCESS
        );
    }

    /// On x86-64 only; elsewhere there is no x32 and nothing to check. The
    /// architecture is tested inside rather than on the test, so the test
    /// exists wherever its module does (`check_mutation_anchors.py` reads a
    /// test's platform from `target_os` alone).
    #[test]
    fn the_filter_refuses_a_call_by_its_x32_number() {
        #[cfg(target_arch = "x86_64")]
        {
            let program = program().expect("a filter for this architecture");
            let arch = program[1].k;
            for call in [libc::SYS_socket, libc::SYS_read] {
                assert_eq!(verdict(&program, arch, call as u32 | X32_BIT), REFUSED);
            }
        }
    }

    #[test]
    fn the_boundary_line_names_what_the_kernel_lacks() {
        assert!(describe(0).contains("no worker can start"));
        let ubuntu = describe(4);
        assert!(ubuntu.contains("TCP bind and connect refused"), "{ubuntu}");
        assert!(ubuntu.contains("no scopes"), "{ubuntu}");
        assert!(!ubuntu.contains("truncating"), "{ubuntu}");
        let newer = describe(7);
        assert!(newer.contains("abstract sockets"), "{newer}");
        assert!(describe(2).contains("truncating"));
        assert!(describe(3).contains("no network rule"));
    }
}
