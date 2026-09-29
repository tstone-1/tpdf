//! `tpdf` on the command line: sign, verify, list signing identities, describe
//! a document, read its text, list its form fields and fill them, and redact.
//!
//! ## The same security model as the application, not a lighter one
//!
//! This is the application's signing and verifying with the window taken away,
//! and each half keeps the authority it has there and nothing more:
//!
//! - **The document is parsed only in a worker**, spawned by
//!   [`crate::save::InWorker`] exactly as the application spawns the one that
//!   reads a signed file back: this executable re-exec'd with
//!   `worker::WORKER_ARGV`, sandboxed by `sandbox_init` on macOS and contained by
//!   a low-integrity token in a job object on Windows. So this binary is also
//!   the worker, which is why [`main`] answers that marker before anything else
//!   --- `docs/TRAPS.md`, *A probe that spawns a worker is also the worker*.
//!   Nothing in this process maps PDFium or hands a document to `lopdf`.
//! - **The key never leaves the OS.** Signing is `sign_cms::finish` over a
//!   [`sign_cms::Key`](crate::sign_cms::Key) the OS store holds (`keystore.rs`),
//!   and the prompt the OS may show --- keychain access, a smart card's PIN ---
//!   is the only interaction there is. Unattended use relies on the reader
//!   having answered macOS's prompt with *Always Allow*; nothing here works
//!   around what the OS decides.
//! - **Every signed file goes through the application's writer**
//!   (`save::write_signed`), after `sign_cms::finish` has refused anything
//!   `integrity::check` does not call intact, and a fresh worker then reads the
//!   written file back; success is reported only when it finds the new
//!   signature intact. A filled file is written by the application's save
//!   (`save::write_copy`) and read back the same way (`cli/fill.rs`).
//!
//! ## Adding a command
//!
//! One module and one line. A command is a type that implements
//! [`Subcommand`], parsed by a function the module registers as a
//! [`Registered`] --- its name, its usage lines and its summary --- and the
//! registration is appended to [`COMMANDS`], which is all the dispatch, the
//! usage text and `help` read. What the modules share lives here and is not to
//! be restated in one: [`Env`] (the key store, the library directory and the
//! clock), [`Env::worker`] (a document is parsed only through it), [`opened`]
//! (the input's handle and length), [`say`] and [`json`] (one write each), and
//! [`Exit`] and [`Failure`] (the exit-code contract). `identities.rs` is the
//! smallest example; `sign.rs` the largest.
//!
//! ## Exit codes
//!
//! [`Exit`] is the list of record, and `README.md` repeats it for readers:
//! 0 done; 1 `verify --strict` found something that is not intact and trusted,
//! or `redact` wrote a copy it could not prove clean; 2 the command line is malformed; 3 tpdf refused (the identity, the document,
//! the output) or the OS did; 4 tpdf itself failed. Errors go to stderr, one
//! sentence each; with `--json`, stdout carries exactly one JSON document
//! whenever the exit code is 0 or 1, and for `verify` also when it is 3 or 4,
//! since other documents in the same run may have been read.

pub mod args;
mod edit;
pub mod fields;
pub mod fill;
pub mod identities;
pub mod info;
mod pages;
pub mod redact;
pub mod regions;
mod render;
pub mod report;
pub mod sign;
pub mod text;
pub mod verify;
pub mod words;

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::save::InWorker;
use crate::save_outside::Declined;
use crate::sign_cms::Key;

/// The application's identifier, which names the saved signature image's
/// store. `tauri.conf.json`'s `identifier`; a test holds the two equal.
pub const IDENTIFIER: &str = "com.timostein.tpdf";

/// How the process ends. The numbers are the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exit {
    /// Done.
    Ok = 0,
    /// `verify --strict`: a document with no signature, or a signature that is
    /// not both intact and trusted. `redact`: the copy was written and could
    /// not be proved clean; the file is kept and every reason is reported.
    Strict = 1,
    /// The command line is malformed.
    Usage = 2,
    /// tpdf, or the OS, refused: an identity that is unknown, ambiguous or
    /// cannot sign; a document that cannot be signed or read; an output that
    /// already exists; a key the OS would not use.
    Refused = 3,
    /// tpdf's own failure: a worker that died or did not answer, a store that
    /// could not be searched, a written file whose read-back disagreed.
    Internal = 4,
}

impl Exit {
    /// The process exit code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// A command that did not finish, and how the process should end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The exit code.
    pub exit: Exit,
    /// One sentence for stderr.
    pub message: String,
}

