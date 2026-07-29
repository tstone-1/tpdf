//! Windows containment: what `sandbox_init` is on the other platform.
//!
//! macOS applies its boundary from *inside* the child, after PDFium is bound and
//! before the document is opened, and `worker_child.rs` explains at length why
//! that ordering is the whole security argument there. **Windows has no such
//! ordering to get right, and that is not a simplification.** The token is chosen
//! at `CreateProcess` time and is in force from the first instruction, so there
//! is no "before" in which to load a library --- which is exactly why a
//! restricting SID cannot be used here without Chromium's two-token handover
//! (`bin/win_sandbox_probe.rs` measures the failure: `STATUS_DLL_NOT_FOUND`,
//! before `main`).
//!
//! What this module provides is the rung that *was* measured to work: a **job
//! object** for resource limits and a **low integrity level** for authority. The
//! probe established that PDFium renders byte-identically under it while losing
//! the ability to write the user profile or open the parent process.
//!
//! ## What low integrity does not buy
//!
//! It governs **writes**, not reads. A contained worker can still read any file
//! its user can, so the document must be handed over as an already-open handle
//! rather than as a path --- not for convenience, but because a path is authority
//! we would be re-granting. That is the Windows analogue of the `dup2` the macOS
//! worker does before it drops privilege, and `Shm`'s nameless section is the
//! object being handed.
//!
//! ## Why the handle list is not optional
//!
//! `bInheritHandles: TRUE` inherits **every** inheritable handle the parent
//! holds, which for a process that has opened other documents is a great deal
//! more than the child was meant to get. `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
//! narrows that to an explicit set. Handles in the list must still be marked
//! inheritable --- the list restricts, it does not grant --- so both steps are
//! required and neither alone is sufficient.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, DuplicateTokenEx, FreeSid, GetLengthSid, GetSidSubAuthority,
    GetSidSubAuthorityCount, GetTokenInformation, SecurityImpersonation, SetTokenInformation,
    TokenIntegrityLevel, TokenPrimary, PSID, SID_AND_ATTRIBUTES, SID_IDENTIFIER_AUTHORITY,
    TOKEN_ALL_ACCESS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
    GetExitCodeProcess, InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// Low integrity, `S-1-16-4096`.
const SECURITY_MANDATORY_LOW_RID: u32 = 0x1000;

/// The mandatory-label authority, `S-1-16`.
const LABEL_AUTHORITY: [u8; 6] = [0, 0, 0, 0, 0, 16];

/// How much a contained child may allocate before the kernel refuses it.
///
/// A **real** bound, unlike macOS, where the kernel refuses every relevant rlimit
/// and the substitute is a poll that can only bound a leak, never a burst
/// (`docs/THREAT-MODEL.md` §T3). This is the one place the Windows story is
/// stronger than the macOS one, and it costs nothing to take.
pub const WORKER_MEMORY_CAP: usize = 1024 * 1024 * 1024;

/// What a contained spawn asks for.
pub struct Containment {
    /// Cap on the child's committed memory, in bytes.
    pub memory_cap: usize,
    /// Whether to lower the child to low integrity.
    ///
    /// Not always wanted: the probe runs an uncontained control, and a control
    /// that quietly got a token would not be one.
    pub low_integrity: bool,
}

impl Default for Containment {
    fn default() -> Self {
        Self {
            memory_cap: WORKER_MEMORY_CAP,
            low_integrity: true,
        }
    }
}

/// A job object. Closing it kills everything still inside.
pub struct Job(HANDLE);

// The handle is an ordinary kernel object; moving it between threads is fine.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// Creates a job with the worker limits applied.
    ///
    /// `ActiveProcessLimit` is 1 rather than "generous": a render worker has no
    /// reason to start a process, so anything that tries is either a bug or the
    /// thing this boundary exists to stop, and both are worth failing loudly.
    ///
    /// # Errors
    ///
    /// Either Win32 call failing.
    pub fn create(memory_cap: usize) -> Result<Self, String> {
        if memory_cap == 0 {
            // The kernel refuses this too --- `SetInformationJobObject` returns
            // ERROR_INVALID_PARAMETER --- but "error 87" names neither the field
            // nor the value, and zero is exactly what an unset config field
            // would hold. Refused here so the message says which.
            return Err("job refused: zero memory cap".into());
        }
        // SAFETY: both arguments are optional; null is the documented default.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(format!("CreateJobObject failed: {}", last_error()));
        }
        let job = Job(handle);

        // SAFETY: a zeroed struct is the documented starting point.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        info.BasicLimitInformation.ActiveProcessLimit = 1;
        info.ProcessMemoryLimit = memory_cap;

        // SAFETY: the struct outlives the call and its length is its own size.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                size_of_u32::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
            )
        };
        if ok == 0 {
            return Err(format!("SetInformationJobObject failed: {}", last_error()));
        }
        Ok(job)
    }

    /// Puts a process under this job's limits.
    ///
    /// # Errors
    ///
    /// The assignment failing, which it does if the process has already exited.
    ///
    /// # Safety
    ///
    /// `process` must be a live process handle. Clippy asked for this and it is
    /// right to: nothing in the type distinguishes a live `HANDLE` from a closed
    /// or forged one, so the obligation belongs to the caller and belongs in the
    /// signature where a caller has to acknowledge it.
    pub unsafe fn assign(&self, process: HANDLE) -> Result<(), String> {
        // SAFETY: both handles are live and owned by the caller.
        let ok = unsafe { AssignProcessToJobObject(self.0, process) };
        if ok == 0 {
            return Err(format!("AssignProcessToJobObject failed: {}", last_error()));
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: created by `CreateJobObjectW`, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// A token handle, closed on drop.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: every constructor here produces an owned token handle.
        unsafe { CloseHandle(self.0) };
    }
}

