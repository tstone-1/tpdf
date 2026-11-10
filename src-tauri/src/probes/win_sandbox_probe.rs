//! The Windows containment ladder, measured one rung at a time.
//!
//! Four rungs, each running the *same* child code against the *same* document so
//! that the only variable is the containment --- plus two diagnostics that exist
//! only to attribute a failure to one ingredient rather than to a token:
//!
//! | rung | what it adds | what it is meant to stop |
//! |------|--------------|--------------------------|
//! | `bare` | nothing --- the control | nothing; it is what Windows does today |
//! | `job` | a job object: memory cap, one process, kill-on-close | a bomb, a fork bomb, an orphan |
//! | `lowil` | job + low integrity level | writing anything, opening our own process |
//! | `noprivs` | *diagnostic*: privileges dropped, no restricting SID | nothing on its own |
//! | `sidonly` | *diagnostic*: restricting SID, privileges kept | --- |
//! | `restricted` | job + a restricted token | reaching anything at all |
//!
//! The two diagnostics were added after `restricted` failed, because one failing
//! row cannot say *which* of the two ingredients failed --- and the answer turned
//! out to matter: the restricting SID is the whole cause, and dropping privileges
//! costs nothing. A rung marked diagnostic is excluded from the verdict, since
//! `noprivs` renders perfectly and denies nothing, and a verdict that simply took
//! the last row that worked would recommend it.
//!
//! **The control is not decoration.** `bare` exists so that a difference between
//! the in-process reference and a contained render can be attributed to the
//! containment rather than to the harness --- if `bare` already differs, the
//! transport is wrong and every other row is meaningless. `AGENTS.md` records
//! two separate cases here of a check that could not fail; a comparison whose
//! baseline is never itself compared is that shape.
//!
//! **Pixels, not exit codes.** The macOS work already caught a sandboxed PDFium
//! returning `ok` while quietly substituting a typeface, so "the child exited 0"
//! and "the child rendered the document" are different claims. The default
//! fixture is `text-base14.pdf` for exactly that reason: base-14 fonts are *not*
//! embedded, so PDFium must go and find a system face, which is the first thing
//! any containment breaks and the failure that does not announce itself.
//!
//! **What the `job` column claims is now measured, two thirds of it.** That row
//! promised "a bomb, a fork bomb, an orphan" from the day it was written and
//! probed none of the three: the three original authority probes are all
//! *integrity level* properties, so every row was reporting on `lowil` and above
//! while the job's own two limits went unexercised. `commit_past_cap` and
//! `second_process` close that, and the control earns its keep --- `bare` commits
//! 1 GB and spawns a process, every rung with a job is refused with 1455
//! (commit charge) and 1816 (process quota). The third, an orphan outliving the
//! parent, is `KILL_ON_JOB_CLOSE` and is still only claimed: testing it means
//! killing *this* process, which a probe cannot do and then report.
//!
//! **Handles, not paths.** The child is handed its document and its output as
//! inherited handles and never opens a path for either. That is not incidental
//! --- a contained child *cannot* open a path, so this is the only transport
//! available, and it is the Windows analogue of the `dup2` the macOS worker
//! does before it drops authority. Proving it works here is half of what a real
//! Windows worker needs.

use std::ffi::{c_void, OsStr};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use tpdf_lib::document::OpenDocument;

use tpdf_lib::progressive::{self, CancelToken, TileSpec};
// The one piece genuinely shared with the production path. The rest of the
// Win32 below is deliberately NOT routed through `tpdf_lib::sandbox_win`:
// this probe is the record of a measurement, and a record that changes when
// the thing it measured is refactored has stopped being evidence. A decoder
// is different --- two copies of a status table drift, and the copy that
// drifts is the one nobody re-reads (`docs/TRAPS.md`).
use tpdf_lib::sandbox_win::describe_exit;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, CreateRestrictedToken, DuplicateTokenEx, FreeSid, GetLengthSid,
    SecurityImpersonation, SetTokenInformation, TokenIntegrityLevel, TokenPrimary,
    DISABLE_MAX_PRIVILEGE, PSID, SID_AND_ATTRIBUTES, SID_IDENTIFIER_AUTHORITY, TOKEN_ALL_ACCESS,
    TOKEN_MANDATORY_LABEL,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, GetCurrentProcess, GetExitCodeProcess, OpenProcess,
    OpenProcessToken, ResumeThread, WaitForSingleObject, CREATE_SUSPENDED, INFINITE,
    PROCESS_INFORMATION, PROCESS_VM_WRITE, STARTUPINFOW,
};

/// Marks the re-exec. Not `--render-worker`: this child is not a worker and must
/// never be mistaken for one by `lib.rs`'s dispatch, which would hand it a
/// protocol it does not speak.
const CHILD_ARGV: &str = "--contained-child";

/// The document the child renders, and the tile it renders from it.
///
/// One tile, at the origin, at scale 1. Not the whole page: the comparison wants
/// a deterministic buffer of a known size, and a tile is what the real renderer
/// deals in anyway.
const TILE: TileSpec = TileSpec {
    scale: 1.0,
    turns: 0,
    x: 0,
    y: 0,
    width: 400,
    height: 400,
};

/// Memory ceiling for the job object, in bytes.
///
/// Generous on purpose. This probe is asking whether a limit *applies*, not
/// where it should sit --- a cap tight enough to be interesting would fail for
/// reasons that have nothing to do with containment, and the resulting row would
/// read as "containment breaks PDFium" when it means "512 MB was not enough".
const JOB_MEMORY_CAP: usize = 512 * 1024 * 1024;

/// Low integrity, `S-1-16-4096`.
const SECURITY_MANDATORY_LOW_RID: u32 = 0x1000;
/// `S-1-5-12`, the SID a restricted token restricts *to*.
const SECURITY_RESTRICTED_CODE_RID: u32 = 12;

