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
use crate::save::{InWorker, Job, Outside, Refusal, Reread, Rewriter, Verifier};
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

/// Runs one exchange on a thread of its own, and **releases the worker before
/// the answer is sent**.
///
/// The ordering is the whole reason this is a function rather than four copies
/// of a `thread::spawn`. Every caller here hands its worker a mapping of the
/// file the coordinator is about to act on: `Reread::pages` maps the file an
/// append has just written, and each `Rewriter` method maps the document being
/// rewritten. The coordinator's answer to a bad read-back is
/// `save::append_through`'s roll-back, which is `set_len` on that same file ---
/// and on Windows a file with a section object open on it cannot be resized at
/// all. `SetFileInformationByHandle` fails with `ERROR_USER_MAPPED_FILE`
/// (1224), so the reader was told *"the saved file could not be read back ---
/// and it could not be put back"* and kept the update the re-read had just
/// refused, on the platform this cannot be tested from.
///
/// Sending last is what makes the release *observable* to the waiter: a `drop`
/// written after the `send` runs at some unrelated moment on a thread nobody is
/// watching, so "the mapping is gone by the time the answer arrives" was true
/// only by luck. Here it is true by construction, and
/// `a_worker_is_released_before_its_answer_is_sent` is what pins it.
///
/// Generic over what is being released rather than over `Worker`, so a test can
/// hand it something whose `Drop` is visible --- there is no way to observe a
/// real worker's death from inside a `cargo test`.
fn asked_on_a_thread<R, T>(
    resource: R,
    ask: impl FnOnce(&mut R) -> T + Send + 'static,
) -> std::sync::mpsc::Receiver<T>
where
    R: Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut resource = resource;
        let answer = ask(&mut resource);
        // Before the send, never after. See the note above.
        drop(resource);
        let _ = tx.send(answer);
    });
    rx
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
        let rx = asked_on_a_thread(worker, move |worker| Self::ask(worker, key.as_deref()));
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

impl Verifier for InWorker {
    fn scan(
        &self,
        file: &mut std::fs::File,
        len: usize,
        needles: &[String],
        password: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
        // The handle, never the path. Same reason as `Reread::pages`: mapping by
        // name would scan whichever file has that name now, and this one is the
        // file the redaction just wrote.
        let mapped = Shm::map_open_file(file, len)?;
        let worker = Worker::spawn_shared(std::sync::Arc::new(mapped), &self.library_dir)?;

        let pid = worker.pid();
        let key = password.map(str::to_string);
        let asked = needles.to_vec();
        let rx = asked_on_a_thread(worker, move |worker| {
            Self::ask_scan(worker, &asked, key.as_deref())
        });
        awaited(&rx, DEFAULT_DEADLINE, pid)?
    }
}

impl InWorker {
    /// The two requests the scan makes, on the thread that owns the worker.
    ///
    /// **The unlock is not optional, and its absence is the reassuring failure.**
    /// A redacted copy of an encrypted document is re-encrypted, so a worker that
    /// was never given the key parses no objects at all --- and a walk over no
    /// objects finds no needles. `crate::verify::scan` is built so that this
    /// reports *not verified* rather than clean, which is what makes the failure
    /// safe; asking first is what makes it answerable.
    fn ask_scan(
        worker: &mut Worker,
        needles: &[String],
        password: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
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

        let answered = worker.call(&Request::Verify {
            needles: needles.to_vec(),
        })?;
        if !answered.ok {
            return Err(answered.error);
        }
        match answered.reply {
            Some(Reply::Verified(report)) => Ok(*report),
            // A well-formed message answering a different question --- see
            // `InWorker::ask`, which says why the caller checks this rather than
            // the protocol.
            other => Err(format!(
                "the worker answered the verification with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )),
        }
    }
}

impl Outside for InWorker {}

// A mapped source can change underneath a renderer. Raster redaction instead
// hands the worker an immutable snapshot of exactly the bytes that were opened.
fn raster_snapshot(source: &mut std::fs::File, len: usize, plan: &Plan) -> Result<Shm, Refusal> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    let expected = plan.opened_as.as_ref().ok_or_else(|| {
        Refusal::from("Reopen the source before creating an image-only redaction")
    })?;
    if len > 512 * 1024 * 1024 || len as u64 != expected.len {
        return Err("The source is too large or has changed since it was opened".into());
    }
    let mut mapped = Shm::create(len)?;
    source.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    source
        .read_exact(mapped.as_mut_slice())
        .map_err(|e| e.to_string())?;
    let digest: [u8; 32] = Sha256::digest(mapped.as_slice()).into();
    if digest != expected.digest {
        return Err(Refusal::changed(
            "The source changed. Reopen it and mark the regions again",
        ));
    }
    Ok(mapped)
}

impl Rewriter for InWorker {
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        job: Job,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        // The handles, never the pathnames. See [`Rewriter`].
        let mapped = if matches!(job, Job::RasterRedact | Job::RedactionFill) {
            raster_snapshot(source, len, plan)?
        } else {
            Shm::map_open_file(source, len)?
        };
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
        let rx = asked_on_a_thread(worker, move |worker| {
            Self::ask_rewrite(worker, &plan, job, key.as_deref())
        });
        let deadline = if job == Job::RasterRedact {
            std::time::Duration::from_secs(180)
        } else {
            DEFAULT_DEADLINE
        };
        awaited(&rx, deadline, pid)?
    }

