//! tpdf --- the application shell, and the harness that proved it could exist.
//!
//! It began as the second of those: everything here was written to answer the
//! feasibility questions in docs/PLAN.md section 9 with numbers. Phase 0 closed
//! and the viewer now runs on the same pieces, so the file is no longer
//! throwaway --- but the spike entry points are still here, still reachable by
//! their `TPDF_*` environment variables, and are still how every number in
//! `AGENTS.md` is reproduced. Do not delete one because nothing calls it: the
//! caller is a shell command in `BUILD.md`.

pub mod annots;
pub mod ber;
pub mod content;
pub mod diag;
pub mod docgraph;
pub mod docinfo;
pub mod docmodel;
pub mod document;
pub mod edits;
pub mod encoding;
pub mod fingerprint;
pub mod images;
pub mod invert;
#[cfg(target_os = "macos")]
pub mod keylayout;
pub mod launch;
pub mod links;
pub mod menu;
pub mod merge;
pub mod objects;
pub mod ocr;
pub mod ocr_gate;
#[cfg(target_os = "macos")]
pub mod ocr_vision;
#[cfg(windows)]
pub mod ocr_windows;
pub mod ocr_worker;
pub mod outline;
pub mod pagetree;
pub mod print;
#[cfg(target_os = "macos")]
pub mod print_macos;
#[cfg(windows)]
pub mod print_win;
pub mod progressive;
mod protocol;
mod queue;
pub mod recentdocs;
pub mod redact;
pub mod render;
/// Windows containment, which is what `worker_child`'s `sandbox_init` is on the
/// other platform. Gated because job objects, integrity levels and attribute
/// lists are all Win32 with no portable counterpart.
#[cfg(windows)]
pub mod sandbox_win;
pub mod save;
pub mod search;
pub mod session;
pub mod startup;
pub mod structure;
pub mod sweep;
pub mod text;
pub mod verify;

/// Helpers shared by this crate's own tests. Not compiled into any binary.
#[cfg(test)]
mod testutil;
pub mod textbox;
pub mod worker;
pub mod xmp;
// The four modules `worker.rs` was split into at 2,861 lines. Public, and
// re-exported by `worker` itself, so both the defining path and the path every
// caller already used resolve --- a split that renamed a path would have had to
// edit its consumers to prove it changed nothing.
pub mod worker_argv;
// The child half of the process boundary. POSIX and Windows both, since
// 2026-07-29: the mapping handover and the boundary itself are what differ, and
// each is one function with two implementations rather than a module that only
// exists on one platform. Everything between them --- the request loop, the
// queue, the render path --- was always portable and is now compiled as such,
// which is the point: a Windows worker that shared no code with the macOS one
// would be a second worker to keep correct.
pub mod worker_child;
pub mod worker_handover;
pub mod worker_proto;
pub mod worker_shm;
pub mod workers;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use render::{DocumentInfo, RenderService};
use tauri::Manager;

/// Who creates the window, and what it points at (spike 0.7).
///
/// Spike 0.2 left 142 ms warm between `main` and the setup hook unattributed.
/// Tauri creates the windows listed in `tauri.conf.json` *before* calling that
/// hook, so webview creation is inside the interval rather than after it, and no
/// mark can be placed between the two. Moving creation into the hook splits it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellMode {
    /// Tauri creates the window from the config, before the setup hook.
    Config,
    /// The setup hook creates it, so its cost gets its own line.
    Manual,
    /// As `Manual`, but pointing at a page that does the same work with no
    /// framework: no module graph, no Svelte, no `@tauri-apps/api`.
    Blank,
}

impl ShellMode {
    /// Reads `TPDF_SHELL_MODE`, defaulting to the shape the app ships with.
    fn from_env() -> Self {
        match std::env::var("TPDF_SHELL_MODE")
            .unwrap_or_default()
            .as_str()
        {
            "manual" => Self::Manual,
            "blank" => Self::Blank,
            _ => Self::Config,
        }
    }

    /// The page this variant loads.
    fn page(self) -> &'static str {
        match self {
            Self::Blank => "shell.html",
            _ => "index.html",
        }
    }
}

/// A document open that was started before the webview asked for it.
///
/// The path is known at launch --- from a file association, an argument, or
/// `TPDF_STARTUP` here --- and the shell then spends ~95 ms booting a webview
/// that cannot ask for anything. The open can run inside that interval instead
/// of after it. Holding the receiver rather than the result means the frontend
/// blocks only if it beat the render service to the finish.
///
/// The path is kept beside the receiver because this is a *speculative* answer:
/// it is the document one particular path resolved to, and handing it back to
/// whoever asks next would answer a request for file B with file A --- silently,
/// with the right page count for the wrong document. Nothing in the shipped app
/// can ask for a second path first, since the frontend opens what
/// `startup_path()` gave it; that is a precondition of the spike wiring rather
/// than a property of the command, so it is checked rather than relied on.
struct EagerOpen {
    /// What was opened, to be compared against what is asked for.
    path: PathBuf,
    /// The pending result, taken by the first matching request.
    pending: Mutex<Option<ReplyRx<DocumentInfo, progressive::Refusal>>>,
}

/// Whether page geometry should be collected lazily rather than up front.
///
/// Lazy is the default, and it is the reason the Phase 0 startup criterion is
/// met: enumerating every page of the 775-page corpus costs 86 ms on the
/// critical path to buy a scrollbar exactness the scroller estimates anyway
/// (docs/PLAN.md §4). `TPDF_EAGER_GEOMETRY` restores the walk, so the variant
/// that measurement compared against is still reachable.
fn lazy_geometry() -> bool {
    std::env::var_os("TPDF_EAGER_GEOMETRY").is_none()
}

/// The event name a document handed over later will arrive on.
///
/// Asked for rather than agreed in two places. A constant duplicated on both
/// sides fails by *silence* when the two drift --- the app keeps working, and
/// simply stops noticing documents opened while it is already running, which is
/// the half of file associations nobody tests by hand.
///
/// It has to be a separate call from `take_launch_paths`, and in that order: the
/// listener must be registered before the queue is drained, because a path
/// delivered between the drain and the listen is emitted to nobody.
#[tauri::command]
fn launch_open_event() -> &'static str {
    launch::OPEN_EVENT
}

/// The running version, so that a reader can find out which one they have.
///
/// **Nothing in the application said this until 2026-08-19, and the cost was a
/// bug report rather than a missing nicety.** A Windows reader on `26.8.4` hit
/// the defect where an app started with no console could open no document, and
/// could not tell whether the release that fixes it was the one they were
/// running --- so a two-second question became a report, a reproduction and a
/// bisect. `BUILD.md`'s release checklist has told anyone applying an update to
/// "confirm the new version in-app" since the updater landed, which means that
/// step was never performable and nothing said so.
///
/// It comes from `CARGO_PKG_VERSION` rather than from a constant of our own, so
/// it is `src-tauri/Cargo.toml` --- one of the four files a version bump has to
/// move together --- and cannot drift from what was built. Baked in at compile
/// time, so this reads nothing and can fail in no way.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Hands over documents that arrived from outside, and starts listening.
///
/// Called once by the frontend during boot. Everything queued before that ---
/// a double-click that launched the app, a path on the command line --- comes
/// back here; anything arriving afterwards is emitted on `launch::OPEN_EVENT`.
#[tauri::command]
fn take_launch_paths(launch: tauri::State<'_, launch::Launch>) -> Vec<String> {
    launch
        .take()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Where the remembered places are kept.
///
/// `TPDF_SESSION_FILE` overrides it, and every automated run sets it. Without
/// that the session check would read and overwrite whatever the person using
/// this machine was last reading --- and a check that can destroy the state it
/// is checking is not one that can be run twice.
fn session_file(app: &tauri::AppHandle) -> PathBuf {
    if let Some(override_path) = std::env::var_os("TPDF_SESSION_FILE") {
        return PathBuf::from(override_path);
    }
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("session.json")
}

/// Where the diagnostics that outlive the run are kept.
///
/// The log directory rather than the config directory beside the session, and
/// that is the one difference from [`session_file`] worth stating: this is not
/// configuration, it is a record, and both platforms have a place they expect to
/// find one --- `~/Library/Logs/<app>` and `%LOCALAPPDATA%\<app>\logs`. A user
/// asked for it over the phone will be looking there.
///
/// `TPDF_LOG_FILE` overrides it, for the same reason `TPDF_SESSION_FILE` does:
/// an automated run must be able to point this somewhere of its own rather than
/// appending to the file belonging to whoever uses this machine.
fn log_file(app: &tauri::AppHandle) -> PathBuf {
    if let Some(override_path) = std::env::var_os("TPDF_LOG_FILE") {
        return PathBuf::from(override_path);
    }
    app.path()
        .app_log_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("tpdf.log")
}

/// The subdirectory of `vendor/pdfium/` holding the *loadable* library.
///
/// Public, and that is the whole point of it existing. Every spike binary used to
/// carry its own `vendor/pdfium/lib`, which is right on macOS and wrong on
/// Windows --- and wrong in the worst way, because `lib/` genuinely exists there
/// and holds the import library, so the directory check passes and the bind fails
/// much later pointing at a path that is right there. It has now cost two
/// binaries on two separate days (`worker-probe`, then `backend-probe`), which is
/// once more than a fact should be rediscovered.
///
/// Every binary that can run on Windows now takes it. That sentence replaces a
/// count --- "four spike binaries still hardcode `lib`" --- which was nine by the
/// time anyone checked, and which is why this was rediscovered a *third* time, by
/// `text-probe` failing to bind on 2026-08-02. A number in prose is exactly what
/// nobody updates.
///
/// **And a rule in prose is not much better, which is what the fourth time
/// showed.** This paragraph used to end by naming the authority as
/// `grep -rn 'vendor/pdfium/lib' src-tauri/examples` and stating what it should
/// return: the two macOS-only binaries, `fdpass-probe` and `ocr-probe`, where
/// `lib` is simply correct. Nobody ran it. On 2026-08-25 it returned four more
/// --- `crop-probe`, `geometry-probe`, `merge-probe`, `turned-probe` --- every one
/// of them unable to bind on Windows, and `geometry-probe` was found that way:
/// it panicked with `LoadLibraryError` on a path that exists.
///
/// The authority is now `only_the_macos_spikes_hardcode_the_library_directory`,
/// which runs that comparison as a **set** on every `cargo test`. Same rule, with
/// something behind it.
pub const PDFIUM_SUBDIR: &str = if cfg!(windows) { "bin" } else { "lib" };

/// The file whose presence proves [`PDFIUM_SUBDIR`] is the right directory.
///
/// The *library*, not the directory that should contain it --- those are the same
/// question everywhere except the platform this got wrong.
pub const PDFIUM_LOADABLE: &str = if cfg!(windows) {
    "pdfium.dll"
} else if cfg!(target_os = "macos") {
    "libpdfium.dylib"
} else {
    "libpdfium.so"
};

/// Locates the Pdfium dynamic library.
///
/// In development it sits under `vendor/pdfium/` at the repo root. In a bundled
/// app it comes from `tauri.<platform>.conf.json`'s `bundle.resources`. Both are
/// tried, dev first, because `cargo tauri dev` runs from `src-tauri`.
///
/// **Two bundled candidates, because the bundlers disagree about the target
/// directory in a resource map.** Tauri's WiX template ignores a trailing-slash
/// target: measured 2026-07-31 by extracting the MSI with `msiexec /a`, which put
/// `pdfium.dll` directly under `INSTALLDIR` beside `tpdf.exe`, and the generated
/// `main.wxs` confirms it --- the component sits in `INSTALLDIR` with no
/// intermediate `<Directory>`. That is why the resource-directory root is tried.
///
/// **The macOS layout was checked from a Mac on 2026-07-31, and the expectation
/// recorded here was wrong.** `"...libpdfium.dylib": "pdfium/"` did not produce a
/// `pdfium/` directory: the bundler read the value as the target *path* and wrote
/// the dylib as a **file** named `Contents/Resources/pdfium` --- 7,732,336 bytes,
/// `Mach-O 64-bit dynamically linked shared library arm64`, the vendor copy
/// renamed. So neither bundled candidate matched, and a bundle built from this
/// repository could not parse a document at all once the dev tree was out of
/// reach. `tauri.macos.conf.json` now names the file explicitly
/// (`"pdfium/libpdfium.dylib"`), which lands it where the second candidate
/// already looked.
///
/// Two things worth keeping from that. The trailing slash is **not** a directory
/// marker on this bundler, so a map value that omits the filename is a rename and
/// not a placement; and the failure was invisible for as long as it was, because
/// the *dev* candidate is tried first and every check ran in a tree where it hits.
/// Hiding `vendor/pdfium/lib/libpdfium.dylib` is what makes the bundled branch
/// reachable, and it is the only reason this was found --- `BUILD.md`'s release
/// section makes it a step rather than an idea.
///
/// Neither candidate is a guess in the harmful direction: whichever layout a
/// platform produces, the file is found by looking for the *file*.
///
/// **The archive is not laid out the same way on both platforms.** macOS ships
/// the loadable `lib/libpdfium.dylib`; Windows ships the runtime DLL in `bin/`
/// and puts only the *import* library `pdfium.dll.lib` in `lib/`. Joining `lib`
/// unconditionally therefore did not merely miss on Windows --- it found a
/// directory that genuinely exists and holds nothing loadable, so the check
/// below passed and the bind failed much later, pointing at a path that was
/// right there. `scripts/fetch_pdfium.py` encodes the same split and its
/// docstring names this function as the one that had it wrong.
fn pdfium_library_dir(app: &tauri::AppHandle) -> PathBuf {
    let loadable = PDFIUM_LOADABLE;

    // **Debug builds only, and that is a load-path decision rather than tidiness.**
    // `CARGO_MANIFEST_DIR` is baked in at compile time, so a release built by CI
    // carries the *runner's* checkout path --- and this candidate is tried first,
    // ahead of anything inside the bundle. On a machine where that path can be
    // created by an unprivileged account, planting a library there would have
    // every installed copy of tpdf load it into its workers. The worker is
    // contained, so the planted code cannot reach the filesystem or the app
    // process; what it can do is parse every document the reader opens and lie
    // about all of it, which `docs/THREAT-MODEL.md`'s residual 8 does not cover
    // --- that one assumes a worker compromised *by* a document, not one that was
    // never ours.
    //
    // Nothing is lost in a development tree, where `debug_assertions` is on and
    // this is the candidate that hits. `BUILD.md`'s release step of hiding
    // `vendor/pdfium` stays worth doing: it is what proves the bundled branch is
    // reachable, and after this it proves it for the debug build too.
    let dev = dev_library_dir();
    let resources = app.path().resource_dir().ok();

    // The *library*, not the directory that should contain it. Those are the
    // same question everywhere except the one platform this got wrong, which is
    // precisely why the weaker check survived so long -- and it is now also what
    // lets one lookup serve two bundle layouts.
    let candidates = [
        dev,
        resources.as_ref().map(|d| d.join("pdfium")),
        resources.clone(),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.join(loadable).exists() {
            return candidate;
        }
    }

    // Nothing found. Answer with the resource directory rather than `.`, so the
    // bind error names where a bundled app was actually looking.
    resources.unwrap_or_else(|| PathBuf::from("."))
}

/// Where a parse of the reader's own document should happen.
///
/// **One statement of the rule, read by three call sites** --- the rewriting
/// save, the redaction's rewrite, and the append's read-back. All three are
/// parses of attacker-controlled bytes: the document the reader opened, or the
/// previous revision of the file just written, which is the same bytes verbatim.
/// So all three belong in a sandboxed child wherever there can be one, and the
/// question of whether there can be is `render::Backend`'s.
///
/// A platform with no sandbox still saves. Refusing would make it useless rather
/// than uncontained, which is the rule `Backend::default_here` already follows,
/// and it is not silent: `render::UNSANDBOXED_MARK` is what keeps the two runs
/// distinguishable.
///
/// Built here rather than inside the functions that take it, because choosing it
/// needs the app handle and they are reachable from `cargo test`, where there is
/// none. See `save::Outside`.
fn outside_of(app: &tauri::AppHandle, backend: render::Backend) -> Box<dyn save::Outside> {
    match backend {
        render::Backend::Worker => Box::new(save::InWorker::at(pdfium_library_dir(app))),
        render::Backend::InProcess => Box::new(save::Here),
    }
}

/// The vendored library in a development checkout, and `None` in a release build.
///
/// **Debug builds only, and that is a load-path decision rather than tidiness.**
/// `CARGO_MANIFEST_DIR` is baked in at compile time, so a release built by CI
/// carries the *runner's* checkout path --- and [`pdfium_library_dir`] tries this
/// candidate first, ahead of anything inside the bundle. On a machine where that
/// path can be created by an unprivileged account, planting a library there would
/// have every installed copy of tpdf load it into its workers. The worker is
/// contained, so the planted code cannot reach the filesystem or the app process;
/// what it can do is parse every document the reader opens and lie about all of
/// it --- which `docs/THREAT-MODEL.md`'s residual 8 does not cover, because that
/// one assumes a worker compromised *by* a document rather than one that was
/// never ours.
///
/// Nothing is lost in a development tree, where `debug_assertions` is on and this
/// is the candidate that hits. `BUILD.md`'s release step of hiding
/// `vendor/pdfium` stays worth doing: it is what proves the bundled branch is
/// reachable, and it now proves it for the debug build too.
///
/// Its own function so the decision is reachable from a test under either
/// profile --- a `#[cfg]` inside the lookup would be a claim nothing could check.
fn dev_library_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|root| root.join("vendor/pdfium").join(PDFIUM_SUBDIR))
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// Where one command's reply from the render service arrives.
///
/// The runtime's channel and not `std::sync::mpsc`, and the difference is not
/// the channel but what waiting on it costs. Every command below is an
/// `async fn`, so a blocking `recv` parks one of the runtime's few worker
/// threads for as long as the engine takes --- and nothing bounds how many at
/// once: a search walks a document one call per page, and a reader who scrolls
/// during it adds more. Awaiting suspends the *task* instead, which is the
/// resource there are millions of.
///
/// Capacity one, for one message. [`render::Reply`] is `FnOnce`, so the send in
/// [`reply_channel`] cannot find the channel full --- which is what lets it be
/// a `try_send` and never block the render thread either.
type ReplyRx<T, E = String> = tauri::async_runtime::Receiver<Result<T, E>>;