/// One rung of the ladder.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rung {
    Bare,
    Job,
    LowIl,
    /// `lowil` with the job's UI restrictions, which is what the production
    /// worker runs under since `lowil` was measured reading and writing the
    /// reader's clipboard.
    UiLimits,
    NoPrivs,
    SidOnly,
    Restricted,
}

impl Rung {
    const ALL: [Rung; 7] = [
        Rung::Bare,
        Rung::Job,
        Rung::LowIl,
        Rung::UiLimits,
        Rung::NoPrivs,
        Rung::SidOnly,
        Rung::Restricted,
    ];

    fn name(self) -> &'static str {
        match self {
            Rung::Bare => "bare",
            Rung::Job => "job",
            Rung::LowIl => "lowil",
            Rung::UiLimits => "uilimits",
            Rung::NoPrivs => "noprivs",
            Rung::SidOnly => "sidonly",
            Rung::Restricted => "restricted",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Rung::Bare => "control: no containment, what Windows does today",
            Rung::Job => "job object: memory cap, one process, kill-on-close",
            Rung::LowIl => "job + low integrity level",
            Rung::UiLimits => "job + low integrity level + UI restrictions",
            Rung::NoPrivs => "diagnostic: privileges dropped, no restricting SID",
            Rung::SidOnly => "diagnostic: restricting SID, privileges kept",
            Rung::Restricted => "job + restricted token (privileges dropped and SID)",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Rung::ALL.into_iter().find(|r| r.name() == text)
    }

    /// Whether this rung wants a job object.
    fn wants_job(self) -> bool {
        self != Rung::Bare
    }

    /// Whether this rung exists to attribute a failure rather than to be adopted.
    ///
    /// `noprivs` drops privileges and restricts nothing, so it denies none of the
    /// three authority probes --- it is strictly weaker than `lowil` despite
    /// sitting below `restricted` in the list. Without this the verdict picks the
    /// last row that rendered and recommends it, which is how a harness ends up
    /// proposing the one rung that buys nothing.
    fn is_diagnostic(self) -> bool {
        matches!(self, Rung::NoPrivs | Rung::SidOnly)
    }
}

