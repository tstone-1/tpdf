//! Running `tpdf-cli render` for one page, inside a job object this module
//! owns (`docs/THREAT-MODEL.md` §T6.47).
//!
//! The job holds `tpdf-cli` and, nested inside it, the sandboxed worker that
//! `tpdf-cli` starts: Windows has nested jobs since 8, so a job the worker is
//! put in by `tpdf-cli` is a child of this one, and terminating this one ends
//! both. It is created with `KILL_ON_JOB_CLOSE`, so the host process ending ---
//! Explorer recycling its surrogate, `prevhost.exe` closing --- also ends them,
//! and with a memory limit for the two together.
//!
//! The child is put in the job after it starts and before it is given the
//! document. `tpdf-cli` starts its worker only once it has read standard input
//! to the end (`cli.rs`, `opened_or_stdin`), so nothing escapes the job in the
//! gap.

use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{WaitForSingleObject, CREATE_NO_WINDOW};

use crate::plan::{self, Ended, Fit};

/// Why a page did not come back.
#[derive(Debug)]
pub enum Failure {
    /// `tpdf-cli` refused: the document, or a page past its end (exit 3).
    Refused,
    /// It ran out of time and was ended with its worker.
    TimedOut,
    /// Anything else: it could not start, failed, or wrote too much.
    Failed,
}

/// A job object, closed when dropped. Closing it ends whatever is still in it.
struct Job(HANDLE);

impl Job {
    fn new() -> Option<Self> {
        // SAFETY: no attributes and no name; the handle is owned by the value
        // returned and closed exactly once, in `drop`.
        let job = Job(unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.ok()?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
        limits.JobMemoryLimit = plan::JOB_MEMORY;
        // SAFETY: the structure is the one the information class names, and
        // its size is passed with it.
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .ok()?;
        Some(job)
    }

    fn terminate(&self) {
        // SAFETY: a job handle this value owns. A job with nothing left in it
        // is not an error worth reporting.
        let _ = unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: owned, and closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// `document`'s page `page` (1-based) as a PNG no larger than `fit`, from the
/// `tpdf-cli.exe` at `cli`, within `time`.
pub fn page(
    cli: &Path,
    document: Arc<[u8]>,
    fit: Fit,
    page: u32,
    time: Duration,
) -> Result<Vec<u8>, Failure> {
    let mut child = Command::new(cli)
        .args(plan::render_args(fit, page))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|_| Failure::Failed)?;
    let process = HANDLE(child.as_raw_handle());
    let Some(job) = Job::new() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Failed);
    };
    // SAFETY: both handles are live: the job is owned above, the process by
    // `child`, which has not been waited for.
    if unsafe { AssignProcessToJobObject(job.0, process) }.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Failed);
    }

    // The document is written on one thread and the answer read on another,
    // so neither pipe filling up can stall the other.
    let writer = child.stdin.take().map(|mut stdin| {
        std::thread::spawn(move || {
            // A write that fails is `tpdf-cli` having ended; its exit code says why.
            let _ = stdin.write_all(&document);
        })
    });
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            let mut png = Vec::new();
            let mut capped = stdout.take(plan::MAX_PNG as u64 + 1);
            capped.read_to_end(&mut png).map(|_| png)
        })
    });

    let millis = u32::try_from(time.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the process handle is live until `child` is dropped below.
    let finished = unsafe { WaitForSingleObject(process, millis) } == WAIT_OBJECT_0;
    // Whatever is left in the job goes now: everything if time ran out, and
    // otherwise only a worker that outlived `tpdf-cli` and might be holding
    // the pipe the reader is waiting on.
    job.terminate();
    let status = child.wait().ok();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let png = reader
        .and_then(|reader| reader.join().ok())
        .and_then(Result::ok);
    drop(job);

    if !finished {
        return Err(Failure::TimedOut);
    }
    match plan::ended(status.and_then(|status| status.code())) {
        Ended::Done => match png {
            Some(png) if png.len() <= plan::MAX_PNG => Ok(png),
            _ => Err(Failure::Failed),
        },
        Ended::Refused => Err(Failure::Refused),
        Ended::Failed => Err(Failure::Failed),
    }
}
