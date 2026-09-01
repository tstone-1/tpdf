//! The other side of the process boundary --- what `--render-worker` runs.
//!
//! This process holds the document and nothing else. It has no path to it, no
//! network, no writable filesystem and no way to acquire any: everything it may
//! touch was handed over as an already-open object before it lost the authority
//! to open anything itself.
//!
//! **Ordering is the whole of the security argument here**, and it is easy to
//! get backwards. PDFium is bound *before* the boundary is established, because
//! binding opens and maps the library and a policy denying `file-read*` would
//! make that fail. The document is opened *after*, because that is the
//! attacker's input. Anything moved across that line either breaks the worker or
//! defeats it, and neither shows up as an error --- a sandbox applied too late
//! still returns `ok`.
//!
//! ## Two platforms, and three functions that know it
//!
//! Everything here compiles on macOS and Windows both. Exactly three things
//! differ, and each is one function rather than a second copy of the worker:
//! [`adopt_tile`] and [`adopt_document`], because macOS inherits a mapping on an
//! agreed descriptor number and Windows inherits a handle whose *value* has to be
//! told to the child; and [`establish_boundary`], which on macOS applies
//! `sandbox_init` and on Windows can only verify a token that was chosen before
//! this process ran. The request loop, the queue, the render path and the font
//! warming were always portable and are shared, which is the point --- a second
//! worker would be a second thing to keep correct, and the half that goes stale
//! is never the half being looked at.
//!
//! The asymmetry worth remembering: macOS's boundary *fails loudly if it cannot
//! be applied*, Windows's *fails loudly if it was never applied by the parent*.
//! Neither is a guarantee this file can make on its own.
//!
//! Two threads, and they cannot be one. A withdrawal exists to reach a render
//! that is *already running*, so it must be read while the render thread is
//! inside PDFium --- which means a reader thread owning stdin, and the render
//! thread owning the document. The reader never touches the document, which is
//! what makes that sound: `RawDocument` is not `Send`, and concurrent PDFium is
//! undefined behaviour whatever the handles are (`AGENTS.md`).

use crate::document::OpenDocument;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{channel, Sender};

use crate::progressive::{self, CancelToken};
use crate::queue::{Claim, SharedQueue};
use crate::render::{self, PageSize, TileFormat, TileRequest};
#[cfg(windows)]
use crate::worker::{doc_handle_arg, out_handle_arg, tile_handle_arg, Handover};
use crate::worker::{
    doc_len_arg, library_dir_arg, Reply, Request, Response, Shm, PRESPAWN_ARGV, TILE_CAPACITY,
};
#[cfg(target_os = "macos")]
use crate::worker::{recv_document, SANDBOX_PROFILE, SOCK_FD};
#[cfg(unix)]
use crate::worker::{DOC_FD, OUT_ARGV, OUT_FD, TILE_FD};