/// What one rung produced.
struct Outcome {
    rung: Rung,
    /// `None` when the child never wrote a header --- it died first.
    report: Option<serde_json::Value>,
    pixels: Vec<u8>,
    exit_code: u32,
    spawn_error: Option<String>,
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == CHILD_ARGV) {
        child(&args[pos + 1..]);
    }
    match parent(&args) {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("[FAIL] {message}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Parent
// ---------------------------------------------------------------------------

fn parent(args: &[String]) -> Result<i32, String> {
    let repo = repo_root();
    let doc_path = match args.get(1) {
        Some(p) => PathBuf::from(p),
        None => repo.join("testdata/text-base14.pdf"),
    };
    if !doc_path.exists() {
        return Err(format!(
            "{} does not exist --- run the generators in testdata/ first",
            doc_path.display()
        ));
    }
    let lib_dir = repo.join("vendor/pdfium/bin");
    if !lib_dir.join("pdfium.dll").exists() {
        return Err(format!(
            "no pdfium.dll under {} --- run scripts/fetch_pdfium.py",
            lib_dir.display()
        ));
    }

    println!("win-sandbox-probe");
    println!("  document : {}", doc_path.display());
    println!("  library  : {}", lib_dir.display());
    println!("  tile     : {}x{} at scale 1.0", TILE.width, TILE.height);
    println!();

    // The oracle: this process, no containment, no child. Everything below is
    // compared against it, including `bare` --- which is what makes `bare` a
    // control over the *harness* and not merely another row.
    let reference = render_here(&lib_dir, &doc_path)?;
    println!(
        "[OK]   reference rendered in-process, {} bytes, {} non-white",
        reference.len(),
        non_white(&reference)
    );
    println!();

    // Each rung overwrites the clipboard with its markers, so the reader's
    // text is kept and put back, whatever the rungs did.
    let kept = clipboard_text();
    let mut outcomes = Vec::new();
    let mut failed = None;
    for rung in Rung::ALL {
        match run_rung(rung, &lib_dir, &doc_path) {
            Ok(outcome) => outcomes.push(outcome),
            Err(why) => {
                failed = Some(why);
                break;
            }
        }
    }
    if let Ok(Some(text)) = &kept {
        if let Err(why) = set_clipboard_text(text) {
            println!("[WARN] the clipboard's text could not be put back: {why}");
        }
    }
    if let Some(why) = failed {
        return Err(why);
    }

    report(&outcomes, &reference)
}

/// Renders the tile in this process, with no containment of any kind.
fn render_here(lib_dir: &Path, doc_path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(doc_path).map_err(|e| format!("reading {doc_path:?}: {e}"))?;
    render_bytes(lib_dir, bytes)
}

/// The render both sides run. Shared so the comparison cannot drift.
fn render_bytes(lib_dir: &Path, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let pdfium = progressive::bind(lib_dir)?;
    let bindings = progressive::bindings_of(pdfium);
    // Leaked because `open_bytes` needs the buffer to outlive the document, and
    // the document lives until this process exits either way.
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let document = OpenDocument::open_bytes(bindings, bytes, None).map_err(|r| r.reason)?;
    let page = document.page(0)?;
    let (pixels, progress) =
        progressive::render_tile(bindings, &page, TILE, None, &CancelToken::new())?;
    if !progress.outcome.is_done() {
        return Err("render did not complete".into());
    }
    Ok(pixels)
}

/// Builds one rung's containment, runs the child inside it, collects what it wrote.
fn run_rung(rung: Rung, lib_dir: &Path, doc_path: &Path) -> Result<Outcome, String> {
    let out_path = std::env::temp_dir().join(format!("tpdf-win-sandbox-{}.bin", rung.name()));
    let _ = std::fs::remove_file(&out_path);

    // Both handles are opened *here*, with this process's authority, and made
    // inheritable. The child never sees a path it could open --- which is the
    // point, since at the top of the ladder it could not open one.
    let doc_file = std::fs::File::open(doc_path).map_err(|e| format!("opening document: {e}"))?;
    let out_file = std::fs::File::create(&out_path).map_err(|e| format!("creating output: {e}"))?;
    let doc_handle = doc_file.as_raw_handle() as HANDLE;
    let out_handle = out_file.as_raw_handle() as HANDLE;
    make_inheritable(doc_handle)?;
    make_inheritable(out_handle)?;

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let command = quote_all(&[
        exe.to_string_lossy().into_owned(),
        CHILD_ARGV.to_owned(),
        rung.name().to_owned(),
        lib_dir.to_string_lossy().into_owned(),
        (doc_handle as usize).to_string(),
        (out_handle as usize).to_string(),
        std::process::id().to_string(),
        doc_path.to_string_lossy().into_owned(),
    ]);

    let job = if rung.wants_job() {
        Some(Job::create(rung == Rung::UiLimits)?)
    } else {
        None
    };

    // The text the child is asked to read, put there with this process's
    // authority; a failure is recorded rather than fatal, since only the two
    // clipboard rows depend on it.
    let marked = set_clipboard_text(&parent_marker(rung));

    let spawned = spawn_child(&command, rung, job.as_ref());
    // Dropped before reading: the child holds its own copies, and the parent's
    // write handle shares a file pointer with the child's, so reading through it
    // would read from wherever the child left off.
    drop(doc_file);
    drop(out_file);

    let (exit_code, spawn_error) = match spawned {
        Ok(code) => (code, None),
        Err(message) => (u32::MAX, Some(message)),
    };

    let (mut report, pixels) = read_output(&out_path);
    // What the child's write left, read from outside it once it is gone.
    let seen = match (&marked, clipboard_text()) {
        (Err(why), _) => format!("not tested: the parent could not set it ({why})"),
        (Ok(()), Ok(Some(text))) if text == child_marker(rung, std::process::id()) => {
            "allowed".to_string()
        }
        (Ok(()), Ok(Some(text))) if text == parent_marker(rung) => "denied".to_string(),
        (Ok(()), Ok(Some(_))) => "other text: another program wrote".to_string(),
        (Ok(()), Ok(None)) => "emptied".to_string(),
        (Ok(()), Err(why)) => format!("unreadable here ({why})"),
    };
    if let Some(authority) = report
        .as_mut()
        .and_then(|r| r.get_mut("authority"))
        .and_then(|a| a.as_object_mut())
    {
        authority.insert("clipboard_write_seen".into(), seen.into());
    }
    Ok(Outcome {
        rung,
        report,
        pixels,
        exit_code,
        spawn_error,
    })
}

/// Reads back `[u32 len][json][pixels]`, tolerating every way it can be absent.
///
/// A child that died before writing leaves an empty file; one that died midway
/// leaves a truncated one. Both are results, not errors --- at the top of the
/// ladder they are the *expected* result --- so this reports what it found
/// rather than failing.
fn read_output(path: &Path) -> (Option<serde_json::Value>, Vec<u8>) {
    let Ok(raw) = std::fs::read(path) else {
        return (None, Vec::new());
    };
    if raw.len() < 4 {
        return (None, Vec::new());
    }
    let len = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let Some(json_bytes) = raw.get(4..4 + len) else {
        return (None, Vec::new());
    };
    let report = serde_json::from_slice(json_bytes).ok();
    let pixels = raw.get(4 + len..).unwrap_or_default().to_vec();
    (report, pixels)
}

/// Prints the table and decides the exit code.
fn report(outcomes: &[Outcome], reference: &[u8]) -> Result<i32, String> {
    println!("rung        rendered  identical  exit                             containment");
    println!(
        "----------  --------  ---------  -------------------------------  \
         -----------------------------------"
    );
    for o in outcomes {
        let rendered = !o.pixels.is_empty();
        let identical = rendered && o.pixels == reference;
        let exit = if o.spawn_error.is_some() {
            "no spawn".to_owned()
        } else {
            describe_exit(o.exit_code)
        };
        println!(
            "{:<10}  {:<8}  {:<9}  {:<31}  {}",
            o.rung.name(),
            if rendered { "yes" } else { "no" },
            if !rendered {
                "-"
            } else if identical {
                "yes"
            } else {
                "NO"
            },
            exit,
            o.rung.blurb()
        );
    }
    println!();

    for o in outcomes {
        println!("{} --- {}", o.rung.name(), o.rung.blurb());
        if let Some(err) = &o.spawn_error {
            println!("  spawn failed: {err}");
        }
        match &o.report {
            Some(report) => {
                if let Some(error) = report.get("error").and_then(|v| v.as_str()) {
                    println!("  child error: {error}");
                }
                if let Some(auth) = report.get("authority").and_then(|v| v.as_object()) {
                    for (probe, verdict) in auth {
                        println!("  {probe:<16} {}", verdict.as_str().unwrap_or("?"));
                    }
                }
            }
            None if o.spawn_error.is_none() => println!(
                "  no report: the child wrote nothing and exited {}",
                describe_exit(o.exit_code)
            ),
            None => println!("  no report: the child never started"),
        }
        if !o.pixels.is_empty() {
            let same = o.pixels == reference;
            println!(
                "  pixels           {} bytes, {} non-white, {}",
                o.pixels.len(),
                non_white(&o.pixels),
                if same {
                    "identical to reference".to_owned()
                } else {
                    format!(
                        "DIFFERS from reference in {} bytes",
                        diff_count(&o.pixels, reference)
                    )
                }
            );
        }
        println!();
    }

    // The control decides whether anything else can be believed, so it is
    // checked first and separately.
    let bare = outcomes
        .iter()
        .find(|o| o.rung == Rung::Bare)
        .ok_or("no control row")?;
    if bare.pixels.is_empty() || bare.pixels != reference {
        println!("[FAIL] the uncontained control did not reproduce the in-process render.");
        println!("       Nothing below it means anything: the harness is wrong, not the sandbox.");
        return Ok(1);
    }
    println!(
        "[OK]   control reproduces the in-process render, so the rows above are about containment"
    );

    let highest = outcomes
        .iter()
        .rfind(|o| !o.rung.is_diagnostic() && !o.pixels.is_empty() && o.pixels == reference);
    match highest {
        Some(o) if o.rung == Rung::Bare => {
            println!("[WARN] no containment rung rendered correctly --- a Windows worker needs a different design");
            Ok(0)
        }
        Some(o) => {
            println!(
                "[OK]   highest rung that renders identically: {} ({})",
                o.rung.name(),
                o.rung.blurb()
            );
            Ok(0)
        }
        None => {
            println!("[FAIL] not even the control rendered");
            Ok(1)
        }
    }
}

// ---------------------------------------------------------------------------
// Child
// ---------------------------------------------------------------------------

/// Runs contained, writes `[u32 len][json][pixels]` to the inherited handle.
fn child(rest: &[String]) -> ! {
    let out_handle = rest
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    // SAFETY: the parent marked this handle inheritable before `CreateProcess`,
    // so the value is live in this process and owned by nothing else here.
    let mut out = unsafe { std::fs::File::from_raw_handle(out_handle as *mut c_void) };

    let (report, pixels) = match run_child(rest) {
        Ok((report, pixels)) => (report, pixels),
        Err(message) => (
            serde_json::json!({ "ok": false, "error": message }),
            Vec::new(),
        ),
    };

    let json = serde_json::to_vec(&report).unwrap_or_default();
    let len = u32::try_from(json.len()).unwrap_or(0);
    let _ = out.write_all(&len.to_le_bytes());
    let _ = out.write_all(&json);
    let _ = out.write_all(&pixels);
    let _ = out.flush();
    // Explicit, for the reason `worker_child.rs` gives: a handle's exit code
    // that never reaches the process is how months of runs reported success.
    std::process::exit(0);
}

fn run_child(rest: &[String]) -> Result<(serde_json::Value, Vec<u8>), String> {
    // Every other index below is positional, so a shifted argv would silently
    // read the library dir as a handle and report a containment failure that is
    // really a harness failure. Parsing the rung is the cheapest check that the
    // alignment held, and it is the first thing that breaks if it did not.
    let rung = rest
        .first()
        .and_then(|name| Rung::parse(name))
        .ok_or("child: argv does not start with a known rung")?;
    let lib_dir = PathBuf::from(rest.get(1).ok_or("child: no library dir")?);
    let doc_handle: usize = rest
        .get(2)
        .ok_or("child: no document handle")?
        .parse()
        .map_err(|_| "child: unreadable document handle")?;
    let parent_pid: u32 = rest
        .get(4)
        .ok_or("child: no parent pid")?
        .parse()
        .map_err(|_| "child: unreadable parent pid")?;
    let doc_path = rest.get(5).cloned().unwrap_or_default();

    // SAFETY: inherited from the parent, as above.
    let mut doc_file = unsafe { std::fs::File::from_raw_handle(doc_handle as *mut c_void) };
    let mut bytes = Vec::new();
    doc_file
        .read_to_end(&mut bytes)
        .map_err(|e| format!("child: reading the inherited document handle: {e}"))?;

    // Run the authority probes *before* rendering. If PDFium is going to die
    // under this rung, the interesting half of the report is already written.
    let authority = probe_authority(&doc_path, parent_pid);

    let pixels = render_bytes(&lib_dir, bytes)?;
    Ok((
        serde_json::json!({
            "ok": true,
            "rung": rung.name(),
            "authority": authority,
            "bytes": pixels.len(),
        }),
        pixels,
    ))
}

/// What this process can still reach. Three probes, chosen to disagree.
///
/// `read_path` is here because it is the one that should *stay* allowed at the
/// `lowil` rung, and saying so is the point: an integrity level governs writes,
/// not reads, so a low-IL worker can still read every document on the disk. A
/// report showing all three denied together would mean the rung is stronger than
/// it is; a report showing `read_path` allowed while the others are denied is
/// the honest shape of what an integrity level buys.
fn probe_authority(doc_path: &str, parent_pid: u32) -> serde_json::Value {
    let write_home = {
        let dir = std::env::var("USERPROFILE").unwrap_or_default();
        let target = Path::new(&dir).join(format!("tpdf-probe-{}.tmp", std::process::id()));
        match std::fs::write(&target, b"x") {
            Ok(()) => {
                let _ = std::fs::remove_file(&target);
                "allowed"
            }
            Err(_) => "denied",
        }
    };

    let read_path = if doc_path.is_empty() {
        "not tested"
    } else if std::fs::File::open(doc_path).is_ok() {
        "allowed"
    } else {
        "denied"
    };

    let open_parent = {
        // SAFETY: no pointers; a failure returns null and is read as denied.
        let handle = unsafe { OpenProcess(PROCESS_VM_WRITE, 0, parent_pid) };
        if handle.is_null() {
            "denied"
        } else {
            // SAFETY: a handle this call just returned.
            unsafe { CloseHandle(handle) };
            "allowed"
        }
    };

    serde_json::json!({
        "write_home": write_home,
        "read_path": read_path,
        "open_parent": open_parent,
        "commit_past_cap": commit_past_cap(),
        "second_process": second_process(),
        "clipboard_read": clipboard_read_probe(),
        "clipboard_write": clipboard_write_probe(),
    })
}

/// The text the parent leaves on the clipboard before a rung, and the text
/// that rung's child tries to leave there instead. Per rung, so that a
/// verdict cannot be a previous rung's text read back.
fn parent_marker(rung: Rung) -> String {
    format!("tpdf-probe-parent-{}-{}", rung.name(), std::process::id())
}

fn child_marker(rung: Rung, parent_pid: u32) -> String {
    format!("tpdf-probe-child-{}-{parent_pid}", rung.name())
}

/// `CF_UNICODETEXT`.
const UNICODE_TEXT: u32 = 13;

/// Opens the clipboard for `owner`, trying a few times: another program can
/// hold it for a moment, and a busy clipboard is not a denial.
fn open_clipboard(owner: *mut c_void) -> Result<(), u32> {
    use windows_sys::Win32::System::DataExchange::OpenClipboard;
    let mut last = 0;
    for _ in 0..25 {
        // SAFETY: a window this process made, or null.
        if unsafe { OpenClipboard(owner) } != 0 {
            return Ok(());
        }
        // SAFETY: read straight after the call that set it.
        last = unsafe { GetLastError() };
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(last)
}

/// The clipboard's text, or the error that stopped reading it. `Ok(None)` is
/// a clipboard with no text on it.
fn clipboard_text() -> Result<Option<String>, String> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    open_clipboard(std::ptr::null_mut()).map_err(|e| format!("open: error {e}"))?;
    // SAFETY: the clipboard is open on this thread; the handle it gives is the
    // system's, locked for the copy and unlocked before it is closed.
    let text = unsafe {
        let handle = GetClipboardData(UNICODE_TEXT);
        if handle.is_null() {
            let error = GetLastError();
            CloseClipboard();
            // 1168, `ERROR_NOT_FOUND`: open, and no text on it.
            return if error == 0 || error == 1168 {
                Ok(None)
            } else {
                Err(format!("get: error {error}"))
            };
        }
        let at = GlobalLock(handle) as *const u16;
        if at.is_null() {
            let error = GetLastError();
            CloseClipboard();
            return Err(format!("lock: error {error}"));
        }
        let units = GlobalSize(handle) / 2;
        let all = std::slice::from_raw_parts(at, units);
        let end = all.iter().position(|unit| *unit == 0).unwrap_or(units);
        let text = String::from_utf16_lossy(&all[..end]);
        GlobalUnlock(handle);
        text
    };
    // SAFETY: opened above, on this thread.
    unsafe { CloseClipboard() };
    Ok(Some(text))
}

/// Puts `text` on the clipboard, owned by a message-only window made for the
/// purpose: with no owner, `EmptyClipboard` leaves the clipboard ownerless
/// and `SetClipboardData` then fails (`pagecopy.rs`).
fn set_clipboard_text(text: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow};

    let class: Vec<u16> = OsStr::new("STATIC").encode_wide().chain([0]).collect();
    // `HWND_MESSAGE`: a window that is never shown and receives no broadcast.
    let message_only = -3isize as *mut c_void;
    // SAFETY: a system class, no extra data, destroyed below.
    let owner = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            message_only,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if owner.is_null() {
        // SAFETY: read straight after the call that set it.
        return Err(format!("window: error {}", unsafe { GetLastError() }));
    }
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let result = (|| {
        open_clipboard(owner).map_err(|e| format!("open: error {e}"))?;
        // SAFETY: the clipboard is open on this thread and owned by `owner`.
        // The memory is the system's once `SetClipboardData` accepts it and
        // freed here otherwise.
        unsafe {
            if EmptyClipboard() == 0 {
                let error = GetLastError();
                CloseClipboard();
                return Err(format!("empty: error {error}"));
            }
            let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
            if memory.is_null() {
                CloseClipboard();
                return Err("no memory".to_string());
            }
            let at = GlobalLock(memory) as *mut u16;
            std::ptr::copy_nonoverlapping(units.as_ptr(), at, units.len());
            GlobalUnlock(memory);
            let set = SetClipboardData(UNICODE_TEXT, memory);
            let error = GetLastError();
            CloseClipboard();
            if set.is_null() {
                GlobalFree(memory);
                // `EmptyClipboard` succeeded to get here, which is worth
                // saying: emptying is a write the reader can notice.
                return Err(format!("emptied it, then set: error {error}"));
            }
        }
        Ok(())
    })();
    // SAFETY: the window made above.
    unsafe { DestroyWindow(owner) };
    result
}

/// Whether this process can read what the parent put on the clipboard.
///
/// Read against the marker rather than for any text at all: a clipboard that
/// answers with something else (another program's copy landing between the
/// two) is said as that, not as access.
fn clipboard_read_probe() -> String {
    let Some(rung) = std::env::args().nth(2).and_then(|name| Rung::parse(&name)) else {
        return "not tested: no rung".into();
    };
    let expected = parent_marker_for_child(rung);
    match clipboard_text() {
        Ok(Some(text)) if text == expected => "allowed".into(),
        Ok(Some(_)) => "read other text".into(),
        Ok(None) => "no text".into(),
        Err(why) => format!("denied ({why})"),
    }
}

/// The parent's marker as the child computes it: the parent's pid is the
/// child's fifth argument.
fn parent_marker_for_child(rung: Rung) -> String {
    let parent_pid = std::env::args().nth(6).unwrap_or_default();
    format!("tpdf-probe-parent-{}-{parent_pid}", rung.name())
}

/// Whether this process could put text on the clipboard, as it says. The
/// parent reads the clipboard back after the child is gone and records what
/// it found as `clipboard_write_seen`, which is the verdict that counts.
fn clipboard_write_probe() -> String {
    let Some(rung) = std::env::args().nth(2).and_then(|name| Rung::parse(&name)) else {
        return "not tested: no rung".into();
    };
    let parent_pid: u32 = std::env::args()
        .nth(6)
        .and_then(|pid| pid.parse().ok())
        .unwrap_or(0);
    match set_clipboard_text(&child_marker(rung, parent_pid)) {
        Ok(()) => "allowed".into(),
        Err(why) => format!("denied ({why})"),
    }
}

/// Whether this process can commit more memory than the job object allows.
///
/// **The two probes below exist because the table at the top of this file listed
/// three things the `job` rung stops --- "a bomb, a fork bomb, an orphan" --- and
/// only ever tested none of them.** `write_home`, `read_path` and `open_parent`
/// are all *integrity level* properties, so every row of the ladder was reporting
/// on `lowil` and above while the job's own two limits, set thirty lines below in
/// `Job::create`, went unexercised. `AGENTS.md` calls this the shape that matters:
/// a documented guarantee with no check behind it reads as measured. It was the
/// stated answer for two of `worker-bench`'s POSIX modes on Windows (`limits` and
/// `footprint`), which made it load-bearing rather than decorative.
///
/// **Commit, not touch, and that is the interesting half.** The macOS balloon has
/// to write to every page it takes, because the bound there is on *resident*
/// memory and an untouched allocation is invisible to it. `JOB_OBJECT_LIMIT_-`
/// `PROCESS_MEMORY` bounds **committed** memory, which the kernel charges at
/// `VirtualAlloc` time --- so a single `MEM_COMMIT` past the cap is refused before
/// a byte of it exists. Windows therefore stops a decompression bomb one step
/// earlier than macOS can, and the probe is correspondingly cheaper and safer: it
/// never makes the machine find the pages.
///
/// Non-fatal by construction, which is what makes it reportable at all. A failed
/// commit returns null; it does not raise, so the child lives to say so. Had this
/// been written as a Rust allocation it would have hit `handle_alloc_error` and
/// aborted, and the row would have read "no report: the child exited ..." --- a
/// dead child cannot distinguish a working limit from a broken probe.
fn commit_past_cap() -> String {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };

    // Twice the cap, so the request is unambiguous rather than marginal: the
    // child already holds the document and PDFium, and a request of exactly the
    // cap could fail for that reason instead of because of the limit.
    let want = JOB_MEMORY_CAP * 2;
    // SAFETY: a reserve-and-commit of a non-zero size at an address of the
    // kernel's choosing. Null on failure is the documented result and the answer
    // being asked for.
    let at = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            want,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if at.is_null() {
        return format!("denied ({})", std::io::Error::last_os_error());
    }
    // SAFETY: releasing exactly the reservation just returned; `MEM_RELEASE`
    // requires a zero size and the base address, which is what is passed.
    unsafe { VirtualFree(at, 0, MEM_RELEASE) };
    format!("ALLOWED ({} MB committed)", want / 1024 / 1024)
}