/// A reply callback to hand the render service, and where its answer lands.
///
/// Built here rather than at each call site so that the sender's half of the
/// arrangement --- the capacity, and the send that must not block --- is stated
/// once for every caller, the eager open in `start_eager_open` included.
fn reply_channel<T: Send + 'static, E: Send + 'static>() -> (render::ReplyTo<T, E>, ReplyRx<T, E>) {
    let (tx, rx) = tauri::async_runtime::channel(1);
    (
        Box::new(move |result| {
            // A dropped receiver is a command that is no longer waiting, which
            // is a reply with nowhere to go and not an error.
            let _ = tx.try_send(result);
        }),
        rx,
    )
}

/// The password that opened `doc`, or `None`.
///
/// **A key to bytes this process already holds, not a new authority.** The
/// rewrite needs it for the same reason the append does: `lopdf` parses no
/// objects at all without it, so a save that did not ask would see an empty
/// document and every check after it would agree about nothing. `save.rs`'s
/// `checked` then re-encrypts what it wrote with the state the load recorded.
/// `docs/THREAT-MODEL.md` §T6.9 carries what holding it costs.
///
/// **A failure to answer is `None` rather than a refusal**, which is
/// `save_document`'s rule and holds here for the same reason: a plain document
/// has no password to lose, and a locked one that arrives without its key is
/// refused by `checked` with a message naming the lock. What must not happen is
/// a save turned into an error because the service was busy.
///
/// One function rather than the six copies the alternative needs --- the ask is
/// three lines and `docs/TRAPS.md` records more than one defect that was a
/// second copy of a rule drifting from the first.
async fn password_for(service: &RenderService, doc: u32, command: &str) -> Option<String> {
    let (reply, rx) = reply_channel();
    service.password(doc, reply);
    await_reply(command, rx).await.unwrap_or(None)
}

/// Waits for the render service's answer to `command`.
///
/// `command` is a parameter because the failure is otherwise indistinguishable
/// across every caller: all of them see the render thread gone, and a persisted
/// `render thread stopped` (see `diag.rs`) then says a thread died without
/// saying what was being asked of it. The name is the one piece a reader sending
/// the log back cannot supply.
///
/// (This paragraph spent three weeks attached to [`password_for`]. Two `///`
/// runs with no blank line between them are **one** comment in Rust, so it
/// documented the function below it and left this one with nothing --- and
/// nothing goes red, because both halves still compile and rustdoc renders a
/// perfectly good page about the wrong item. The `docs` gate's Rust exemption
/// argued that Rust cannot lose a doc comment this way, which is true and is not
/// the failure: it misattributes one.)
async fn await_reply<T, E>(command: &str, mut rx: ReplyRx<T, E>) -> Result<T, E>
where
    E: From<String>,
{
    rx.recv()
        .await
        .ok_or_else(|| E::from(format!("render thread stopped ({command})")))?
}

/// Opens a document and returns its page geometry.
///
/// Collects an eager open if one is outstanding, which is why this takes the
/// app handle: the pending receiver is managed state that only exists in that
/// variant.
///
/// `password` is what the reader typed after a previous call came back with
/// [`progressive::Refusal::locked`] set. It is a parameter rather than a second
/// command because opening a locked document and opening any other document
/// differ in one argument, and giving them separate entry points would give the
/// pool two ways to acquire a document to keep in step.
///
/// The refusal is structured for one reason: a locked document is not a damaged
/// one, and the frontend has to be able to ask rather than apologise.
#[tauri::command]
async fn open_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    path: String,
    password: Option<String>,
) -> Result<DocumentInfo, progressive::Refusal> {
    let wanted = PathBuf::from(&path);
    // Kept because `wanted` is moved into the open below, and the fingerprint
    // is taken after it.
    let fingerprinting = wanted.clone();
    // The receiver comes out of the lock before anything is awaited, and it has
    // to: the guard is not `Send`, so holding one across the wait below would
    // not compile.
    let eager = app
        .try_state::<EagerOpen>()
        // Only for the path it was started on. A mismatch falls through to an
        // ordinary open and leaves the eager result where it is: it costs the
        // head start, which is what a speculative optimisation is allowed to
        // lose, rather than returning the wrong document.
        .filter(|eager| eager.path == wanted)
        // And never when a password is being offered. The eager open was started
        // before anyone could type one, so its result is the locked refusal that
        // *prompted* this call --- collecting it here would answer the reader's
        // password with the failure that asked for it, forever.
        .filter(|_| password.is_none())
        .and_then(|eager| eager.pending.lock().take());
    // Both branches end at the same place on purpose. The edit model has to be
    // started for the document that was actually opened, and the eager path
    // returns a `DocumentInfo` produced before this call existed --- registering
    // it in only one of the two would leave a reader who opened a file the fast
    // way with no model and no error, which reads as "rotate does nothing".
    let info = if let Some(rx) = eager {
        startup::mark("eager open collected");
        await_reply("open_document", rx).await?
    } else {
        let (reply, rx) = reply_channel();
        service.open(wanted, lazy_geometry(), password, reply);
        await_reply("open_document", rx).await?
    };
    let pages = u32::try_from(info.page_count).map_err(|_| {
        format!(
            "a document of {} pages is past what tpdf can edit",
            info.page_count
        )
    })?;
    // The path, not the fingerprint: `edits` starts the hash on a thread and the
    // open does not wait for it. That is a measurement rather than caution ---
    // 452 ms cold for the 337 MB scan fixture, against a 300 ms cold-start
    // priority --- and the wait is moved to `Edits::plan`, which only a save or a
    // print reaches and which is about to read the whole file regardless.
    //
    // A hash that cannot be taken is recorded as "none" rather than as an error.
    // The document opens and can be read; what it cannot do is be saved over,
    // because a save with no fingerprint is refused. Fail closed, and lose the
    // smaller thing: Save a copy still works, and the original is not at risk.
    edits.open(info.id, pages, Some(fingerprinting));

    // After the open succeeded, so a file that failed to parse is not filed as a
    // document the reader had. Every route in reaches here -- a drop on the
    // window, a double-click in Explorer, a path in argv, the single-instance
    // forward and the panel -- which is why it is here rather than in the dialog
    // handler, where four of the five would have missed it.
    recentdocs::note_opened(&app, Path::new(&path));
    Ok(info)
}

/// Releases every document the backend still holds, for a webview that has just
/// started.
///
/// ## The leak this closes
///
/// `close_document` has exactly one caller in the application: `App.svelte`,
/// when a *successful* subsequent open replaces the current document. So the
/// backend's document table is owned entirely by webview state --- and a webview
/// reload resets that state to nothing while the backend keeps everything.
/// Every document opened before the reload is then unreachable, with its worker
/// pool alive, for the life of the process. `App.svelte`'s own comment names the
/// stake: *"without it a session that opens a dozen files holds a dozen sandboxed
/// children"*, and that reasoning holds only while the webview remembers.
///
/// Nothing else reclaims them. There is no timer, no backend-side owner and no
/// reference count; a document lives until somebody names its id.
///
/// ## Why "the webview started" means "nothing is referenced"
///
/// A freshly loaded page holds no document id, by construction --- ids come back
/// from `open_document`, and it has not called it yet. So every id the backend
/// holds at that moment is one nobody can name.
///
/// **That depends on there being one window, and it is worth stating rather than
/// assuming.** tpdf is single-window: a second launch is forwarded by
/// `tauri-plugin-single-instance` to the running process, which opens the file in
/// the window it already has. If tpdf ever grows a second window, this becomes
/// wrong in the worst way --- one window's startup would close the other's
/// document out from under a reader --- and the fix is a per-window table rather
/// than a guard here.
///
/// ## Why it is not silent
///
/// It answers with a count, and a non-zero one is logged. Zero is the ordinary
/// case and means the reader started the application; anything else means a
/// webview reloaded, which is a thing that happened to somebody and which nothing
/// else in the running system reports.
///
/// ## What it adds to the webview's reach: nothing
///
/// Worth stating rather than leaving to be worked out, because a command that
/// closes *every* document sounds like new authority. It is not. Anything able
/// to call this can already call `close_document` in a loop --- the ids are
/// small integers from zero --- so what this adds is one round trip, not a
/// capability. It writes no file, reads no path and touches no network, so it is
/// outside the five commands `docs/THREAT-MODEL.md` §T6.1 enumerates. The worst
/// a caller does with it is close documents, which is the same denial of service
/// already reachable.
///
/// # Errors
///
/// The render service not answering, which is the same failure any command has.
#[tauri::command]
async fn release_documents(
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
) -> Result<usize, String> {
    // Before the service call, for `close_document`'s reason and with the same
    // consequence: document numbers are reused, so a model left behind under an
    // id the service is about to hand to another file is one document's journal
    // applied to another's pages.
    let models = edits.release_all();
    let (reply, rx) = reply_channel();
    service.release_all(reply);
    let held = await_reply("release_documents", rx).await?;
    if held > 0 || models > 0 {
        // Not `[render]`: this is not a render failing, and the tag is what a
        // reader greps for. A line here means a webview reloaded and the backend
        // was holding documents nobody could reach.
        crate::diag::note(&format!(
            "[open] a new webview released {held} document(s) and {models} model(s) \
             the previous one left behind"
        ));
    }
    Ok(held)
}

/// Releases a document the reader has finished with.
///
/// Called when the window moves to another file, and it matters more than it
/// looks: under the worker backend an unreleased document is a sandboxed
/// process, not a heap allocation, so a session that opens a dozen files would
/// otherwise be holding a dozen of them.
///
/// It waits for the render service's reply rather than returning as soon as the
/// job is posted, so the promise resolving means the process is really gone and
/// a refusal has somewhere to be reported. Whether the *caller* waits on that
/// promise is its own decision, and `App.svelte` does not: nothing outstanding
/// for the outgoing document can lose its worker to this call, and the guarantee
/// belongs to `Workers::close`, which drains the pool before dropping it. Read
/// the argument there rather than a copy of it here --- holding the reader on
/// this promise would put a process teardown on the path to the first page of
/// the file they asked for, and that is the only decision this end makes.
#[tauri::command]
async fn close_document(
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<(), String> {
    // Before the service call rather than after, and not for tidiness: document
    // numbers are reused, so a model left behind under a handle the service is
    // about to hand to another file is one document's journal applied to
    // another's pages.
    edits.close(doc);
    let (reply, rx) = reply_channel();
    service.close(doc, reply);
    await_reply("close_document", rx).await
}

/// Turns one page of the working document, without touching the file.
///
/// The page is named by the identity a state reply gave it, never by its
/// position --- see `edits.rs` on why. Returns the whole edit state rather than
/// an acknowledgement, so the frontend's copy is replaced by the answer rather
/// than advanced by its own arithmetic.
///
/// Synchronous work in an `async fn`, which the note on [`print_document`] warns
/// about, and here it is right: this is a `HashMap` lookup, a journal push and a
/// walk of the page order. Nothing parses and nothing touches the disk.
#[tauri::command]
async fn page_rotate(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    turns: i8,
) -> Result<edits::EditState, String> {
    edits.rotate(doc, page, turns)
}

/// Sets or clears one page's visible box, without touching the file.
///
/// Named by identity like [`page_rotate`]. `to` is `[llx, lly, urx, ury]` in the
/// page's own space, y upwards, or absent to put the file's own box back.
///
/// **The reader sees this through PDFium and saves it through `lopdf`, and the
/// two paths never meet.** Every render and every text extraction hands the box
/// to `RawDocument::page_cropped`, which sets it on the loaded page; a save
/// writes `/CropBox` out of the plan in `save.rs`. That is a real duplication and
/// it is the reason a check comparing what is on screen with what comes back out
/// of the saved file can fail at all.
#[tauri::command]
async fn page_crop(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    to: Option<[f64; 4]>,
) -> Result<edits::EditState, String> {
    edits.crop(doc, page, to)
}

/// The box one page's ink occupies, in the page's own space, or `None` if blank.
///
/// `page` is a position in the **baseline file**, not a page id and not a slot:
/// this asks PDFium about the document on disk, which knows nothing about the
/// model's identities. The caller has the source index in the state reply.
///
/// Measured by rendering the page small and finding the bounding box of
/// everything that is not paper --- see `crate::content` for why the object
/// graph cannot answer this for a scan.
#[tauri::command]
async fn page_content_box(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
) -> Result<Option<[f64; 4]>, String> {
    let (reply, rx) = reply_channel();
    service.content(doc, page, reply);
    await_reply("page_content_box", rx).await
}

/// One page's displayed size under a crop box, or under the file's own.
///
/// The frontend lays out from this and cannot compute it: a crop is in the
/// page's own space, the layout is in display space, and the turn between them
/// is the page's `/Rotate`, which the frontend is never told.
#[tauri::command]
async fn page_geometry(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<render::CropGeometry, String> {
    let (reply, rx) = reply_channel();
    service.geometry(doc, page, crop, reply);
    await_reply("page_geometry", rx).await
}

/// The crop box a rectangle the reader dragged out would produce.
///
/// The inverse of [`page_geometry`], and it exists for the same reason: the
/// frontend has the rectangle in the file's **display** space --- which is where
/// every rectangle in the frontend lives --- and a crop box is in the page's own
/// unrotated space. Turning between them needs the page's `/Rotate`, which the
/// frontend is deliberately never told, so a second copy of the rotation table
/// there is the thing this command exists to avoid.
///
/// The answer goes straight into [`page_crop`], which is why it is in exactly
/// the coordinates [`page_content_box`] answers in: a crop the reader dragged and
/// a crop measured from the ink have to be the same kind of thing, or *Reset
/// page crop* would mean two different amounts of undoing.
#[tauri::command]
async fn page_crop_box(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    rect: [f32; 4],
) -> Result<[f32; 4], String> {
    let (reply, rx) = reply_channel();
    service.crop_box(doc, page, rect, reply);
    await_reply("page_crop_box", rx).await
}

/// What removing each of one page's marked regions would take, and what it would miss.
///
/// **The one thing the redaction review panel cannot work out for itself.** The
/// frontend knows which words a region *covers* --- it holds the character boxes
/// --- and cannot know which text-showing operations those characters belong to,
/// because that is a fact about the content stream. Route B removes a whole
/// operation when any of its glyphs is inside, so the difference between the two
/// answers is exactly the collateral a reader is reviewing for.
///
/// One call per page carrying every region on it, for `page_text`'s reason: the
/// page load and the object walk are the cost and they are per page, while the
/// comparison is per region. `regions` are in the file's display space, the
/// space the model holds a pending redaction in, and the turn into the page's
/// own space happens in the worker --- the same split [`page_crop_box`] exists
/// for.
///
/// **Nothing is removed by asking.** The answer is a count, some sentences and
/// the text those operations draw; the document is not touched, and there is no
/// command that applies one of these yet.
#[tauri::command]
async fn redaction_plans(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    regions: Vec<[f32; 4]>,
) -> Result<Vec<redact::RegionPlan>, String> {
    let (reply, rx) = reply_channel();
    service.redaction_plans(doc, page, regions, reply);
    await_reply("redaction_plans", rx).await
}

/// A redaction worked out against the open document, ready for either writer.
///
/// **The Ask step of `docs/PLAN.md` §6, held apart from the write.** Two
/// commands apply a redaction --- [`redact_copy`] to a new file and
/// [`redact_document`] over the open one --- and they differ in the writer and
/// in nothing else. A second copy of this loop is the drift this repository
/// keeps recording: the two would go on agreeing about the ordinals and
/// eventually disagree about which objects the reader was warned about.
struct Asked {
    /// The reader's plan with the redaction ordinals in it.
    plan: edits::Plan,
    /// The words the regions cover, to look for in what gets written.
    needles: Vec<String>,
    /// What the removal could not take. Not a refusal --- see [`redact_copy`]
    /// --- but a reason the file cannot be called clean, carried to the verdict.
    concerns: Vec<String>,
    /// How many regions were asked about.
    regions: usize,
    /// How many text-showing operations the removal names, after merging.
    shows: usize,
    /// What the OCR gate needs and only the source document can supply.
    ///
    /// Collected here rather than after the write because after the write it
    /// cannot be: the control the gate renders has to be no larger than the
    /// smallest box a region covered, and the removal takes exactly those boxes.
    /// See [`ocr_gate::GatePage`].
    gate: Vec<ocr_gate::GatePage>,
}

/// Works out what removing every marked region would take.
///
/// One call per page carrying regions, which is where the cost is: the page load
/// and the object walk are per page and the comparison is per region.
///
/// **Nothing is written and nothing is journalled.** The document is asked, and
/// a caller that goes no further has changed nothing.
///
/// # Errors
///
/// Nothing marked, or a worker that could not read a page.
async fn ask_redactions(
    edits: &edits::Edits,
    service: &RenderService,
    doc: u32,
) -> Result<Asked, String> {
    let targets = edits.redaction_targets(doc)?;
    if targets.is_empty() {
        return Err("nothing in this document is marked for removal".into());
    }

    let mut planned: Vec<edits::PlannedRedaction> = Vec::new();
    let mut needles: Vec<String> = Vec::new();
    let mut concerns: Vec<String> = Vec::new();
    let mut regions = 0usize;
    let mut shows_total = 0usize;
    let mut gate: Vec<ocr_gate::GatePage> = Vec::new();

    for target in targets {
        let page = target.source;
        regions += target.regions.len();
        // Kept before the move: the gate works in display space, which is the
        // space these arrived in, while `redaction_plans` converts them to the
        // page's own.
        let displayed = target.regions.clone();
        let (reply, rx) = reply_channel();
        service.redaction_plans(doc, page, target.regions, reply);
        let plans = await_reply("redaction_plans", rx).await?;

        // One text extraction per page, on the document as the reader has it.
        // `None` for the crop because `redaction_plans` uses the file's own, and
        // a word list measured from a different corner than the regions were
        // would put every control somewhere else on the page.
        //
        // A page whose text cannot be read is not a refusal: the gate reports
        // *not verified* for its regions, which is the answer either way.
        let (reply, rx) = reply_channel();
        service.text(doc, page, None, reply);
        let text = await_reply("redaction text", rx).await.ok();

        // **The arithmetic lives in `redact.rs`, and this loop is the two
        // questions it needs answered.** Everything between the replies used to
        // be written out here --- a hundred lines of merging, deduplication and
        // sentence-building inside a `#[tauri::command]`'s private helper, which
        // no test could construct and no mutation could aim at. See
        // [`redact::aggregate`].
        let one = redact::aggregate(page, displayed, plans, text.as_ref());
        concerns.extend(one.concerns);
        needles.extend(one.needles);
        shows_total += one.shows;
        gate.push(one.gate);
        planned.push(one.planned);
    }

    let mut plan = edits.plan(doc)?;
    plan.redactions = planned;
    Ok(Asked {
        plan,
        needles,
        concerns,
        regions,
        shows: shows_total,
        gate,
    })
}

/// Runs `docs/PLAN.md` §6 step 4 over a written file, off the async runtime.
///
/// [`ocr_gate::run`] blocks --- it waits on a render and on another process ---
/// and every other step of these two commands is already on a blocking thread
/// for the same reason. The service is reached through the app handle rather
/// than borrowed, because `spawn_blocking` needs what it captures to outlive the
/// call and a `tauri::State` borrow does not.
///
/// **Nothing here refuses.** A join that failed is one more reason the file
/// cannot be called clean, and [`redact::Applied`] is what carries it.
async fn gate_written_file(
    app: &tauri::AppHandle,
    path: String,
    pages: Vec<ocr_gate::GatePage>,
    password: Option<String>,
) -> Vec<String> {
    if pages.is_empty() {
        return Vec::new();
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let service = handle.state::<RenderService>();
        ocr_gate::run(&service, &path, password, &pages)
    })
    .await
    .unwrap_or_else(|e| {
        vec![format!(
            "the removed areas could not be checked, so the file cannot be shown clean: {e}"
        )]
    })
}

/// Writes a copy of the document with every marked region removed, and verifies it.
///
/// **The destructive step, pointed at a new file.** `docs/PLAN.md` §6 describes
/// apply as an in-place rewrite with the journal truncated; this is the same
/// removal written somewhere else, which is the form that can ship first because
/// nothing the reader has can be lost by it. The open document is untouched and
/// the regions stay pending, so a reader who does not like the result closes the
/// file and still has their marks.
///
/// Four steps, and the order is the safety of it:
///
/// 1. **Ask.** For each page holding regions, a worker computes what a removal
///    would take --- against PDFium's own object list, behind the sandbox, which
///    is where every parse of the reader's bytes belongs.
/// 2. **Write.** The ordinals go into the plan and `save::write_copy` takes the
///    ordinary rewrite path, which is what applies them --- see
///    `save::apply_redactions` for why it is safe for that to happen last.
/// 3. **Verify.** The written file is scanned for the words that were supposed
///    to go, and every object the removal could not take is a reason of its own.
///    The answer is *verified*, or *not verified* with every reason --- never a
///    bare success, which is §6 step 4 and is why [`redact::Applied`] cannot
///    carry the first without the second.
///
/// **An object the removal cannot take does not stop the write, and that is a
/// decision rather than an oversight.** §6's deny-by-default rule says such an
/// object is a verification failure and not a shrug, and it is honoured here as
/// a failure to *verify*: the file is written with the text gone and the reader
/// is told, in the sentence that lands afterwards, that it could not be proved
/// clean and why. Refusing instead was tried first and measured: `text-base14`'s
/// own region overlaps a path, and a rule under a line of text is what almost
/// every real document has --- so refusing means tpdf can never redact anything
/// and the reader is told the same thing with nothing to show for it. One rule,
/// *never claim clean*, beats two.
///
/// # Errors
///
/// Nothing marked; the worker refusing to read a page; anything
/// `save::write_copy` refuses (an encrypted source, a page count that disagrees
/// with the baseline, writing over the source); or the written file not being
/// readable back.
#[tauri::command]
async fn redact_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
) -> Result<redact::Applied, String> {
    let asked = ask_redactions(&edits, &service, doc).await?;
    let plan = asked.plan.clone();
    let needles = asked.needles.clone();
    let concerns = asked.concerns.clone();
    let regions = asked.regions;
    let shows_total = asked.shows;

    let out = std::path::PathBuf::from(path);
    let from = std::path::PathBuf::from(source);
    let written = out.clone();
    let out_path = out.to_string_lossy().into_owned();
    let password = password_for(&service, doc, "redact_copy").await;
    // **Kept for the two readers after the write, which need the same key.**
    // The file about to be written is re-encrypted whenever the source was, so
    // a verifier arriving without the password parses no objects at all and
    // reports having found nothing --- which is what a clean file looks like.
    // `verify::scan` refuses to call that verified, and this is what lets it
    // answer the question instead of declining it.
    let key = password.clone();
    let copied = tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(&from, &plan, &out, password.as_deref())
    })
    .await
    .map_err(|e| format!("the redaction did not run: {e}"))?
    .map_err(|why| why.message)?;

    // Read back rather than verified from what was written, which is the same
    // rule the append's own verification follows: what matters is the file on
    // disk, and the buffer that produced it agrees with itself.
    //
    // **By path, and that is a window rather than a guarantee.** `write_copy`
    // has already renamed and closed, so there is no handle left to read
    // through; anything that replaces the file between the rename and this read
    // is what gets verified, and the report would be about somebody else's
    // bytes. Closing it means `write_copy` returning its open file, which is a
    // change to every copy path in `save.rs` --- and the destination is a name
    // the reader has just chosen in a dialog, so the race needs a writer aiming
    // at it in the same second. Disclosed rather than claimed shut: the append,
    // whose destination is the reader's own open document, does hold its handle.
    let verifying = key.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        std::fs::read(&written)
            .map_err(|why| format!("the redacted file could not be read back: {why}"))
            .map(|bytes| verify::scan(&bytes, &needles, verifying.as_deref()))
    })
    .await
    .map_err(|e| format!("the verification did not run: {e}"))??;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on: a picture of the words in the region is a
    // different problem from a scan that could not decode a stream, and only the
    // first tells them the region is still readable.
    let mut why = concerns;
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Then §6 step 4, which is the only one of the two that can see a picture of
    // the words. It runs on the file that was just written, never on the source
    // --- see `ocr::RedactedPixels`, where that is a type-level rule.
    why.extend(gate_written_file(&app, out_path, asked.gate, key).await);
    Ok(redact::Applied {
        regions,
        shows: shows_total,
        changed: copied.changed,
        verified: why.is_empty(),
        why,
    })
}