/// Runs this process as a render worker. Never returns.
pub fn main(args: &[String]) -> ! {
    let code = match serve(args) {
        Ok(()) => 0,
        Err(message) => {
            // stderr is inherited from the parent precisely so this is visible.
            // A worker that dies silently is the hardest failure here to
            // diagnose, and the parent can only report an epitaph.
            //
            // **One write, not `eprintln!`**, and the difference is not style.
            // Rust's stderr is unbuffered and `write_fmt` issues a separate
            // write per format piece --- the literal, then the argument, then the
            // newline. Every worker of every pool inherits *this* handle, so with
            // a pool of six across five services those writes interleave, and a
            // reader can be left holding `[worker] ` with no message after it.
            // That is indistinguishable from a worker that failed with an empty
            // reason, which is the one thing this line exists to rule out. Seen
            // once during a `pool-bench` run of ~120 workers; every error path
            // that reaches here produces non-empty text, and it did not recur on
            // a stderr channel of its own.
            let line = format!("[worker] {message}\n");
            // Best effort by design: this is the last thing the process does and
            // there is nowhere to report a failed report to.
            let _ = std::io::stderr().write_all(line.as_bytes());
            1
        }
    };
    // Flushed and exited explicitly. `AGENTS.md` records months of automated
    // runs reporting success because a handle's exit code never reached the
    // process; this is the same mistake one process further out.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

/// A document naming a base-14 face, opened to warm PDFium's font list.
///
/// 571 bytes, `qpdf --check` clean, and committed as source rather than
/// generated: every other PDF in this repository is a gitignored fixture, and
/// this one has to be inside the shipped binary.
///
/// **Those two numbers were false on every Windows clone until 2026-08-26**, and
/// being inside the binary is exactly why. A small uncompressed PDF holds no NUL
/// in its first 8000 bytes, so git sniffed this one as text and `core.autocrlf`
/// converted it on checkout: 603 bytes, 32 CRLF, and `qpdf --check` reporting a
/// damaged file with an unfindable xref. `.gitattributes` now says `*.pdf binary`.
/// Nothing here could have reported it --- `warm_fonts` returns silently on a
/// failed load, and `git status` called the file unmodified because the same
/// filter normalises on read. The trap index has it.
///
/// It exists because of what `examples/prespawn_bench.rs` measured. A worker's fixed
/// startup is ~6.6 ms, but a document that does *not* embed its fonts pays a
/// further ~7.4 ms while PDFium goes looking for a system face --- which reads
/// like a per-document cost and is not one. It is the machine's font list, so
/// one process can pay it once, before it knows which file it will be given.
const WARM_DOCUMENT: &[u8] = include_bytes!("warm.pdf");

/// Sets the process up and serves requests until stdin closes.
fn serve(args: &[String]) -> Result<(), String> {
    let library_dir = library_dir_arg(args).ok_or("--lib is missing")?;
    // A pre-spawned worker has no document yet, and is told so rather than
    // inferring it from a missing `--doc-len`: an argument that failed to parse
    // would otherwise be indistinguishable from a deliberate omission, and the
    // worker would sit waiting on a socket nobody is going to write to.
    let prespawned = args.iter().any(|a| a == PRESPAWN_ARGV);

    let mut tile_shm = adopt_tile(args)?;
    // Adopted here rather than at the request, because a descriptor is handed
    // over at `exec` and there is nothing to look up later. `None` for every
    // worker but one spawned to write, which is what makes a rewrite request to
    // an ordinary worker a refusal with a reason rather than a write into
    // whatever fd 6 happens to be.
    let mut out_file = adopt_output(args);

    // Adopted before the sandbox when there is one to adopt. Read-only: a worker
    // must not be able to write the reader's file.
    let early_doc = if prespawned {
        None
    } else {
        let doc_len = doc_len_arg(args).ok_or("--doc-len is missing or unreadable")?;
        Some(adopt_document(args, doc_len)?)
    };

    // BEFORE the sandbox. Binding opens and maps libpdfium, which a policy
    // denying file reads forbids.
    //
    // **Answered, not died on**, for the reason the open below gives and with a
    // worse history: this returned `Err`, the process exited 1, and the message
    // naming the missing library went to a stderr a GUI process does not have.
    // What a reader saw was `worker stopped answering (exited with 1)` for every
    // document in an installation that shipped the library under the wrong name
    // --- reported against 26.8.8 on Windows, where the bundler had renamed
    // `pdfium.dll` to `pdfium` and the app could not find it.
    //
    // The boundary is applied first where a platform has one, so the refusing
    // process is contained like any other. Its own failure is discarded: there
    // is nothing left to protect here --- no document is ever loaded on this path
    // --- and refusing to refuse would put the reader back where they started.
    let pdfium = match bind(&library_dir) {
        Ok(pdfium) => pdfium,
        Err(e) => {
            let _ = establish_boundary();
            return refuse(&format!(
                "tpdf could not load its PDF engine. Reinstalling should fix it. ({e})"
            ));
        }
    };
    let bindings = progressive::bindings_of(pdfium);

    establish_boundary()?;

    let doc_shm = match early_doc {
        Some(shm) => shm,
        None => {
            // Both of these happen while nobody is waiting, which is the entire
            // value of pre-spawning. Warming first, because the descriptor may
            // already be on its way.
            warm_fonts(bindings);
            // Said out loud, on the ordinary reply channel, because "warm" is not
            // observable from outside otherwise -- and a pre-spawn that silently
            // stopped warming would look exactly like one that worked, which is
            // the failure shape this repository keeps meeting. The parent waits
            // for this in `PreWorker::adopt`.
            reply(&mut std::io::stdout(), &Response::reply(Reply::Warm))?;
            wait_for_document()?
        }
    };

    // AFTER the sandbox: this is the hostile input.
    //
    // SAFETY: the mapping is forgotten immediately below, so the bytes outlive
    // every use. PDFium reads them for as long as the document is open, which
    // here is until the process exits.
    let bytes: &'static [u8] = unsafe { doc_shm.as_static() };
    std::mem::forget(doc_shm);
    let document = match OpenDocument::open_bytes(bindings, bytes, None) {
        Ok(document) => document,
        // **Asked about, not refused.** A locked document is the one refusal a
        // reader can answer, so it becomes a conversation rather than an
        // epitaph. `unlock` serves the same stdin every later request arrives
        // on and returns the opened document, so the loop below is reached with
        // nothing about it different.
        Err(refusal) if refusal.locked => unlock(bindings, bytes, &refusal.reason)?,
        // **Answered, not died on.** `open_failure` writes the four reasons a
        // document does not open in a reader's words --- it needs a password, it
        // uses a scheme we cannot read, it is not a PDF, it could not be read ---
        // and returning `Err` here threw every one of them away: the process
        // exited 1, the message went to stderr, and the parent could only report
        // the epitaph. A **GUI process has no stderr**, so what a reader saw was
        // `worker stopped answering (exited with 1 (0x00000001))` for a file
        // whose real problem tpdf had diagnosed correctly and could not say.
        //
        // Reported 2026-08-21 against 26.8.6 on Windows and reproduced here on
        // macOS in one command, with two different causes --- so it was never a
        // platform defect, only a platform where the message is invisible.
        Err(refusal) => return refuse(&refusal.reason),
    };

    // The same state machine the in-process renderer uses, for the same reason:
    // a claim moves a request from queued to in flight under one lock, so a
    // withdrawal arriving at any instant either finds it queued and marks it, or
    // finds it running and cancels it. Tested in `queue.rs`.
    let queue = SharedQueue::default();
    let (tx, rx) = channel::<Request>();
    spawn_reader(tx, queue.clone());

    let mut out = std::io::stdout();
    for request in rx {
        let response = handle(
            bindings,
            &document,
            &queue,
            &mut tile_shm,
            out_file.as_mut(),
            &request,
        );
        reply(&mut out, &response)?;
    }
    Ok(())
}

/// Adopts the tile mapping the parent handed over, writable.
#[cfg(unix)]
fn adopt_tile(_args: &[String]) -> Result<Shm, String> {
    // SAFETY: the parent dup2'd a live descriptor to this number before exec, and
    // nothing else in this process owns it.
    unsafe { Shm::from_fd(TILE_FD, TILE_CAPACITY, true) }
}

/// Adopts the tile mapping the parent handed over, writable.
///
/// The handle arrives in argv rather than on a fixed number, because Windows
/// inherits handles by value. See [`crate::worker::DOC_HANDLE_ARGV`].
#[cfg(windows)]
fn adopt_tile(args: &[String]) -> Result<Shm, String> {
    let handle = tile_handle_arg(args).ok_or("--tile-handle is missing or unreadable")?;
    // SAFETY: the parent named this handle in the spawn's handle list, so it is
    // live here, and nothing else in this process owns it.
    unsafe { Shm::from_handle(handle, TILE_CAPACITY, true) }
}

/// Adopts the output file the parent handed over, where it handed one over.
///
/// **Read from argv rather than probed**, because an unused descriptor number
/// and one that was handed over are indistinguishable from inside this process:
/// `File::from_raw_fd(6)` on a worker that got none is a handle to whatever is
/// there, and writing a document into it is the failure that would follow. See
/// [`crate::worker::OUT_ARGV`].
#[cfg(unix)]
fn adopt_output(args: &[String]) -> Option<std::fs::File> {
    use std::os::fd::FromRawFd;

    if !args.iter().any(|arg| arg == OUT_ARGV) {
        return None;
    }
    // SAFETY: the parent dup2'd a live descriptor to this number before exec
    // when it passed the marker, and nothing else in this process owns it.
    Some(unsafe { std::fs::File::from_raw_fd(OUT_FD) })
}

/// Adopts the output file the parent handed over, where it handed one over.
///
/// The handle arrives in argv rather than on a fixed number, for the reason
/// [`adopt_tile`] gives.
#[cfg(windows)]
fn adopt_output(args: &[String]) -> Option<std::fs::File> {
    use std::os::windows::io::FromRawHandle;

    let handle = out_handle_arg(args)?;
    // SAFETY: the parent named this handle in the spawn's handle list, so it is
    // live here, and nothing else in this process owns it.
    Some(unsafe { std::fs::File::from_raw_handle(handle as *mut std::ffi::c_void) })
}

/// Adopts the document mapping, read-only.
#[cfg(unix)]
fn adopt_document(_args: &[String], doc_len: usize) -> Result<Shm, String> {
    // SAFETY: as `adopt_tile`.
    unsafe { Shm::from_fd(DOC_FD, doc_len, false) }
}

/// Adopts the document mapping, read-only.
#[cfg(windows)]
fn adopt_document(args: &[String], doc_len: usize) -> Result<Shm, String> {
    let handle = doc_handle_arg(args).ok_or("--doc-handle is missing or unreadable")?;
    // SAFETY: as `adopt_tile`.
    unsafe { Shm::from_handle(handle, doc_len, false) }
}

/// Puts the process boundary in force, or refuses to serve.
///
/// **The two platforms do different things here and the difference is real.**
/// macOS *applies* a policy: this call is what costs the process its authority,
/// and everything above it happens while the process still has some. Windows
/// *checks* one: the token was chosen at `CreateProcess` and has been in force
/// since the first instruction, so there is nothing left to apply and the only
/// question is whether whoever spawned us actually did it.
///
/// Kept at the same point in `serve` regardless, which on Windows is later than
/// it strictly needs to be. The invariant a reader has to be able to find is that
/// **the document is opened after this line and never before**, and that is one
/// line only if both platforms use the same one. The cost of checking late is
/// some wasted work in a process that is about to exit.
///
/// # Errors
///
/// macOS: a profile the kernel refuses. Windows: not being contained.
/// Elsewhere: always, because neither exists.
fn establish_boundary() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        apply_sandbox(SANDBOX_PROFILE)
    }
    #[cfg(windows)]
    {
        crate::sandbox_win::assert_contained()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err("no process boundary is implemented on this platform".into())
    }
}