/// Whether this process can start another one.
///
/// `ActiveProcessLimit = 1` is the fork-bomb half of the job, and the same
/// argument as above applies: a render worker has no business spawning anything,
/// so a limit that turns out not to apply would matter and nothing was asking.
///
/// The error is reported rather than reduced to allowed/denied, because this probe
/// can be refused for two unrelated reasons and only one of them is the job's.
/// `ERROR_NOT_ENOUGH_QUOTA` (1816) is the limit doing its work; an access denial
/// would be the integrity level, which is a different claim on a different rung.
/// The ladder exists precisely so that one failing row can name its ingredient.
///
/// Cleaned up on the path where it succeeds --- at the `bare` control it will ---
/// so the probe cannot leave a stray process behind. `ping` exits on its own in
/// milliseconds and needs no console; see `workers.rs` for why `timeout.exe` is
/// not usable as a stand-in here.
fn second_process() -> String {
    match std::process::Command::new("ping.exe")
        .args(["-n", "1", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            let _ = child.wait();
            "ALLOWED".to_owned()
        }
        Err(e) => format!("denied ({e})"),
    }
}

// ---------------------------------------------------------------------------
// Win32
// ---------------------------------------------------------------------------

/// A job object, closed on drop --- which with `KILL_ON_JOB_CLOSE` is also what
/// guarantees no child outlives this probe.
struct Job(HANDLE);

