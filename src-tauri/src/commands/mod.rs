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
//! parse happens. All but [`outside_of`], which needs the app handle, and the
//! bounded wait [`answered`] are `render_reply.rs`'s, re-exported here, because
//! the redaction steps outside this directory need them too.

pub mod addtext;
pub mod app;
pub mod compress;
pub mod document;
pub mod edit;
pub mod hidden;
pub mod menubar;
pub mod ocr;
pub mod print;
pub mod protect;
pub mod read;
pub mod redact;
pub mod save;
pub mod session;
pub mod sign;
pub mod spike;
pub mod validation;

use crate::render_reply::outside_at;
pub(crate) use crate::render_reply::{await_reply, password_for, reply_channel, ReplyRx};
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
    outside_at(pdfium_library_dir(app), backend)
}

/// The longest any single answer from the render service is waited for by a
/// command that walks a document on a blocking thread.
///
/// [`crate::ocr_gate`]'s bound and its reason: a wait that never ends cannot be
/// reported, and the slowest legitimate answer is the open of a large file.
const ANSWER_BOUND: std::time::Duration = std::time::Duration::from_secs(60);

/// Drives one of the render service's callback-shaped calls to an answer, on
/// a thread that may block.
///
/// For the walks that run under `spawn_blocking` --- a text recognition, the
/// check for text the pages do not show --- where [`await_reply`] has no task
/// to suspend. `doing` ends the sentence a reader is shown when no answer
/// comes: `recognising text`.
pub(crate) fn answered<T: Send + 'static, E: Send + 'static + From<String>>(
    doing: &str,
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(E::from(said!(
            "the render service did not answer within {secs} s while {doing}",
            secs = ANSWER_BOUND.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(E::from(said!("the render service stopped")))
        }
    }
}