/// Serves a locked document's requests until a password opens it.
///
/// The one refusal a reader can do something about, so it is a question rather
/// than an epitaph. Every request that is not [`Request::Unlock`] is answered
/// with `locked` --- including the [`Request::Open`] the parent sends first ---
/// and an `Unlock` retries the load with what the reader typed.
///
/// **Retrying in place is legal because a failed load poisons nothing.** That is
/// measured rather than assumed: loading one AES-256 fixture's bytes with no
/// password, the right password, a wrong one and the right one again, in a
/// single process, opens on both correct attempts and refuses on both others.
/// So a wrong password costs a reply, not a process. `docs/PLAN.md` §5 has the
/// run.
///
/// **Reads stdin the way [`wait_for_document`] does, and for its reason.** Both
/// this and [`spawn_reader`] go through `std::io::stdin()`, whose buffer is
/// shared; a private `BufReader` here would swallow whatever arrived promptly
/// behind the password into a buffer that is then dropped, and the first request
/// of the reader's session would vanish. `refuse` may wrap stdin because it never
/// hands the stream on to anybody.
///
/// The wording of a second failure is chosen here rather than by
/// [`progressive::open_failure`], which cannot know: PDFium answers
/// `FPDF_ERR_PASSWORD` for a document given no password and one given the wrong
/// password alike. This loop is the only place that knows a password was tried.
///
/// # Errors
///
/// The pipe closing, which is how a reader who dismisses the prompt lets the
/// worker exit; or a failure that is no longer about the password, which no
/// password will fix.
fn unlock(
    bindings: progressive::Bindings,
    bytes: &'static [u8],
    first: &str,
) -> Result<OpenDocument, String> {
    use std::io::BufRead;

    let mut out = std::io::stdout();
    let mut reason = first.to_string();
    loop {
        let mut line = String::new();
        let read = std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("reading a password: {e}"))?;
        if read == 0 {
            return Err("the parent closed the pipe while the document was locked".into());
        }
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<Request>(line.trim()) {
            Ok(request) => request,
            Err(e) => {
                reply(&mut out, &Response::err(format!("unreadable request: {e}")))?;
                continue;
            }
        };
        // Skipped rather than answered, exactly as on the ordinary path: a
        // withdrawal has no reply, and answering one would leave the parent a
        // reply ahead of itself for the rest of this worker's life.
        if matches!(request, Request::Withdraw { .. }) {
            continue;
        }
        let Request::Unlock { password } = request else {
            reply(&mut out, &Response::locked(&reason))?;
            continue;
        };
        match OpenDocument::open_bytes(bindings, bytes, Some(&password)) {
            Ok(document) => {
                reply(&mut out, &Response::reply(Reply::Unlocked))?;
                return Ok(document);
            }
            Err(refusal) if refusal.locked => {
                reason = "That password did not open this document.".into();
                reply(&mut out, &Response::locked(&reason))?;
            }
            // No longer a password problem, so no password will fix it.
            Err(refusal) => {
                reply(&mut out, &Response::err(&refusal.reason))?;
                return Err(refusal.reason);
            }
        }
    }
}