/// A SID with one sub-authority, freed on drop.
struct Sid(PSID);

impl Sid {
    fn allocate(authority: [u8; 6], rid: u32) -> Result<Self, String> {
        let authority = SID_IDENTIFIER_AUTHORITY { Value: authority };
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: `authority` outlives the call; one sub-authority declared and
        // one given; `sid` outlives the call.
        let ok = unsafe {
            AllocateAndInitializeSid(
                std::ptr::from_ref(&authority),
                1,
                rid,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                &raw mut sid,
            )
        };
        if ok == 0 {
            return Err(format!("AllocateAndInitializeSid failed: {}", last_error()));
        }
        Ok(Sid(sid))
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: allocated by `AllocateAndInitializeSid`, freed once.
        unsafe { FreeSid(self.0) };
    }
}

/// A copy of this process's token, lowered to low integrity.
///
/// Nothing here elevates: `CreateProcessAsUser` waives
/// `SE_ASSIGNPRIMARYTOKEN_NAME` for a token it recognises as a lowered or
/// restricted version of the caller's own, which is the only kind this makes.
fn low_integrity_token() -> Result<Token, String> {
    let mut own: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle to self; `own` outlives the call.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut own) };
    if ok == 0 {
        return Err(format!("OpenProcessToken failed: {}", last_error()));
    }
    let own = Token(own);

    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: `own.0` is live; `dup` outlives the call.
    let ok = unsafe {
        DuplicateTokenEx(
            own.0,
            TOKEN_ALL_ACCESS,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &raw mut dup,
        )
    };
    if ok == 0 {
        return Err(format!("DuplicateTokenEx failed: {}", last_error()));
    }
    let dup = Token(dup);

    let sid = Sid::allocate(LABEL_AUTHORITY, SECURITY_MANDATORY_LOW_RID)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.0,
            // `windows-sys` types this `i32` while the field is `u32`; the bit
            // pattern is 0x20 either way.
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    // The documented length is the struct *plus* the SID it points at: a SID is
    // variable-length and lives outside the struct, so `size_of` alone is short
    // and the call fails with a parameter error that names neither.
    // SAFETY: `sid` is a live SID allocated above.
    let length =
        std::mem::size_of::<TOKEN_MANDATORY_LABEL>() + unsafe { GetLengthSid(sid.0) } as usize;
    // SAFETY: `label` outlives the call and `length` describes it.
    let ok = unsafe {
        SetTokenInformation(
            dup.0,
            TokenIntegrityLevel,
            std::ptr::from_ref(&label).cast(),
            u32::try_from(length).unwrap_or(0),
        )
    };
    if ok == 0 {
        return Err(format!(
            "SetTokenInformation(integrity) failed: {}",
            last_error()
        ));
    }
    Ok(dup)
}

/// An owned `PROC_THREAD_ATTRIBUTE_LIST` naming exactly the inheritable handles.
///
/// The handle array must outlive the list *and* the `CreateProcess` call, since
/// `UpdateProcThreadAttribute` stores the pointer rather than copying --- which
/// is why the handles are owned by this struct and not borrowed from the caller.
struct AttributeList {
    buffer: Vec<u8>,
    /// Kept alive because the attribute list points into it.
    _handles: Vec<HANDLE>,
}

impl AttributeList {
    /// Returns `None` for an empty set, which is not an oversight.
    ///
    /// `UpdateProcThreadAttribute` refuses a zero-length handle list with
    /// `ERROR_BAD_LENGTH`, so "inherit nothing" cannot be said with an attribute
    /// --- it is said by omitting the attribute *and* passing
    /// `bInheritHandles: FALSE`, which [`spawn_contained`] does. Encoding that as
    /// `Option` rather than as an empty list keeps the caller from constructing
    /// a request Win32 has no way to represent.
    fn new(handles: Vec<HANDLE>) -> Result<Option<Self>, String> {
        if handles.is_empty() {
            return Ok(None);
        }
        let mut size: usize = 0;
        // First call always "fails" with ERROR_INSUFFICIENT_BUFFER and reports
        // the size; its return value is not the thing to check.
        // SAFETY: a null list with a live out-parameter is the documented way to
        // ask for the size.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &raw mut size) };
        if size == 0 {
            return Err(format!(
                "InitializeProcThreadAttributeList sizing failed: {}",
                last_error()
            ));
        }

        let mut buffer = vec![0u8; size];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = buffer.as_mut_ptr().cast();
        // SAFETY: `buffer` is at least `size` bytes and outlives the list.
        let ok = unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut size) };
        if ok == 0 {
            return Err(format!(
                "InitializeProcThreadAttributeList failed: {}",
                last_error()
            ));
        }

        let mut this = Self {
            buffer,
            _handles: handles,
        };
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = this.buffer.as_mut_ptr().cast();
        // SAFETY: the handle array outlives this struct, and the byte length is
        // its own size.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                this._handles.as_ptr().cast(),
                std::mem::size_of_val(this._handles.as_slice()),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "UpdateProcThreadAttribute failed: {}",
                last_error()
            ));
        }
        Ok(Some(this))
    }

    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = self.buffer.as_mut_ptr().cast();
        // SAFETY: initialised in `new`, deleted once.
        unsafe { DeleteProcThreadAttributeList(list) };
    }
}