impl Job {
    /// `ui_limits` adds every `JOB_OBJECT_UILIMIT_*` restriction, the set the
    /// production `Job::create` applies.
    fn create(ui_limits: bool) -> Result<Self, String> {
        // SAFETY: both arguments are optional and null is the documented default.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(format!("CreateJobObject failed: {}", last_error()));
        }
        let job = Job(handle);

        // SAFETY: a zeroed struct is the documented starting point; every field
        // read below is one this sets.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        info.BasicLimitInformation.ActiveProcessLimit = 1;
        info.ProcessMemoryLimit = JOB_MEMORY_CAP;

        // SAFETY: the struct outlives the call and its length is its own size.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .unwrap_or(0),
            )
        };
        if ok == 0 {
            return Err(format!("SetInformationJobObject failed: {}", last_error()));
        }
        if ui_limits {
            use windows_sys::Win32::System::JobObjects::{
                JobObjectBasicUIRestrictions, JOBOBJECT_BASIC_UI_RESTRICTIONS,
                JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS,
                JOB_OBJECT_UILIMIT_EXITWINDOWS, JOB_OBJECT_UILIMIT_GLOBALATOMS,
                JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
                JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
            };
            let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                    | JOB_OBJECT_UILIMIT_EXITWINDOWS
                    | JOB_OBJECT_UILIMIT_GLOBALATOMS
                    | JOB_OBJECT_UILIMIT_HANDLES
                    | JOB_OBJECT_UILIMIT_READCLIPBOARD
                    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
            };
            // SAFETY: as above.
            let ok = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectBasicUIRestrictions,
                    std::ptr::from_ref(&ui).cast(),
                    u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>())
                        .unwrap_or(0),
                )
            };
            if ok == 0 {
                return Err(format!("UI restrictions refused: {}", last_error()));
            }
        }
        Ok(job)
    }

    fn assign(&self, process: HANDLE) -> Result<(), String> {
        // SAFETY: both handles are live and owned here.
        let ok = unsafe { AssignProcessToJobObject(self.0, process) };
        if ok == 0 {
            return Err(format!("AssignProcessToJobObject failed: {}", last_error()));
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: created by `CreateJobObjectW` and not closed elsewhere.
        unsafe { CloseHandle(self.0) };
    }
}