impl Failure {
    fn new(exit: Exit, message: impl Into<String>) -> Self {
        Failure {
            exit,
            message: message.into(),
        }
    }
}

impl From<Declined> for Failure {
    fn from(declined: Declined) -> Self {
        match declined {
            Declined::Refused(why) | Declined::Locked(why) => Failure::new(Exit::Refused, why),
            Declined::Failed(why) => Failure::new(Exit::Internal, why),
        }
    }
}

/// A certificate with a key behind it, as a [`Store`] lists it.
pub struct Held {
    /// The certificate, DER.
    pub certificate: Vec<u8>,
    /// What the OS chain API returned above it, DER.
    pub chain: Vec<Vec<u8>>,
    /// Asks whoever holds the key to sign one digest.
    pub key: Box<dyn Key>,
}

/// Where identities and the saved signature image come from.
///
/// **The seam the tests sign through**, and the only one: [`OsStore`] is the
/// shipped implementation and asks the keychain or the certificate store; a
/// test supplies software keys (`sign_cms/testkeys.rs`'s kind) and never
/// touches either. Everything else --- the workers, the writer, the read-back
/// --- is the path a reader's command takes.
pub trait Store {
    /// Every certificate with a key behind it, unfiltered.
    ///
    /// # Errors
    ///
    /// The store could not be searched.
    fn identities(&self) -> Result<Vec<Held>, String>;

    /// The reader's saved visual signature, when one is saved.
    ///
    /// # Errors
    ///
    /// The protected store could not be read.
    fn saved_image(&self) -> Result<Option<crate::signature::Image>, String>;
}

/// The reader's own OS store: `keystore.rs` and `signature_store.rs`.
pub struct OsStore;

impl Store for OsStore {
    fn identities(&self) -> Result<Vec<Held>, String> {
        Ok(crate::keystore::identities()?
            .into_iter()
            .map(|identity| Held {
                certificate: identity.certificate.clone(),
                chain: identity.chain.clone(),
                key: Box::new(identity),
            })
            .collect())
    }

    fn saved_image(&self) -> Result<Option<crate::signature::Image>, String> {
        // The store the application writes (`commands::session::signature_store`):
        // the Keychain item `<identifier>.signature` on macOS, DPAPI ciphertext in
        // the application's local data directory on Windows.
        let service = format!("{IDENTIFIER}.signature");
        let path = local_data_dir()
            .unwrap_or_default()
            .join(IDENTIFIER)
            .join("signature.bin");
        crate::signature_store::perform(&service, &path, crate::signature_store::Action::Load)
    }
}

/// `%LOCALAPPDATA%` on Windows, which Tauri's `app_local_data_dir` is under.
/// Unused on macOS, where the image is a Keychain item and has no path.
fn local_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    }
}

/// What a run needs from outside the command line.
pub struct Env<'a> {
    /// Identities and the saved image.
    pub store: &'a dyn Store,
    /// Where `libpdfium` is, for the workers.
    pub library_dir: PathBuf,
    /// Seconds since the epoch: the signing time, and the moment a
    /// certificate's validity is judged at.
    pub now: u64,
    /// The name the program was run as, for the usage text.
    pub program: String,
    /// The roots a timestamp authority's chain must end at before `sign
    /// --long-term` fetches anything for it: `Anchors::System` in the tool,
    /// a test's own roots in a test, so no test touches the reader's store.
    pub anchors: crate::trust::Anchors<'a>,
}

/// Where the application's resources are, from where this executable sits.
///
/// The bundle's layout, not a search: `tpdf.app/Contents/MacOS/tpdf-cli` beside
/// `Contents/Resources` on macOS, and the install directory itself on Windows,
/// which is where Tauri's resource directory is there. The executable path is
/// canonicalised first, because on macOS the tool is reached through the link
/// in `/usr/local/bin` and `current_exe` answers with the path it was run by.
#[must_use]
pub fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let dir = exe.parent()?;
    if cfg!(target_os = "macos") {
        Some(dir.parent()?.join("Resources"))
    } else {
        Some(dir.to_path_buf())
    }
}