/// The exit code [`Contained::kill`] terminates a job with.
///
/// It used to be `1`, on the reasoning that nothing reads a killed worker's exit
/// code. Something does: Windows has no signals, so "killed by signal 11" --- the
/// tell `AGENTS.md` records a crash test turning on --- has no counterpart, and
/// the *only* channel left for saying "this worker did not choose to exit" is the
/// code itself. With `1`, a worker we terminated and a worker that failed on its
/// own were the same sentence.
///
/// The 0xE0000000 bit is the NTSTATUS customer-code flag: reserved for
/// application-defined values and never produced by Windows. That does not make
/// the value impossible for a child to exit with --- nothing does, and
/// `docs/TRAPS.md` records that every `u32` is a legal exit code --- but it does
/// make it a value no ordinary failure path produces by accident.
pub const KILLED_EXIT: u32 = 0xE000_0001;

/// How long [`Contained::epitaph`] waits for a child it believes is dying.
///
/// Milliseconds, and the size is not load-bearing --- the gap being closed is
/// the microseconds between a process's handles closing and its process object
/// being signalled. It is bounded rather than generous on purpose: an epitaph is
/// produced on an error path, and a diagnostic that can stall a UI thread is one
/// nobody will leave in.
const EPITAPH_GRACE: u32 = 100;

/// A spawned child, still suspended, with the handles that reach it.
pub struct Contained {
    pub process: HANDLE,
    pub thread: HANDLE,
    pub pid: u32,
    /// Kept so the job outlives the child: dropping it kills the child.
    pub job: Job,
}

// As [`Job`]: process and thread handles are ordinary kernel objects with no
// thread affinity, so moving one between threads is fine and sharing a reference
// is too --- every method here takes `&self` and none caches anything.
//
// Not decoration. `RenderService::prewarm` in `workers.rs` builds its workers
// inside a spawned thread, so a `Worker` --- which owns one of these off unix ---
// has to be `Send`, and a struct holding raw pointers is not one by default.
unsafe impl Send for Contained {}
unsafe impl Sync for Contained {}

impl Contained {
    /// Lets the child run. Separate from spawning **on purpose**.
    ///
    /// The child is created suspended and the job is applied before it executes
    /// an instruction. Assigning a job to a process that is already running is a
    /// race the process can win --- it could allocate past the cap, or start a
    /// grandchild, before the limit exists --- and a limit that is usually
    /// applied in time is not a limit.
    ///
    /// # Errors
    ///
    /// `ResumeThread` failing.
    pub fn resume(&self) -> Result<(), String> {
        // SAFETY: a live suspended thread handle owned here.
        if unsafe { ResumeThread(self.thread) } == u32::MAX {
            return Err(format!("ResumeThread failed: {}", last_error()));
        }
        Ok(())
    }

    /// Blocks until the child exits and reports its code.
    ///
    /// The code is returned raw rather than reduced to a bool, because the
    /// interesting failures here are NTSTATUS values a boolean would discard ---
    /// a child killed by the loader and one that refused a document are both
    /// "not zero", and only the first says the containment is the problem. Pass
    /// it through [`describe_exit`] before showing it to anyone.
    ///
    /// # Errors
    ///
    /// The wait failing, which leaves the child's state unknown.
    pub fn wait(&self) -> Result<u32, String> {
        // SAFETY: a live process handle owned here.
        if unsafe { WaitForSingleObject(self.process, INFINITE) } == WAIT_FAILED {
            return Err(format!("WaitForSingleObject failed: {}", last_error()));
        }
        self.exit_code()
    }

    /// The child's exit code if it has one, without blocking.
    ///
    /// `None` means still running. **Not `GetExitCodeProcess` alone**, which is
    /// the trap this exists to avoid: that call succeeds on a live process and
    /// reports `STILL_ACTIVE` (259), an ordinary `u32` that a process is
    /// perfectly entitled to exit with. Distinguishing the two by value is
    /// therefore wrong in exactly the case that matters --- a worker that really
    /// did exit 259 would read as running forever. A zero-timeout wait answers
    /// the liveness question on its own, and the code is only read once it has.
    ///
    /// # Errors
    ///
    /// Either call failing, which leaves the child's state unknown --- reported
    /// rather than folded into "still running", since a lost error there becomes
    /// a worker nothing ever reaps.
    pub fn try_wait(&self) -> Result<Option<u32>, String> {
        self.wait_timeout(0)
    }

    /// Waits up to `millis` for the child to exit. `None` means it is still
    /// running when the time is up.
    ///
    /// Exists because [`Contained::wait`] cannot be used to *test* anything. A
    /// caller asserting that some action ended the child has, with an unbounded
    /// wait, exactly two outcomes: the assertion passes, or the process blocks
    /// forever --- and a blocked test is not a failing test. It is a suite that
    /// never finishes, reported by whatever timeout eventually notices, on a
    /// harness that then cannot say which check was to blame. Found exactly that
    /// way: a mutation making `kill` a no-op did not turn the lifecycle test red,
    /// it made it take 177 seconds and hit the harness timeout, which printed a
    /// pass and a hang in the same breath.
    ///
    /// # Errors
    ///
    /// The wait failing, or the exit code not being readable afterwards.
    pub fn wait_timeout(&self, millis: u32) -> Result<Option<u32>, String> {
        // SAFETY: a live process handle owned here.
        match unsafe { WaitForSingleObject(self.process, millis) } {
            WAIT_OBJECT_0 => self.exit_code().map(Some),
            WAIT_TIMEOUT => Ok(None),
            WAIT_FAILED => Err(format!("WaitForSingleObject failed: {}", last_error())),
            other => Err(format!("WaitForSingleObject returned {other}")),
        }
    }

