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
//!
//! A render can be ended from another thread through its [`Cancel`], which
//! the preview pane uses when the reader moves to another file.

use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

// SAFETY: a job handle may be used from any thread, and `Job` only ever
// terminates it and, once, in `drop`, closes it. Needed to share one through
// an `Arc` with the thread that cancels it.
unsafe impl Send for Job {}
// SAFETY: as above; `terminate` takes `&self` and is safe to call from two
// threads at once, since terminating a job twice is harmless.
unsafe impl Sync for Job {}

/// Ends one render from another thread.
///
/// **Who closes the job handle is settled by `Arc`, not by ordering.** The
/// render shares its job with this value while the job has a process in it,
/// and a cancel that arrives then holds its own reference for as long as it
/// terminates. The handle is closed when the last reference goes, so a
/// cancel can never terminate a closed handle, and it is never closed twice;
/// terminating it twice, from the cancel and from the render's own end,
/// is harmless. A cancel that arrives before the job exists is remembered, and
/// the render ends its child as soon as the job is there; one that arrives
/// after the render returned finds nothing to end.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Mutex<Armed>>);

#[derive(Default)]
struct Armed {
    job: Option<Arc<Job>>,
    cancelled: bool,
}

impl Cancel {
    /// Ends the render, `tpdf-cli` and its worker together, now or as soon
    /// as it has started them. Never panics: it is called from `prevhost.exe`'s
    /// own thread, under COM.
    pub fn cancel(&self) {
        let job = {
            let mut armed = self.armed();
            armed.cancelled = true;
            armed.job.clone()
        };
        if let Some(job) = job {
            job.terminate();
        }
    }

    /// Shares `job` with [`Cancel::cancel`]. False when the render was
    /// cancelled already, so the caller ends it itself.
    fn arm(&self, job: &Arc<Job>) -> bool {
        let mut armed = self.armed();
        if armed.cancelled {
            return false;
        }
        armed.job = Some(Arc::clone(job));
        true
    }

    /// Takes the job back, once nothing is left in it to end.
    fn disarm(&self) {
        self.armed().job = None;
    }

    /// The state, whether or not a thread panicked holding it: none of the
    /// sections above can leave it half written.
    fn armed(&self) -> MutexGuard<'_, Armed> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `document`'s page `page` (1-based) as a PNG no larger than `fit`, from the
/// `tpdf-cli.exe` at `cli`, within `time`, unless `cancel` ends it first.
pub fn page(
    cli: &Path,
    document: Arc<[u8]>,
    fit: Fit,
    page: u32,
    time: Duration,
    cancel: &Cancel,
) -> Result<Vec<u8>, Failure> {
    let mut command = Command::new(cli);
    command.args(plan::render_args(fit, page));
    run(command, document, time, cancel)
}

/// Runs `command` in a job of its own, the document on its standard input and
/// the answer read from its standard output.
fn run(
    mut command: Command,
    document: Arc<[u8]>,
    time: Duration,
    cancel: &Cancel,
) -> Result<Vec<u8>, Failure> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|_| Failure::Failed)?;
    let process = HANDLE(child.as_raw_handle());
    let Some(job) = Job::new().map(Arc::new) else {
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
    // Before the document is written, so a render cancelled already never
    // reads it. Its child is in the job, which ends it.
    if !cancel.arm(&job) {
        job.terminate();
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
    // the pipe the reader is waiting on. A cancel ends the wait above the
    // same way, by ending `tpdf-cli`, and its result is a failure the pane
    // has stopped waiting for.
    job.terminate();
    cancel.disarm();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};

    /// Which part the helper below plays: `child` for the process a render
    /// starts, `grandchild` for the one that starts in turn, as `tpdf-cli`
    /// starts its worker.
    const ROLE: &str = "TPDF_SHELL_TEST_ROLE";
    /// Where the grandchild writes its process id once it runs.
    const MARKER: &str = "TPDF_SHELL_TEST_MARKER";
    const HELPER: &str = "render::tests::a_render_child_that_waits";

    fn helper(role: &str, marker: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("this test binary"));
        command
            .args(["--exact", HELPER, "--nocapture"])
            .env(ROLE, role)
            .env(MARKER, marker);
        command
    }

    fn marker(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("tpdf-shell-{name}-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Started in a process of its own by the tests below; inert when the
    /// test runner lists or runs it without the variable. It waits a minute
    /// at most, so one that is never ended does not outlive the run for long.
    #[test]
    fn a_render_child_that_waits() {
        let (Some(role), Some(marker)) = (std::env::var_os(ROLE), std::env::var_os(MARKER)) else {
            return;
        };
        let marker = PathBuf::from(marker);
        if role == "child" {
            // Standard output is inherited, so the grandchild holds the pipe
            // the render reads, as a worker can.
            let _grandchild = helper("grandchild", &marker).spawn();
        } else {
            let _ = std::fs::write(&marker, std::process::id().to_string());
        }
        std::thread::sleep(Duration::from_secs(60));
    }

    /// The control and the case: a render cancelled while its child and
    /// grandchild run returns at once, both processes ended, where without
    /// the cancel it would wait out its 30 seconds.
    #[test]
    fn a_cancelled_render_ends_its_process_and_the_one_that_started() {
        let marker = marker("cancel");
        let cancel = Cancel::default();
        let render = {
            let cancel = cancel.clone();
            let command = helper("child", &marker);
            std::thread::spawn(move || {
                run(
                    command,
                    Arc::from(&b"%PDF-1.7"[..]),
                    Duration::from_secs(30),
                    &cancel,
                )
            })
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&marker)
                .ok()
                .and_then(|text| text.parse::<u32>().ok())
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the grandchild never ran");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            cancel.armed().job.is_some(),
            "the render shares its job while it runs"
        );

        let asked = Instant::now();
        cancel.cancel();
        let result = render.join().expect("the render thread");
        let took = asked.elapsed();
        assert!(
            took < Duration::from_secs(3),
            "the cancelled render took {took:?} to return"
        );
        assert!(matches!(result, Err(Failure::Failed)), "{result:?}");
        assert!(
            cancel.armed().job.is_none(),
            "the job is taken back when the render returns"
        );
        // A second cancel, after the render: nothing to end, and no handle
        // to use.
        cancel.cancel();

        // SAFETY: a handle opened here and closed here; a process that is
        // gone already cannot be opened, which is the answer too.
        if let Ok(grandchild) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
            // SAFETY: the handle opened above.
            let ended = unsafe { WaitForSingleObject(grandchild, 2000) } == WAIT_OBJECT_0;
            // SAFETY: as above, closed once.
            let _ = unsafe { CloseHandle(grandchild) };
            assert!(ended, "the grandchild {pid} outlived the cancel");
        }
        let _ = std::fs::remove_file(&marker);
    }

    /// A cancel that arrives before the render has its job is not lost: the
    /// render ends its child as soon as the job is there.
    #[test]
    fn a_render_cancelled_before_it_started_ends_at_once() {
        let marker = marker("early");
        let cancel = Cancel::default();
        cancel.cancel();
        let asked = Instant::now();
        let result = run(
            helper("child", &marker),
            Arc::from(&b"%PDF-1.7"[..]),
            Duration::from_secs(30),
            &cancel,
        );
        let took = asked.elapsed();
        assert!(took < Duration::from_secs(3), "it took {took:?}");
        assert!(matches!(result, Err(Failure::Failed)), "{result:?}");
        assert!(cancel.armed().job.is_none(), "the job was never shared");
        let _ = std::fs::remove_file(&marker);
    }
}