/// Spawns the child suspended, contains it, resumes it, waits, returns its code.
///
/// **Suspended, then assigned, then resumed.** Assigning a job to a process that
/// is already running is a race the process can win --- it could spawn a
/// grandchild, or allocate past the cap, before the limit exists. `AGENTS.md`
/// records a documented count that was one sample of a race making an honest run
/// look like a defect; this is the same hazard one layer down, and the fix is to
/// not have a window at all.
fn spawn_child(command: &str, rung: Rung, job: Option<&Job>) -> Result<u32, String> {
    let mut cmdline: Vec<u16> = OsStr::new(command).encode_wide().chain(Some(0)).collect();
    // SAFETY: zeroed is the documented initial state; `cb` is set below.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = u32::try_from(std::mem::size_of::<STARTUPINFOW>()).unwrap_or(0);
    // SAFETY: overwritten wholesale by a successful CreateProcess.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let token = match rung {
        Rung::Bare | Rung::Job => None,
        Rung::LowIl | Rung::UiLimits => Some(low_integrity_token()?),
        // The two diagnostics differ from `restricted` in exactly one ingredient
        // each, which is the only way to attribute a spawn failure to one of them.
        Rung::NoPrivs => Some(restricted_token(DISABLE_MAX_PRIVILEGE, false)?),
        Rung::SidOnly => Some(restricted_token(0, true)?),
        Rung::Restricted => Some(restricted_token(DISABLE_MAX_PRIVILEGE, true)?),
    };

    // Inheriting *all* inheritable handles rather than an explicit list. A real
    // worker must use PROC_THREAD_ATTRIBUTE_HANDLE_LIST --- this process has
    // other inheritable handles and a hostile child should get none of them ---
    // but that is a hardening detail of the worker, not of the question being
    // asked here, and pretending otherwise would put untested code in the probe.
    let created = match token {
        None => unsafe {
            // SAFETY: cmdline is a live NUL-terminated buffer; the two structs
            // outlive the call.
            CreateProcessW(
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_SUSPENDED,
                std::ptr::null(),
                std::ptr::null(),
                &raw const startup,
                &raw mut info,
            )
        },
        Some(token) => unsafe {
            // SAFETY: as above, plus a token this process created.
            CreateProcessAsUserW(
                token,
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_SUSPENDED,
                std::ptr::null(),
                std::ptr::null(),
                &raw const startup,
                &raw mut info,
            )
        },
    };
    if let Some(token) = token {
        // SAFETY: created here, and CreateProcess has taken its own reference.
        unsafe { CloseHandle(token) };
    }
    if created == 0 {
        return Err(format!("CreateProcess failed: {}", last_error()));
    }

    let guard = ProcessHandles(info.hProcess, info.hThread);
    if let Some(job) = job {
        job.assign(guard.0)?;
    }
    // SAFETY: a live suspended thread handle.
    if unsafe { ResumeThread(guard.1) } == u32::MAX {
        return Err(format!("ResumeThread failed: {}", last_error()));
    }
    // SAFETY: a live process handle.
    unsafe { WaitForSingleObject(guard.0, INFINITE) };
    let mut code: u32 = 0;
    // SAFETY: as above; `code` outlives the call.
    unsafe { GetExitCodeProcess(guard.0, &raw mut code) };
    Ok(code)
}

