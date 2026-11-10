//! Asking the render service from async code: the reply channel, the wait on
//! it and the password that opened a document, and the choice of where a
//! parse of the reader's own document happens.
//!
//! The plumbing the window's commands share (`commands` re-exports it), here
//! rather than among them because the redaction steps the command line runs
//! need it too.

use crate::render::{self, RenderService};

/// `commands::outside_of` with the library directory already found.
///
/// **For the command-line tool**, which has no app handle to ask and finds the
/// directory by the same search from where its executable sits (`cli::Env`).
/// One choice, two ways to reach it, so `tpdf redact` parses where the window's
/// redaction parses.
pub(crate) fn outside_at(
    library: std::path::PathBuf,
    backend: render::Backend,
) -> Box<dyn crate::save::Outside> {
    match backend {
        render::Backend::Worker => Box::new(crate::save::InWorker::at(library)),
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
pub(crate) async fn password_for(
    service: &RenderService,
    doc: u32,
    command: &str,
) -> Option<String> {
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
        .ok_or_else(|| E::from(said!("render thread stopped ({command})")))?
}