/// Removes every marked region from the file the reader opened, and verifies it.
///
/// **The destructive step pointed at the reader's own file**, which is
/// `docs/PLAN.md` §6 step 3 as that section states it. [`redact_copy`] is the
/// same removal written somewhere else, and it shipped first because nothing a
/// reader has can be lost by it; this one is the operation a reader actually
/// wants, and there is no original left afterwards.
///
/// **The journal truncation §6 asks for is the close, and it is stronger than a
/// truncation.** Truncating the journal at the apply would leave every earlier
/// command undoable, which for a redaction means a reader could step back to a
/// state whose regions were still pending and wonder which file they were
/// looking at. [`save_document`]'s close spends the journal whole: the model is
/// dropped, the reader reopens from the path, and there is no undo that reaches
/// across it. Nothing here had to be built for that --- it is what an in-place
/// write already does --- and saying so is worth more than a mechanism would be.
///
/// **Always a rewrite, and that is what a redaction is rather than a choice.**
/// [`save_document`] asks `save::mode_for_source` whether a plan can be appended;
/// this does not ask, because an append adds objects and never touches a content
/// stream, so appending a redaction would write a file with every word still in
/// it. `Plan::is_appendable` refuses a plan carrying a redaction for exactly
/// that reason and has a test named for it --- so the property holds at the
/// predicate as well as here, and neither place is relying on the other.
///
/// The order is [`save_document`]'s, for [`save_document`]'s reasons: stage
/// beside the source while the document is still open and every refusal can
/// arrive harmlessly, close, then rename. What is added is the verification, and
/// it happens **after** the rename, against the file the reader now has --- the
/// same rule the copy follows, and sharper here, because the bytes on that path
/// are the only bytes left.
///
/// **A file that could not be proved clean is still the file.** §6's rule is
/// *never claim clean*, not *never write*; the removal happened, the reader is
/// told what could not be shown gone, and the alternative --- rolling back to the
/// unredacted document --- would hand them the words they asked to destroy while
/// reporting a failure. [`redact_copy`] carries the same decision and the same
/// worked reason.
///
/// # Errors
///
/// Nothing marked, a worker that could not read a page, or anything
/// `save::stage_in_place` refuses --- all of them with the document untouched.
/// Past the close, a rename that did not happen or a file that could not be read
/// back, both of which say `reopen`.
#[tauri::command]
async fn redact_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    source: String,
) -> Result<redact::Applied, SaveFailure> {
    let asked = ask_redactions(&edits, &service, doc)
        .await
        .map_err(SaveFailure::refused)?;

    let staging = source.clone();
    let plan = asked.plan.clone();
    let password = password_for(&service, doc, "redact_document").await;
    // Kept past the close for the two readers after the write --- `redact_copy`
    // says why, and here it is also the only copy of the document left. Taken
    // now rather than later because the service is closed a few lines below and
    // there is nothing to ask by then.
    let key = password.clone();
    let writing = outside_of(&app, service.backend());
    let staged = tauri::async_runtime::spawn_blocking(move || {
        save::stage_in_place(Path::new(&staging), &plan, password.as_deref(), &*writing)
    })
    .await
    .map_err(|e| SaveFailure::refused(format!("the redaction did not run: {e}")))?
    .map_err(SaveFailure::refused_by)?;

    // Past this line every failure is an `after_close`, for `save_document`'s
    // reason: the reader's document is being taken apart, and the honest thing
    // to report is that they have to open the file again.
    //
    // The model first --- document numbers are reused, and a journal left under a
    // handle the service is free to hand to another file is one document's edits
    // applied to another's pages. Here that close is also the truncation.
    edits.close(doc);
    let (reply, rx) = reply_channel();
    service.close(doc, reply);
    let closed = await_reply("redact_document", rx).await;

    let committing = source.clone();
    let needles = asked.needles.clone();
    let verifying = key.clone();
    let landed = tauri::async_runtime::spawn_blocking(move || {
        let at = Path::new(&committing);
        // One more look before the rename, closing the window staging opens.
        save::verify_before_commit(&staged, at).map_err(SaveFailure::after_close_by)?;
        save::commit_in_place(&staged.path, at).map_err(SaveFailure::after_close)?;
        // Read back rather than verified from what was written, which is the
        // rule the copy and the append both follow: what matters is the file on
        // disk, and the buffer that produced it agrees with itself.
        std::fs::read(at)
            .map_err(|why| {
                SaveFailure::after_close(format!(
                    "the file was written but could not be read back to check it: {why}"
                ))
            })
            .map(|bytes| verify::scan(&bytes, &needles, verifying.as_deref()))
    })
    .await
    .map_err(|e| SaveFailure::after_close(format!("the redaction did not finish: {e}")))?;

    let report = landed.map_err(|why| with_close_note(why, closed))?;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on --- see `redact_copy`, which orders them the
    // same way and for the same reason.
    let mut why = asked.concerns.clone();
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Then §6 step 4, against the reader's own file --- which is now the only
    // copy, so this is the sharper of the two places it runs.
    why.extend(gate_written_file(&app, source, asked.gate, key).await);
    Ok(redact::Applied {
        regions: asked.regions,
        shows: asked.shows,
        // The source *is* the file being written, so the copy's question --- has
        // the document changed under the one on screen --- is asked and answered
        // by `stage_in_place`, which refuses rather than reporting. Reaching here
        // means it had not.
        changed: false,
        verified: why.is_empty(),
        why,
    })
}

/// Removes one page from the working document, without touching the file.
///
/// Named by identity like [`page_rotate`], and here that is not a nicety: the
/// reply this id came from may already be one state behind, and a position would
/// then delete whichever page had moved into that slot. An id cannot mean the
/// wrong page --- it either names a live one, a deleted one, or nothing, and the
/// model tells the three apart.
#[tauri::command]
async fn page_delete(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
) -> Result<edits::EditState, String> {
    edits.delete(doc, page)
}

/// Moves one page of the working document, without touching the file.
///
/// `after` is the id of the page the moved one should end up behind, and `null`
/// means the front. Both ends are identities, for the reason [`page_delete`]
/// gives twice over: a destination *index* would be read against an order the
/// frontend may no longer have, and the page would land beside whatever had
/// taken that position.
#[tauri::command]
async fn page_move(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    after: Option<u64>,
) -> Result<edits::EditState, String> {
    edits.move_page(doc, page, after)
}

/// Puts a new blank page into the working document, without touching the file.
///
/// `after` is the id of the page it should sit behind, and `null` means the
/// front --- both ends identities, for the reason [`page_move`] gives.
///
/// `size` is `[width, height]` in points and comes from the frontend because
/// that is the side holding the page the reader is looking at. **A default here
/// would be wrong for one continent or the other**, and worse, wrong invisibly:
/// a letter-size blank in an A4 document lays out and prints without complaint.
#[tauri::command]
async fn page_insert(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    after: Option<u64>,
    size: [f64; 2],
) -> Result<edits::EditState, String> {
    edits.insert(doc, after, size)
}

/// Puts a mark on a page, over the rectangles a reader dragged across.
///
/// **One command for all three kinds rather than three commands.** The kind is
/// a field on [`edits::NewMark`], so a highlight, an underline and a strikeout
/// travel one path, are refused by one set of preconditions and are written by
/// one writer. Three commands would be three chances for the fourth kind to
/// reach only two of them. It was called `annot_highlight` while there was only
/// one kind; the name is part of the wire format, so renaming it is a protocol
/// change and is done here rather than left to read wrongly.
///
/// The page is named by identity, as [`page_rotate`] names it, and for the
/// sharper version of the same reason: a mark is placed by *coordinates*, so a
/// stale position would put a reader's highlight on a different page at the
/// place the words used to be.
///
/// **The timestamp is taken here and cannot be sent.** `edits::NewMark` has no
/// field for it: what a mark claims about when it was made is the application's
/// statement, not the frontend's, and a `made` on the wire would be one more
/// attacker-controlled string in a file tpdf signs its name to.
///
/// Synchronous work in an `async fn`, which is right for the same reason it is
/// right in [`page_rotate`]: a lock, a journal push and a page walk. The
/// coordinate mapping and the writing happen at save time, not here.
#[tauri::command]
async fn annot_mark(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: edits::NewMark,
) -> Result<edits::EditState, String> {
    edits.annotate(doc, mark, save::pdf_date(std::time::SystemTime::now()))
}

/// Takes one mark off the page it is on.
///
/// `sweep` names the gesture this belongs to, or is zero for a removal that
/// stands alone --- see [`edits::Edits::erase`]. One sweep of the eraser can
/// take several whole marks, and they go back together.
#[tauri::command]
async fn annot_remove(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    sweep: u64,
) -> Result<edits::EditState, String> {
    edits.unannotate(doc, mark, sweep)
}

/// Marks a region of one page for removal.
///
/// **Nothing is destroyed by this command.** It is `docs/PLAN.md` §6 step 1:
/// the region joins the review list and the overlay outlines it. Applying is a
/// separate command, and the whole point of the split is that a reader looks at
/// the list first.
///
/// Named `redact_mark` beside [`annot_mark`], and the pairing is deliberate ---
/// they are the same gesture producing two different things, and the names
/// should make the difference legible in a stack trace as well as in a menu.
///
/// The page is named by identity for [`annot_mark`]'s sharper reason: a region
/// is placed by coordinates, so a stale position would mark a different page at
/// the spot the words used to be --- and here that would be a reader certifying
/// the removal of something they never looked at.
#[tauri::command]
async fn redact_mark(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    area: [f32; 4],
) -> Result<edits::EditState, String> {
    edits.redact(doc, page, area)
}

/// Takes one pending redaction back off its page.
#[tauri::command]
async fn redact_remove(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    redaction: u64,
) -> Result<edits::EditState, String> {
    edits.unredact(doc, redaction)
}

/// Rubs strokes out of one drawing.
///
/// `remove` is positions into the drawing's current stroke list, not points ---
/// see [`edits::Edits::erase`] for why the frontend does not get to send back
/// geometry through a command that only removes. One call per *drawing*, and
/// `sweep` is what makes a gesture that crossed several of them one undo; a
/// sweep that takes the last stroke takes the drawing with it.
#[tauri::command]
async fn annot_erase(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    remove: Vec<usize>,
    sweep: u64,
) -> Result<edits::EditState, String> {
    edits.erase(doc, mark, remove, sweep)
}

/// Replaces what one mark says.
///
/// The whole note, not an edit to it --- see [`docmodel::Command::Renote`]. The
/// text is the reader's own words rather than the document's, which is why
/// nothing sanitises it here: it goes into `/Contents` on the way out, and the
/// path that reads it back in is `annots.rs`, where a *stranger's* string
/// arrives and is treated as one.
#[tauri::command]
async fn annot_note(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    note: String,
) -> Result<edits::EditState, String> {
    edits.renote(doc, mark, note)
}

/// Replaces what a comment out of the file says.
///
/// [`annot_note`]'s counterpart for an annotation the reader did not make, and
/// the parameters are where the two part company. A mark is named by an id this
/// application issued; a foreign comment is named by the **object the file gave
/// it**, because `annots::Comment::id` is a position in one scan and every id
/// after an inserted comment moves. `object` is `annots::Comment::object`, sent
/// back exactly as it arrived.
///
/// `page` is the model's identity for the page it sits on, which the frontend
/// resolves from the comment's file page through the map it already holds. It is
/// what makes a deleted page take the edit with it --- see
/// [`docmodel::Command::Rewrite`].
///
/// The date is this application's clock, taken here, exactly as
/// [`annot_mark`]'s is: the caller does not get to choose what a comment claims
/// about when it was changed.
#[tauri::command]
async fn annot_rewrite(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    object: (u32, u16),
    page: u64,
    body: String,
) -> Result<edits::EditState, String> {
    edits.rewrite(
        doc,
        object,
        page,
        body,
        save::pdf_date(std::time::SystemTime::now()),
    )
}

/// Takes a comment out of the file off the page it is on.
///
/// [`annot_rewrite`]'s counterpart, and its two parameters mean exactly what
/// they mean there: `object` is the name the **file** gave the annotation, and
/// `page` is the model's identity for the page, which is what makes a deleted
/// page take the deletion with it.
///
/// **No date**, unlike every other write command here. `/M` says when a comment
/// was last modified, and a comment that is gone has nothing to say it about ---
/// so there is no clock reading to take and nothing for a caller to choose.
///
/// ⚠ **This is the one edit that forces a full rewrite of the file on save.** An
/// incremental save only adds objects; a deletion has nothing it can add. See
/// `edits::Plan::is_appendable`.
#[tauri::command]
async fn annot_discard(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    object: (u32, u16),
    page: u64,
) -> Result<edits::EditState, String> {
    edits.discard(doc, object, page)
}

/// Replaces what one mark is drawn in.
///
/// The whole colour, not a channel --- see [`docmodel::Command::Recolor`]. Three
/// floats from the webview, clamped into `0..=1` at the `edits.rs` boundary the
/// same way a new mark's are, because this is the second route into `/C` and a
/// non-finite channel would be three letters in the middle of a content stream.
#[tauri::command]
async fn annot_recolor(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    color: [f32; 3],
) -> Result<edits::EditState, String> {
    edits.recolor(doc, mark, color)
}

