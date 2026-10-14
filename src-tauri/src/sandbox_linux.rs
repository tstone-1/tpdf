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
//! - **seccomp** refuses `socket` and `socketpair` outright, so no socket of any
//!   family can be made: a worker's channels to the app are inherited. It also
//!   refuses `ptrace` and reading or writing another process's memory.
//!
//! **What is required, and what is taken where the kernel has it.** The file
//! system half is required: a kernel without Landlock, or one that does not
//! enforce the ruleset, gets a refusal, as a refused profile does on macOS. The
//! network and scope halves are taken where the kernel has them; seccomp covers
//! sockets on every kernel regardless. The worker also refuses to drop its
//! authority while it has more than one thread, because Landlock confines only
//! the thread that asks and its later children, and a thread started earlier
//! would keep the authority.

use std::collections::BTreeMap;

use landlock::{
    Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, ABI,
};
use seccompiler::{SeccompAction, SeccompFilter, SeccompRule, TargetArch};

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

fn seccomp() -> Result<(), String> {
    let denied = [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
    ];
    let rules: BTreeMap<i64, Vec<SeccompRule>> =
        denied.into_iter().map(|call| (call, Vec::new())).collect();
    let arch = TargetArch::try_from(std::env::consts::ARCH)
        .map_err(|e| format!("seccomp: no filter for this architecture: {e}"))?;
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        // `EPERM` rather than killing the process, so a refusal arrives as an
        // error the parser reports, the way a denied file read does.
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )
    .map_err(|e| format!("seccomp: {e}"))?;
    let program: seccompiler::BpfProgram =
        filter.try_into().map_err(|e| format!("seccomp: {e}"))?;
    // Every thread, though `apply` has made sure there is only one.
    seccompiler::apply_filter_all_threads(&program).map_err(|e| format!("seccomp: {e}"))
}
