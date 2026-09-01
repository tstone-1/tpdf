//! The worker-side halves of the save seams: what [`InWorker`] actually does.
//!
//! **Why this is not in `save.rs`.** That module is the document writer --- it
//! reads the reader's plan, applies it to an object graph and produces bytes ---
//! and its header says so. The [`Reread`]/[`Rewriter`] seams and the two choices
//! a save picks between belong there, because the choice is part of what a save
//! *is*. The implementation of the worker choice does not: it spawns a process,
//! maps a shared segment, speaks the worker protocol and ends a pid on a
//! deadline, which is four modules of the process boundary reached from a module
//! whose import header named none of them.
//!
//! `docs/TRAPS.md` records the inverse shape --- a module header that says "and
//! nothing more" over a `use` block that says otherwise --- and this was that
//! failure with the halves swapped: the `use` block was the half-truth, because
//! every one of those four modules was written out in full inside a function
//! body, where no reader of the header would meet it. So the arrow now points
//! the way the rest of the crate's do: the process-boundary side depends on the
//! writer's seam, and `save.rs` depends on nothing here. The `use` block below
//! is that coupling, stated once, at the top of the file that has it.
//!
//! **What stayed behind.** [`Reread`], [`Rewriter`] and [`Outside`] are the seam,
//! declared where the save that chooses between them lives; `save::Here` is the
//! in-process fallback and needs nothing from this side; and [`InWorker`]'s
//! declaration stays there too, beside `Here`, so that the pair a caller picks
//! from is named in one place. What moved is the behaviour, which is what
//! carried the dependency.
//!
//! Nothing changed in the move. The bodies are the same code, and the only edits
//! are the fully-qualified paths becoming imports --- which is the point.

use crate::edits::Plan;
use crate::save::{InWorker, Outside, Refusal, Reread, Rewriter, NO_VIEW_TURN};
use crate::worker::Worker;
use crate::worker_proto::{Reply, Request};
use crate::worker_shm::Shm;
use crate::workers::{kill_pid, DEFAULT_DEADLINE};

/// Waits for a worker's answer, and ends the worker if it does not come.
///
/// **The bound this path did not have.** `InWorker::pages` spawns its worker
/// outside the pool, so the pool's supervisor --- the thing that owns
/// [`DEFAULT_DEADLINE`] --- never sees it, and `Worker::call` is a blocking read
/// bounded only by how *long* a reply may be. A document
/// whose cross-reference sends `lopdf` round in circles would hold the
/// `spawn_blocking` thread for ever, with the reader's document already closed
/// and the appended bytes on disk unconfirmed.
///
/// `within` is a parameter for `overdue`'s reason: a check whose failure mode is
/// a wait cannot be exercised, so the decision has to be reachable without
/// hanging anything.
///
/// The pid is killed rather than the thread being asked to stop, because the
/// thread is blocked inside a pipe read and nothing can interrupt it. Ending the
/// process closes the pipe, the read fails, the thread drops its `Worker` and
/// exits --- so the timeout leaks neither a process nor a thread.
fn awaited<T>(
    rx: &std::sync::mpsc::Receiver<T>,
    within: std::time::Duration,
    pid: u32,
) -> Result<T, String> {
    match rx.recv_timeout(within) {
        Ok(answer) => Ok(answer),
        Err(_) => {
            kill_pid(pid);
            Err(format!(
                "the worker checking the saved file did not answer within {:.0} s, so the \
                 save could not be confirmed",
                within.as_secs_f64()
            ))
        }
    }
}

impl Reread for InWorker {
    fn pages(
        &self,
        file: &mut std::fs::File,
        len: usize,
        password: Option<&str>,
    ) -> Result<usize, String> {
        // The handle, never `source`. See [`Reread`]: mapping by name would
        // verify whichever file has that name now.
        let mapped = Shm::map_open_file(file, len)?;
        let worker = Worker::spawn_shared(std::sync::Arc::new(mapped), &self.library_dir)?;

        // **Asked on a thread so the answer can be waited for with a bound.**
        // See `awaited`. The pid is read before the move, because afterwards
        // this thread no longer owns the worker.
        let pid = worker.pid();
        let key = password.map(str::to_string);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut worker = worker;
            let _ = tx.send(Self::ask(&mut worker, key.as_deref()));
        });
        awaited(&rx, DEFAULT_DEADLINE, pid)?
    }
}

impl InWorker {
    /// The two requests the read-back makes, on the thread that owns the worker.
    fn ask(worker: &mut Worker, password: Option<&str>) -> Result<usize, String> {
        // **Before the question, and only when there is one.** A locked document
        // that is not unlocked first parses to zero objects, so the count would
        // come back as 0 against the pages the save expects and roll back a file
        // that is correct --- the same failure `reread_pages` names, arriving one
        // process further out.
        if let Some(password) = password {
            let answered = worker.call(&Request::Unlock {
                password: password.to_string(),
            })?;
            if !answered.ok {
                return Err(format!(
                    "the worker could not take the document's password: {}",
                    answered.error
                ));
            }
        }

        let answered = worker.call(&Request::Reread)?;
        if !answered.ok {
            return Err(answered.error);
        }
        match answered.reply {
            Some(Reply::Reread(pages)) => Ok(pages),
            // A well-formed message answering a different question. Nothing in
            // the protocol checks that a reply matches its request --- `Reply`'s
            // own documentation says so --- so the caller does, and says which it
            // got rather than reporting a parse failure for a protocol one.
            other => Err(format!(
                "the worker answered the re-read with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )),
        }
    }
}

impl Outside for InWorker {}

impl Rewriter for InWorker {
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        // The handles, never the pathnames. See [`Rewriter`].
        let mapped = Shm::map_open_file(source, len)?;
        let worker = Worker::spawn_writing(std::sync::Arc::new(mapped), out, &self.library_dir)?;

        // **Asked on a thread so the answer can be waited for with a bound**, as
        // in [`InWorker::pages`]: this worker is outside the pool, so nothing
        // else owns a deadline for it, and a document that sends `lopdf` round
        // in circles would otherwise hold the blocking thread for ever. The pid
        // is read before the move, because afterwards this thread no longer owns
        // the worker.
        let pid = worker.pid();
        let key = password.map(str::to_string);
        let plan = plan.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut worker = worker;
            let _ = tx.send(Self::ask_rewrite(&mut worker, &plan, key.as_deref()));
        });
        awaited(&rx, DEFAULT_DEADLINE, pid)?
    }
}