/// Moves one mark by an offset, in the page's display space.
///
/// An offset rather than a new rectangle, which is what makes it a move --- see
/// [`docmodel::Doc::displace`]. The frontend clamps it against the page before
/// sending, because the page's size in points is not something the model holds.
///
/// One call per drag, so one undo puts the mark back where it was rather than
/// stepping it home a pointer event at a time.
#[tauri::command]
async fn annot_move(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    dx: f32,
    dy: f32,
) -> Result<edits::EditState, String> {
    edits.displace(doc, mark, dx, dy)
}

/// Steps the edit journal back one command.
#[tauri::command]
async fn edit_undo(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.undo(doc)
}

/// Steps the edit journal forward one command.
#[tauri::command]
async fn edit_redo(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.redo(doc)
}

/// The edit state of an open document.
///
/// Asked for once after an open, so the frontend starts from the model's answer
/// rather than from an assumption that a freshly opened document is unedited.
/// Those are the same thing today and will not be once a session can carry
/// edits, and the difference is invisible until it is wrong.
#[tauri::command]
async fn edit_state(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.state(doc)
}

/// Why a save did not happen, and whether the reader still has their document.
///
/// **Two refusals, not one**, for the reason `docmodel.rs` gives about its own
/// pair: they need different answers from the caller. A save refused *before*
/// anything was taken apart --- an encrypted document, a file that changed
/// underneath, a plan that cannot be written --- has disturbed nothing, and the
/// reader carries on reading. A save that failed *after* the document was
/// closed has no document to carry on with, and the caller has to open the file
/// again; the edits are gone, because the journal went with the close.
///
/// Serialized rather than a bare string, so that distinction crosses the IPC
/// boundary as a field the frontend can branch on. A message a human reads and
/// a fact a program acts on are different things, and packing the second into
/// the first is how a frontend comes to match on wording.
#[derive(serde::Serialize)]
struct SaveFailure {
    message: String,
    /// Whether the caller must open the document again.
    reopen: bool,
    /// Whether the file changed on disk since the reader opened it.
    ///
    /// A field for the reason `reopen` is one, and `save::Refusal` carries the
    /// argument in full: it is what lets the window offer Reload for this
    /// refusal and withhold it for the ones where reloading would discard the
    /// reader's work in exchange for nothing.
    changed: bool,
}

impl SaveFailure {
    /// Nothing was touched: no file written, no document closed.
    fn refused(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reopen: false,
            changed: false,
        }
    }

    /// The document is closed, whatever became of the file.
    fn after_close(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reopen: true,
            changed: false,
        }
    }

    /// A refusal `save.rs` produced, carrying its own reason forward.
    ///
    /// The two above take a bare message and are for refusals this file states
    /// itself. These two exist so that `changed` is *carried* rather than
    /// re-derived: deciding it again at this end would mean asking the same
    /// question twice and, worse, matching on the message to answer it.
    fn refused_by(why: save::Refusal) -> Self {
        Self {
            message: why.message,
            reopen: false,
            changed: why.changed,
        }
    }

    /// The same, for a refusal that arrives after the document is closed.
    fn after_close_by(why: save::Refusal) -> Self {
        Self {
            message: why.message,
            reopen: true,
            changed: why.changed,
        }
    }
}

/// Writes the working document over the file the reader opened.
///
/// **Three steps, in an order that is the whole of the design.** The bytes are
/// staged beside the source; the document is closed; the staged file is renamed
/// over the source. Staging first is what lets every refusal `save.rs` states
/// arrive while the reader still has their document. Closing before the rename
/// is not a tidiness: a `rename` over a memory-mapped file succeeds on macOS and
/// leaves the mapping serving the inode that is no longer at that path, so the
/// worker would go on rendering the document as it was before the save --- and
/// Windows refuses the rename outright while a section is open. One order is
/// right on both platforms, and neither platform's failure is loud.
///
/// **The caller reopens.** Nothing here rebuilds the document, because every
/// object identity in the file has just changed --- `docs/PLAN.md` §5 --- so the
/// baseline the journal replays against is gone and the model is closed with it.
/// A reopen from the same path is the rebase, and it is the frontend's because
/// the frontend is what knows where the reader was looking.
///
/// On the blocking pool for the parse and the serialisation, for the reason
/// [`save_copy`] gives. The close and the rename are not: one is a channel
/// round trip and the other is a rename in a directory that has just been
/// written to.
#[tauri::command]
async fn save_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    source: String,
) -> Result<(), SaveFailure> {
    // Read rather than assumed from the command being offered at all. The
    // palette withholds Save on a document with nothing to save, and that guard
    // is a frontend that may be a reply behind; this one is the model's own
    // answer. Writing a clean document would still produce a correct file, but
    // it would rewrite every object id in a file the reader did not change.
    let state = edits.state(doc).map_err(SaveFailure::refused)?;
    if !state.dirty {
        return Err(SaveFailure::refused(
            "there is nothing to save --- this document has no unsaved changes",
        ));
    }
    let plan = edits.plan(doc).map_err(SaveFailure::refused)?;

    // The file has to still be the file. Everything below rewrites the object
    // graph the plan was made against, and a `source` that something else has
    // written to since is a different graph -- the reader's edits would be
    // replayed onto pages they were never made on, and the write is atomic, so
    // the result is a confidently wrong file rather than a visibly broken one.
    //
    // The check itself lives in `save.rs`, on the plan, where `write_copy` and
    // `stage_in_place` both reach it and where a test can drive it -- a guard
    // written inline in a command has no failing case, which `docs/TRAPS.md`
    // records twice over. Nothing is read here: what the second look below needs
    // comes back from the staging, which is the moment it should be comparing
    // against rather than the moment the reader opened the file.
    //
    // **Two writers, and which one runs is the plan's answer.** A save that adds
    // nothing but marks is written as an update section appended to the file,
    // which leaves every existing byte where it is: on a 337 MB scan that is
    // 29 ms and 723 bytes against 239 ms and a rewritten copy of the whole
    // document. Anything else --- a deleted page, a move, a turn, a crop --- is
    // reserialised, which is what every save did until 2026-08-22. See
    // `save::mode_for`, and `docs/PLAN.md` §5 for the measurement.
    //
    // Both halves have the same shape and the same reason for it: prepare while
    // the document is still open and nothing is at stake, then close, then
    // apply. The document has to be closed in between either way --- a rename
    // over a mapped file leaves the mapping serving the old inode on macOS, and
    // an append to one is a file the worker's cached parse no longer describes.
    // **Size decides too, since 2026-08-22.** An append is prepared inside a
    // worker, and a worker is bounded -- by a job object on Windows and by the
    // machine on macOS -- so past `save::APPEND_MAX_BYTES` the document is
    // reserialised instead. That is slower and it loses the byte-for-byte
    // previous revision; it is what makes a large document saveable at all,
    // against a worker that would otherwise be refused the memory to prepare
    // one and abort.
    //
    // **A file that cannot be measured takes the rewrite**, which is the arm
    // with no memory bound over it and is correct for every plan. `AGENTS.md`
    // records a migration whose `if (checked -and safe) {stop}` collapsed
    // "checked, fine to proceed" with "could not check at all" and force-pushed
    // on the second; the failure path here goes the safe way by construction
    // rather than by ordering.
    let mode = save::mode_for_source(&plan, Path::new(&source));

    // **Before the match, because both arms need it now.** It used to be asked
    // after, for the append alone --- the rewrite arm read `Prepared::Rewrite(_)
    // => None` and did not need a key, because it refused every encrypted
    // document outright. Since 2026-08-28 a rewrite re-encrypts what it writes,
    // so the key is what makes it possible rather than what it would have
    // leaked. The rewrite also needs it *earlier* than the append does: the
    // append's parse happens in the worker below, while the rewrite parses on
    // the pool inside this match.
    let password = password_for(&service, doc, "save_document").await;

    let prepared = match mode {
        // **The append's parse happens in the worker**, which is the one
        // difference between the two arms and the reason they are not one
        // `spawn_blocking`. `save::append_update` is a pure function of the
        // document's bytes and the plan, and those bytes are the attacker's ---
        // so it runs in the process that already holds this document under a
        // sandbox, a deadline and a restart, and that has already parsed it with
        // `lopdf` for its comments, links and properties. What comes back is
        // bytes and two numbers. Every decision about the file stays here:
        // `append_ready` measures and fingerprints it before the request, and
        // `save::appended` refuses an answer built against a different length.
        //
        // Asked before the close below, which the order already required for an
        // unrelated reason --- there is no document to build from afterwards.
        save::Mode::Append => {
            let checking = source.clone();
            let asking = plan.clone();
            let ready = tauri::async_runtime::spawn_blocking(move || {
                save::append_ready(Path::new(&checking), &asking)
            })
            .await
            .map_err(|e| SaveFailure::refused(format!("the save did not run: {e}")))?
            .map_err(SaveFailure::refused_by)?;

            let (reply, rx) = reply_channel();
            service.append(doc, plan, reply);
            let update = await_reply("save_document", rx)
                .await
                .map_err(SaveFailure::refused)?;
            save::appended(ready, update)
                .map(Prepared::Append)
                .map_err(SaveFailure::refused_by)?
        }
        save::Mode::Rewrite => {
            let staging = source.clone();
            let key = password.clone();
            // **The rewrite's parse happens in the worker too, since
            // 2026-08-28**, and it is the same argument the append arm above
            // makes: `save::rewrite_update` is a pure function of the document's
            // bytes and the plan, and those bytes are the attacker's. What took
            // longer is where the answer goes --- an append's is kilobytes and
            // fits in a reply, a rewrite's is the whole document --- so the
            // worker is handed the staging file's own descriptor and writes down
            // it. `docs/THREAT-MODEL.md` residual risk 18.
            let writing = outside_of(&app, service.backend());
            tauri::async_runtime::spawn_blocking(move || {
                save::stage_in_place(Path::new(&staging), &plan, key.as_deref(), &*writing)
            })
            .await
            .map_err(|e| SaveFailure::refused(format!("the save did not run: {e}")))?
            .map(Prepared::Rewrite)
            .map_err(SaveFailure::refused_by)?
        }
    };

    // **Before the close, because after it there is no document to ask.** An
    // append to an encrypted document re-reads the file it wrote to check the
    // cross-reference chained correctly, and `lopdf` parses no objects at all
    // without the key --- so that check would count zero pages against the two
    // it expects and roll a correct save back. Asked only for the arm that
    // needs it, and dropped when this function returns:
    // `docs/THREAT-MODEL.md` §T6.9.
    //
    // A failure to answer is not a refusal. The document is about to be closed
    // either way and a plain document has no password to lose, so `None` is the
    // right answer for both "it has none" and "the service could not say" ---
    // and if the second is wrong, the append's own read-back refuses and rolls
    // back rather than writing something unchecked.
    // Already held: asked once above the match, where the rewrite arm needs it.
    // This was a second ask keyed on which arm ran, and both arms now want the
    // same answer.

    // Past this line every failure is an `after_close`: the reader's document is
    // being taken apart, and the honest thing to report is that they have to
    // open the file again rather than a message that reads like a refusal.
    //
    // The model first, for the reason `close_document` gives --- document
    // numbers are reused, and a journal left under a handle the service is free
    // to hand to another file is one document's edits applied to another's
    // pages.
    edits.close(doc);
    let (reply, rx) = reply_channel();
    service.close(doc, reply);
    let closed = await_reply("save_document", rx).await;

    // Attempted whether or not the close was acknowledged. The model is gone
    // either way, so the reader is reopening either way, and a rename that the
    // mapping really did block reports that itself --- which is a better message
    // than one this end guesses from a close reply.
    //
    // **On the blocking pool, and this whole match had been on the async
    // runtime until 2026-08-23.** Every arm below does real file work on a file
    // the size of the reader's document: the rewrite hashes every byte of it in
    // `verify_before_commit` and then renames, and the append writes, waits for
    // the platter, reads the whole file back and *parses it with `lopdf`*. That
    // last one is a parse of attacker-derived bytes --- the previous revision is
    // the document the reader opened --- so it belongs where the other three
    // coordinator-side parses already are. `docs/THREAT-MODEL.md` residual risk
    // 17 said the append had moved into the worker; its *preparation* had, and
    // this is the half that had not.
    //
    // **Who re-reads the file the append writes**, chosen the same way the
    // render backend is and for the same reason: the previous revision of that
    // file is the document the reader opened, so the parse belongs in a
    // sandboxed child wherever there can be one. A platform with none still
    // saves --- refusing would make it useless rather than uncontained, which is
    // the rule `Backend::default_here` already follows --- and it is not silent:
    // `render::UNSANDBOXED_MARK` is what keeps the two runs distinguishable.
    //
    // Built here rather than inside `save::append_in_place`, because choosing it
    // needs the app handle and that function is reachable from `cargo test`,
    // where there is none.
    let reread = outside_of(&app, service.backend());

    // `prepared` is consumed rather than borrowed, which is what lets it cross
    // into the closure, and nothing after this line reads it.
    let landed = tauri::async_runtime::spawn_blocking(move || match prepared {
        Prepared::Rewrite(staged) => {
            // One more look before the rename, closing the window the split
            // above opens. What it compares and why it compares against staging
            // rather than against the open is on the function, where a test can
            // reach it.
            //
            // `after_close`, and this is worth stating because the comment here
            // said the opposite until 2026-08-19 while the code did what it does
            // now. Nothing has been renamed, so it is tempting to call this a
            // refusal that costs nothing --- but the close two statements up has
            // already happened, so the reader's model and their journal are
            // gone. `refused` would tell them their document is still open when
            // it is not, which is the one thing that flag decides.
            save::verify_before_commit(&staged, Path::new(&source))
                .map_err(SaveFailure::after_close_by)?;
            save::commit_in_place(&staged.path, Path::new(&source))
                .map_err(SaveFailure::after_close)
        }
        // No second look of its own because it takes its own, and it has to be
        // its own: `verify_before_commit` compares against a *path*, and an
        // append writes through a *handle*. `append_in_place` opens the file,
        // asks `Appended::verified` the same length-and-timestamp question
        // through that handle, writes, reads back and rolls back through it, and
        // finally checks that the pathname still names it.
        //
        // This comment claimed the opposite until 2026-08-22 --- that comparing
        // a length alone was "a sharper answer" than comparing a length and a
        // timestamp --- and the code agreed with it. It is the wrong way round,
        // `fingerprint.rs` says so in its own header, and `docs/TRAPS.md` has
        // had *Equal length is not no change* since before either was written.
        Prepared::Append(appended) => {
            save::append_in_place(&appended, Path::new(&source), password.as_deref(), &*reread)
                .map_err(SaveFailure::after_close)
        }
    })
    .await
    // The pool itself failing --- a panic in the closure, or a runtime shutting
    // down. `after_close` for the same reason every arm above is: the document
    // is gone whatever happened to the file.
    .map_err(|e| SaveFailure::after_close(format!("the save did not finish: {e}")))?;

    landed.map_err(|why| with_close_note(why, closed))
}

/// Adds what became of the close to a failure that happened after it.
///
/// **Both arms of the save carry this now, and the rewrite's first failure did
/// not before.** `verify_before_commit`'s refusal used to leave through `?` and
/// skip it, which was an accident of where the early return sat rather than a
/// decision: the close has happened either way, so a reader whose document also
/// failed to close should be told once, on whichever refusal reaches them.
///
/// **A function rather than a closure inside the command, so it has a failing
/// case.** `save_document` is an async Tauri command that needs a running app,
/// a render service and a real file, so nothing in `cargo test` can call it ---
/// which is this repository's own rule about a guard written inline in a
/// command, arriving in the same function whose comments already cite it twice.
///
/// The fields a program branches on are untouched. Only `message` grows, which
/// is the one part of a `SaveFailure` written for a human.
fn with_close_note(mut why: SaveFailure, closed: Result<(), String>) -> SaveFailure {
    if let Err(also) = closed {
        why.message = format!(
            "{} --- and the document did not close cleanly: {also}",
            why.message
        );
    }
    why
}

/// A save that has been prepared, by whichever writer the plan chose.
///
/// The two carry different things --- a rewrite carries a path to rename, an
/// append carries bytes and the length they go after --- and an enum is what
/// keeps the caller from having to know which fields mean anything. See
/// `save::Mode`.
enum Prepared {
    Rewrite(save::Staged),
    Append(save::Appended),
}