    /// Reads the exit code of a child already known to have exited.
    fn exit_code(&self) -> Result<u32, String> {
        let mut code: u32 = 0;
        // SAFETY: a live process handle owned here; `code` outlives the call.
        let ok = unsafe { GetExitCodeProcess(self.process, &raw mut code) };
        if ok == 0 {
            return Err(format!("GetExitCodeProcess failed: {}", last_error()));
        }
        Ok(code)
    }

    /// Ends the child now.
    ///
    /// Terminating the *job* rather than the process, which is not a detail: the
    /// job is what the containment is, so this ends everything inside it whether
    /// or not the process this struct names is still the only member. A child
    /// that has already exited is not an error --- the caller wants it gone, and
    /// it is.
    ///
    /// # Errors
    ///
    /// The termination failing for a reason other than the job being empty.
    pub fn kill(&self) -> Result<(), String> {
        // SAFETY: a live job handle owned here.
        let ok = unsafe { TerminateJobObject(self.job.0, KILLED_EXIT) };
        if ok == 0 {
            return Err(format!("TerminateJobObject failed: {}", last_error()));
        }
        Ok(())
    }

    /// How the child died, in words, for a parent reporting an epitaph.
    ///
    /// Never fails: this is called *because* something already went wrong, and a
    /// diagnostic that can itself fail to be produced is one more thing to
    /// explain in the moment it is least wanted. An error becomes part of the
    /// sentence.
    ///
    /// **Waits [`EPITAPH_GRACE`] rather than asking once**, and that is a fix
    /// rather than a courtesy. A dying child's handles are closed before its
    /// process object becomes signalled, so the parent sees its pipe reach end
    /// of file *first* --- and the epitaph is asked precisely then, from the
    /// `read_reply` that just got EOF. Asked with a zero timeout it answers
    /// "still running" about a process that has already exited, which is the one
    /// answer that sends a reader looking in the wrong place. Caught by
    /// `a_worker_whose_child_dies_says_so_rather_than_blocking`, which failed on
    /// exactly this and not on the plumbing it was written for.
    ///
    /// A live worker whose pipe broke for some other reason still reads as
    /// running; the grace delays that answer, it does not change it.
    #[must_use]
    pub fn epitaph(&self) -> String {
        match self.wait_timeout(EPITAPH_GRACE) {
            Ok(None) => "still running".into(),
            Ok(Some(code)) => format!("exited with {}", describe_exit(code)),
            Err(e) => format!("could not be waited on: {e}"),
        }
    }
}

impl Drop for Contained {
    fn drop(&mut self) {
        // SAFETY: both handles came from `CreateProcess` and are closed once.
        // The job is dropped after, and with KILL_ON_JOB_CLOSE that is what
        // guarantees no contained child outlives its parent's interest in it.
        unsafe {
            CloseHandle(self.thread);
            CloseHandle(self.process);
        }
    }
}

/// An anonymous pipe, returned as `(read, write)`.
///
/// **Neither end is inheritable.** That is the whole reason this does not use
/// `CreatePipe`'s security attributes to mark them: doing so marks *both*, and
/// the end the parent keeps must never reach the child --- a worker holding the
/// read end of its own reply pipe can watch every answer it gives, and one
/// holding the write end of its own request pipe can feed itself. Only the end
/// actually handed over is marked, by [`spawn_contained`], from the list.
///
/// # Errors
///
/// `CreatePipe` failing.
pub fn pipe() -> Result<(HANDLE, HANDLE), String> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // SAFETY: both out-parameters outlive the call; a null attribute pointer
    // means default security and no inheritance.
    let ok = unsafe { CreatePipe(&raw mut read, &raw mut write, std::ptr::null(), 0) };
    if ok == 0 {
        return Err(format!("CreatePipe failed: {}", last_error()));
    }
    Ok((read, write))
}

/// The three handles a contained child gets as its standard streams.
///
/// All three, always, with no `Option` per stream --- because `STARTUPINFO` has no
/// way to say "this one, but leave the others alone". Setting
/// `STARTF_USESTDHANDLES` makes the child take **all three** from this struct, so
/// a stream left null is a child with no stderr rather than a child with the
/// parent's. A caller that wants to keep one passes the parent's own handle for
/// it; [`Stdio::with_inherited_stderr`] is that case, which is every case here.
pub struct Stdio {
    /// What the child reads requests from.
    pub stdin: HANDLE,
    /// What the child writes replies to.
    pub stdout: HANDLE,
    /// Where a dying worker's epitaph goes. See [`Stdio::with_inherited_stderr`].
    pub stderr: HANDLE,
}

impl Stdio {
    /// Two pipe ends plus this process's own stderr.
    ///
    /// stderr is shared rather than piped for the reason `worker_child.rs` gives
    /// at length: a worker that dies silently is the hardest failure here to
    /// diagnose, and nothing in the parent is reading a third pipe at the moment
    /// a child dies. Sharing the console means the message lands wherever the
    /// app's own messages land.
    ///
    /// # Errors
    ///
    /// `GetStdHandle` failing, which it does when there is no stderr at all.
    pub fn with_inherited_stderr(stdin: HANDLE, stdout: HANDLE) -> Result<Self, String> {
        // SAFETY: a documented constant; the call takes no pointers.
        let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if stderr.is_null() || stderr == INVALID_HANDLE_VALUE {
            return Err("this process has no stderr to share".into());
        }
        Ok(Self {
            stdin,
            stdout,
            stderr,
        })
    }
}