    fn merge(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        inputs: crate::save::Inputs<'_>,
        password: Option<&str>,
    ) -> Result<(usize, u32), Refusal> {
        // **A third mapping, and it is the incoming documents.** `write` and
        // `write_range` hand the worker one document and a file to write; this
        // hands it the files the reader chose as well, concatenated and
        // read-only. See `crate::worker::IN_FD`.
        //
        // The segment is the caller's --- `crate::save::concatenated` reads the
        // files straight into it --- and is handed on rather than copied. It was
        // copied into a fresh `Shm` here, which was a second full copy of every
        // incoming document in this process, on top of the `Vec` the reads went
        // into first.
        let mapped = Shm::map_open_file(source, len)?;
        let worker = Worker::spawn_merging(
            std::sync::Arc::new(mapped),
            inputs.whole,
            out,
            &self.library_dir,
        )?;

        let pid = worker.pid();
        let key = password.map(str::to_string);
        let plan = plan.clone();
        let incoming = inputs.each.to_vec();
        let rx = asked_on_a_thread(worker, move |worker| {
            Self::ask_merge(worker, &plan, &incoming, key.as_deref())
        });
        // **The incoming segment outlives the wait**, which the borrow rather
        // than a comment is what guarantees now: `inputs` is the caller's and
        // `crate::save::write_merged` holds it across this call. Unmapping those
        // pages while the child is reading them would fault it on a document
        // that is perfectly good.
        awaited(&rx, DEFAULT_DEADLINE, pid)?
    }