/// Answers every request with the reason the document would not open.
///
/// A worker with no document can do nothing, and the parent drops it as soon as
/// the open fails --- so this loop serves exactly one request in practice. What
/// it buys is that the request is *answered*: `Workers::open` already has an
/// `if !response.ok { return Err(response.error) }` branch, and until now that
/// branch was unreachable for the one failure a reader actually meets.
///
/// A withdrawal is skipped rather than answered, the same as on the ordinary
/// path: it is not a request that has a reply, and answering it would leave the
/// parent one reply ahead of itself for the rest of the worker's life.
///
/// Returns `Ok`, because a document PDFium refuses is a fact about the document
/// and not a failure of this process. Exiting 0 is what keeps the two apart in
/// the epitaph if anything ever does read it.
fn refuse(reason: &str) -> Result<(), String> {
    let mut out = std::io::stdout();
    let stdin = BufReader::new(std::io::stdin());
    for line in stdin.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if matches!(
            serde_json::from_str::<Request>(&line),
            Ok(Request::Withdraw { .. })
        ) {
            continue;
        }
        reply(&mut out, &Response::err(reason))?;
    }
    Ok(())
}

/// Reads requests from stdin until it closes.
///
/// Withdrawals are applied here rather than being forwarded, which is the point
/// of the thread: by the time a queued withdrawal reached the render loop, the
/// render it withdraws would already have finished.
fn spawn_reader(tx: Sender<Request>, queue: SharedQueue) {
    std::thread::Builder::new()
        .name("tpdf-worker-reader".into())
        .spawn(move || {
            let stdin = BufReader::new(std::io::stdin());
            for line in stdin.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Request>(&line) {
                    Ok(Request::Withdraw { rid }) => queue.with(|queue| queue.withdraw(rid)),
                    Ok(request) => {
                        if let Request::Tile { rid, .. } = request {
                            // Registered on arrival, so a withdrawal that beats
                            // the render thread to it still finds something to
                            // mark.
                            queue.with(|queue| queue.enqueue(rid));
                        }
                        if tx.send(request).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        // Not fatal: a malformed line is a bug in the parent, and
                        // killing the worker would lose the document over it.
                        eprintln!("[worker] unreadable request {line:?}: {e}");
                    }
                }
            }
        })
        .expect("failed to spawn the worker's reader thread");
}