/// Spawns `command` contained, suspended, reaching only `handles`.
///
/// `handles` are made inheritable here rather than by the caller, because
/// inheritability and list-membership are two halves of one decision and
/// splitting them is how a handle ends up in the list but not inheritable ---
/// which fails the spawn with a parameter error that names neither.
///
/// `stdio`, when given, is folded into that same set. It has to be: with a handle
/// list present, a standard handle the child is told to use and that is *not* in
/// the list is not inherited, and the child starts with a stream it cannot read.
/// The caller therefore does not pass its stdio handles in `handles` --- doing so
/// would be harmless but would suggest the two sets are independent, and they are
/// not.
///
/// # Errors
///
/// Any of the token, job, attribute-list or `CreateProcess` steps failing.
pub fn spawn_contained(
    command: &str,
    handles: &[HANDLE],
    containment: &Containment,
    stdio: Option<&Stdio>,
) -> Result<Contained, String> {
    let mut handles = handles.to_vec();
    if let Some(stdio) = stdio {
        handles.extend_from_slice(&[stdio.stdin, stdio.stdout, stdio.stderr]);
    }
    for handle in &handles {
        // SAFETY: the caller's obligation, restated on this function.
        unsafe { make_inheritable(*handle)? };
    }

    let token = if containment.low_integrity {
        Some(low_integrity_token()?)
    } else {
        None
    };
    let job = Job::create(containment.memory_cap)?;

    let mut cmdline: Vec<u16> = OsStr::new(command).encode_wide().chain(Some(0)).collect();
    let mut attributes = AttributeList::new(handles)?;

    // SAFETY: zeroed is the documented initial state; `cb` is set below.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of_u32::<STARTUPINFOEXW>();
    if let Some(stdio) = stdio {
        startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = stdio.stdin;
        startup.StartupInfo.hStdOutput = stdio.stdout;
        startup.StartupInfo.hStdError = stdio.stderr;
    }

    // With no handles to pass, inheritance is switched off entirely rather than
    // narrowed to nothing --- see `AttributeList::new`. Leaving `bInheritHandles`
    // true with no list would inherit *every* inheritable handle this process
    // holds, which is the opposite of what an empty request means and the most
    // expensive possible way to misread it.
    let (inherit, flags) = match attributes.as_mut() {
        Some(list) => {
            startup.lpAttributeList = list.as_ptr();
            (1, CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT)
        }
        None => (0, CREATE_SUSPENDED),
    };
    // SAFETY: overwritten wholesale by a successful CreateProcess.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = match &token {
        // SAFETY: `cmdline` is a live NUL-terminated buffer; the structs outlive
        // the call; the attribute list is initialised.
        None => unsafe {
            CreateProcessW(
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                inherit,
                flags,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::from_ref(&startup).cast(),
                &raw mut info,
            )
        },
        // SAFETY: as above, plus a token this process derived from its own.
        Some(token) => unsafe {
            CreateProcessAsUserW(
                token.0,
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                inherit,
                flags,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::from_ref(&startup).cast(),
                &raw mut info,
            )
        },
    };
    if created == 0 {
        return Err(format!("CreateProcess failed: {}", last_error()));
    }

    // Before the child runs, which is the whole reason it was created suspended.
    // SAFETY: a handle `CreateProcess` returned moments ago.
    unsafe { job.assign(info.hProcess)? };

    Ok(Contained {
        process: info.hProcess,
        thread: info.hThread,
        pid: info.dwProcessId,
        job,
    })
}

/// Marks a handle inheritable. See [`spawn_contained`] for why it lives there.
///
/// # Errors
///
/// The Win32 call failing, which it does on a pseudo-handle.
///
/// # Safety
///
/// `handle` must be a live handle owned by this process.
pub unsafe fn make_inheritable(handle: HANDLE) -> Result<(), String> {
    if handle.is_null() {
        return Err("handle is null".into());
    }
    // SAFETY: a live handle owned by this process.
    let ok = unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    if ok == 0 {
        return Err(format!("SetHandleInformation failed: {}", last_error()));
    }
    Ok(())
}

