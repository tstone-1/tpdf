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
};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, DuplicateTokenEx, FreeSid, GetLengthSid, SecurityImpersonation,
    SetTokenInformation, TokenIntegrityLevel, TokenPrimary, PSID, SID_AND_ATTRIBUTES,
    SID_IDENTIFIER_AUTHORITY, TOKEN_ALL_ACCESS, TOKEN_MANDATORY_LABEL,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread, UpdateProcThreadAttribute,
    CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTUPINFOEXW,
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

/// A spawned child, still suspended, with the handles that reach it.
pub struct Contained {
    pub process: HANDLE,
    pub thread: HANDLE,
    pub pid: u32,
    /// Kept so the job outlives the child: dropping it kills the child.
    pub job: Job,
}

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
        use windows_sys::Win32::Foundation::WAIT_FAILED;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, WaitForSingleObject, INFINITE,
        };

        // SAFETY: a live process handle owned here.
        if unsafe { WaitForSingleObject(self.process, INFINITE) } == WAIT_FAILED {
            return Err(format!("WaitForSingleObject failed: {}", last_error()));
        }
        let mut code: u32 = 0;
        // SAFETY: as above; `code` outlives the call.
        let ok = unsafe { GetExitCodeProcess(self.process, &raw mut code) };
        if ok == 0 {
            return Err(format!("GetExitCodeProcess failed: {}", last_error()));
        }
        Ok(code)
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

/// Spawns `command` contained, suspended, reaching only `handles`.
///
/// `handles` are made inheritable here rather than by the caller, because
/// inheritability and list-membership are two halves of one decision and
/// splitting them is how a handle ends up in the list but not inheritable ---
/// which fails the spawn with a parameter error that names neither.
///
/// # Errors
///
/// Any of the token, job, attribute-list or `CreateProcess` steps failing.
pub fn spawn_contained(
    command: &str,
    handles: &[HANDLE],
    containment: &Containment,
) -> Result<Contained, String> {
    for handle in handles {
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
    let mut attributes = AttributeList::new(handles.to_vec())?;

    // SAFETY: zeroed is the documented initial state; `cb` is set below.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of_u32::<STARTUPINFOEXW>();

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