/// Serves one request.
fn handle(
    bindings: progressive::Bindings,
    document: &OpenDocument,
    queue: &SharedQueue,
    tile: &mut Shm,
    out: Option<&mut std::fs::File>,
    request: &Request,
) -> Response {
    match request {
        Request::Open { lazy_geometry } => open(document, *lazy_geometry),
        Request::Tile { .. } => render(bindings, document, queue, tile, request),
        // Consumed on the reader thread; reaching here would mean the dispatch
        // above changed and this arm was forgotten.
        Request::Withdraw { .. } => Response::err("a withdrawal is not a request to answer"),
        Request::Text { page, crop } => match render::run_text(document, *page, *crop) {
            Ok(text) => Response::reply(Reply::Text(text)),
            Err(e) => Response::err(e),
        },
        Request::Search {
            page,
            query,
            options,
            carry,
        } => match render::run_search(document, *page, query, *options, carry.as_ref()) {
            Ok(matches) => Response::reply(Reply::Search(matches)),
            Err(e) => Response::err(e),
        },
        Request::Content { page } => {
            match render::run_content(bindings, document, *page, &CancelToken::default()) {
                Ok(found) => Response::reply(Reply::Content(found)),
                Err(e) => Response::err(e),
            }
        }
        Request::Geometry { page, crop } => match render::geometry_of(document, *page, *crop) {
            Ok(size) => Response::reply(Reply::Geometry(size)),
            Err(e) => Response::err(e),
        },
        Request::CropBox { page, rect } => match render::crop_box_of(document, *page, *rect) {
            Ok(want) => Response::reply(Reply::CropBox(want)),
            Err(e) => Response::err(e),
        },
        Request::RedactPlans { page, regions } => {
            match render::redaction_plans_of(document, *page, regions) {
                Ok(plans) => Response::reply(Reply::RedactPlans(plans)),
                Err(e) => Response::err(e),
            }
        }
        Request::Outline => Response::reply(Reply::Outline(render::run_outline(document))),
        Request::Comments => match render::run_comments(document) {
            Ok(comments) => Response::reply(Reply::Comments(comments)),
            Err(e) => Response::err(e),
        },
        Request::Links => match render::run_links(document) {
            Ok(links) => Response::reply(Reply::Links(links)),
            Err(e) => Response::err(e),
        },
        Request::Mapping => Response::reply(Reply::Mapping(render::run_mapping(document))),
        Request::Properties => match render::run_properties(document) {
            Ok(properties) => Response::reply(Reply::Properties(Box::new(properties))),
            Err(e) => Response::err(e),
        },
        Request::Append { plan } => match render::run_append(document, plan) {
            Ok(update) => Response::reply(Reply::Append(update)),
            Err(e) => Response::err(e),
        },
        Request::Rewrite { plan, job } => rewrite(document, out, plan, *job),
        Request::Reread => match render::run_reread(document) {
            Ok(pages) => Response::reply(Reply::Reread(pages)),
            Err(e) => Response::err(e),
        },
        // Reached when the document opened without one --- a reader who typed a
        // password for a file that did not need it, or a second worker for a
        // document whose encryption an empty user password already satisfied.
        // Accepted rather than refused: the request asks for a document this
        // process can read, and it has one. Refusing would report a failure for
        // a state that is exactly what was wanted.
        Request::Unlock { .. } => Response::reply(Reply::Unlocked),
    }
}