/// This process's own integrity level, as a mandatory-label RID.
///
/// The value to compare against is [`SECURITY_MANDATORY_LOW_RID`] and friends;
/// they are ordered, so "at most low" is a `<=`.
///
/// # Errors
///
/// Any of the three token calls failing, or a label whose SID has no
/// sub-authorities --- which cannot happen for a mandatory label, and is reported
/// rather than indexed past.
pub fn integrity_level() -> Result<u32, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle to self; `token` outlives the call.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) };
    if ok == 0 {
        return Err(format!("OpenProcessToken failed: {}", last_error()));
    }
    let token = Token(token);

    // Asked for its size first. The label is a header plus a variable-length SID,
    // so there is no fixed struct to hand in --- and a fixed-size buffer here
    // would work on every machine until it met an unusual label.
    let mut needed: u32 = 0;
    // SAFETY: a null buffer with zero length is the documented way to ask.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        );
    }
    if needed == 0 {
        return Err(format!("token label has no size: {}", last_error()));
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` is `needed` bytes, which is what the call just asked for.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };
    if ok == 0 {
        return Err(format!("GetTokenInformation failed: {}", last_error()));
    }

    // SAFETY: on success the buffer holds a TOKEN_MANDATORY_LABEL whose `Sid`
    // points into that same buffer, so it is live for as long as `buffer` is.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    if sid.is_null() {
        return Err("token label has no SID".into());
    }
    // SAFETY: `sid` is a valid SID for the lifetime of `buffer`.
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    if count == 0 {
        return Err("token label SID has no sub-authorities".into());
    }
    // The RID is the last sub-authority: S-1-16-<level>.
    // SAFETY: `count` is in range by the check above.
    Ok(unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) })
}

/// Whether this process is inside **any** job object.
///
/// A *necessary* condition for containment rather than a sufficient one:
/// `IsProcessInJob` with a null job answers "in any job at all", and a debugger, a
/// container or a terminal host can put a process in one for reasons of their own.
/// So `false` disproves containment and `true` does not prove it, which is how
/// [`contained_verdict`] treats it.
///
/// # Errors
///
/// The query failing.
pub fn in_any_job() -> Result<bool, String> {
    let mut in_job: i32 = 0;
    // SAFETY: a pseudo-handle to self, a null job meaning "any", and an out
    // parameter that outlives the call.
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &raw mut in_job) };
    if ok == 0 {
        return Err(format!("IsProcessInJob failed: {}", last_error()));
    }
    Ok(in_job != 0)
}

/// The containment policy, over facts that have already been gathered.
///
/// Split out from [`assert_contained`] because the two conditions cannot both be
/// exercised through it. A test runner is not contained, so it fails the integrity
/// check and returns --- and the job clause below is then unreachable, which is
/// indistinguishable from its not being there. Deleting it was tried as a mutation
/// and every test still passed. As a pure function of two values, all four
/// combinations are reachable and each clause has to earn its place.
///
/// # Errors
///
/// Either condition not holding, naming which.
pub fn contained_verdict(level: u32, in_job: bool) -> Result<(), String> {
    if level > SECURITY_MANDATORY_LOW_RID {
        return Err(format!(
            "not contained: integrity level is 0x{level:04X}, expected at most \
             0x{SECURITY_MANDATORY_LOW_RID:04X} (low)"
        ));
    }
    if !in_job {
        return Err("not contained: this process is in no job object".into());
    }
    Ok(())
}

/// Refuses unless this process is running contained.
///
/// **This is a verification, where macOS has an application**, and the difference
/// is worth stating rather than smoothing over. `apply_sandbox` *causes* the macOS
/// child to lose authority and fails loudly if it cannot; by the time a Windows
/// child runs its first instruction the decision was already taken by whoever
/// called [`spawn_contained`], and nothing it does can change it. All it can do is
/// check --- but checking is what turns "the parent is supposed to contain us" into
/// something that fails when the parent stopped doing so.
///
/// # Errors
///
/// Either query failing, or [`contained_verdict`] refusing what they say.
pub fn assert_contained() -> Result<(), String> {
    contained_verdict(integrity_level()?, in_any_job()?)
}

/// `size_of` as the `u32` every Win32 `cb`/length field wants.
fn size_of_u32<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(0)
}

fn last_error() -> String {
    // SAFETY: no arguments, no pointers.
    format!("error {}", unsafe { GetLastError() })
}

/// Decodes an exit code the way the thing that produced it meant it.
///
/// A child killed by the loader exits `3221225781`, which is `0xC0000135`, which
/// is `STATUS_DLL_NOT_FOUND` --- and only the third spelling says what happened.
/// Only the statuses this code can actually provoke are named: a table
/// pretending to cover NTSTATUS would rot and still miss the next one.
#[must_use]
pub fn describe_exit(code: u32) -> String {
    let name = match code {
        0 => return "0".to_owned(),
        // Ours, and named so an epitaph can say a worker was killed rather than
        // leaving a reader to recognise a magic number. See [`KILLED_EXIT`].
        KILLED_EXIT => return "killed by its parent".to_owned(),
        0xC000_0135 => "STATUS_DLL_NOT_FOUND",
        0xC000_0142 => "STATUS_DLL_INIT_FAILED",
        0xC000_0022 => "STATUS_ACCESS_DENIED",
        0xC000_00FD => "STATUS_STACK_OVERFLOW",
        0xC000_0005 => "STATUS_ACCESS_VIOLATION",
        0xC000_0017 => "STATUS_NO_MEMORY",
        _ => return format!("{code} (0x{code:08X})"),
    };
    format!("0x{code:08X} {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job can be created and carries the limits asked for.
    ///
    /// Creating one proves little on its own --- what it rules out is a wrong
    /// `cb` or flag combination, which `SetInformationJobObject` rejects rather
    /// than ignoring, and which is the failure this code is most likely to have.
    #[test]
    fn a_job_accepts_the_worker_limits() {
        Job::create(WORKER_MEMORY_CAP).expect("a job with the worker limits");
    }

    /// A zero cap is refused, and this check was written asserting the opposite.
    ///
    /// The guess was that the kernel would accept `0` and instantly kill every
    /// worker, so the guard had to be ours. It does not: `SetInformationJobObject`
    /// returns `ERROR_INVALID_PARAMETER`. The guard is still ours, for a smaller
    /// reason worth keeping --- "error 87" names neither the field nor the value,
    /// and zero is exactly what an unset config field would hold.
    #[test]
    fn a_job_with_no_memory_at_all_is_refused() {
        let err = Job::create(0)
            .map(|_| ())
            .expect_err("a zero cap is refused");
        assert!(err.contains("zero memory cap"), "{err}");
    }

    /// The attribute list survives being built with a realistic handle set.
    ///
    /// The hazard it pins is the sizing dance: `InitializeProcThreadAttributeList`
    /// reports its buffer size through an out-parameter on a call that returns
    /// failure, so reading the return value instead of the size produces a list
    /// that is never initialised and a spawn that fails much later.
    #[test]
    fn an_attribute_list_is_built_from_real_handles() {
        // SAFETY: a pseudo-handle, valid to name in a list.
        let handles = vec![unsafe { GetCurrentProcess() }];
        let list = AttributeList::new(handles).expect("an attribute list over one handle");
        assert!(list.is_some(), "a non-empty set must become a list");
    }

    /// "Inherit nothing" cannot be an attribute list, so it is `None`.
    ///
    /// Also written asserting the opposite, and also wrong:
    /// `UpdateProcThreadAttribute` refuses a zero-length list with
    /// `ERROR_BAD_LENGTH`. That is a design constraint rather than a quirk to
    /// work around --- an empty request has to reach `CreateProcess` as
    /// `bInheritHandles: FALSE`, because leaving it true with no list inherits
    /// *everything*, which is the exact opposite of what was asked for.
    #[test]
    fn an_empty_handle_set_is_not_an_attribute_list() {
        let list = AttributeList::new(Vec::new()).expect("an empty set is legal to ask for");
        assert!(list.is_none(), "an empty set must not become a list");
    }

    /// A low-integrity token can be derived from this process's own.
    ///
    /// The failure this catches is the `TOKEN_MANDATORY_LABEL` length: the
    /// documented size is the struct *plus* the variable-length SID, and passing
    /// `size_of` alone fails with a parameter error naming neither.
    #[test]
    fn a_low_integrity_token_is_derived_without_elevation() {
        low_integrity_token().expect("a lowered copy of our own token");
    }

    /// Exit codes are reported in the units the platform meant.
    #[test]
    fn an_exit_code_is_decoded_rather_than_printed_in_decimal() {
        assert_eq!(describe_exit(0), "0");
        assert!(describe_exit(0xC000_0135).contains("STATUS_DLL_NOT_FOUND"));
        // The hex is present for a status with no name, so an unrecognised one
        // is still greppable against a reference rather than a decimal nobody
        // would look up.
        assert!(describe_exit(0xDEAD_BEEF).contains("0xDEADBEEF"));
    }

    /// The test process reads its own integrity level, and it is above low.
    ///
    /// Deliberately not `== 0x2000`: an elevated run is high, and pinning medium
    /// would make the check fail on a correct machine. What it does pin is that a
    /// level is read at all --- a stub returning zero, or the sub-authority index
    /// being off by one (which yields the authority, `16`), both land below low
    /// and go red here.
    #[test]
    fn this_process_can_read_its_own_integrity_level() {
        let level = integrity_level().expect("an integrity level");
        assert!(
            level > SECURITY_MANDATORY_LOW_RID,
            "a test runner should not be low integrity, got 0x{level:04X}"
        );
    }

    /// The containment check refuses this process, which is not contained.
    ///
    /// This is the whole value of `assert_contained`, and the only way to test it
    /// from a runner that is by definition uncontained: prove it says **no** when
    /// the answer is no. A version that returned `Ok` unconditionally --- the
    /// shape this would most plausibly rot into --- fails here, which a test that
    /// could only run inside a real worker would never catch.
    #[test]
    fn an_uncontained_process_is_told_so() {
        let err = assert_contained().expect_err("a test runner is not contained");
        assert!(err.contains("not contained"), "{err}");
    }

    /// Job membership can be read, whatever it says here.
    ///
    /// Deliberately asserts nothing about the value: a terminal host or a debugger
    /// puts its children in a job, so both answers are correct depending on how
    /// the suite was started, and an assertion either way would be a machine
    /// property dressed as a code property. What it does pin is that the call
    /// succeeds --- a wrong argument to `IsProcessInJob` errors rather than lying.
    #[test]
    fn job_membership_is_readable() {
        in_any_job().expect("job membership is queryable");
    }

    /// Both containment conditions are load-bearing, over all four combinations.
    ///
    /// The reason this is a separate function rather than four calls to
    /// `assert_contained`: through the real one, a test runner fails the integrity
    /// check first and the job clause is never reached, so deleting that clause
    /// passed every test. Here neither can hide behind the other.
    #[test]
    fn containment_needs_both_a_low_level_and_a_job() {
        const LOW: u32 = SECURITY_MANDATORY_LOW_RID;
        const MEDIUM: u32 = 0x2000;

        contained_verdict(LOW, true).expect("low integrity inside a job is contained");

        let err = contained_verdict(MEDIUM, true).expect_err("medium integrity is not contained");
        assert!(err.contains("integrity level"), "{err}");

        let err = contained_verdict(LOW, false).expect_err("no job is not contained");
        assert!(err.contains("job object"), "{err}");

        contained_verdict(MEDIUM, false).expect_err("neither condition holds");
    }

    /// A level below low still counts as contained.
    ///
    /// Untrusted is `0x0000`, and the comparison is `>` rather than `!=` precisely
    /// so a *stricter* level than asked for passes. Worth pinning: `!=` reads as
    /// the obvious spelling and would refuse the most contained process there is.
    #[test]
    fn a_level_stricter_than_low_is_still_contained() {
        contained_verdict(0, true).expect("untrusted integrity is contained");
    }

    /// A contained child speaks through a pipe, end to end.
    ///
    /// The one test here that is not a unit test, and it earns that: the pipe
    /// wiring has four parts that only fail *together*, and each failure looks
    /// like the others from outside. `STARTF_USESTDHANDLES` without the handles
    /// in the attribute list gives a child with unusable streams; handles in the
    /// list but not marked inheritable fails the spawn with a parameter error
    /// naming neither; and the parent forgetting to close its copy of the child's
    /// write end gives a read that never sees EOF --- a hang, not an error. Only
    /// reading a known string back proves all four.
    ///
    /// `cmd.exe` rather than a fixture binary, deliberately: it also answers
    /// whether an ordinary program *runs at all* under this containment, which is
    /// the question the whole rung turns on.
    #[test]
    fn a_contained_child_talks_back_through_a_pipe() {
        use std::io::Read;
        use std::os::windows::io::FromRawHandle;

        let (their_stdin, my_stdin) = pipe().expect("a request pipe");
        let (my_stdout, their_stdout) = pipe().expect("a reply pipe");
        let stdio =
            Stdio::with_inherited_stderr(their_stdin, their_stdout).expect("stdio for the child");

        let child = spawn_contained(
            "cmd.exe /c echo tpdf-contained",
            &[],
            &Containment::default(),
            Some(&stdio),
        )
        .expect("a contained child");
        child.resume().expect("the child runs");

        // The parent's copies of the child's ends go now. Holding the write end
        // of the reply pipe would keep it open forever and the read below would
        // block rather than return.
        // SAFETY: both are live handles this process owns and closes once.
        unsafe {
            CloseHandle(their_stdin);
            CloseHandle(their_stdout);
            CloseHandle(my_stdin);
        }

        // SAFETY: a live pipe end this process owns; `File` closes it on drop.
        let mut replies = unsafe { std::fs::File::from_raw_handle(my_stdout.cast()) };
        let mut said = String::new();
        replies
            .read_to_string(&mut said)
            .expect("the child's output");

        assert!(
            said.contains("tpdf-contained"),
            "a contained child should still run and be heard, got {said:?}"
        );
        assert_eq!(child.wait().map(describe_exit).as_deref(), Ok("0"));
    }

    /// A suspended child is running; a killed one is not, and says how it died.
    ///
    /// The three observations a parent needs, on one child, in the order it needs
    /// them. `try_wait` must say `None` while the child is alive --- and the child
    /// here is *suspended*, which is the state a spawn returns and therefore the
    /// one a mistaken "has it finished?" would be asked in first.
    ///
    /// `kill` goes through the job rather than the process, so this also pins
    /// that the job really does contain the child: terminating it ends a process
    /// the call never names.
    #[test]
    fn a_contained_child_reports_running_then_how_it_died() {
        // `pause` reads stdin and blocks, so the child stays alive on its own
        // without this test having to time anything.
        let (their_stdin, my_stdin) = pipe().expect("a request pipe");
        let (my_stdout, their_stdout) = pipe().expect("a reply pipe");
        let stdio =
            Stdio::with_inherited_stderr(their_stdin, their_stdout).expect("stdio for the child");
        let child = spawn_contained(
            "cmd.exe /c pause",
            &[],
            &Containment::default(),
            Some(&stdio),
        )
        .expect("a contained child");

        assert_eq!(
            child.try_wait(),
            Ok(None),
            "a suspended child has not exited"
        );
        assert_eq!(child.epitaph(), "still running");

        child.resume().expect("the child runs");
        child.kill().expect("the child is killed");
        // Bounded, and that is the point. `cmd /c pause` blocks on a stdin this
        // test still holds the write end of, so a `kill` that did nothing would
        // make an unbounded wait here hang rather than fail --- which is how this
        // was found: the no-op mutation took 177 seconds and tripped the harness
        // timeout instead of going red. Ten seconds is generous for a process
        // whose job has just been terminated; a slow machine does not need more.
        let code = child
            .wait_timeout(10_000)
            .expect("a wait after killing")
            .expect("a killed child has exited within ten seconds");
        assert_ne!(code, 0, "a killed child did not exit cleanly");
        assert!(
            child.epitaph().starts_with("exited with"),
            "{}",
            child.epitaph()
        );

        // SAFETY: live handles this process owns and closes once.
        unsafe {
            CloseHandle(their_stdin);
            CloseHandle(their_stdout);
            CloseHandle(my_stdin);
            CloseHandle(my_stdout);
        }
    }

    /// A child that exits 259 is finished, not "still active".
    ///
    /// `GetExitCodeProcess` reports `STILL_ACTIVE` --- which *is* 259 --- for a live
    /// process, so code that tells the two apart by value is wrong for exactly
    /// one input, and that input is a perfectly legal exit code. This is the test
    /// for that one input. It is worth having precisely because the wrong version
    /// passes every other check: a worker that really did exit 259 would read as
    /// running forever, and the pool would wait on a process that is gone.
    #[test]
    fn a_child_that_exits_with_still_active_is_not_still_active() {
        let (their_stdin, my_stdin) = pipe().expect("a request pipe");
        let (my_stdout, their_stdout) = pipe().expect("a reply pipe");
        let stdio =
            Stdio::with_inherited_stderr(their_stdin, their_stdout).expect("stdio for the child");
        let child = spawn_contained(
            "cmd.exe /c exit 259",
            &[],
            &Containment::default(),
            Some(&stdio),
        )
        .expect("a contained child");
        child.resume().expect("the child runs");

        assert_eq!(child.wait(), Ok(259), "the child chose this code");
        assert_eq!(
            child.try_wait(),
            Ok(Some(259)),
            "259 is an exit code, not a liveness flag"
        );

        // SAFETY: live handles this process owns and closes once.
        unsafe {
            CloseHandle(their_stdin);
            CloseHandle(their_stdout);
            CloseHandle(my_stdin);
            CloseHandle(my_stdout);
        }
    }

    /// A null handle is refused rather than passed to Win32.
    #[test]
    fn a_null_handle_cannot_be_made_inheritable() {
        // SAFETY: null is not a live handle, which is the case under test --- the
        // guard returns before anything is dereferenced.
        let err =
            unsafe { make_inheritable(std::ptr::null_mut()) }.expect_err("null must be refused");
        assert!(err.contains("null"), "{err}");
    }
}