/// Writes the working document to a new file.
///
/// `source` comes from the frontend, which is what [`print_document`] does and
/// for the same reason: the render service holds the document, and the path is
/// the frontend's own record of what it asked to open.
///
/// On the blocking pool, unlike the four commands above: this parses the whole
/// document with `lopdf` and serialises it, which on the 337 MB scan is not work
/// to do on a runtime worker. Read the argument in [`print_document`] --- it is
/// the same one, and the two commands are the two members of this repository
/// that genuinely belong there.
#[tauri::command]
async fn save_copy(
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
) -> Result<save::Copied, String> {
    // Read out of the model *before* the move onto the pool. The state is behind
    // a mutex that is not held across an await anywhere in this file, and taking
    // a `State` handle into a `spawn_blocking` closure would need it to outlive
    // the command.
    let plan = edits.plan(doc)?;
    // **A copy is written even from a source that changed**, and this comment
    // said the opposite until 2026-08-19: it described the copy as refused "in
    // the same words" and named opening the file again as the way out. That was
    // a dead end wearing a helpful sentence. Save a copy IS the fallback the
    // in-place refusal points at, and reopening is exactly what spends the edits
    // the copy exists to keep -- so a reader whose file changed had nowhere at
    // all to put their work.
    //
    // What comes back says whether the source had changed, because a copy built
    // from a document that is no longer the one on screen is a fact the reader
    // has to be told rather than a failure. `save.rs`'s `OnChange` carries the
    // argument, including what still refuses: a changed file that also changed
    // shape is caught by the page-count guard whichever path asks.
    let password = password_for(&service, doc, "save_copy").await;
    tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            password.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("the save did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes a subset of the working document's pages to a new file.
///
/// Everything [`save_copy`] does, over a selection rather than the whole
/// document, and it shares that command's whole write path --- so the three
/// refusals `save.rs` states (encrypted source, a page count that disagrees with
/// the baseline, writing over the source) apply here unchanged and are not
/// restated.
///
/// `slots` are positions in the **current** order, deduplicated and ascending;
/// `edits::Edits::plan_subset` refuses anything else rather than normalising it,
/// so a defect on the way here is a message and not a file with pages in an
/// order nobody asked for.
///
/// On the blocking pool for the same reason as [`save_copy`], which is the
/// reason it does not simply call it: the plan has to be read out of the model
/// before the move onto the pool, and the only difference between the two
/// commands is which plan.
#[tauri::command]
async fn extract_pages(
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    slots: Vec<u32>,
) -> Result<save::Copied, String> {
    let plan = edits.plan_subset(doc, &slots)?;
    // Same outcome as `save_copy` and for the same reason: an extract is a copy
    // of some of the pages, so it is written from a changed source too, and the
    // reader is told the same way.
    let password = password_for(&service, doc, "extract_pages").await;
    tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            password.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("the extract did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes the working document's pages to several new files, one per group.
///
/// [`extract_pages`] repeated, which is what the plan said it would be: each
/// group becomes its own plan and its own `write_copy`-shaped write, so the
/// three refusals `save.rs` states apply to every file and are not restated.
/// `save::write_split` adds one more that only a split needs --- no destination
/// may already exist --- and its doc comment carries the reason.
///
/// **Changes nothing about the open document.** No command is journalled and
/// there is nothing to undo, which is [`extract_pages`]' and [`merge_documents`]'
/// property: all three read the document and write elsewhere.
///
/// Every plan is built **before** the move onto the pool, for [`save_copy`]'s
/// reason and one more: a group that names an unknown slot must refuse before
/// any file is written, not after the first two are on disk.
///
/// `groups` are positions in the current order, each deduplicated and ascending,
/// which `edits::Edits::plan_subset` enforces per group. Nothing here checks
/// that the groups *partition* the document: `parseSplitPoints` builds them and
/// a caller sending overlapping groups gets overlapping files, which is a
/// stranger request than it is a dangerous one.
#[tauri::command]
async fn split_document(
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    groups: Vec<Vec<u32>>,
) -> Result<save::Split, String> {
    let plans = groups
        .iter()
        .map(|slots| edits.plan_subset(doc, slots))
        .collect::<Result<Vec<_>, String>>()?;
    let password = password_for(&service, doc, "split_document").await;
    tauri::async_runtime::spawn_blocking(move || {
        save::write_split(
            Path::new(&source),
            &plans,
            Path::new(&path),
            password.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("the split did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes the working document followed by other files' pages, to a new file.
///
/// The open document goes in as the reader has it and the others go in as they
/// are on disk --- `save::write_merged` holds that asymmetry and the reason for
/// it. Everything [`save_copy`] refuses about the open document is refused here
/// unchanged and is not restated.
///
/// **Changes nothing about the open document.** No command is journalled, the
/// order is untouched and there is nothing to undo, which is [`extract_pages`]'s
/// property arriving from the other direction: extract reads some of one file,
/// merge reads all of several, and neither is an edit.
///
/// `others` are paths the reader chose in a file dialog. They are opened here,
/// in the coordinator, which is where every other `lopdf` parse on a save path
/// runs --- see `docs/THREAT-MODEL.md` residual risk 18, which this widens by
/// one file per merge rather than by a new kind of access.
///
/// On the blocking pool for [`save_copy`]'s reason, and rather more so: this one
/// parses every file it was given.
#[tauri::command]
async fn merge_documents(
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    others: Vec<String>,
) -> Result<save::Merged, String> {
    // Out of the model before the move onto the pool, as `save_copy` does and
    // for the same reason.
    let plan = edits.plan(doc)?;
    let password = password_for(&service, doc, "merge_documents").await;
    tauri::async_runtime::spawn_blocking(move || {
        let others: Vec<std::path::PathBuf> = others.into_iter().map(Into::into).collect();
        save::write_merged(
            Path::new(&source),
            &plan,
            &others,
            Path::new(&path),
            password.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("the merge did not run: {e}"))?
    .map_err(|why| why.message)
}

/// What this keyboard prints on the keys a shortcut can name by position.
///
/// Keyed by `KeyboardEvent.code`. Empty on every platform but macOS, and empty
/// there too when the active input source carries no Unicode layout --- the
/// caller falls back to the character its binding declares, which is the label
/// the palette showed before this existed.
///
/// **On the main thread**, because HIToolbox aborts the process when the Text
/// Input Sources API is entered from two threads at once and says outright that
/// a UI application must call it from the main one. Same hop as `menu.rs`, for a
/// stricter reason: there it is AppKit's requirement, here it is a deliberate
/// `abort()` with a message naming the rule.
#[cfg(target_os = "macos")]
#[tauri::command]
async fn keyboard_positions(
    app: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.blocking_send(keylayout::positions());
    })
    .map_err(|e| format!("could not reach the main thread to read the keyboard: {e}"))?;
    rx.recv()
        .await
        .ok_or_else(|| "the keyboard layout reader did not answer".to_string())
}

/// The non-macOS answer: no layout lookup, so every label stays its character.
///
/// An empty map rather than a refusal, for the same reason [`set_menu`] answers
/// `None`: there is nothing wrong on Windows, and the palette's own rendering is
/// what that platform has always shown.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn keyboard_positions() -> Result<std::collections::HashMap<String, String>, String> {
    Ok(std::collections::HashMap::new())
}

/// Installs the native menu bar from the layout the frontend holds.
///
/// Returns the event name a chosen item will arrive on, or `None` where there is
/// no menu bar --- which is every platform but macOS.
///
/// An answer rather than a refusal: nothing is wrong on Windows, the palette is
/// that platform's route, and an error there would put a red line in front of a
/// reader about a thing that was never meant to happen. So this is a capability
/// question, and the frontend stops sending enablement updates for a menu that
/// does not exist rather than pushing them into a silent no-op.
///
/// **The event name travels with the answer** for the reason `launch_open_event`
/// exists: a constant agreed in two languages fails by silence, and this one
/// would fail as a menu bar that is fully built, fully enabled, and does nothing
/// when clicked. One call carries both, so the name cannot be fetched for a menu
/// that was never installed.
///
/// The spec is built from the command registry; see `src/lib/menubar.ts`.
/// **One arm, with the platform question inside `menu.rs`.** There were two
/// until 2026-08-28, and the pair cost more than the duplication: the non-macOS
/// one took the payload as an unread `serde_json::Value`, because
/// `menu::SectionSpec` did not exist there --- so the contract between
/// `menubar.ts` and this command was type-checked on exactly one of the two
/// platforms tpdf ships. `menu::INSTALLS` carries the decision now, and the spec
/// is parsed into the same shape everywhere.
#[tauri::command]
async fn set_menu(
    app: tauri::AppHandle,
    sections: Vec<menu::SectionSpec>,
) -> Result<Option<&'static str>, String> {
    menu::install(&app, sections).await?;
    Ok(menu::INSTALLS.then_some(menu::RUN_EVENT))
}

/// Enables or disables menu items to match the commands' own guards.
///
/// Separate from [`set_menu`] because a rebuild per edit would rebuild the whole
/// bar several times a second while a reader works --- every rotation changes
/// whether Undo is live.
/// One arm, for [`set_menu`]'s reason.
#[tauri::command]
async fn set_menu_enabled(
    app: tauri::AppHandle,
    state: std::collections::HashMap<String, bool>,
) -> Result<(), String> {
    menu::set_enabled(&app, state).await
}

/// Extracts one page's characters and their positions.
///
/// Selection, search and the accessibility tree all read this, and they read
/// the same one deliberately --- three extractions would disagree in ways no
/// test catches, each being self-consistent. Cached on the frontend rather than
/// here: what a page's text costs to *re-request* is an IPC round trip, and what
/// it costs to re-extract is measured in `examples/text_probe.rs`.
#[tauri::command]
async fn page_text(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<text::PageText, String> {
    let (reply, rx) = reply_channel();
    service.text(doc, page, crop, reply);
    await_reply("page_text", rx).await
}

/// Finds a query in one page, returning character ranges.
///
/// One page per call, because the render thread is FIFO and a whole-document
/// scan would sit in front of every tile --- see `RenderService::search`. The
/// caller walks the document and stops when it wants to cancel.
///
/// `carry` is the previous page's tail, handed back by the previous call. It is
/// how a phrase that runs over a page break is found without either side
/// holding two pages at once --- see `search::Carry`.
#[tauri::command]
async fn search_page(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    query: String,
    options: search::Options,
    carry: Option<search::Carry>,
) -> Result<search::PageMatches, String> {
    let (reply, rx) = reply_channel();
    service.search(doc, page, query, options, carry, reply);
    await_reply("search_page", rx).await
}

/// Reads a document's outline --- its bookmarks --- as a bounded tree.
///
/// Bounded is the operative word: the outline of a malformed document can be
/// infinite, and PDFium documents that it is our job to notice. See
/// `outline.rs`.
#[tauri::command]
async fn document_outline(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<outline::Outline, String> {
    let (reply, rx) = reply_channel();
    service.outline(doc, reply);
    await_reply("document_outline", rx).await
}

/// Reads every comment in a document --- notes, highlights, replies.
///
/// Document-level rather than per page, because the answer comes from one
/// `lopdf` parse of the whole file: asking per page would repeat that parse
/// once per page to return a slice of the same list. Lazy for the reason
/// `document_mapping` is --- it is off the startup path, and a reader who never
/// opens the comments panel never pays for it.
///
/// A failure is an error rather than an empty list. "This document has no
/// comments" and "this document could not be read" are different things to tell
/// a reader, and the frontend shows them differently. See `annots.rs`.
#[tauri::command]
async fn document_comments(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<annots::Comments, String> {
    let (reply, rx) = reply_channel();
    service.comments(doc, reply);
    await_reply("document_comments", rx).await
}

/// Reads every link in a document --- the rectangles a reader clicks.
///
/// Document-level for the same reason `document_comments` is, and asked for
/// once just after first paint rather than on demand: nothing opens a panel
/// before clicking a cross-reference, so a lazy version would mean the first
/// click on any document goes nowhere.
///
/// A failure is an error rather than an empty list. A document whose links
/// could not be read is one whose cross-references silently do nothing, which
/// is worth telling a reader rather than leaving them to click.
#[tauri::command]
async fn document_links(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<links::Links, String> {
    let (reply, rx) = reply_channel();
    service.links(doc, reply);
    await_reply("document_links", rx).await
}

/// Reads what a document says about itself: properties, encryption, signatures.
///
/// Document-level like `document_comments`, and asked for only when a reader
/// opens the dialog --- it is the one `lopdf` parse nothing on the reading path
/// ever needs, so a reader who never asks never pays for it.
///
/// A failure is an error rather than an empty readout. `crate::docinfo` reports
/// what it could not read through its own limits, so an error here means the
/// document could not be parsed at all, which is worth saying.
#[tauri::command]
async fn document_properties(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<docinfo::Properties, String> {
    let (reply, rx) = reply_channel();
    service.properties(doc, reply);
    await_reply("document_properties", rx).await
}

/// Reports, per page, whether the text means anything or PDFium is guessing.
///
/// A CID font with no `/ToUnicode` makes PDFium read glyph ids as character
/// codes, so a page comes back with text of the right length, in the right
/// places, that means nothing --- and the reader searching for a word they can
/// plainly see is told there are no matches. `encoding.rs` has the rule.
///
/// **Asked for lazily, and the cost is measured rather than assumed.** 0.1 ms on
/// a small document, 5.8 ms on the 775-page one, 11.9 ms on the 337 MB scan ---
/// `lopdf` reads the xref and object headers, not every stream, so this tracks
/// object count and not file size. Cheap, and still not free: warm startup has
/// ~25 ms of margin against its target, so this is deliberately kept off the
/// critical path rather than done at open. Cached for the document's lifetime.
///
/// In practice the frontend asks on the first frame after open, from the
/// accessibility layer, rather than only after a fruitless search --- a
/// screen-reader user may never search and is the reader least able to tell that
/// what they are being read is nonsense.
#[tauri::command]
async fn document_mapping(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<Vec<encoding::PageMapping>, String> {
    let (reply, rx) = reply_channel();
    service.mapping(doc, reply);
    await_reply("document_mapping", rx).await
}

/// Reads the remembered places, most recently read first.
///
/// Synchronous on purpose: it is asked for during startup, where the whole
/// application budget is ~50 ms, and reading a few kilobytes costs microseconds
/// against the round trip that would be needed to hand it back later.
#[tauri::command]
fn session_load(app: tauri::AppHandle) -> session::Session {
    session::Session::load(&session_file(&app))
}

/// Serializes the session file's read-modify-write cycles.
///
/// Both writers below load, edit and save, which is only safe against a
/// concurrent writer if the three happen together. That used to be true by
/// accident: a synchronous `#[tauri::command]` runs on the thread the IPC
/// arrives on, so the main thread serialized them and nothing said so. Moving
/// the work to the blocking pool removes that and would leave a lost update ---
/// `session_set_invert_pages` is called directly rather than through the
/// frontend's write chain, so it really can overlap a throttled place write.
/// The lock is what the main thread used to be.
///
/// `parking_lot`'s, like every other lock here, so a panic mid-write cannot
/// poison it. That is the behaviour wanted rather than merely the one that
/// comes free: the guarded thing is a file, and `Session::save` is a
/// write-and-rename, so a write that panicked left the old file whole and the
/// next writer has nothing to recover from.
static SESSION_WRITE: Mutex<()> = Mutex::new(());

/// Loads, edits and saves the session file under [`SESSION_WRITE`].
fn with_session<F: FnOnce(&mut session::Session)>(path: &Path, edit: F) -> Result<(), String> {
    let _guard = SESSION_WRITE.lock();
    let mut session = session::Session::load(path);
    edit(&mut session);
    // Named, because this string crosses the IPC boundary and is the only thing
    // the reader is shown. `io::Error`'s own text is "permission denied" with no
    // subject, which is true of every path in the process.
    session
        .save(path)
        .map_err(|e| format!("could not write the session file {}: {e}", path.display()))
}

/// Records where a document was left.
///
/// Read-modify-write on every call rather than holding the session in managed
/// state: the file is the record, and a second window --- or a crash that skips
/// whatever teardown would have flushed it --- must not be able to roll back a
/// place already written.
///
/// **On the blocking pool, because this is on the scroll path.** The frontend
/// throttles to one write per second, but a write is a file read, a parse, a
/// serialize and a write-and-rename, and as a synchronous command all of that
/// ran on the thread the webview draws on. Measured release-profile on a full
/// 32-place session, 2,000 cycles: mean **0.911 ms**, p99 **1.381 ms**, max
/// **13.870 ms**. The mean is comfortably inside a frame and the maximum is not
/// --- 13.9 ms is past a 120 Hz frame at 8.3 ms --- so this was an occasional
/// visible hitch while scrolling rather than a steady cost. `async` alone would
/// only move the stall onto a runtime worker, which is the mistake
/// `print_document` records; the work is synchronous file I/O, so it belongs on
/// the pool built for it.
///
/// Returns `Result` so a failure to write is *visible* to the caller. Nothing
/// currently acts on it, and the frontend deliberately does not surface it: a
/// dialog because the position could not be saved would be worse than the lost
/// position.
#[tauri::command]
async fn session_remember(app: tauri::AppHandle, place: session::Place) -> Result<(), String> {
    let path = session_file(&app);
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&path, |session| session.remember(place))
    })
    .await
    .map_err(|e| format!("the session write did not run: {e}"))?
}

/// Records whether pages are shown inverted.
///
/// Its own command rather than a field on `session_remember`, because it is a
/// preference and not a place. Folding it into the place payload would also make
/// it invisible to the writer's own de-duplication: that compares consecutive
/// places, so toggling the mode without moving would compare equal and never be
/// written at all.
///
/// Called directly instead of through the throttle, since a reader inverts the
/// page deliberately and rarely, where a place changes on every frame.
///
/// On the pool for the same reason as [`session_remember`], though the case for
/// it is weaker --- a rare deliberate keypress can afford a stall a scroll
/// cannot. It goes there anyway because it is the *other* half of the pair
/// [`SESSION_WRITE`] exists for: bypassing the frontend's write chain is
/// exactly what makes it able to overlap a place write, and a writer that took
/// the lock on one thread while the other took it on another would be two
/// copies of one rule.
#[tauri::command]
async fn session_set_invert_pages(app: tauri::AppHandle, invert: bool) -> Result<(), String> {
    let path = session_file(&app);
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&path, |session| session.invert_pages = invert)
    })
    .await
    .map_err(|e| format!("the session write did not run: {e}"))?
}

/// Builds a print job and opens the platform print dialog for it.
///
/// `async` keeps the build off the thread the webview draws on: `print::build`
/// parses the whole document, and on a 337 MB scan that is not something to do
/// there. Only the panel is dispatched back.
///
/// **The build runs on the blocking pool, and that is not the choice the seven
/// render-service commands made.** Being `async` puts this on the runtime rather
/// than the main thread, which was the whole of the original argument and is
/// only half of one: a synchronous parse inside an `async fn` occupies one of
/// the runtime's few worker threads for its entire duration, and it is `await`
/// that yields a thread, not `async`. The bridges above rejected
/// `spawn_blocking` because the work they wait for happens on the render thread,
/// so moving the *wait* to a larger pool raises the bound instead of removing
/// it. Here the work is in this function, CPU-bound and synchronous, which is
/// what the blocking pool is for. The two look like the same fix and are
/// opposite readings of where the time is spent.
///
/// Returns as soon as the panel has been *asked for*, not when it closes. The
/// outcome is deliberately not reported: `runOperation` answers one boolean for
/// both "printed" and "cancelled" (see `print_macos::present`), so a caller
/// waiting for it could only turn a Cancel into an error message.
#[tauri::command]
async fn print_document(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    path: String,
    doc: Option<u32>,
    pages: Option<Vec<u32>>,
    turns: u8,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    // Read here rather than inside the chooser, so that what decides the shape of
    // the job is a pure function of the plan and the range --- and lives in the
    // module that owns `Pages`, where its tests are under the same filter as the
    // rest of them.
    let plan = doc.map(|doc| edits.plan(doc)).transpose()?;
    let route = print::route(plan.as_ref(), pages, turns);
    // How many pages the readback should find. `None` for the passthrough, where
    // the answer is "whatever the file has" and there is no count to compare
    // against --- see `expect_pages`, which treats `None` as "everything".
    let expected = match (&route, &plan) {
        (print::Route::Passthrough, _) => None,
        (print::Route::Working, Some(plan)) => Some(plan.pages.len()),
        // Unreachable: `route` answers `Working` only with a plan in hand. Kept
        // as the safe arm rather than an `unwrap`, because what a panic would
        // replace is a *count that is not compared*, which is the outcome the
        // passthrough already produces.
        (print::Route::Working, None) => None,
        (print::Route::Range(job), _) => match &job.pages {
            print::Pages::Only(wanted) => Some(wanted.len()),
            // `None` for both, and for one reason each. `All` has no count to
            // compare against; `Unlistable` is unreachable here --- `route`
            // sends a plan carrying one to `Working` --- and the safe arm is the
            // same "do not compare" the passthrough already answers.
            print::Pages::All | print::Pages::Unlistable => None,
        },
    };
    // Read before `source` is moved onto the pool; the name is wanted whether or
    // not the build succeeds, and cloning the path to keep it would be carrying
    // a second copy of the thing that is about to be parsed.
    let title = source.file_name().map_or_else(
        || "Document".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    // **The working document goes through the writer a save uses**, which is what
    // puts the reader's marks and crops on the paper --- see `print::Route`. The
    // plan is moved onto the pool with it, so nothing here holds the model's lock
    // while a 337 MB document is parsed.
    //
    // A panicking build would otherwise surface as a command that returned
    // nothing, which is indistinguishable from a panel the reader dismissed.
    // **For a refusal, not for a print job.** `save::print_bytes` refuses an
    // encrypted document either way --- neither re-encrypting nor decrypting is
    // right for a printer --- but without the key the parse in front of that
    // refusal fails first and tells the reader to open the document with the
    // password it is already open with. See `print_bytes`, which carries the
    // reasoning. `None` for the passthrough and the range, which do not rewrite.
    let password = match doc {
        Some(doc) => password_for(&service, doc, "print_document").await,
        None => None,
    };
    let build = move || -> Result<Vec<u8>, String> {
        match route {
            print::Route::Passthrough => {
                std::fs::read(&source).map_err(|e| format!("could not read {source:?}: {e}"))
            }
            print::Route::Working => {
                let plan = plan.ok_or("the working document has no plan to print")?;
                save::print_bytes(&source, &plan, turns, password.as_deref())
                    .map_err(|refused| refused.message)
            }
            print::Route::Range(job) => print::build(&source, &job),
        }
    };
    let bytes = tauri::async_runtime::spawn_blocking(build)
        .await
        .map_err(|e| format!("the print job could not be built: {e}"))??;
    present_job(&app, bytes, title, expected)
}

/// Hands built bytes to the platform, having first read them back.
#[cfg(target_os = "macos")]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    // Re-parsed by PDFKit before anything is offered to a printer --- a third
    // parser, and the one the print system will use itself. Refusing here costs
    // a dialog; not refusing costs paper.
    let reading = print_macos::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;

    app.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            // Unreachable by construction, and silence here would be a print
            // command that does nothing and says nothing.
            diag::note("[print] dispatched off the main thread; no panel shown");
            return;
        };
        if let Err(e) = print_macos::present(&bytes, &title, mtm) {
            diag::note(&format!("[print] {e}"));
        }
    })
    // The same rule: what failed was the hop to the main thread, and without
    // saying so the reader gets a bare runtime message for a print that silently
    // did not happen.
    .map_err(|e| format!("the print panel could not be shown on the main thread: {e}"))
}

/// Hands built bytes to Windows, having first read them back.
///
/// Structurally the same as the macOS arm above and for the same reasons: an
/// independent parser reads the job, the page count is checked against what was
/// asked for, and only then does a panel open. `Windows.Data.Pdf` stands where
/// PDFKit stands --- the operating system's own PDF stack, independent of the
/// `lopdf` that wrote the job and of the PDFium that drew what the reader saw.
/// Refusing here costs a dialog; not refusing costs paper.
///
/// Two differences from macOS, both real and neither a shortcut. Windows has no
/// in-box PDF print API, so `print_win::present` rasterises each page onto the
/// printer's device context --- see that module for what raster output costs. And
/// the dialog is modal on the calling thread, so it runs on a blocking task rather
/// than through `run_on_main_thread`: `PrintDlgW` pumps its own message loop, and
/// occupying Tauri's main thread with it would freeze the window behind it for as
/// long as the panel is open.
#[cfg(windows)]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    let reading = print_win::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;

    // The owner window, so the panel is modal to the document rather than floating
    // free. `None` is degradation and not failure: a print dialog with no owner is
    // still a print dialog, where refusing to print because a window handle could
    // not be found would be a worse outcome than a slightly misplaced panel.
    // Carried across the thread boundary as an integer, not as an `HWND`. A raw
    // handle is a `*mut c_void` and therefore not `Send`, and the compiler is right
    // to say so in general --- but a window handle is a process-wide kernel-managed
    // value with no thread affinity for this use, and `PrintDlgW` only ever reads
    // it to parent a dialog. Reconstructed on the far side rather than smuggled
    // through a wrapper type, so the one unsound-looking step is one line and is
    // where the reasoning is written down.
    let owner = {
        use tauri::Manager;
        app.get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize)
    };

    std::thread::spawn(move || {
        let owner = owner.map(|h| windows::Win32::Foundation::HWND(h as *mut std::ffi::c_void));
        if let Err(e) = print_win::present(&bytes, &title, owner) {
            diag::note(&format!("[print] {e}"));
        }
    });
    Ok(())
}

/// The remaining platforms, where nothing is written.
///
/// An error rather than a no-op: a print command that quietly does nothing is the
/// worse of the two failures. Both shipping targets have an implementation above,
/// so this arm exists for a Linux build that does not yet exist.
#[cfg(not(any(target_os = "macos", windows)))]
fn present_job(
    _app: &tauri::AppHandle,
    _bytes: Vec<u8>,
    _title: String,
    _expected: Option<usize>,
) -> Result<(), String> {
    Err("printing is implemented on macOS and Windows only".into())
}

/// Milliseconds since process exec, so the frontend can place its own marks on
/// the same timeline as the Rust side (spike 0.2).
#[tauri::command]
fn process_elapsed_ms() -> f64 {
    startup::since_process_start_ms()
}

/// The mark that says the webview executed a line of JavaScript.
const WEBVIEW_ALIVE: &str = "webview alive";

/// Reads a spike's environment variable, recording that the webview asked.
///
/// Every spike entry point begins by asking Rust for its path or config, so the
/// *first* of these calls is proof that the page loaded and ran. That matters
/// because the alternative failure --- WebKit suspending a page whose window is
/// occluded --- produces no output at all, and is otherwise indistinguishable
/// from a run that is merely slow. The watchdog keys its diagnosis on this mark;
/// `mark` is first-wins, so the four callers leave one entry between them.
fn spike_env(key: &str) -> Option<String> {
    startup::mark(WEBVIEW_ALIVE);
    std::env::var(key).ok()
}

/// Path to auto-benchmark on startup, from `TPDF_AUTOBENCH`.
///
/// The webview half of spike 0.1 has to run inside a real webview, but a
/// measurement that needs someone to click a button is a measurement that does
/// not get repeated. With this set, the app opens the document, runs the
/// transfer benchmark and exits, so the whole thing is one shell command.
#[tauri::command]
fn autobench_path() -> Option<String> {
    spike_env("TPDF_AUTOBENCH")
}

/// What the file-association check should assert, from `TPDF_OPENCHECK`.
///
/// Like the session check, this observes the real boot rather than replacing
/// it. Note the environment reaches the app even when Launch Services starts it:
/// `TPDF_OPENCHECK=... open -a tpdf.app file.pdf` does propagate, which is what
/// makes the actual double-click path testable rather than merely argued.
#[tauri::command]
fn opencheck_mode() -> Option<String> {
    spike_env("TPDF_OPENCHECK")
}

/// What the mark check should do this launch, from `TPDF_MARKCHECK`.
///
/// [`sessioncheck_mode`]'s posture rather than [`opencheck_mode`]'s, and for a
/// sharper version of the same reason: what it checks is the *wiring* between a
/// command, a gesture on the viewer, the edit model and the overlay --- all of
/// which lives in `App.svelte` and none of which exists in a harness that builds
/// its own `Viewer`. So the app boots normally and the check drives it through
/// the same handles a reader's pointer reaches.
///
/// It exists because a shape drawn on the last page of a document was silently
/// dropped for a fortnight while every gate stayed green: each side of that join
/// asserted its own half and was right about it. See `src/lib/markcheck.ts`.
#[tauri::command]
fn markcheck_mode() -> Option<String> {
    spike_env("TPDF_MARKCHECK")
}

/// What the session check should do this launch, from `TPDF_SESSIONCHECK`.
///
/// Unlike the other spike entry points this one does *not* replace the
/// application: session restore happens during the real boot, so a check that
/// bypassed it would be checking a second implementation. The mode says which
/// half of a two-launch run this is; the app boots normally either way and the
/// check observes it. See `src/lib/sessioncheck.ts`.
#[tauri::command]
fn sessioncheck_mode() -> Option<String> {
    spike_env("TPDF_SESSIONCHECK")
}

/// Everything the scroll benchmark needs to run without a human (spike 0.8).
///
/// Read from the environment rather than compiled in, so a variant sweep --- a
/// different scroll speed, a different tile size --- is a shell line rather than
/// a rebuild. Defaults are the shape docs/PLAN.md section 4 arrived at: the
/// fewest, largest tiles, and one screen of prefetch either way.
#[derive(serde::Serialize)]
struct ScrollBenchConfig {
    path: String,
    rounds: usize,
    frames: usize,
    warmup_frames: usize,
    px_per_frame: f64,
    tile_px: u32,
    zooms: Vec<f64>,
    layouts: Vec<String>,
    cache_tiles: usize,
    max_in_flight: usize,
    prefetch_screens: f64,
    /// Whether stale requests are withdrawn, as a variant dimension so the two
    /// behaviours can be interleaved rather than compared across runs.
    cancels: Vec<u8>,
}

/// What `raw` means for `name`, or `None` --- having said so through `say`.
///
/// A *set* value that cannot be read is announced, because the alternative is a
/// run that quietly used the default and reported it as the variant that was
/// asked for: `TPDF_SCROLL_ROUNDS=1O` measures five rounds, and every number
/// downstream is then about a configuration nobody chose. An absent variable is
/// the ordinary case and says nothing --- the callers return before reaching
/// here.
///
/// The sink is a parameter for the reason `diag::note_to` takes one: the line is
/// otherwise observable only on stderr, so a check for it would have to re-exec
/// the test binary to read its own output.
fn parse_setting<T: std::str::FromStr>(name: &str, raw: &str, say: &dyn Fn(&str)) -> Option<T> {
    match raw.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            // Quoted, so the two values that are invisible in a shell line ---
            // an empty one, and one carrying whitespace --- can be seen here.
            say(&format!(
                "[WARN] {name}={raw:?} could not be read; using the default"
            ));
            None
        }
    }
}