/// Rewrites the document under a plan and writes it down the output channel.
///
/// **The one request whose answer does not come back in the reply.** The reason
/// is size: [`crate::worker_proto::MAX_REPLY_BYTES`] is 32 MB and a scanned
/// document is ten times that, so a rewrite could not move into a worker until
/// there was somewhere for the bytes to go. See
/// [`crate::worker_proto::Request::Rewrite`].
///
/// **Refused, not ignored, when there is nowhere to write.** A worker without an
/// output file was not spawned to write, and a rewrite request reaching it is a
/// defect on the other side of the pipe --- said in words rather than by writing
/// a document into whichever descriptor happens to be open at that number.
///
/// What checks the write landed whole is on the other side: the coordinator
/// compares the staged file's own size against the length below. That is also
/// what would catch a second rewrite on one worker, which would append rather
/// than replace --- nothing sends one, and the check does not depend on that
/// staying true.
fn rewrite(
    document: &OpenDocument,
    out: Option<&mut std::fs::File>,
    plan: &crate::edits::Plan,
    job: crate::save::Job,
) -> Response {
    let Some(out) = out else {
        return Response::err(
            "this worker was not started with anywhere to write, so it cannot rewrite a document",
        );
    };
    let bytes = match render::run_rewrite(document, plan, job) {
        Ok(bytes) => bytes,
        // `refused`, not `err`: some of these refusals are answerable by
        // reloading and the rest are not, and which is which is a fact the
        // coordinator cannot recover from a sentence. See `Response::changed`.
        Err(why) => return Response::refused(&why),
    };
    // Every byte into the kernel, and no further: the parent owns the file, and
    // it is the parent's `sync_data` before the rename that makes the contents a
    // statement about the platter rather than about a buffer. Syncing here as
    // well would cost a second flush of the same data for nothing.
    match out.write_all(&bytes).and_then(|()| out.flush()) {
        Ok(()) => Response::reply(Reply::Rewrote(bytes.len())),
        Err(e) => Response::err(format!("the rewritten document could not be written: {e}")),
    }
}

/// Reports the document's geometry.
///
/// Deliberately not `render::open_document`: that one opens from a path, which
/// is the thing this process does not have. The document is already open by the
/// time anything is served.
fn open(document: &OpenDocument, lazy_geometry: bool) -> Response {
    let page_count = document.page_count();

    let size_of = |index: u32| -> Result<PageSize, String> {
        let page = document.page(index)?;
        Ok(PageSize {
            width_pt: page.width_pt(),
            height_pt: page.height_pt(),
        })
    };

    // One page when lazy, because the first page's size is what the viewer needs
    // to lay out its first frame; enumerating 775 pages costs 86 ms on the
    // critical path (PLAN §4).
    let pages: Result<Vec<PageSize>, String> = if lazy_geometry {
        match page_count {
            0 => Ok(Vec::new()),
            _ => size_of(0).map(|first| vec![first]),
        }
    } else {
        (0..page_count).map(size_of).collect()
    };

    match pages {
        Ok(pages) => Response::reply(Reply::Open {
            pages,
            page_count: page_count as usize,
            lazy_geometry,
        }),
        Err(e) => Response::err(e),
    }
}