/// The command-line tool's `main`. Returns the process exit code.
#[must_use]
pub fn main() -> i32 {
    let argv: Vec<String> = match std::env::args_os()
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(argv) => argv,
        Err(arg) => {
            let _ = writeln!(
                std::io::stderr(),
                "tpdf: an argument is not valid Unicode ({arg:?}), and tpdf reads paths as text"
            );
            return Exit::Usage.code();
        }
    };
    // Before anything else: this process may be one of its own workers. It
    // never returns from here. The OCR worker first, as `lib.rs`'s `run` orders
    // them: `redact`'s gate spawns one, and a child that found no marker here
    // would fall into this parser and exit --- `ocr_worker::child_main_if_asked`
    // records the day that read as an engine that crashed.
    crate::ocr_worker::child_main_if_asked(&argv);
    if argv.get(1).map(String::as_str) == Some(crate::worker::WORKER_ARGV) {
        crate::worker_child::main(&argv);
    }
    let program = argv
        .first()
        .and_then(|p| Path::new(p).file_stem())
        .map_or_else(|| "tpdf".to_string(), |s| s.to_string_lossy().into_owned());
    // Where the engine is, from where this executable sits, or nowhere. The
    // shared search answers `.` when it is given no resource directory, and a
    // worker would then load a `libpdfium` from whatever directory the tool was
    // started in --- for a command-line tool, the reader's own folder, which may
    // be a download directory (release audit, 2026-09-27). `current_exe`
    // failing is rare; searching the working directory when it does is not an
    // answer this tool gives.
    let Some(resources) = resource_dir() else {
        let _ = writeln!(
            std::io::stderr(),
            "tpdf: could not work out where tpdf is installed, so it will not look for its engine anywhere else"
        );
        return Exit::Internal.code();
    };
    let env = Env {
        store: &OsStore,
        library_dir: crate::library_dir_among(Some(resources)),
        now: now(),
        program,
        anchors: crate::trust::Anchors::System,
    };
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    run(&argv[1..], &env, &mut stdout.lock(), &mut stderr.lock())
}

/// Seconds since the epoch, now.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Runs one command line, writing to `out` and `err`. Returns the exit code.
pub fn run(args: &[String], env: &Env<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let wants_json = args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json");
    let mut output = Counted {
        inner: out,
        bytes: 0,
    };
    let out = &mut output;
    let line = match args::parse(args) {
        Ok(line) => line,
        Err(why) => {
            if wants_json {
                failure_json(out, args, &Failure::new(Exit::Usage, &why));
            }
            say(err, &format!("{}: {why}", env.program));
            say(
                err,
                &format!("Run `{} help` for the commands.", env.program),
            );
            return Exit::Usage.code();
        }
    };
    let result = match line {
        args::Line::Help => {
            if wants_json {
                json(
                    out,
                    &report::Help {
                        schema: report::SCHEMA,
                        command: "help".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                        commands: COMMANDS
                            .iter()
                            .map(|c| report::Command {
                                name: c.name.into(),
                                usage: c.usage.into(),
                                summary: c.summary.into(),
                            })
                            .collect(),
                    },
                );
            } else {
                say(out, &usage(&env.program));
            }
            Ok(Exit::Ok)
        }
        args::Line::Version => {
            say(out, &format!("tpdf {}", env!("CARGO_PKG_VERSION")));
            Ok(Exit::Ok)
        }
        args::Line::Run(command) => command.run(env, out, err),
    };
    match result {
        Ok(exit) => exit.code(),
        Err(failure) => {
            // Some commands already returned a detailed report (including fill's
            // field errors). Never append a second JSON document to that report.
            if wants_json && out.bytes == 0 {
                failure_json(out, args, &failure);
            }
            say(err, &format!("{}: {}", env.program, failure.message));
            failure.exit.code()
        }
    }
}

struct Counted<'a> {
    inner: &'a mut dyn Write,
    bytes: usize,
}

impl Write for Counted<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.bytes += n;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn failure_json(out: &mut dyn Write, args: &[String], failure: &Failure) {
    json(
        out,
        &report::Failed {
            schema: report::SCHEMA,
            command: args.first().cloned().unwrap_or_default(),
            error: report::CommandError::from_failure(failure),
        },
    );
}

