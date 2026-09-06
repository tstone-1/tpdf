//! Every `#[tauri::command]` the app registers, grouped by the state it touches.
//!
//! `lib.rs` was 4,291 lines carrying 66 command bodies alongside the entry
//! point, so the file that answers *what happens when the app starts* also held
//! every answer to *what happens when the reader presses a key*. The grouping
//! here is by state rather than by feature: [`document`] asks the render
//! service, [`edit`] changes the journal, [`session`] owns the one file on
//! disk, and so on --- which is the property that decides where a new command
//! goes, and the reason the groups are uneven in size.
//!
//! **The registry stays in `lib.rs`.** `generate_handler!` is what the frontend
//! is checked against (`src/lib/ipc.test.ts` reads it as source text and wants
//! plain identifiers), so the names are brought into scope there by a glob per
//! group rather than written as paths. That also keeps the list in one place
//! instead of one per file.
//!
//! **A command is `pub`, and that is not decoration.** `scripts/check_writers.py`
//! finds them with a regex over the source that admits `pub` and nothing
//! narrower, and Tauri only exports a command's wrapper macro to the crate root
//! for a function that is visible --- a private one leaves `generate_handler!`
//! unable to find it.
//!
//! What is here rather than in a group is the plumbing every group shares: the
//! reply channel, the wait on it, the password lookup and the choice of where a
//! parse happens.

pub mod app;
pub mod document;
pub mod edit;
pub mod menubar;
pub mod print;
pub mod read;
pub mod redact;
pub mod save;
pub mod session;
pub mod spike;

use crate::render::RenderService;
use crate::{pdfium_library_dir, render};

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
fn outside_of(app: &tauri::AppHandle, backend: render::Backend) -> Box<dyn crate::save::Outside> {
    match backend {
        render::Backend::Worker => Box::new(crate::save::InWorker::at(pdfium_library_dir(app))),
        render::Backend::InProcess => Box::new(crate::save::Here),
    }
}

/// Where one command's reply from the render service arrives.
///
/// The runtime's channel and not `std::sync::mpsc`, and the difference is not
/// the channel but what waiting on it costs. Every command that waits on one is
/// an `async fn`, so a blocking `recv` parks one of the runtime's few worker
/// threads for as long as the engine takes --- and nothing bounds how many at
/// once: a search walks a document one call per page, and a reader who scrolls
/// during it adds more. Awaiting suspends the *task* instead, which is the
/// resource there are millions of.
///
/// Capacity one, for one message. [`render::Reply`] is `FnOnce`, so the send in
/// [`reply_channel`] cannot find the channel full --- which is what lets it be
/// a `try_send` and never block the render thread either.
pub(crate) type ReplyRx<T, E = String> = tauri::async_runtime::Receiver<Result<T, E>>;

/// A reply callback to hand the render service, and where its answer lands.
///
/// Built here rather than at each call site so that the sender's half of the
/// arrangement --- the capacity, and the send that must not block --- is stated
/// once for every caller, the eager open in `start_eager_open` included.
pub(crate) fn reply_channel<T: Send + 'static, E: Send + 'static>(
) -> (render::ReplyTo<T, E>, ReplyRx<T, E>) {
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
pub(crate) async fn await_reply<T, E>(command: &str, mut rx: ReplyRx<T, E>) -> Result<T, E>
where
    E: From<String>,
{
    rx.recv()
        .await
        .ok_or_else(|| E::from(format!("render thread stopped ({command})")))?
}
