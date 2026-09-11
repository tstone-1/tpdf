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
/// Every `#[tauri::command]` this crate registers, grouped by the state it
/// touches. The registry below stays here; the bodies do not.
mod commands;
pub mod content;
pub mod diag;
pub mod docgraph;
pub mod docinfo;
pub mod docmodel;
pub mod document;
pub mod edits;
pub mod encoding;
pub mod failure;
pub mod fields;
pub mod fingerprint;
pub mod forms;
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
// The OS opener, and the one place a `/URI` string is judged. Separate modules
// because they are separate questions: `weburl` decides whether an address may
// be opened and what a reader is shown, `opener` hands the result to the
// platform. The split is what lets the first be tested without the second
// reaching the window server.
pub mod opener;
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
pub mod raster_redact;
pub mod recentdocs;
pub mod redact;
pub mod redaction_fill;
pub mod render;
/// One serialised sample of every named reply payload, which
/// `src/lib/replyshapes.test.ts` checks the TypeScript mirror against.
#[cfg(test)]
mod replies;
/// Windows containment, which is what `worker_child`'s `sandbox_init` is on the
/// other platform. Gated because job objects, integrity levels and attribute
/// lists are all Win32 with no portable counterpart.
#[cfg(windows)]
pub mod sandbox_win;
pub mod save;
mod save_order;
pub mod save_outside;
pub mod search;
pub mod session;
pub mod startup;
pub mod structure;
pub mod sweep;
pub mod text;
pub mod textcache;
pub mod verify;

/// Helpers shared by this crate's own tests. Not compiled into any binary.
#[cfg(test)]
mod testutil;
pub mod textbox;
pub mod webopen;
pub mod weburl;
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

use std::path::PathBuf;

use render::RenderService;
use tauri::Manager;

// One glob per group, so `generate_handler!` below can list plain identifiers
// -- which is what `src/lib/ipc.test.ts` reads it as, and what keeps the set of
// registered names in exactly one place. `commands/mod.rs` says why the bodies
// moved and what decides which file a new one goes in.
use commands::document::start_eager_open;
use commands::{
    app::*, document::*, edit::*, menubar::*, print::*, read::*, redact::*, save::*, session::*,
    spike::*,
};

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