/// Reads a `TPDF_`-prefixed environment variable, falling back to `default`.
fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    parse_setting(name, &raw, &diag::note).unwrap_or(default)
}

/// Reads a comma-separated list, falling back to `default`.
///
/// Per item, so a list with one unreadable entry names that entry rather than
/// the whole value --- and keeps the entries either side of it, which is what
/// it did before anything was said out loud.
fn env_list<T: std::str::FromStr>(name: &str, default: Vec<T>) -> Vec<T> {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    let parsed: Vec<T> = raw
        .split(',')
        .filter_map(|item| parse_setting(name, item.trim(), &diag::note))
        .collect();
    if parsed.is_empty() {
        default
    } else {
        parsed
    }
}

/// The scroll benchmark's configuration, or `None` if none was requested.
#[tauri::command]
fn scrollbench_config() -> Option<ScrollBenchConfig> {
    let path = spike_env("TPDF_SCROLLBENCH")?;

    Some(ScrollBenchConfig {
        path,
        rounds: env_or("TPDF_SCROLL_ROUNDS", 5),
        frames: env_or("TPDF_SCROLL_FRAMES", 300),
        warmup_frames: env_or("TPDF_SCROLL_WARMUP", 180),
        // A brisk flick rather than a reading scroll: ~3600 css px/s at 120 Hz.
        // The demanding case is the one the criterion is about.
        px_per_frame: env_or("TPDF_SCROLL_PX", 30.0),
        tile_px: env_or("TPDF_SCROLL_TILE", 1024),
        zooms: env_list("TPDF_SCROLL_ZOOMS", vec![1.0, 4.0]),
        layouts: env_list(
            "TPDF_SCROLL_LAYOUTS",
            vec!["tiles".to_string(), "viewport".to_string()],
        ),
        cache_tiles: env_or("TPDF_SCROLL_CACHE", 32),
        max_in_flight: env_or("TPDF_SCROLL_INFLIGHT", 4),
        prefetch_screens: env_or("TPDF_SCROLL_PREFETCH", 1.0),
        // One value by default, so an ordinary run is not twice the size. Pass
        // `0,1` to measure what withdrawal is worth.
        cancels: env_list("TPDF_SCROLL_CANCEL", vec![1]),
    })
}

/// Path to run the viewer's functional check against, from `TPDF_VIEWERCHECK`.
///
/// Unlike the benchmarks either side of it this one asserts rather than
/// measures --- see `src/lib/viewercheck.ts` --- and it needs a real webview for
/// the same reason they do: the frame loop, the input handlers and the layout it
/// checks do not exist anywhere else.
#[tauri::command]
fn viewercheck_path() -> Option<String> {
    spike_env("TPDF_VIEWERCHECK")
}

/// A writable path a check may save to, from `TPDF_VIEWERCHECK_SCRATCH`.
///
/// The webview has no filesystem, so a phase that wants to compare what the
/// overlay draws against what the *file* renders has nowhere to put the file.
/// `viewer_check.py` makes a temporary path, binds it here and deletes it after
/// the run; a check that gets `None` says it had nowhere to write rather than
/// passing.
///
/// Deliberately a path and not a directory: a check writing wherever it liked
/// inside the app process is a wider authority than any of these need, and one
/// name is the smallest thing that makes the comparison possible.
#[tauri::command]
fn viewercheck_scratch() -> Option<String> {
    spike_env("TPDF_VIEWERCHECK_SCRATCH")
}

/// The reading-order expectations a check should assert against, if any.
///
/// Returns the *contents* of the file named by `TPDF_READING_MANIFEST`, because
/// the webview has no filesystem and the manifest is written by whatever
/// generated the fixture --- which is the point of it. A missing or unreadable
/// file is `None`, and the check then says it had nothing to compare against
/// rather than passing.
#[tauri::command]
fn reading_manifest() -> Option<String> {
    std::fs::read_to_string(spike_env("TPDF_READING_MANIFEST")?).ok()
}

/// The page geometry a check should assert the layout against, if any.
///
/// The same arrangement as [`reading_manifest`] and separate from it on purpose.
/// `viewer_check.py` binds any `<fixture>-manifest.json` to that variable and the
/// reading-order check then asserts it page by page, so a fixture that makes no
/// claim about reading order cannot use that name --- `testdata/mixed.pdf`
/// carries markers at its own corners rather than a sentence, and a manifest
/// under the other name would enrol it in a check it was not built for and
/// cannot pass. Its generator writes `mixed-geometry.json`, and this is the
/// variable that carries it.
#[tauri::command]
fn geometry_manifest() -> Option<String> {
    std::fs::read_to_string(spike_env("TPDF_GEOMETRY_MANIFEST")?).ok()
}

/// What a corpus's generator says is in it, if the fixture has such a sidecar.
///
/// The third of these, on the same arrangement and separate for the same reason.
/// `<fixture>-corpus.json` is written by `make_comments_pdf.py` and states, among
/// other things, the words the one bare mark in the corpus is drawn over --- so
/// the comments panel's covered-words check compares against a string a
/// different program wrote, rather than against anything derived from the reader
/// it is testing.
///
/// It carried that expectation for one commit with nothing reading it, which is
/// a claim written down and not enforced. The first hand-written version of it
/// named the wrong line.
///
/// Keyed here rather than in the webview, because the sidecar covers **several**
/// fixtures --- one generator writes `comments.pdf` and `comments-rotated.pdf`
/// --- and the process that knows which one is open is this one. The key is the
/// file name of [`viewercheck_path`], so a check reading this is looking at the
/// entry for the document it has in front of it and cannot silently assert one
/// fixture's expectations against another's.
///
/// `None` where there is no sidecar, no entry for this fixture, or nothing
/// readable --- all of which a check must report as "nothing to compare
/// against" rather than as a pass.
#[tauri::command]
fn corpus_manifest() -> Option<String> {
    let raw = std::fs::read_to_string(spike_env("TPDF_CORPUS_MANIFEST")?).ok()?;
    let all: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let path = viewercheck_path()?;
    let name = std::path::Path::new(&path).file_name()?.to_str()?;
    Some(all.get(name)?.to_string())
}

/// Path to time a cold open of on startup, from `TPDF_STARTUP` (spike 0.2).
#[tauri::command]
fn startup_path() -> Option<String> {
    spike_env("TPDF_STARTUP")
}

/// Records a webview-observed milestone on the process timeline.
///
/// `at_ms` is required rather than stamped here: every mark the webview cares
/// about happened before it could tell us, so stamping on arrival would measure
/// the IPC call instead of the event.
#[tauri::command]
fn startup_mark(name: String, at_ms: f64) {
    startup::mark_at(&name, at_ms);
}

/// The full startup timeline, Rust and webview marks merged.
#[tauri::command]
fn startup_timeline() -> Vec<(String, f64)> {
    startup::timeline()
}

/// Whether the pre-`main` interval could be measured on this platform.
///
/// The frontend needs to know, because a timeline that silently starts at
/// `main` would report a startup budget that excludes dyld.
#[tauri::command]
fn startup_pre_main_ms() -> Option<f64> {
    startup::pre_main_ms()
}

/// Prints spike output on the process's stdout.
///
/// Webview `console.log` does not reliably reach the terminal across platforms,
/// and the results need to land somewhere a script can read.
#[tauri::command]
fn spike_print(text: String) {
    println!("{text}");
}

