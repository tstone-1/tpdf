//! The other side of the process boundary --- what `--render-worker` runs.
//!
//! This process holds the document and nothing else. It has no path to it, no
//! network, no writable filesystem and no way to acquire any: everything it may
//! touch was handed over as a descriptor before it dropped its authority.
//!
//! **Ordering is the whole of the security argument here**, and it is easy to
//! get backwards. PDFium is bound *before* the sandbox is applied, because
//! binding opens and maps the dylib and a policy denying `file-read*` would make
//! that fail. The document is opened *after*, because that is the attacker's
//! input. Anything moved across that line either breaks the worker or defeats
//! it, and neither shows up as an error --- a sandbox applied too late still
//! returns `ok`.
//!
//! Two threads, and they cannot be one. A withdrawal exists to reach a render
//! that is *already running*, so it must be read while the render thread is
//! inside PDFium --- which means a reader thread owning stdin, and the render
//! thread owning the document. The reader never touches the document, which is
//! what makes that sound: `RawDocument` is not `Send`, and concurrent PDFium is
//! undefined behaviour whatever the handles are (`AGENTS.md`).

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{channel, Sender};

use crate::progressive::{self, CancelToken, RawDocument};
use crate::queue::{Claim, SharedQueue};
use crate::render::{self, PageSize, TileFormat, TileRequest};
use crate::worker::{
    doc_len_arg, library_dir_arg, Request, Response, Shm, DOC_FD, PRESPAWN_ARGV, SANDBOX_PROFILE,
    TILE_CAPACITY, TILE_FD,
};
#[cfg(target_os = "macos")]
use crate::worker::{recv_document, SOCK_FD};

/// Runs this process as a render worker. Never returns.
pub fn main(args: &[String]) -> ! {
    let code = match serve(args) {
        Ok(()) => 0,
        Err(message) => {
            // stderr is inherited from the parent precisely so this is visible.
            // A worker that dies silently is the hardest failure here to
            // diagnose, and the parent can only report an epitaph.
            eprintln!("[worker] {message}");
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
/// It exists because of what `bin/prespawn_bench.rs` measured. A worker's fixed
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

    // SAFETY: the parent dup2'd a live descriptor to this number before exec, and
    // nothing else in this process owns it.
    let mut tile_shm = unsafe { Shm::from_fd(TILE_FD, TILE_CAPACITY, true) }?;

    // Adopted before the sandbox when there is one to adopt. Read-only: a worker
    // must not be able to write the reader's file.
    let early_doc = if prespawned {
        None
    } else {
        let doc_len = doc_len_arg(args).ok_or("--doc-len is missing or unreadable")?;
        // SAFETY: as above.
        Some(unsafe { Shm::from_fd(DOC_FD, doc_len, false) }?)
    };

    // BEFORE the sandbox. Binding opens and maps libpdfium, which a policy
    // denying file reads forbids.
    let pdfium = bind(&library_dir)?;
    let bindings = progressive::bindings_of(pdfium);

    apply_sandbox(SANDBOX_PROFILE)?;

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
            reply(
                &mut std::io::stdout(),
                &Response::json(&serde_json::json!({ "prespawn": "warm" })),
            )?;
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
    let document = RawDocument::open_bytes(bindings, bytes)?;

    // The same state machine the in-process renderer uses, for the same reason:
    // a claim moves a request from queued to in flight under one lock, so a
    // withdrawal arriving at any instant either finds it queued and marks it, or
    // finds it running and cancels it. Tested in `queue.rs`.
    let queue = SharedQueue::default();
    let (tx, rx) = channel::<Request>();
    spawn_reader(tx, queue.clone());

    let mut out = std::io::stdout();
    for request in rx {
        let response = handle(bindings, &document, &queue, &mut tile_shm, &request);
        reply(&mut out, &response)?;
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
    document: &RawDocument,
    queue: &SharedQueue,
    tile: &mut Shm,
    request: &Request,
) -> Response {
    match request {
        Request::Open { lazy_geometry } => open(document, *lazy_geometry),
        Request::Tile { .. } => render(bindings, document, queue, tile, request),
        // Consumed on the reader thread; reaching here would mean the dispatch
        // above changed and this arm was forgotten.
        Request::Withdraw { .. } => Response::err("a withdrawal is not a request to answer"),
        Request::Text { page } => match render::run_text(document, *page) {
            Ok(text) => Response::json(&text),
            Err(e) => Response::err(e),
        },
        Request::Search { page, query } => match render::run_search(document, *page, query) {
            Ok(matches) => Response::json(&matches),
            Err(e) => Response::err(e),
        },
        Request::Outline => Response::json(&render::run_outline(document)),
    }
}

/// Reports the document's geometry.
///
/// Deliberately not `render::open_document`: that one opens from a path, which
/// is the thing this process does not have. The document is already open by the
/// time anything is served.
fn open(document: &RawDocument, lazy_geometry: bool) -> Response {
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
        Ok(pages) => Response::json(&serde_json::json!({
            "pages": pages,
            "page_count": page_count as usize,
            "lazy_geometry": lazy_geometry,
        })),
        Err(e) => Response::err(e),
    }
}

/// Renders one tile into the shared mapping.
fn render(
    bindings: progressive::Bindings,
    document: &RawDocument,
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

/// Not reachable: a worker refuses to start at all off macOS.
#[cfg(not(target_os = "macos"))]
fn wait_for_document() -> Result<Shm, String> {
    Err("pre-spawned workers are implemented on macOS only".into())
}

/// Makes PDFium build its system font list, before any document needs it.
///
/// Deliberately ignores every failure. This is an optimisation: a worker that
/// could not warm itself is still a correct worker, and a hard failure here would
/// turn a missed 7 ms into a document that does not open. What it must not do is
/// *look* like it worked --- `bin/prespawn_bench.rs --mode warm` measures the
/// effect, so a warm that silently stopped working shows up as the saving
/// disappearing rather than as nothing at all.
fn warm_fonts(bindings: progressive::Bindings) {
    let Ok(document) = RawDocument::open_bytes(bindings, WARM_DOCUMENT) else {
        return;
    };
    // Through `render_tile`, not a bespoke render: warming has to exercise the
    // path a real tile takes, or it warms something adjacent to what is measured.
    let request = TileRequest {
        rid: 0,
        doc: 0,
        page: 0,
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

// Moved to `progressive::bind` so the Windows containment probe can use the same
// binding rather than a sixth copy of it --- this module is `#[cfg(unix)]`, which
// made the one *public* binding the one a Windows probe could not reach. Re-exported
// rather than relocated at the call sites: `fdpass_probe.rs` imports it from here
// beside `apply_sandbox`, which genuinely is macOS-only, and splitting that import
// would suggest the two have different homes for a reason a reader could find.
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