/// Where the diagnostics that outlive the run are kept.
///
/// The log directory rather than the config directory beside the session, and
/// that is the one difference from `session_file` in `commands/session.rs` worth stating: this is not
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
pub(crate) fn pdfium_library_dir(app: &tauri::AppHandle) -> PathBuf {
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
/// The type is [`crate::failure::Failure`], which is that distinction and more
/// of them: the two booleans a save answers with are one wire shape of an
/// [`crate::failure::Action`], and the model refusals in `docmodel.rs` convert
/// into the same type without being flattened to a sentence first. The name
/// stays because the two commands that answer one are both saves, and because
/// `save_order.rs` composes one.
pub(crate) use crate::failure::Failure as SaveFailure;

/// Adds what became of the close to a failure that happened after it.
///
/// **Both arms of the save carry this now, and the rewrite's first failure did
/// not before.** `verify_before_commit`'s refusal used to leave through `?` and
/// skip it, which was an accident of where the early return sat rather than a
/// decision: the close has happened either way, so a reader whose document also
/// failed to close should be told once, on whichever refusal reaches them.
///
/// **A function rather than a closure inside the command, so it has a failing
/// case.** `save_document` was an async Tauri command holding the whole
/// sequence, needing a running app, a render service and a real file, so
/// nothing in `cargo test` could call it --- which is this repository's own rule
/// about a guard written inline in a command, arriving in the function whose
/// comments already cited it twice. Since 2026-08-31 the sequence is in
/// `save_order.rs` and *is* reachable, so this is no longer the only tested
/// thing on that path; it stays a function because the rule that put it here is
/// still the right one and its own tests are aimed at it.
///
/// The fields a program branches on are untouched. Only `message` grows, which
/// is the one part of a `SaveFailure` written for a human.
pub(crate) fn with_close_note(mut why: SaveFailure, closed: Result<(), String>) -> SaveFailure {
    if let Err(also) = closed {
        why.message = format!(
            "{} --- and the document did not close cleanly: {also}",
            why.message
        );
    }
    why
}

/// The mark that says the webview executed a line of JavaScript.
pub(crate) const WEBVIEW_ALIVE: &str = "webview alive";

/// Reads a spike's environment variable, recording that the webview asked.
///
/// Every spike entry point begins by asking Rust for its path or config, so the
/// *first* of these calls is proof that the page loaded and ran. That matters
/// because the alternative failure --- WebKit suspending a page whose window is
/// occluded --- produces no output at all, and is otherwise indistinguishable
/// from a run that is merely slow. The watchdog keys its diagnosis on this mark;
/// `mark` is first-wins, so the four callers leave one entry between them.
pub(crate) fn spike_env(key: &str) -> Option<String> {
    startup::mark(WEBVIEW_ALIVE);
    std::env::var(key).ok()
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
pub(crate) fn parse_setting<T: std::str::FromStr>(
    name: &str,
    raw: &str,
    say: &dyn Fn(&str),
) -> Option<T> {
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
pub(crate) fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
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
pub(crate) fn env_list<T: std::str::FromStr>(name: &str, default: Vec<T>) -> Vec<T> {
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
        // The addresses behind a document's web links, which the webview is
        // given a token for and never receives. Managed on the builder for the
        // same reason the edit models are: it needs nothing from the app, and a
        // document can be opened before the setup hook runs.
        .manage(webopen::Registry::default())
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
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                let suffix = if tauri::is_dev() { " DEV" } else { "" };
                window.set_title(&format!("tpdf v{}{suffix}", env!("CARGO_PKG_VERSION")))?;
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
            redact_raster_copy,
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
            document_form,
            form_fill,
            document_comments,
            document_links,
            open_web_link,
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
        // Native Quit must pass the same unsaved-tab check as the close button.
        // Explicit exit codes belong to the unattended probes and bypass it.
        if let tauri::RunEvent::ExitRequested {
            api, code: None, ..
        } = &event
        {
            use tauri::Manager;
            if let Some(window) = _handle.get_webview_window("main") {
                api.prevent_exit();
                let _ = window.close();
            }
        }
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

    use crate::save;

    /// A [`save::Verifier`] that answers what it is told to and records the ask.
    ///
    /// The double that makes the seam observable, and the same shape as
    /// `save::tests::Fake`: without it the only way to ask whether the
    /// coordinator delegated the scan or did it itself is to read the source,
    /// and a source-level assertion proves a shape rather than an ordering.
    ///
    /// It implements [`save::Verifier`] and nothing else, which is what
    /// narrowing [`crate::commands::redact::scan_written_file`] from `Outside` bought.
    struct FakeScanner {
        answer: Result<crate::verify::Report, String>,
        asked: std::sync::Mutex<Vec<Ask>>,
    }

    /// One call to [`save::Verifier::scan`], as the double saw it.
    ///
    /// A named struct rather than the tuple this was, because clippy refuses a
    /// type that complex --- and it reads better in the assertion, where three
    /// positional fields would have to be counted.
    #[derive(Debug, PartialEq, Eq)]
    struct Ask {
        len: usize,
        needles: Vec<String>,
        password: Option<String>,
    }

    impl save::Verifier for FakeScanner {
        fn scan(
            &self,
            _file: &mut std::fs::File,
            len: usize,
            needles: &[String],
            password: Option<&str>,
        ) -> Result<crate::verify::Report, String> {
            self.asked.lock().expect("record the ask").push(Ask {
                len,
                needles: needles.to_vec(),
                password: password.map(str::to_string),
            });
            self.answer.clone()
        }
    }

    /// The redaction read-back asks the verifier, and does not parse the file.
    ///
    /// **The keystone for the third seam**, and it is red on the code this
    /// replaced. The file scanned here is not a PDF at all --- so a coordinator
    /// that parsed it would report a blind spot saying the file could not be
    /// parsed, whatever any verifier said. Getting the verifier's answer back
    /// verbatim, on exactly those bytes, is what says the parse is somewhere
    /// else now.
    ///
    /// It is the accounting observable for a property that is otherwise
    /// invisible: every number a caller can see is identical whether the scan
    /// happened here or in a worker, because the two agree wherever both answer,
    /// so the thing to assert is *who was asked*, with what, and about how much.
    #[test]
    fn the_redaction_read_back_does_not_parse_the_file_it_wrote() {
        let at = std::env::temp_dir().join("tpdf-scan-written-file.bin");
        std::fs::write(&at, b"this is not a PDF at all").expect("write the scratch file");

        let scanner = FakeScanner {
            answer: Ok(crate::verify::Report {
                objects: 7,
                ..Default::default()
            }),
            asked: std::sync::Mutex::new(Vec::new()),
        };
        let needles = vec!["secret".to_string()];
        let report =
            crate::commands::redact::scan_written_file(&scanner, &at, &needles, Some("key"))
                .expect("the verifier answered");

        // The answer is the verifier's, unaltered. A coordinator that parsed
        // these bytes could not have produced it: `verify::scan` reaches no
        // objects in a file that is not a PDF, and says so in `blind`.
        assert_eq!(report.objects, 7, "the answer is the verifier's");
        assert!(
            report.blind.is_empty(),
            "nothing here second-guessed the verifier: {:?}",
            report.blind
        );

        // And what crossed. The length is the file's, taken from the handle
        // that was opened rather than from the name; the needles and the key are
        // the caller's.
        let asked = scanner.asked.lock().expect("read the record");
        assert_eq!(asked.len(), 1, "asked exactly once");
        assert_eq!(
            asked[0],
            Ask {
                len: 24,
                needles: needles.clone(),
                password: Some("key".to_string()),
            },
            "the verifier was handed the file's length, the needles and the key"
        );
        drop(asked);
        let _ = std::fs::remove_file(&at);
    }

    /// A file that is not there is an error, not an empty report.
    ///
    /// The control for the test above, and it is the direction that matters: a
    /// read-back which answered `Report::default()` for a missing file would
    /// report *verified* about a file nobody looked at, which is the one thing
    /// `docs/PLAN.md` §6 forbids. The verifier is never reached, so a fake that
    /// would answer cleanly is what makes the assertion mean something.
    #[test]
    fn a_read_back_of_a_file_that_is_not_there_is_an_error() {
        let scanner = FakeScanner {
            answer: Ok(crate::verify::Report::default()),
            asked: std::sync::Mutex::new(Vec::new()),
        };
        let missing = std::env::temp_dir().join("tpdf-scan-written-file-absent.bin");
        let _ = std::fs::remove_file(&missing);

        let why = crate::commands::redact::scan_written_file(&scanner, &missing, &[], None)
            .expect_err("a file that is not there cannot be scanned");
        assert!(why.contains("read back"), "{why}");
        assert!(
            scanner.asked.lock().expect("read the record").is_empty(),
            "the verifier was never asked about a file that does not exist"
        );
    }

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
        env_list, env_or, parse_setting, spike_env, with_close_note, SaveFailure, WEBVIEW_ALIVE,
    };
    // The four that moved out of this file with their command groups. Named by
    // their new paths rather than through the globs above, so a reader of the
    // test can see which group owns the thing it is about.
    use crate::commands::app::app_version;
    use crate::commands::print::print_job;
    use crate::commands::session::with_session;
    use crate::commands::{await_reply, reply_channel};

    /// A plan made against a file, as a print of an unedited document carries one.
    ///
    /// One page and a fingerprint, which is everything `print_job` reads on the
    /// route below --- the passthrough hands the file over without parsing it,
    /// so a plan describing a document this scratch file is not would be checked
    /// by nothing here and is not worth pretending otherwise.
    fn plan_opened_as(source: &std::path::Path) -> crate::edits::Plan {
        crate::edits::Plan {
            baseline: 1,
            opened_as: Some(
                crate::fingerprint::Fingerprint::of(source).expect("fingerprint the scratch file"),
            ),
            pages: vec![crate::edits::PageView {
                id: 1,
                source: crate::docmodel::PageSource::Baseline(0),
                turns: 0,
                crop: None,
            }],
            marks: Vec::new(),
            redactions: Vec::new(),
            notes: Vec::new(),
            discards: Vec::new(),
            forms: Vec::new(),
        }
    }

    /// A print of a file that was replaced reaches the command carrying `changed`.
    ///
    /// **This is what `print_job` being a function buys**, and the assertion
    /// is on the flag rather than on the sentence for the reason `save::Refusal`
    /// carries one at all: the window offers Reload for this refusal and must
    /// not offer it for the others, and a `map_err` reaching for `.message`
    /// anywhere between here and the rejection takes that decision away while
    /// leaving a correct sentence behind. Nothing else in the tree can see that
    /// happen --- `print_document` is a `#[tauri::command]` and no test calls
    /// one.
    ///
    /// The control runs first, so what follows is a difference and not a
    /// reading: the same call on the same path before anything lands over it.
    /// Bytes rather than a PDF, because the passthrough reads and does not
    /// parse, and the guard under test is a hash of the file.
    #[test]
    fn a_print_of_a_replaced_file_rejects_with_the_flag_that_offers_reload() {
        let scratch =
            std::env::temp_dir().join(format!("tpdf-print-refusal-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory of this test's own");
        let at = scratch.join("open.pdf");
        std::fs::write(&at, b"the document the reader opened").expect("plant it");
        let plan = plan_opened_as(&at);

        let bytes = print_job(
            &at,
            &crate::print::Route::Passthrough,
            Some(&plan),
            0,
            None,
            &crate::save::Here,
        )
        .expect("an unchanged file is the whole point of the guard letting it through");
        assert_eq!(bytes, b"the document the reader opened".as_slice());

        std::fs::write(&at, b"a newer copy landing over the open document").expect("replace it");
        let why = print_job(
            &at,
            &crate::print::Route::Passthrough,
            Some(&plan),
            0,
            None,
            &crate::save::Here,
        )
        .expect_err("a print job over a file that is not the one opened must be refused");
        assert!(
            why.changed,
            "the refusal is about the file, which is what the window branches on: {}",
            why.message
        );
        assert_eq!(
            serde_json::to_value(&why).expect("the command rejects with this value"),
            serde_json::json!({ "message": why.message, "changed": true }),
            "and it crosses the boundary whole"
        );

        // **The other direction, which is the one that costs the reader their
        // work.** A flag that is set for every refusal offers Reload for one
        // reloading cannot answer, and the assertion above cannot tell that
        // apart from a flag that is carried correctly. This route refuses
        // before it reads anything, so what it produces is a plain refusal ---
        // and a plain refusal is `changed: false` because `From<&str>` says so
        // and not because anything here decided it.
        let unplanned = print_job(
            &at,
            &crate::print::Route::Working,
            None,
            0,
            None,
            &crate::save::Here,
        )
        .expect_err("the working document cannot be printed without its plan");
        assert!(
            !unplanned.changed,
            "only the fingerprint sets the flag: {}",
            unplanned.message
        );

        std::fs::remove_dir_all(&scratch).expect("leave nothing behind");
    }

    /// A save that failed after the close, with the document closing cleanly.
    ///
    /// The message is left alone, which is the ordinary case: nothing else went
    /// wrong and a note about the close would be a sentence about nothing.
    #[test]
    fn a_clean_close_adds_nothing_to_a_failure() {
        let why = with_close_note(SaveFailure::after_close("the rename failed"), Ok(()));
        assert_eq!(why.message, "the rename failed");
        assert!(why.reopen());
        assert!(!why.changed());
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
            action: crate::failure::Action::ReopenChanged,
        };
        let after = with_close_note(before, Err("also this".into()));
        assert!(after.reopen());
        assert!(after.changed());
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
    /// is a property of the build rather than of any code written here. It was
    /// written for `save`, `save_copy` and `extract_pages`, which parsed
    /// attacker-controlled bytes with `lopdf` in the coordinator under
    /// `spawn_blocking`; every one of those parses is in a worker as of
    /// 2026-09-01, and the property is still load-bearing for two reasons.
    /// `save::Here` is the fallback a platform with no sandbox gets and parses
    /// here exactly as they did. And a `spawn_blocking` task that panics for
    /// any other reason --- a poisoned lock, a slice out of bounds in code
    /// nobody was thinking about --- reaches the reader as a refusal only while
    /// the crate unwinds.
    ///
    /// Adding `panic = "abort"` to a release profile --- a one-line change made
    /// for binary size, with nothing about parsing in view --- would turn every
    /// one of those into a process death taking the reader's unsaved journal
    /// with it, and no other check here would notice.
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