/// Ends an automated spike run, with the code the run asked for.
///
/// **`AppHandle::exit` does not set the process's exit code.** It ends the event
/// loop, `App::run` then returns normally, `run()` returns, `main` returns unit
/// --- and the process exits 0 whatever was asked for. Every automated run here
/// therefore reported success for its whole existence, including
/// `scripts/viewer_check.py`, whose `return completed.returncode` could not fail.
/// Found 2026-07-27 by a session-check phase that printed `[FAIL]` and `0/1
/// checks passed` above a harness verdict of `[OK]`.
///
/// `process::exit` skips destructors, which is right here rather than merely
/// acceptable: the render thread owns PDFium handles and a spike that has
/// printed its results has nothing left to tear down. Stdout is flushed first
/// because that is the entire product of the run.
#[tauri::command]
fn spike_exit(code: i32) {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

/// Kills the process if an automated spike run has not finished in time.
///
/// Every automated run ends by calling `spike_exit` from the webview, so a
/// webview that never gets there leaves the app sitting in its event loop with
/// no output at all --- indistinguishable from a slow run, and the harness's own
/// timeout reports only that something took too long. Printing the marks that
/// *were* reached says where it stopped.
fn start_watchdog() {
    // The scroll benchmark is frame-driven, which is exactly the thing WebKit
    // stops doing when the window stops being visible, so it needs the watchdog
    // more than the others do --- and it needs far longer, since it runs every
    // variant in one launch rather than one launch per sample.
    let seconds: u64 = if std::env::var_os("TPDF_SCROLLBENCH").is_some() {
        env_or("TPDF_SCROLL_TIMEOUT", 900)
    } else if std::env::var_os("TPDF_VIEWERCHECK").is_some() {
        // Frame-driven like the scroll benchmark, and so exposed to the same
        // suspension, but it waits on renders rather than counting frames.
        env_or("TPDF_VIEWERCHECK_TIMEOUT", 300)
    } else if std::env::var_os("TPDF_OPENCHECK").is_some() {
        // One of its phases deliberately waits for a document that another
        // process sends it, so it outlives a plain boot by design.
        env_or("TPDF_OPENCHECK_TIMEOUT", 120)
    } else if std::env::var_os("TPDF_SESSIONCHECK").is_some() {
        // Opens a document and waits for one screen, twice per two-launch run.
        env_or("TPDF_SESSIONCHECK_TIMEOUT", 120)
    } else if std::env::var_os("TPDF_STARTUP").is_some()
        || std::env::var_os("TPDF_AUTOBENCH").is_some()
    {
        30
    } else {
        return;
    };

    std::thread::Builder::new()
        .name("tpdf-watchdog".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            eprintln!("[FAIL] spike run did not finish within {seconds} s. Reached:");
            let marks = startup::timeline();
            for (name, at) in &marks {
                eprintln!("  {name:<30} {at:>9.1}");
            }

            // The difference between "slow" and "never started" is one mark, and
            // without saying so out loud this reads as a hang in whatever was
            // most recently changed. It is usually not: WebKit suspends a page
            // whose window is fully covered, and an occluded window is not a
            // locked screen, so `webview_guard.py` passes and nothing runs.
            if !marks.iter().any(|(name, _)| name == WEBVIEW_ALIVE) {
                for line in [
                    format!("No `{WEBVIEW_ALIVE}` mark: the page never ran a line of JavaScript,"),
                    "so this is not a slow run. WebKit suspends a page whose window is".into(),
                    "occluded --- covered by another window, or on another Space --- and".into(),
                    "an unlocked screen is not a visible one.".into(),
                    String::new(),
                    "Re-run with TPDF_RAISE=1, or with nothing covering the window.".into(),
                    "See BUILD.md.".into(),
                ] {
                    eprintln!("       {line}");
                }
            }
            // Straight out, not through the app handle: the point of this path
            // is that the event loop may be the thing that is stuck.
            std::process::exit(2);
        })
        .expect("failed to spawn watchdog thread");
}