/// Renders one tile into the shared mapping.
fn render(
    bindings: progressive::Bindings,
    document: &OpenDocument,
    queue: &SharedQueue,
    tile: &mut Shm,
    request: &Request,
) -> Response {
    let Request::Tile {
        rid,
        page,
        scale,
        turns,
        invert,
        x,
        y,
        width,
        height,
        png,
        crop,
    } = *request
    else {
        return Response::err("not a tile request");
    };

    let token = match queue.with(|queue| queue.claim(rid)) {
        Claim::Start(token) => token,
        Claim::Withdrawn => {
            return Response {
                ok: true,
                abandoned: true,
                ..Default::default()
            }
        }
    };

    let req = TileRequest {
        rid,
        doc: 0,
        page,
        crop,
        scale,
        turns,
        invert,
        x,
        y,
        width,
        height,
        format: if png {
            TileFormat::Png
        } else {
            TileFormat::Raw
        },
    };

    let outcome = render::render_tile(bindings, document, &req, &token);
    queue.with(|queue| queue.release(rid));

    match outcome {
        Err(e) => Response::err(e),
        Ok(render::TileOutcome::Abandoned) => Response {
            ok: true,
            abandoned: true,
            ..Default::default()
        },
        Ok(render::TileOutcome::Rendered(rendered)) => {
            let payload = rendered.bytes;
            let room = tile.len();
            if payload.len() > room {
                // Refused rather than truncated. A short tile is a picture with
                // the bottom missing, which reads as a rendering bug forever
                // after; a refusal names its own cause once.
                return Response::err(format!(
                    "tile is {} bytes and the shared mapping holds {room}",
                    payload.len()
                ));
            }
            tile.as_mut_slice()[..payload.len()].copy_from_slice(&payload);
            Response {
                ok: true,
                bytes: payload.len(),
                render_us: rendered.render_us,
                encode_us: rendered.encode_us,
                ..Default::default()
            }
        }
    }
}

/// Writes one reply line.
fn reply(out: &mut impl Write, response: &Response) -> Result<(), String> {
    let mut line = serde_json::to_string(response).map_err(|e| e.to_string())?;
    line.push('\n');
    out.write_all(line.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("could not reply: {e}"))
}

/// Blocks until the parent hands over a document mapping.
///
/// # Errors
///
/// The socket closing --- which is how a pre-spawned worker that is never given a
/// file learns to exit rather than waiting forever --- or a malformed handover.
#[cfg(target_os = "macos")]
fn wait_for_document() -> Result<Shm, String> {
    use std::os::fd::IntoRawFd;

    // SAFETY: the parent dup2'd its half of a socket pair to this number before
    // exec, and nothing else in this process reads it.
    let (fd, len) = unsafe { recv_document(SOCK_FD) }?;
    // `into_raw_fd` rather than `as_raw_fd` and a forget: `Shm` adopts the
    // descriptor and closes it on drop, so leaving the `OwnedFd` alive would
    // close it twice.
    let raw = fd.into_raw_fd();
    // SAFETY: just received, owned here, and handed to `Shm` which now owns it.
    unsafe { Shm::from_fd(raw, len, false) }
}

/// Blocks until the parent hands over a document mapping.
///
/// Read off **stdin**, which is the same pipe every later request arrives on,
/// and that is safe for exactly one reason worth stating: [`spawn_reader`] is
/// not started until `serve` has a document, so at this point nothing else in
/// the process is reading. Both readers go through `std::io::stdin()`, so any
/// bytes over-read into its buffer here are still there for the reader thread
/// --- a private `BufReader` on the raw handle would swallow a request that
/// arrived promptly behind the handover.
///
/// The handle in the message is already this process's: the parent duplicated
/// it here before writing. See [`Handover`].
///
/// # Errors
///
/// The pipe closing --- which is how a pre-spawned worker that is never given a
/// file learns to exit rather than waiting forever --- or a malformed handover.
#[cfg(windows)]
fn wait_for_document() -> Result<Shm, String> {
    use std::io::BufRead;

    let mut line = String::new();
    let read = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("reading the document handover: {e}"))?;
    if read == 0 {
        return Err("the parent closed the pipe before handing over a document".into());
    }
    let handover: Handover = serde_json::from_str(line.trim())
        .map_err(|e| format!("unreadable document handover {line:?}: {e}"))?;
    // SAFETY: the parent duplicated this section into our table before naming it,
    // and nothing else in this process owns it.
    unsafe { Shm::from_handle(handover.handle, handover.len, false) }
}

/// Not reachable: a worker refuses to start at all on this platform.
#[cfg(not(any(target_os = "macos", windows)))]
fn wait_for_document() -> Result<Shm, String> {
    Err("pre-spawned workers are implemented on macOS and Windows only".into())
}