/// One command: what it was asked, and how it runs.
///
/// `Debug` so a parsed line can be shown in a test's failure message.
pub trait Subcommand: std::fmt::Debug {
    /// Runs it. Errors go back as a [`Failure`], which [`run`] prints and
    /// turns into the exit code; a command that has a result and a non-zero
    /// exit code at once (`verify --strict`) returns `Ok` with that code.
    ///
    /// # Errors
    ///
    /// Why the command did not finish, and the exit code that says so.
    fn run(&self, env: &Env<'_>, out: &mut dyn Write, err: &mut dyn Write)
        -> Result<Exit, Failure>;
}

/// A command as the dispatch, the usage text and `help` know it.
pub struct Registered {
    /// The word that selects it.
    pub name: &'static str,
    /// Its synopsis after the program's name; later lines indented eight spaces.
    pub usage: &'static str,
    /// What it does, for `help`; later lines indented twelve spaces.
    pub summary: &'static str,
    /// Reads the arguments after the name. A refusal is exit code 2.
    pub parse: Parser,
}

/// A command's parser, as [`Registered`] holds it.
pub type Parser = fn(&[String]) -> Result<Box<dyn Subcommand>, String>;

/// Every command, in the order `help` lists them. Adding one is a module and a
/// line here.
pub const COMMANDS: &[Registered] = &[
    sign::COMMAND,
    verify::COMMAND,
    identities::COMMAND,
    info::COMMAND,
    text::COMMAND,
    fields::COMMAND,
    fill::COMMAND,
    redact::COMMAND,
    pages::MERGE,
    pages::EXTRACT,
    pages::SPLIT,
    pages::ROTATE,
    pages::CROP,
    edit::COMMAND,
    edit::COMMENTS,
    edit::TEXT_RUNS,
    render::COMMAND,
];

impl Env<'_> {
    /// The worker every document is parsed in: `save::InWorker`, spawned as
    /// the application spawns it. **The only route to a document's contents**
    /// a command has --- a command that parsed one itself would be the defect
    /// this module's note is about.
    #[must_use]
    pub fn worker(&self) -> InWorker {
        InWorker::at(self.library_dir.clone())
    }
}

/// One line, written whole: `docs/TRAPS.md`, *`eprintln!` is not one write* ---
/// every worker shares this process's stderr.
fn say(to: &mut dyn Write, line: &str) {
    let mut text = String::with_capacity(line.len() + 1);
    text.push_str(line);
    text.push('\n');
    let _ = to.write_all(text.as_bytes());
    let _ = to.flush();
}

/// JSON, pretty, as one write.
fn json(to: &mut dyn Write, value: &impl serde::Serialize) {
    match ascii_json(value) {
        Ok(text) => say(to, &text),
        Err(e) => say(to, &format!("{{\"error\":\"could not encode: {e}\"}}")),
    }
}

/// `value` as pretty JSON in which every character outside ASCII is a `\uXXXX`
/// escape, surrogate pairs above U+FFFF.
///
/// **Because a Windows script reads this through a code page.** PowerShell
/// decodes a program's output with `[Console]::OutputEncoding`, which is the
/// console's OEM code page --- 437 or 850, not UTF-8 --- so `$r = tpdf-cli verify
/// --json "Prüfung.pdf" | ConvertFrom-Json` gave back a `path` of `PrÃ¼fung.pdf`
/// that named no file (measured on a Windows desktop at the 26.9.21 release). An
/// escape is the same JSON string to every parser and has no bytes a code page
/// can misread. It is applied to the finished text, which is sound because
/// `serde_json` writes a character outside ASCII only inside a string literal,
/// where an escape means that character.
///
/// # Errors
///
/// Whatever `serde_json` cannot encode.
pub(crate) fn ascii_json(value: &impl serde::Serialize) -> serde_json::Result<String> {
    let text = serde_json::to_string_pretty(value)?;
    let mut ascii = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            ascii.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                ascii.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    Ok(ascii)
}

/// The usage text, from [`COMMANDS`].
#[must_use]
pub fn usage(program: &str) -> String {
    let mut text = format!(
        "tpdf {} --- sign, verify, describe, read, fill and redact PDF documents from the command line\n\nUsage:\n",
        env!("CARGO_PKG_VERSION")
    );
    for command in COMMANDS {
        text.push_str(&format!("  {program} {}\n", command.usage));
    }
    text.push_str(&format!("  {program} help | --version\n\n"));
    for command in COMMANDS {
        text.push_str(&format!("{:<12}{}\n", command.name, command.summary));
    }
    text.push_str(
        "\nExit codes: 0 done; 1 verify --strict found a signature that is not intact\n\
         and trusted, or a document with none, or redact wrote a copy it could not\n\
         prove clean; 2 the command line is malformed; 3 refused (identity,\n\
         document, answers, output, or the OS); 4 tpdf failed.",
    );
    text
}

/// Opens `path` and returns the handle and its length, refusing an empty file.
fn opened(path: &Path) -> Result<(std::fs::File, usize), String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("could not measure {}: {e}", path.display()))?
        .len();
    if len == 0 {
        return Err(format!("{} is empty", path.display()));
    }
    let len = usize::try_from(len).map_err(|_| format!("{} is too large", path.display()))?;
    Ok((file, len))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod form_tests;

#[cfg(test)]
mod redact_tests;