/// Closes the pair `CreateProcess` hands back, including on the error paths.
struct ProcessHandles(HANDLE, HANDLE);

impl Drop for ProcessHandles {
    fn drop(&mut self) {
        // SAFETY: both were produced by CreateProcess and are closed once.
        unsafe {
            CloseHandle(self.1);
            CloseHandle(self.0);
        }
    }
}

/// A copy of this process's token, lowered to low integrity.
fn low_integrity_token() -> Result<HANDLE, String> {
    let token = duplicate_own_token()?;
    let sid = Sid::allocate([0, 0, 0, 0, 0, 16], SECURITY_MANDATORY_LOW_RID)?;

    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.0,
            // `windows-sys` types this one `i32` while the field is `u32`; the
            // bit pattern is the same 0x20 either way.
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    // The documented length is the struct plus the SID it points at, not the
    // struct alone --- the SID is variable-length and lives outside it.
    // SAFETY: `sid` is a live SID allocated above.
    let length =
        std::mem::size_of::<TOKEN_MANDATORY_LABEL>() + unsafe { GetLengthSid(sid.0) } as usize;
    // SAFETY: `label` outlives the call and `length` describes it.
    let ok = unsafe {
        SetTokenInformation(
            token,
            TokenIntegrityLevel,
            std::ptr::from_ref(&label).cast(),
            u32::try_from(length).unwrap_or(0),
        )
    };
    if ok == 0 {
        let error = last_error();
        // SAFETY: created by `duplicate_own_token`, not yet handed anywhere.
        unsafe { CloseHandle(token) };
        return Err(format!("SetTokenInformation(integrity) failed: {error}"));
    }
    Ok(token)
}