impl InWorker {
    /// The two requests a rewrite makes, on the thread that owns the worker.
    ///
    /// [`InWorker::ask`]'s counterpart, and the unlock in front of it is there
    /// for the same reason: `lopdf` parses no objects at all for a document it
    /// cannot authenticate, so a locked document would rewrite to an empty one
    /// rather than refusing.
    fn ask_rewrite(
        worker: &mut Worker,
        plan: &Plan,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        if let Some(password) = password {
            let answered = worker.call(&Request::Unlock {
                password: password.to_string(),
            })?;
            if !answered.ok {
                return Err(format!(
                    "the worker could not take the document's password: {}",
                    answered.error
                )
                .into());
            }
        }

        let answered = worker.call(&Request::Rewrite {
            plan: plan.clone(),
            view: NO_VIEW_TURN,
        })?;
        if !answered.ok {
            // The one bit that has to survive the pipe: whether Reload is the
            // answer. See `Response::changed`.
            return Err(Refusal {
                message: answered.error,
                changed: answered.changed,
            });
        }
        match answered.reply {
            Some(Reply::Rewrote(bytes)) => Ok(bytes),
            // A well-formed message answering a different question --- see
            // [`InWorker::ask`], which says why the caller checks this rather
            // than the protocol.
            other => Err(format!(
                "the worker answered the rewrite with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )
            .into()),
        }
    }
}

#[cfg(test)]
mod tests {
    // **Gated the same way the tests below are, and `use super::*` is what does
    // not work here.** Both tests need a real process to stand in for a worker,
    // so both are `#[cfg(unix)]` --- which leaves this module empty on Windows,
    // and an empty module's glob import is an `unused_imports` error under the
    // `clippy` gate's `-D warnings`. It compiled and linted clean on this Mac
    // and failed `scripts/check_windows.py`, which is that script's whole
    // purpose: a Mac compiler never parses the arms the other platform keeps.
    #[cfg(unix)]
    use super::awaited;

    /// A read-back that never answers must end its worker, not wait for ever.
    ///
    /// `InWorker::pages` spawns its worker outside the pool, so the supervisor
    /// that owns the deadline never sees it and `Worker::call` blocks on a pipe
    /// with no bound. The document is already closed by then and the appended
    /// bytes are already on disk, so "for ever" means a save that can never be
    /// confirmed or rolled back.
    ///
    /// Exercised through `awaited` with a real process standing in for the
    /// worker, because the decision takes its duration as an argument --- a check
    /// whose only failure mode is a wait cannot fail.
    #[test]
    #[cfg(unix)]
    fn a_read_back_that_never_answers_ends_the_worker() {
        let mut victim = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn a stand-in worker");
        let pid = victim.id();
        // Nobody ever sends. `_tx` is held so the channel is not simply closed,
        // which would be a different outcome from silence.
        let (_tx, rx) = std::sync::mpsc::channel::<usize>();

        let began = std::time::Instant::now();
        let within = std::time::Duration::from_millis(150);
        let why = awaited(&rx, within, pid)
            .expect_err("a wait that gets no answer must not report success");
        let waited = began.elapsed();
        assert!(
            waited >= within,
            "it has to have waited for the deadline it was given, and waited {waited:?}"
        );
        // **The upper bound is the half that has teeth.** A lower bound alone is
        // satisfied by *any* longer wait, so a deadline a thousand times too long
        // passes it --- measured: the same assertion stayed green while the test
        // took 150 seconds instead of 0.17. A bound whose failure mode is a
        // longer wait is not a bound. Twenty times the deadline is loose enough
        // for a loaded runner and nowhere near a mistake worth catching.
        assert!(
            waited < within * 20,
            "the wait has to be about the deadline it was given, and took {waited:?}"
        );
        assert!(
            why.contains("did not answer"),
            "the refusal has to say what happened: {why}"
        );

        let mut gone = false;
        for _ in 0..200 {
            if victim.try_wait().expect("wait").is_some() {
                gone = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = victim.kill();
        let _ = victim.wait();
        assert!(
            gone,
            "the worker must be ended --- otherwise the timeout leaks the process and the \
             thread blocked reading its pipe"
        );
    }

    /// The control: an answer that arrives leaves the worker alone.
    ///
    /// Without it, an `awaited` that killed unconditionally would pass the test
    /// above, and every ordinary save would be ending a healthy worker.
    #[test]
    #[cfg(unix)]
    fn a_read_back_that_answers_in_time_leaves_the_worker_alone() {
        let mut victim = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn a stand-in worker");
        let pid = victim.id();
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        tx.send(7).expect("send the answer");

        assert_eq!(
            awaited(&rx, std::time::Duration::from_secs(5), pid).expect("the answer arrives"),
            7
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        let still_there = victim.try_wait().expect("wait").is_none();
        let _ = victim.kill();
        let _ = victim.wait();
        assert!(
            still_there,
            "a call that was answered must not have its worker killed"
        );
    }
}