    fn write_range(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        job: &crate::print::Job,
    ) -> Result<usize, Refusal> {
        // Everything here is `write`'s and means the same: the handles rather
        // than the pathnames, a worker spawned for this one answer, and the ask
        // on a thread so a document that sends `lopdf` round in circles cannot
        // hold the blocking pool for ever.
        let mapped = Shm::map_open_file(source, len)?;
        let worker = Worker::spawn_writing(std::sync::Arc::new(mapped), out, &self.library_dir)?;

        let pid = worker.pid();
        let job = job.clone();
        let rx = asked_on_a_thread(worker, move |worker| Self::ask_print_range(worker, &job));
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
        job: Job,
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
            job,
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

    /// The two requests a merge makes, on the thread that owns the worker.
    ///
    /// [`InWorker::ask_rewrite`]'s counterpart, and the unlock in front of it is
    /// there for the same reason: `lopdf` parses no objects at all for a
    /// document it cannot authenticate, so a locked base would merge into an
    /// empty document rather than refusing. The password is the base's; the
    /// incoming files are refused if they are encrypted, and tpdf holds no key
    /// for them anyway.
    fn ask_merge(
        worker: &mut Worker,
        plan: &Plan,
        incoming: &[crate::save::Incoming],
        password: Option<&str>,
    ) -> Result<(usize, u32), Refusal> {
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

        let answered = worker.call(&Request::Merge {
            plan: plan.clone(),
            incoming: incoming.to_vec(),
        })?;
        if !answered.ok {
            return Err(Refusal {
                message: answered.error,
                changed: answered.changed,
            });
        }
        match answered.reply {
            Some(Reply::Merged { bytes, pages }) => Ok((bytes, pages)),
            other => Err(format!(
                "the worker answered the merge with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )
            .into()),
        }
    }

    /// The one request a page-range print makes, on the thread that owns the
    /// worker.
    ///
    /// **No unlock in front of it**, which is the difference from
    /// [`InWorker::ask_rewrite`] and is not an omission: `print::build_update`
    /// refuses an encrypted document whether or not the key is held, so sending
    /// a password would buy a decrypted copy of a document somebody encrypted
    /// deliberately and nothing else.
    fn ask_print_range(worker: &mut Worker, job: &crate::print::Job) -> Result<usize, Refusal> {
        let answered = worker.call(&Request::PrintRange { job: job.clone() })?;
        if !answered.ok {
            return Err(Refusal {
                message: answered.error,
                changed: answered.changed,
            });
        }
        match answered.reply {
            Some(Reply::Rewrote(bytes)) => Ok(bytes),
            other => Err(format!(
                "the worker answered the print job with {}",
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
    #[test]
    fn raster_snapshot_binds_bytes_and_survives_source_changes() {
        use std::io::{Seek, SeekFrom, Write};
        let path =
            std::env::temp_dir().join(format!("tpdf-raster-snapshot-{}", std::process::id()));
        let mut source = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        source.write_all(b"synthetic original").unwrap();
        let fingerprint = crate::fingerprint::Fingerprint::of_open(
            &source,
            std::path::Path::new("synthetic-source"),
        )
        .unwrap();
        let mut plan = crate::edits::Plan {
            baseline: 0,
            opened_as: Some(fingerprint),
            pages: vec![],
            marks: vec![],
            redactions: vec![],
            notes: vec![],
            discards: vec![],
            forms: Vec::new(),
            text_edits: Vec::new(),
        };
        let snapshot = super::raster_snapshot(&mut source, 18, &plan).unwrap();
        source.seek(SeekFrom::Start(0)).unwrap();
        source.write_all(b"synthetic replaced").unwrap();
        assert_eq!(snapshot.as_slice(), b"synthetic original");
        let error = super::raster_snapshot(&mut source, 18, &plan)
            .err()
            .unwrap();
        assert!(error.changed);
        plan.opened_as = None;
        assert!(super::raster_snapshot(&mut source, 18, &plan).is_err());
        drop(source);
        std::fs::remove_file(path).unwrap();
    }

    // **Gated the same way the tests below are, and `use super::*` is what does
    // not work here.** Both tests need a real process to stand in for a worker,
    // so both are `#[cfg(unix)]` --- which leaves this module empty on Windows,
    // and an empty module's glob import is an `unused_imports` error under the
    // `clippy` gate's `-D warnings`. It compiled and linted clean on this Mac
    // and failed `scripts/check_windows.py`, which is that script's whole
    // purpose: a Mac compiler never parses the arms the other platform keeps.
    #[cfg(unix)]
    use super::awaited;
    // Not gated: the ordering it exercises is the same on both platforms, and
    // the platform it was written for is the one that cannot run it.
    use super::asked_on_a_thread;

    /// The worker is released before its answer is sent, never after.
    ///
    /// What the worker owns is an `Arc<Shm>` mapping the file the coordinator is
    /// about to act on, and the coordinator's first act on a bad answer is
    /// `set_len` on that file. On Windows a file with a section object open on it
    /// cannot be resized: the roll-back fails with `ERROR_USER_MAPPED_FILE`, and
    /// the reader is told the file could not be put back while keeping an update
    /// that was just refused.
    ///
    /// A stand-in stands in for the worker because a `cargo test` cannot spawn a
    /// real one --- and because what has to be observed is a *drop*, which a real
    /// worker gives no way to see. The sleep inside it is deliberate: with the
    /// send first, the waiter wins this race every time, so the pre-fix ordering
    /// fails this test on every run rather than on one in ten.
    #[test]
    fn a_worker_is_released_before_its_answer_is_sent() {
        use std::sync::{Arc, Mutex};

        struct Mapping(Arc<Mutex<Vec<&'static str>>>);
        impl Drop for Mapping {
            fn drop(&mut self) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                note(&self.0, "released");
            }
        }
        fn note(order: &Arc<Mutex<Vec<&'static str>>>, what: &'static str) {
            order.lock().unwrap_or_else(|e| e.into_inner()).push(what);
        }

        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = asked_on_a_thread(Mapping(Arc::clone(&order)), |_| 7_usize);
        assert_eq!(rx.recv().expect("the answer arrives"), 7);
        note(&order, "answered");

        // Both entries are waited for, so that a pre-fix run is judged on the
        // order and not on whether the other thread had got round to it.
        for _ in 0..200 {
            if order.lock().unwrap_or_else(|e| e.into_inner()).len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let order = order.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            order.as_slice(),
            ["released", "answered"],
            "the mapping has to be gone by the time the coordinator can act on the answer"
        );
    }

    /// Why the ordering above is load-bearing, in the platform's own terms.
    ///
    /// Windows refuses to resize a file while a section object is open on it, so
    /// `save::append_through`'s roll-back --- a `set_len` back to the length the
    /// file had before the update --- fails outright while the worker still
    /// holds its mapping. Both directions are asserted: the refusal while it is
    /// mapped is what makes the success after the drop mean something.
    ///
    /// Cannot run on macOS, where `ftruncate` succeeds either way. The
    /// consequence it describes is Windows-only for exactly that reason.
    #[test]
    #[cfg(windows)]
    fn a_mapped_file_cannot_be_cut_back_until_the_mapping_is_released() {
        use crate::worker_shm::Shm;

        let dir = std::env::temp_dir().join(format!("tpdf-mapped-cut-back-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let at = dir.join("doc.pdf");
        std::fs::write(&at, vec![b'x'; 2_048]).expect("write the subject");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&at)
            .expect("open it as the save does");

        let mapped = Shm::map_open_file(&file, 2_048).expect("map it as a worker would");
        let refused = file.set_len(1_024);
        assert!(
            refused.is_err(),
            "a file with a section open on it must not resize, or this test is measuring nothing"
        );
        // ERROR_USER_MAPPED_FILE. Named as a number because that is what the
        // reader sees in the refusal, and what a search for this failure finds.
        assert_eq!(
            refused.unwrap_err().raw_os_error(),
            Some(1224),
            "and the reason is the mapping, not permissions"
        );

        drop(mapped);
        file.set_len(1_024)
            .expect("with the mapping released the roll-back is an ordinary truncation");

        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