/// Starts the document open now, before anything can ask for it.
///
/// Returns `None` unless both a path and the opt-in are set, so the variant is
/// off by default and the baseline stays the baseline.
fn start_eager_open(service: &RenderService) -> Option<EagerOpen> {
    std::env::var_os("TPDF_EAGER_OPEN")?;
    let path = PathBuf::from(std::env::var("TPDF_STARTUP").ok()?);

    let (reply, rx) = reply_channel();
    // No password: nothing has had the chance to ask for one this early, and a
    // locked document simply comes back locked for `open_document` to relay.
    service.open(path.clone(), lazy_geometry(), None, reply);
    startup::mark("eager open requested");
    Some(EagerOpen {
        path,
        pending: Mutex::new(Some(rx)),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    startup::mark_process_start();

    // Before anything else, and before the watchdog: this process may not be the
    // app at all. A worker is this executable re-exec'd with a marker argument,
    // and everything below --- the watchdog, the Tauri context, a window ---
    // would be wrong for it. It never returns.
    let args: Vec<String> = std::env::args().collect();
    // The OCR worker, checked first because it is the narrower marker and
    // because it shares nothing with the parser worker but this dispatch: it
    // maps no PDF library, opens no document, and applies a different profile.
    // Through the helper rather than spelled out here, because this dispatch is
    // not unique to the application --- the two probes that re-exec themselves
    // as their own OCR worker carry it too, and the platform gate that used to
    // be written on this line was widened here and nowhere else.
    ocr_worker::child_main_if_asked(&args);
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        // No platform gate here any more. The refusal that mattered was never
        // this one --- it is `establish_boundary`, inside the worker, which fails
        // where there is no boundary to establish and takes the process down
        // before a document is opened. Refusing here as well would have looked
        // like belt and braces while actually hiding which of the two is
        // load-bearing.
        worker_child::main(&args);
    }

    // Also before the watchdog, and for a reason the panic in `RenderService::start`
    // cannot serve. That call happens in the setup hook, which `App::run` invokes
    // from inside AppKit's frames --- so a panic there is *non-unwinding*, aborts
    // through a backtrace with no symbols, and races the watchdog's own 30-second
    // report about an occluded webview. A misspelt environment variable would then
    // be diagnosed as a suspended page. Read it here, where there is no event loop
    // to lose the message in and no window to be occluded.
    if let Err(e) = render::Backend::from_env() {
        eprintln!("[FAIL] {e}");
        std::process::exit(2);
    }

    start_watchdog();
    let mode = ShellMode::from_env();

    let mut context = tauri::generate_context!();
    // Everything before this is ours: reading the embedded config and building
    // the asset table. Everything after it, up to the setup hook, is Tauri's.
    startup::mark("context built");
    if mode != ShellMode::Config {
        context.config_mut().app.windows.clear();
    }

    // Managed on the builder rather than in the setup hook, and the difference
    // is not stylistic. **`RunEvent::Opened` fires before setup runs**, so with
    // this registered there `state::<Launch>()` panics inside the run callback
    // on exactly the path it exists to serve: a cold double-click. The window
    // appears, nothing else happens, and the last startup mark is `app built`.
    //
    // Queued here for the same reason: on Windows a double-click arrives in
    // `argv`, long before there is a webview to tell about it.
    let launch = launch::Launch::default();
    for path in launch::paths_from_args(std::env::args()) {
        launch.deliver(path);
    }

    let mut builder = tauri::Builder::default()
        .manage(launch)
        // One edit model per open document. Managed on the builder rather than in
        // the setup hook because it needs nothing from the app --- no path, no
        // library directory --- and because `RunEvent::Opened` can fire before
        // the hook runs, which is the trap the render service works around.
        .manage(edits::Edits::default())
        .plugin(tauri_plugin_dialog::init())
        // The one place tpdf talks to the network, and the only code path that
        // can replace the binary. It is deliberately inert until the frontend
        // asks: the plugin registers commands and makes no request of its own,
        // so a launch that never calls `check()` reaches no endpoint at all ---
        // which is what keeps every spike and check run offline.
        //
        // What makes this safe to have at all is that the payload is verified
        // against `plugins.updater.pubkey` in `tauri.conf.json` BEFORE anything
        // is unpacked, so the archive parsers this pulls in (zip, tar) never see
        // bytes that were not signed by the key in `docs/THREAT-MODEL.md` §T9.
        .plugin(tauri_plugin_updater::Builder::new().build());

    // The native menu bar. macOS only, for the reason `menu.rs` gives: there the
    // bar is outside the window and its emptiness was the defect, and on Windows
    // it would be chrome inside the window that this application exists to avoid.
    //
    // Registered on the builder rather than in the setup hook, and the handler
    // has to be: a menu event can only arrive once there is a menu, which the
    // frontend installs, but `on_menu_event` is a builder method and there is no
    // later place to add one.
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .manage(menu::MenuItems::<tauri::Wry>::default())
            // The id travels to the frontend and is run through the same
            // registry the palette uses. Nothing is decided here --- a menu that
            // acted in Rust would be a second implementation of every command in
            // it, with its own copy of each `enabled` guard.
            .on_menu_event(|app, event| menu::forward(app, event.id().as_ref()));
    }

    // The Windows counterpart of the `RunEvent::Opened` arm at the bottom of this
    // file, and the reason it exists is parity rather than tidiness: without it a
    // second launch is a **second process**, with its own window and its own worker
    // pool, where macOS hands the document to the app already running. That was
    // measured by `open_check.py` before it was fixed --- two phases skipped there
    // with exactly that reason printed.
    //
    // Registered first, before anything else can run, because the plugin's job is
    // partly to *not* start: in the second process it forwards argv to the first and
    // then exits. A plugin registered after something with side effects would let
    // the doomed process do that work first.
    //
    // The callback deliberately goes through the same `Launch` queue and the same
    // `OPEN_EVENT` as every other route into the app. A second mechanism for "open
    // this document" is a second place for the queue-versus-emit decision to drift,
    // and `docs/TRAPS.md` records what two copies of one distinction cost.
    #[cfg(windows)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::{Emitter, Manager};
            // `try_state`, for the same reason the macOS arm gives: a panic inside a
            // plugin callback is invisible, and the degradation is one document not
            // opening rather than a window with nothing in it.
            let Some(launch) = app.try_state::<launch::Launch>() else {
                return;
            };
            // Raising the window is the visible half. A handover that silently
            // loaded the document behind whatever the reader was looking at would
            // read as "the double-click did nothing", which is the failure this
            // whole path exists to avoid.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            for path in launch::paths_from_args(argv) {
                if let launch::Delivery::Emit(path) = launch.deliver(path) {
                    let _ = app.emit(launch::OPEN_EVENT, path.to_string_lossy().into_owned());
                }
            }
        }));
    }
    if std::env::var_os("TPDF_EMPTY_MENU").is_some() {
        // Tauri installs a full default application menu on macOS. Building it
        // means constructing every item and submenu through AppKit, which is
        // not obviously free at 37 ms of builder time --- so it gets measured
        // rather than assumed.
        builder = builder.menu(tauri::menu::Menu::new);
    }

    let app = builder
        .setup(move |app| {
            startup::mark("tauri setup");
            // First, and before the render service exists, so that everything
            // said on the way up is caught rather than only what happens once
            // the application is running. It is a `OnceLock` set and nothing
            // else --- no directory is created and no file is opened until
            // there is a line to write --- so a launch that never has anything
            // to say pays nothing for this.
            //
            // This is also the earliest it *can* happen: the path comes from
            // Tauri's resolver, which needs the app. Anything diagnosed before
            // here --- the backend refusal in `run`, the watchdog --- is on
            // stderr only, which is correct for both: they are reached under a
            // `TPDF_*` variable by a harness that captures stderr, and the
            // first of them exits before there is an event loop to lose a
            // message in.
            diag::start(log_file(app.handle()));
            let dir = pdfium_library_dir(app.handle());
            let service = RenderService::start(dir);
            if let Some(pending) = start_eager_open(&service) {
                app.manage(pending);
            }
            app.manage(service);

            // A frame-rate measurement in an unfocused window measures the
            // throttle, not the platform. The app is launched from a script, so
            // nothing else would raise it, and the resulting cadence would look
            // exactly like a ceiling WebKit had imposed on us.
            //
            // The viewer *check* does not do this by default: it asserts
            // behaviour rather than timing it, so an unfocused window costs it
            // nothing --- and raising a window over whatever someone is doing,
            // every time a check runs, is its own bug.
            //
            // But unfocused and *occluded* are different things, and the
            // difference is not cosmetic: WebKit suspends a page whose window
            // is fully covered, so a check launched from a shell behind a
            // full-screen terminal never runs a single line of frontend code.
            // It does not fail --- it produces nothing, which is why
            // `TPDF_RAISE` exists. Opt-in, so the default stays polite and a
            // run that has nowhere visible to put a window can still say what
            // it needs.
            if std::env::var_os("TPDF_SCROLLBENCH").is_some()
                || std::env::var_os("TPDF_RAISE").is_some()
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }

            if mode != ShellMode::Config {
                startup::mark("window build start");
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App(mode.page().into()),
                )
                .title("tpdf")
                .inner_size(1200.0, 900.0)
                .build()?;
                // `build()` returns once the webview exists and has been told
                // what to load, not once it has loaded it.
                startup::mark("window built");
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("tile", |ctx, request, responder| {
            let service = ctx.app_handle().state::<RenderService>();
            protocol::handle(&service, request, responder);
        })
        .invoke_handler(tauri::generate_handler![
            open_document,
            page_rotate,
            page_crop,
            page_content_box,
            page_geometry,
            page_crop_box,
            page_delete,
            page_move,
            page_insert,
            annot_mark,
            annot_remove,
            redact_mark,
            redact_remove,
            redaction_plans,
            redact_copy,
            redact_document,
            annot_erase,
            annot_note,
            annot_rewrite,
            annot_discard,
            annot_recolor,
            annot_move,
            edit_undo,
            edit_redo,
            edit_state,
            save_document,
            save_copy,
            extract_pages,
            split_document,
            merge_documents,
            keyboard_positions,
            set_menu,
            set_menu_enabled,
            close_document,
            release_documents,
            page_text,
            search_page,
            document_outline,
            document_comments,
            document_links,
            document_properties,
            document_mapping,
            launch_open_event,
            app_version,
            take_launch_paths,
            session_load,
            session_remember,
            session_set_invert_pages,
            print_document,
            process_elapsed_ms,
            autobench_path,
            viewercheck_path,
            viewercheck_scratch,
            reading_manifest,
            corpus_manifest,
            geometry_manifest,
            sessioncheck_mode,
            opencheck_mode,
            markcheck_mode,
            startup_path,
            scrollbench_config,
            startup_mark,
            startup_timeline,
            startup_pre_main_ms,
            spike_print,
            spike_exit
        ])
        .build(context)
        .expect("error while building tpdf");

    // Distinct from the setup hook: everything the builder does after it ---
    // menus, tray, remaining runtime wiring --- lands here.
    startup::mark("app built");

    app.run(|_handle, event| {
        if matches!(event, tauri::RunEvent::Ready) {
            startup::mark("event loop ready");
        }

        // How a double-click reaches tpdf on macOS. Launch Services sends an
        // Apple Event and nothing appears in `argv` at all, so this arm is the
        // *only* route for the way most people will open a document --- and it
        // can fire before the webview exists, which is why it queues rather
        // than emitting unconditionally.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &event {
            use tauri::{Emitter, Manager};
            // `try_state`, not `state`: the latter panics on unmanaged state,
            // and this arm runs before the setup hook. It is managed on the
            // builder now so this cannot be `None`, but a panic here is
            // invisible --- a window with nothing in it --- and the degradation
            // is one document not opening.
            let Some(launch) = _handle.try_state::<launch::Launch>() else {
                return;
            };
            for url in urls {
                let Some(path) = launch::path_from_url(url) else {
                    continue;
                };
                if let launch::Delivery::Emit(path) = launch.deliver(path) {
                    let _ = _handle.emit(launch::OPEN_EVENT, path.to_string_lossy().into_owned());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {

    /// The compile-time development path must not be a candidate in a release.
    ///
    /// **Both arms are real, and the release one is why this exists.**
    /// `CARGO_MANIFEST_DIR` is the *build* machine's checkout, so a release built
    /// by CI would otherwise look inside the runner's tree first, on every
    /// launch, on every machine that installed it. Under `cargo test` the debug
    /// arm runs and is the control --- it proves the path is still found where
    /// developers need it, so the release arm is a decision rather than the
    /// function having quietly stopped working.
    ///
    /// Run the other arm with `cargo test --release`.
    #[test]
    fn the_development_library_path_is_a_debug_only_candidate() {
        let dev = super::dev_library_dir();
        if cfg!(debug_assertions) {
            let dev = dev.expect("a debug build must still find the vendored library");
            assert!(
                dev.ends_with(std::path::Path::new("vendor/pdfium").join(super::PDFIUM_SUBDIR)),
                "the debug candidate is the vendored tree: {dev:?}"
            );
        } else {
            assert_eq!(
                dev, None,
                "a release build must not consult the build machine's checkout"
            );
        }
    }

    /// Only the two macOS-only spikes may hardcode `vendor/pdfium/lib`.
    ///
    /// [`PDFIUM_SUBDIR`]'s own note states this invariant and names the command
    /// that checks it --- `grep -rn 'vendor/pdfium/lib' src-tauri/examples` ---
    /// and says what it should return. Nothing ran that command. The constant
    /// exists because the fact had been rediscovered three times, by
    /// `worker-probe`, then `backend-probe`, then `text-probe`; on 2026-08-25 it
    /// was a fourth, with `crop-probe`, `geometry-probe`, `merge-probe` and
    /// `turned-probe` all unable to bind on Windows because `lib/` holds the
    /// *import* library there, so the directory exists and the load fails much
    /// later pointing at a path that is right there.
    ///
    /// So this is the same rule with a test behind it, which is the difference
    /// between a rule and a comment. It is deliberately a **set** comparison and
    /// not a count: `PDFIUM_SUBDIR`'s note records that the count in its own
    /// prose said four when the real number was nine, which is why that sentence
    /// was replaced by a rule in the first place.
    ///
    /// A binary that genuinely is macOS-only belongs in [`MAC_ONLY`] with the
    /// reason; anything else must ask the constant.
    #[test]
    fn only_the_macos_spikes_hardcode_the_library_directory() {
        /// The ones where `lib` is simply correct, because they do not build
        /// anywhere else: `fdpass-probe` carries a POSIX `SCM_RIGHTS` handover,
        /// and the two remaining OCR spikes drive macOS Vision itself ---
        /// `ocr-probe` the binding, `ocr-sandbox-probe` the SBPL profiles it runs
        /// under.
        ///
        /// **`ocr-worker-probe` left this list on 2026-08-29** and the shape of
        /// why is worth keeping: it measures the *worker*, not the engine, and it
        /// was pinned here only because its in-process baseline named `Vision`
        /// directly. With `WindowsOcr` behind the same `ocr::Recogniser` the
        /// baseline is three lines of platform and the rest is the trait --- so a
        /// spike is macOS-only when its *subject* is, never when one line of its
        /// scaffolding is.
        const MAC_ONLY: [&str; 3] = ["fdpass_probe.rs", "ocr_probe.rs", "ocr_sandbox_probe.rs"];

        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut found: Vec<String> = Vec::new();
        let mut scanned = 0usize;
        let entries = std::fs::read_dir(&examples).expect("the examples directory must be there");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            scanned += 1;
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if source.contains("vendor/pdfium/lib") {
                found.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }

        // The refusal that makes the rest mean anything: a scan that read no
        // files finds no offenders, and passes exactly like a clean tree.
        assert!(
            scanned > 20,
            "only {scanned} example sources were scanned; the directory walk is wrong"
        );

        found.sort();
        let mut allowed: Vec<String> = MAC_ONLY.iter().map(|s| (*s).to_string()).collect();
        allowed.sort();
        assert_eq!(
            found, allowed,
            "every portable spike must join PDFIUM_SUBDIR rather than `lib`, which is the \
             import library on Windows and binds to nothing"
        );
    }
    use super::{
        app_version, await_reply, env_list, env_or, parse_setting, reply_channel, spike_env,
        with_close_note, with_session, SaveFailure, WEBVIEW_ALIVE,
    };

    /// A save that failed after the close, with the document closing cleanly.
    ///
    /// The message is left alone, which is the ordinary case: nothing else went
    /// wrong and a note about the close would be a sentence about nothing.
    #[test]
    fn a_clean_close_adds_nothing_to_a_failure() {
        let why = with_close_note(SaveFailure::after_close("the rename failed"), Ok(()));
        assert_eq!(why.message, "the rename failed");
        assert!(why.reopen);
        assert!(!why.changed);
    }

    /// Both things went wrong, and the reader is told both once.
    #[test]
    fn a_failed_close_is_added_to_the_failure_the_reader_sees() {
        let why = with_close_note(
            SaveFailure::after_close("the rename failed"),
            Err("the worker did not answer".into()),
        );
        assert_eq!(
            why.message,
            concat!(
                "the rename failed --- and the document did not close cleanly: ",
                "the worker did not answer"
            )
        );
    }

    /// **The flags are what a program branches on, and this must not touch
    /// them.** `changed` decides whether the window offers Reload, and a note
    /// about the close says nothing about whether the file moved --- so a
    /// decoration that reset it would withdraw the one action that helps.
    #[test]
    fn a_close_note_changes_the_sentence_and_not_the_fields() {
        let before = SaveFailure {
            message: "refused".into(),
            reopen: true,
            changed: true,
        };
        let after = with_close_note(before, Err("also this".into()));
        assert!(after.reopen);
        assert!(after.changed);
        assert!(after.message.starts_with("refused --- and the document"));
    }

    /// The four files a version bump has to move together, checked at build time.
    ///
    /// `BUILD.md` step 2 lists them and nothing enforced the list: `package.json`,
    /// `package-lock.json`, `Cargo.toml` and `tauri.conf.json` were kept in step by
    /// hand, and a bump that moved three of them produced an installer whose
    /// filename, whose `Cargo.lock` and whose reported version could disagree with
    /// no gate going red. The application now *reports* its version to a reader,
    /// which turns a silent inconsistency into a wrong answer given confidently.
    ///
    /// Two of the four are reachable from here through `include_str!`, so they are
    /// compared for real rather than described. `package-lock.json` is not: it is
    /// two copies of the same string in one file, `npm version` writes both, and
    /// pulling a 400 kB lockfile into the binary to check it is the wrong trade.
    #[test]
    fn the_version_files_agree_with_the_crate() {
        let cargo = env!("CARGO_PKG_VERSION");
        assert_eq!(
            app_version(),
            cargo,
            "the command must report the crate's own version"
        );

        for (label, source) in [
            ("tauri.conf.json", include_str!("../tauri.conf.json")),
            ("package.json", include_str!("../../package.json")),
        ] {
            let parsed: serde_json::Value =
                serde_json::from_str(source).unwrap_or_else(|e| panic!("{label} is not JSON: {e}"));
            let found = parsed
                .get("version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("{label} has no string `version`"));
            assert_eq!(found, cargo, "{label} disagrees with Cargo.toml");
        }
    }

    /// Where the bundler puts PDFium, checked against where the app looks for it.
    ///
    /// **A trailing slash in a Tauri resource map is a rename, not a directory**,
    /// and `docs/TRAPS.md` records that from the macOS side --- which is why the
    /// macOS config names `pdfium/libpdfium.dylib` in full. The Windows twin was
    /// written as `"pdfium/"` and shipped the runtime DLL as a file called
    /// `pdfium`, with no extension. `pdfium_library_dir` then found no
    /// `pdfium.dll` in either bundled candidate, the worker's bind failed, and
    /// every worker exited 1 --- so the installed 26.8.8 could not open any
    /// document at all. It was invisible here because a *locally built* install
    /// is rescued by the first candidate, the dev tree baked in at compile time;
    /// a release binary carries the runner's path, which exists on no machine
    /// that installs it.
    ///
    /// Both configs are checked from whichever host runs, through `include_str!`.
    /// That is the whole point: a Mac never parses the Windows config, which is
    /// how one half of a twin kept a bug the other half had already fixed.
    #[test]
    fn the_bundle_puts_pdfium_where_the_app_looks_for_it() {
        for (label, source, loadable) in [
            (
                "tauri.windows.conf.json",
                include_str!("../tauri.windows.conf.json"),
                "pdfium.dll",
            ),
            (
                "tauri.macos.conf.json",
                include_str!("../tauri.macos.conf.json"),
                "libpdfium.dylib",
            ),
        ] {
            let parsed: serde_json::Value =
                serde_json::from_str(source).unwrap_or_else(|e| panic!("{label} is not JSON: {e}"));
            let resources = parsed
                .get("bundle")
                .and_then(|b| b.get("resources"))
                .and_then(serde_json::Value::as_object)
                .unwrap_or_else(|| panic!("{label} has no bundle.resources map"));

            let (from, to) = resources
                .iter()
                .find(|(from, _)| from.contains("vendor/pdfium"))
                .map(|(from, to)| {
                    (
                        from.clone(),
                        to.as_str()
                            .unwrap_or_else(|| panic!("{label}: {from} maps to a non-string"))
                            .to_owned(),
                    )
                })
                .unwrap_or_else(|| panic!("{label} maps nothing out of vendor/pdfium"));

            assert!(
                !to.ends_with('/'),
                "{label}: {from} -> {to:?} ends in a slash, which renames the file"
            );
            assert_eq!(
                to,
                format!("pdfium/{loadable}"),
                "{label}: {from} must land in the pdfium/ directory the app searches, named as it looks"
            );
        }
    }

    /// The Windows installer is told to clear the way for that directory.
    ///
    /// The fix above changed what the bundle *contains*; it could not change
    /// what a machine already has. 26.8.8 installed the engine as a file named
    /// `pdfium`, and the generated `installer.nsi` copies resources with
    /// `CreateDirectory "$INSTDIR\pdfium"` followed by a `File` into it ---
    /// `CreateDirectory` against an existing file fails and says nothing, so
    /// the `File` reports `Error opening file for writing` and offers Abort,
    /// Retry, Ignore. Under `/S`, which is how `tauri-plugin-updater` runs it,
    /// that becomes Ignore: the installer skips the payload, writes everything
    /// else, registers itself and **exits 0**. An install that looks complete
    /// from every angle a caller can see, with no PDF engine in it.
    ///
    /// `NSIS_HOOK_PREINSTALL` is inserted immediately after `SetOutPath
    /// $INSTDIR` and before the resource copies, which is the one place the
    /// leftover can be removed in time.
    ///
    /// **Two of the three ways to get this wrong are loud, and the third is
    /// not.** Measured on 2026-08-24 rather than assumed. A mistyped key is
    /// refused by the build script's own schema (*"unknown field
    /// `installerHooksTypo`, expected one of ... `installerHooks`"*). A path
    /// naming a file that is not there is refused by the bundler (*"failed to
    /// resolve `bundle > windows > nsis > installerHooks`"*), though only at
    /// bundle time, which is a CI leg rather than a gate. But a file that
    /// exists and defines nothing, or defines a macro under another name, is
    /// swallowed: the generated script guards the call with `!ifmacrodef
    /// NSIS_HOOK_PREINSTALL`, so the bundle builds, the installer runs, and the
    /// step simply does not happen. That last one is what the two `contains`
    /// assertions below are for; the config check above them is cheap
    /// belt-and-braces that fails earlier than the bundler would.
    ///
    /// **And this is a source-level assertion, which cannot see behaviour.** It
    /// says the config names the file and the file says what it should; it
    /// cannot say Tauri included it, or that NSIS ran it, or that it ran early
    /// enough. `BUILD.md`'s release checklist carries the A/B that can --- the
    /// released previous installer against the new one, over the same planted
    /// stray, reading the answer off the filesystem rather than off the exit
    /// code.
    #[test]
    fn the_windows_installer_clears_the_way_for_the_pdfium_directory() {
        const HOOKS: &str = "installer-hooks.nsh";

        let source = include_str!("../tauri.windows.conf.json");
        let parsed: serde_json::Value = serde_json::from_str(source)
            .unwrap_or_else(|e| panic!("tauri.windows.conf.json is not JSON: {e}"));
        let declared = parsed
            .get("bundle")
            .and_then(|b| b.get("windows"))
            .and_then(|w| w.get("nsis"))
            .and_then(|n| n.get("installerHooks"))
            .and_then(serde_json::Value::as_str)
            .expect("tauri.windows.conf.json declares no bundle.windows.nsis.installerHooks");
        assert_eq!(
            declared, HOOKS,
            "the config must name the hook file this test reads, or the two can drift apart"
        );

        let hooks = include_str!("../installer-hooks.nsh");
        assert!(
            hooks.contains("!macro NSIS_HOOK_PREINSTALL"),
            "{HOOKS} defines no NSIS_HOOK_PREINSTALL, so !ifmacrodef skips it in silence"
        );
        assert!(
            hooks.contains("Delete \"$INSTDIR\\pdfium\""),
            "{HOOKS} does not remove the stray file, which is the whole reason it exists"
        );
    }

    use crate::{session, startup};
    use std::cell::RefCell;
    use tauri::async_runtime::block_on;

    /// A place for `path`, with the other fields at values a reader could have.
    fn place_at(path: &str) -> session::Place {
        session::Place {
            path: path.to_owned(),
            page: 3,
            top_pt: 12.0,
            zoom: 1.0,
            fit: session::Fit::default(),
            turns: 0,
            sidebar: false,
            page_count: 12,
        }
    }

    /// Two writers on the pool must not lose each other's edits.
    ///
    /// This is the property the main thread used to provide for free. Both
    /// commands load, edit and save, and `session_set_invert_pages` bypasses the
    /// frontend's write chain, so once the work moved to the blocking pool the
    /// two could interleave and the later save would carry a session read before
    /// the earlier one landed.
    ///
    /// Written as a race rather than as a claim about the lock: sixteen paths
    /// from two threads, all of which have to survive, repeated enough that an
    /// unguarded read-modify-write loses one essentially every run. Verified by
    /// removing the guard --- it fails on the first repetition.
    #[test]
    fn two_session_writers_do_not_lose_each_other_s_edits() {
        let dir = std::env::temp_dir().join(format!("tpdf-session-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("session.json");

        for round in 0..20 {
            let _ = std::fs::remove_file(&path);
            // Seeded, so that "the file was never written" cannot pass as
            // "every edit survived".
            with_session(&path, |s| s.remember(place_at("seed.pdf"))).expect("seed");

            std::thread::scope(|scope| {
                for writer in 0..2 {
                    let path = path.clone();
                    scope.spawn(move || {
                        for n in 0..8 {
                            let name = format!("w{writer}-{n}.pdf");
                            with_session(&path, |s| s.remember(place_at(&name))).expect("write");
                        }
                    });
                }
            });

            let session = session::Session::load(&path);
            let kept: Vec<&str> = session.places.iter().map(|p| p.path.as_str()).collect();
            for writer in 0..2 {
                for n in 0..8 {
                    let name = format!("w{writer}-{n}.pdf");
                    assert!(
                        kept.contains(&name.as_str()),
                        "round {round}: {name} was lost; file holds {kept:?}"
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn alive() -> bool {
        startup::timeline()
            .iter()
            .any(|(name, _)| name == WEBVIEW_ALIVE)
    }

    /// The watchdog's diagnosis is gated on this mark's *absence*, so the mark
    /// has to be produced by something the page cannot reach without running.
    ///
    /// The first assertion is the control and is the point of the test: without
    /// it, a mark that was somehow always present would pass the second one, and
    /// the diagnosis would then never fire --- which is indistinguishable from a
    /// harness that simply never hits the failure.
    ///
    /// Note this is the only test in the crate that touches the global mark
    /// table, which is what makes asserting its emptiness first safe under
    /// `cargo test`'s parallelism.
    #[test]
    fn asking_for_a_spike_path_marks_the_webview_alive() {
        assert!(!alive(), "the mark exists before anything asked for it");
        // Unset on purpose: the mark records that the *page asked*, which it
        // does on every launch, not that the spike was requested.
        assert_eq!(spike_env("TPDF_NO_SUCH_VARIABLE_4711"), None);
        assert!(alive());
    }

    /// The reply the render service was never able to send has to say which
    /// command was waiting for it.
    ///
    /// Every one of the seven shares this failure and used to share the whole
    /// sentence, so an error persisted by `diag.rs` could say that a thread had
    /// stopped and nothing about what had been asked of it.
    /// A parser panic inside a save must reach the reader as a refusal, not as a
    /// closed window.
    ///
    /// **`docs/THREAT-MODEL.md` §3 and residual risk 18 rest on this**, and it
    /// is a property of the build rather than of any code written here: `save`,
    /// `save_copy` and `extract_pages` all parse attacker-controlled bytes with
    /// `lopdf` inside the coordinator, under `spawn_blocking`. That containment
    /// exists only while the crate unwinds. Adding `panic = "abort"` to a
    /// release profile --- a one-line change made for binary size, with nothing
    /// about parsing in view --- would turn every one of those into a process
    /// death taking the reader's unsaved journal with it, and no other check
    /// here would notice.
    ///
    /// So the disclosure is pinned rather than asserted. A claim about runtime
    /// behaviour belongs in an experiment, not in a document, which
    /// `docs/TRAPS.md` records under that name.
    #[test]
    fn a_panic_in_a_blocking_task_is_reported_rather_than_fatal() {
        // The control first: the same call shape with no panic in it, so a
        // runtime that lost every answer could not satisfy the assertion below.
        let fine = block_on(tauri::async_runtime::spawn_blocking(|| 4711_u32));
        assert_eq!(
            fine.ok(),
            Some(4711),
            "the control: an ordinary task answers"
        );

        let panicked = block_on(tauri::async_runtime::spawn_blocking(|| {
            panic!("a parser gave up on a document");
        }));
        assert!(
            panicked.is_err(),
            "a panicking blocking task must come back as an error rather than ending the process"
        );

        // **And the half that actually guards the disclosure.** The two
        // assertions above run under the *test* profile, which does not inherit
        // `[profile.release]` --- so a release-only `panic = "abort"`, a
        // one-line change somebody makes for binary size with nothing about
        // parsing in view, would leave them green while the shipped binary died
        // on the panic they are about. A test that cannot see the change it
        // exists to catch is the recurring subject of `docs/TRAPS.md`. This is
        // the source-level half, and it is the one with teeth.
        //
        // Scope, stated rather than assumed: it reads the crate manifest. A
        // `panic` key in a `.cargo/config.toml` or a workspace root would not be
        // seen, and neither file exists in this repository.
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        )
        .expect("read the crate manifest");
        assert!(
            !manifest.contains("panic"),
            "no profile may set a panic strategy: unwinding is what makes a parser panic on \
             the save path a refusal instead of a closed window (THREAT-MODEL residual risk 18)"
        );
    }

    #[test]
    fn a_lost_reply_names_the_command_that_was_waiting_for_it() {
        // The control, and the reason the two below mean anything: a helper
        // that always failed --- or one that lost the answer --- would satisfy
        // an assertion that only looked at the error.
        let (reply, rx) = reply_channel::<u32, String>();
        reply(Ok(4711));
        assert_eq!(block_on(await_reply("page_text", rx)), Ok(4711));

        // And the service's own refusals pass through untouched, rather than
        // being reworded into a channel failure.
        let (reply, rx) = reply_channel::<u32, String>();
        reply(Err("no such document".to_string()));
        assert_eq!(
            block_on(await_reply("page_text", rx)),
            Err("no such document".to_string())
        );

        // Dropping the callback without calling it is what a caller sees when
        // the thread behind it is gone.
        let (reply, rx) = reply_channel::<u32, String>();
        drop(reply);
        let said = block_on(await_reply("page_text", rx)).unwrap_err();
        assert!(said.contains("render thread stopped"), "{said:?}");
        assert!(
            said.contains("page_text"),
            "the command is the one part of this a reader sending the log back cannot supply: {said:?}"
        );

        // A second name, because a constant baked into the helper would pass
        // every assertion above.
        let (reply, rx) = reply_channel::<u32, String>();
        drop(reply);
        let said = block_on(await_reply("document_outline", rx)).unwrap_err();
        assert!(said.contains("document_outline"), "{said:?}");
    }

    /// A sink that keeps what it was told, standing in for `diag::note`.
    fn recorded(lines: &RefCell<Vec<String>>) -> impl Fn(&str) + '_ {
        |line: &str| lines.borrow_mut().push(line.to_owned())
    }

    #[test]
    fn a_setting_that_cannot_be_read_names_itself_and_the_value_it_refused() {
        let lines = RefCell::new(Vec::new());
        let say = recorded(&lines);

        // The control. Announcing every value read would satisfy a check that
        // only asserts the malformed one produced a line.
        assert_eq!(
            parse_setting::<usize>("TPDF_SCROLL_ROUNDS", "5", &say),
            Some(5)
        );
        assert!(
            lines.borrow().is_empty(),
            "a value that was read fine was announced: {:?}",
            lines.borrow()
        );

        assert_eq!(
            parse_setting::<usize>("TPDF_SCROLL_ROUNDS", "1O", &say),
            None
        );
        let lines = lines.borrow();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("[WARN]"), "{:?}", lines[0]);
        assert!(
            lines[0].contains("TPDF_SCROLL_ROUNDS"),
            "the variable is what the reader has to go and correct: {:?}",
            lines[0]
        );
        assert!(
            lines[0].contains("1O"),
            "the rejected value says which end the typo is at: {:?}",
            lines[0]
        );
    }

    /// An absent variable still reaches its default through both readers.
    ///
    /// It asserts the value and **not** the silence beside it, and the name says
    /// so on purpose: the two callers return before the announcer is reachable,
    /// but a line written there would go to stderr, which this process cannot
    /// read without re-execing itself the way `diag::tests` does. Nothing is set
    /// here either --- `cargo test` runs these in one process, and setting a
    /// variable beside a thread reading one is a data race whatever the name is.
    #[test]
    fn an_unset_setting_falls_back_to_the_default() {
        assert_eq!(env_or("TPDF_NO_SUCH_VARIABLE_4711", 5_usize), 5);
        assert_eq!(
            env_list("TPDF_NO_SUCH_VARIABLE_4711", vec![1.0_f64]),
            vec![1.0]
        );
    }
}