/// A restricted copy of this process's token: no privileges, `RESTRICTED` SID.
///
/// `flags` and `restrict` are parameters rather than constants because the first
/// attempt at this rung failed to spawn with `ERROR_PRIVILEGE_NOT_HELD` and one
/// attempt cannot say which ingredient caused it. The caller sweeps the
/// combinations; publishing "restricted tokens do not work on Windows" off a
/// single derivation would be exactly the shape `docs/TRAPS.md` records under
/// *a list of documented blockers can be wrong in the direction that looks
/// thorough*.
fn restricted_token(flags: u32, restrict: bool) -> Result<HANDLE, String> {
    // Chromium's order, which is not the obvious one: restrict the process token
    // *first* and duplicate the result, rather than duplicating and then
    // restricting. `CreateProcessAsUser` waives `SE_ASSIGNPRIMARYTOKEN_NAME`
    // only for a token it recognises as a restricted version of the caller's
    // own, and whether a duplicate still qualifies is precisely what the first
    // attempt got wrong.
    let mut own: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle to self; `own` outlives the call.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut own) };
    if ok == 0 {
        return Err(format!("OpenProcessToken failed: {}", last_error()));
    }

    let sid = Sid::allocate([0, 0, 0, 0, 0, 5], SECURITY_RESTRICTED_CODE_RID)?;
    let entry = SID_AND_ATTRIBUTES {
        Sid: sid.0,
        Attributes: 0,
    };
    let (count, list) = if restrict {
        (1, std::ptr::from_ref(&entry))
    } else {
        (0, std::ptr::null())
    };

    let mut restricted: HANDLE = std::ptr::null_mut();
    // SAFETY: `own` is live, `entry` outlives the call, and the count matches
    // the array given.
    let ok = unsafe {
        CreateRestrictedToken(
            own,
            flags,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            count,
            list,
            &raw mut restricted,
        )
    };
    // SAFETY: CreateRestrictedToken does not consume it.
    unsafe { CloseHandle(own) };
    if ok == 0 {
        return Err(format!("CreateRestrictedToken failed: {}", last_error()));
    }

    // The restricted token inherits the type of the token it came from, which is
    // already primary here --- duplicated anyway so the handle carries the access
    // rights `CreateProcessAsUser` wants rather than whatever survived.
    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: `restricted` is live; `dup` outlives the call.
    let ok = unsafe {
        DuplicateTokenEx(
            restricted,
            TOKEN_ALL_ACCESS,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &raw mut dup,
        )
    };
    // SAFETY: duplicated above.
    unsafe { CloseHandle(restricted) };
    if ok == 0 {
        return Err(format!("DuplicateTokenEx failed: {}", last_error()));
    }
    Ok(dup)
}

/// A primary-token duplicate of this process's own token.
///
/// `CreateProcessAsUser` normally needs `SE_ASSIGNPRIMARYTOKEN_NAME`, and does
/// not when the token is a restricted or lowered version of the caller's own ---
/// which is the only kind this probe makes. That is why nothing here elevates.
fn duplicate_own_token() -> Result<HANDLE, String> {
    let mut own: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle to self, and `own` outlives the call.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut own) };
    if ok == 0 {
        return Err(format!("OpenProcessToken failed: {}", last_error()));
    }
    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: `own` is live; `dup` outlives the call.
    let ok = unsafe {
        DuplicateTokenEx(
            own,
            TOKEN_ALL_ACCESS,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &raw mut dup,
        )
    };
    // SAFETY: duplicated above; the original is no longer needed.
    unsafe { CloseHandle(own) };
    if ok == 0 {
        return Err(format!("DuplicateTokenEx failed: {}", last_error()));
    }
    Ok(dup)
}

/// A SID with one sub-authority, freed on drop.
struct Sid(PSID);

impl Sid {
    fn allocate(authority: [u8; 6], rid: u32) -> Result<Self, String> {
        let authority = SID_IDENTIFIER_AUTHORITY { Value: authority };
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: `authority` outlives the call; one sub-authority is declared
        // and one is given; `sid` outlives the call.
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
        // SAFETY: allocated by `AllocateAndInitializeSid` and freed once.
        unsafe { FreeSid(self.0) };
    }
}

fn make_inheritable(handle: HANDLE) -> Result<(), String> {
    if handle == INVALID_HANDLE_VALUE {
        return Err("handle is invalid".into());
    }
    // SAFETY: a live handle owned by this process.
    let ok = unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    if ok == 0 {
        return Err(format!("SetHandleInformation failed: {}", last_error()));
    }
    Ok(())
}

fn last_error() -> String {
    // SAFETY: no arguments, no pointers.
    format!("error {}", unsafe { GetLastError() })
}

// ---------------------------------------------------------------------------
// Odds and ends
// ---------------------------------------------------------------------------

/// Quotes every argument, because paths here contain spaces on any real machine.
fn quote_all(args: &[String]) -> String {
    args.iter()
        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// How many pixels are not pure white.
///
/// A tile of the right size full of nothing passes a length check and a "did it
/// render" check identically to a correct one, and a blank page is exactly what
/// a broken font path produces. This is the cheap discriminator; the byte
/// comparison against the reference is the real one.
fn non_white(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| p[..3] != [255, 255, 255])
        .count()
}

fn diff_count(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len())
}