/// Makes PDFium build its system font list, before any document needs it.
///
/// Deliberately ignores every failure. This is an optimisation: a worker that
/// could not warm itself is still a correct worker, and a hard failure here would
/// turn a missed 7 ms into a document that does not open. What it must not do is
/// *look* like it worked --- `examples/prespawn_bench.rs --mode warm` measures the
/// effect, so a warm that silently stopped working shows up as the saving
/// disappearing rather than as nothing at all.
fn warm_fonts(bindings: progressive::Bindings) {
    let Ok(document) = OpenDocument::open_bytes(bindings, WARM_DOCUMENT, None) else {
        return;
    };
    // Through `render_tile`, not a bespoke render: warming has to exercise the
    // path a real tile takes, or it warms something adjacent to what is measured.
    let request = TileRequest {
        rid: 0,
        doc: 0,
        page: 0,
        // The warm document's own box, because warming has to take the path a
        // real tile takes and the overwhelming majority of tiles carry no crop.
        crop: None,
        scale: 1.0,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        // The whole 20x20 page. Rendering is what forces the lookup --- opening
        // the page does not resolve the face, because nothing has asked for a
        // glyph yet.
        width: 20,
        height: 20,
        format: TileFormat::Raw,
    };
    let _ = render::render_tile(bindings, &document, &request, &CancelToken::new());
}

// Moved to `progressive::bind` so the Windows containment probe could use the
// same binding rather than a sixth copy of it. The reason it *had* to move is
// gone --- this module was `#[cfg(unix)]` at the time, which made the one public
// binding the one a Windows probe could not reach --- and the move is kept anyway:
// `progressive` is where the thing being bound lives, and a re-export costs
// nothing while moving it back would churn five call sites to no end. Re-exported
// rather than relocated at those call sites because `fdpass_probe.rs` imports it
// here beside `apply_sandbox`, which genuinely is macOS-only, and splitting that
// import would suggest the two have different homes for a findable reason.
pub use crate::progressive::bind;

#[cfg(target_os = "macos")]
extern "C" {
    /// `sandbox_init` is deprecated in the SDK headers and is what every
    /// sandboxed process on this platform still uses.
    fn sandbox_init(
        profile: *const std::os::raw::c_char,
        flags: u64,
        errorbuf: *mut *mut std::os::raw::c_char,
    ) -> std::os::raw::c_int;
}

/// Drops this process's authority.
///
/// # Errors
///
/// A profile the kernel refuses, which is reported with the message
/// `sandbox_init` produced rather than as a bare code.
#[cfg(target_os = "macos")]
pub fn apply_sandbox(profile: &str) -> Result<(), String> {
    let c_profile = std::ffi::CString::new(profile).map_err(|e| format!("bad profile: {e}"))?;
    let mut error: *mut std::os::raw::c_char = std::ptr::null_mut();
    // SAFETY: the profile is a live NUL-terminated string for the call, and
    // `error` is a valid out-parameter.
    let rc = unsafe { sandbox_init(c_profile.as_ptr(), 0, &raw mut error) };
    if rc == 0 {
        return Ok(());
    }
    // SAFETY: on failure sandbox_init sets `error` to a NUL-terminated string.
    let message = unsafe {
        if error.is_null() {
            "no reason given".to_string()
        } else {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        }
    };
    Err(format!("sandbox_init failed ({rc}): {message}"))
}

/// Drops this process's authority.
///
/// # Errors
///
/// Always, off macOS. Returning `Ok` here would leave every containment claim in
/// `docs/THREAT-MODEL.md` asserted by a process that has none.
#[cfg(not(target_os = "macos"))]
pub fn apply_sandbox(_profile: &str) -> Result<(), String> {
    Err("no sandbox is implemented on this platform".into())
}

#[cfg(test)]
mod tests {
    /// An uncontained process cannot get past `establish_boundary`.
    ///
    /// This test exists because a refusal was **deleted** to make room for it.
    /// `lib.rs` used to gate the `--render-worker` dispatch on `cfg(unix)`, so no
    /// Windows process could reach this module at all; now every platform can, and
    /// the only thing standing between an argv and a document being parsed
    /// uncontained is this call. A guard that replaces another guard has to be
    /// shown to hold, or the change was a removal wearing the clothes of a port.
    ///
    /// **Not run on macOS, and the reason is the whole difference between the two
    /// platforms.** There, `establish_boundary` does not check anything --- it
    /// *applies* `sandbox_init`, and would succeed. The test would pass, and every
    /// test scheduled after it in the same process would then be running inside a
    /// sandbox with no filesystem, failing for reasons with nothing to do with
    /// what they assert. A check that quietly re-configures the process it runs in
    /// is worse than no check.
    ///
    /// So this covers exactly the platform whose guard is new. macOS's is the
    /// pre-existing one, exercised end to end by `examples/fdpass_probe.rs` in a child
    /// process, which is where a call with this blast radius belongs.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_worker_that_is_not_contained_refuses_to_serve() {
        let err = super::establish_boundary().expect_err("a test runner is not contained");
        assert!(!err.is_empty(), "a refusal has to say why");
    }
}
