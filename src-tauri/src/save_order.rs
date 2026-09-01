//! In what order does a save over the reader's own file happen, and what does
//! each failure leave them holding?
//!
//! **Why this is not in `lib.rs`.** The sequence below is the most dangerous
//! ordering in the application --- it is the only path that closes the reader's
//! document and renames over their file --- and until 2026-08-31 it was the body
//! of a `#[tauri::command]` taking three `tauri::State` handles. Nothing in
//! `cargo test` can call such a function: it needs a running app, a render
//! service and a real document. So the ordering was covered by a hand-run,
//! screen-needing check and by mutations aimed one layer down at `save.rs`,
//! which is the arrangement `docs/TRAPS.md` records the cost of twice over.
//!
//! **Why it is not in `save.rs` either.** That module is the document writer:
//! it turns a plan and a file into bytes, and every function in it is reachable
//! from a test with nothing but a path. What is here is the *coordination* ---
//! ask the model, pick a writer, close the document, land the write, and decide
//! what each failure means to a reader --- which is a different question and one
//! `save.rs`'s header does not claim.
//!
//! **What the seam is.** [`Saving`] is everything this order asks of the running
//! application, and it is the same shape `save.rs` already uses for `Outside`:
//! the decisions stay here and the round trips are the caller's. `lib.rs`
//! implements it over the real render service and the real model; the tests
//! below implement it over a recorder, which is what makes a claim about the
//! *order* something an assertion can hold.
//!
//! **What deliberately stayed in `lib.rs`.** The landing --- the rename, or the
//! append through the file's own handle --- is a closure this function is
//! handed. Two reasons, and the second is not cosmetic. It needs the app handle
//! to choose where the read-back parses, which is a thing no test has; and
//! `scripts/check_writers.py` derives *which commands can write a file* from
//! the terminal writers named in each command's own body, so a save whose
//! `save::commit_in_place` moved out of `save_document` would drop out of the
//! set `docs/THREAT-MODEL.md` §3 is checked against. A security gate keyed on
//! where a call is written is a good reason to leave the call there.
//!
//! Nothing about the sequence changed in the move. Every refusal string, the
//! order of the steps, which failures say `reopen` and how the close note is
//! composed are what they were; what is new is that each of them now has a test.

use std::future::Future;
use std::path::Path;

use crate::{edits, save, with_close_note, SaveFailure};

/// What a save over the reader's own file asks of the running application.
///
/// Every method here is a round trip the sequence cannot make itself: the
/// model's answer, the render service's, or work that belongs on the blocking
/// pool. Nothing here decides anything --- the decisions are in [`save_over`],
/// which is the point of the split.
///
/// `async fn` is spelled out as `impl Future + Send` rather than written with
/// the `async` keyword because the caller is a Tauri command, whose future has
/// to be `Send`; the sugar does not promise that and the command then does not
/// compile.
pub(crate) trait Saving {
    /// What this application's writers hand back once a save is prepared.
    ///
    /// An associated type rather than a concrete enum, and that is what keeps
    /// the payload out of here: this order never looks inside a prepared save,
    /// it only carries it from the writer that made it to the landing that
    /// applies it. `lib.rs` fills it with the two-armed `Prepared`; a test fills
    /// it with whatever is cheapest to recognise.
    type Prepared: Send;

    /// The model's own answer about whether there is anything to save.
    fn state(&self) -> Result<edits::EditState, String>;

    /// The plan every writer below is given.
    fn plan(&self) -> Result<edits::Plan, String>;

    /// The password the reader opened this document with, if it needed one.
    fn password(&self) -> impl Future<Output = Option<String>> + Send;

    /// Prepares an update section to add to the end of the file.
    fn prepare_append(
        &self,
        plan: edits::Plan,
    ) -> impl Future<Output = Result<Self::Prepared, save::Refusal>> + Send;

    /// Prepares a whole new document, staged beside the source.
    fn prepare_rewrite(
        &self,
        plan: edits::Plan,
        password: Option<String>,
    ) -> impl Future<Output = Result<Self::Prepared, save::Refusal>> + Send;

    /// Spends the reader's journal. Has no failure: the model is gone either way.
    fn close_model(&self);

    /// Gives the document back to the render service, and says whether it went.
    fn close_document(&self) -> impl Future<Output = Result<(), String>> + Send;
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
/// **`land` is handed the prepared save and the reader's key, and returns two
/// nested results because they mean different things.** The outer one is the
/// blocking pool itself failing --- a panic, or a runtime shutting down --- and
/// the inner one is the write failing. Only the inner gets the close note: that
/// is what the command did before this function existed, and it is preserved
/// deliberately rather than tidied, because changing which failures carry that
/// sentence is a change to what a reader is told.
///
/// # Errors
///
/// A model with nothing to save or one that cannot answer; anything the chosen
/// writer refuses, all of which arrive with the document still open; and,
/// past the close, whatever the landing reports --- with `reopen` set, because
/// by then the reader has no document to carry on with.
pub(crate) async fn save_over<S, L, F>(
    saving: &S,
    source: &Path,
    land: L,
) -> Result<(), SaveFailure>
where
    S: Saving + Sync,
    L: FnOnce(S::Prepared, Option<String>) -> F + Send,
    F: Future<Output = Result<Result<(), SaveFailure>, SaveFailure>> + Send,
{
    // Read rather than assumed from the command being offered at all. The
    // palette withholds Save on a document with nothing to save, and that guard
    // is a frontend that may be a reply behind; this one is the model's own
    // answer. Writing a clean document would still produce a correct file, but
    // it would rewrite every object id in a file the reader did not change.
    let state = saving.state().map_err(SaveFailure::refused)?;
    if !state.dirty {
        return Err(SaveFailure::refused(
            "there is nothing to save --- this document has no unsaved changes",
        ));
    }
    let plan = saving.plan().map_err(SaveFailure::refused)?;

    // The file has to still be the file. Everything below rewrites the object
    // graph the plan was made against, and a `source` that something else has
    // written to since is a different graph -- the reader's edits would be
    // replayed onto pages they were never made on, and the write is atomic, so
    // the result is a confidently wrong file rather than a visibly broken one.
    //
    // The check itself lives in `save.rs`, on the plan, where `write_copy` and
    // `stage_in_place` both reach it and where a test can drive it. Nothing is
    // read here: what the landing's second look needs comes back from the
    // staging, which is the moment it should be comparing against rather than
    // the moment the reader opened the file.
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
    // rather than by ordering. Asked of the *file* rather than of the plan,
    // which is the half a test can otherwise not tell apart: a plan that only
    // adds marks is appendable and a 400 MB file holding it is not.
    let mode = save::mode_for_source(&plan, source);

    // **Before the match, because both arms need it now.** It used to be asked
    // after, for the append alone --- the rewrite arm read `Prepared::Rewrite(_)
    // => None` and did not need a key, because it refused every encrypted
    // document outright. Since 2026-08-28 a rewrite re-encrypts what it writes,
    // so the key is what makes it possible rather than what it would have
    // leaked. The rewrite also needs it *earlier* than the append does: the
    // append's parse happens in the worker, while the rewrite parses on the pool
    // inside its own arm.
    //
    // **And before the close, because after it there is no document to ask.** An
    // append to an encrypted document re-reads the file it wrote to check the
    // cross-reference chained correctly, and `lopdf` parses no objects at all
    // without the key --- so that check would count zero pages against the two
    // it expects and roll a correct save back. Dropped when this function
    // returns: `docs/THREAT-MODEL.md` §T6.9.
    //
    // A failure to answer is not a refusal. The document is about to be closed
    // either way and a plain document has no password to lose, so `None` is the
    // right answer for both "it has none" and "the service could not say" ---
    // and if the second is wrong, the append's own read-back refuses and rolls
    // back rather than writing something unchecked.
    let password = saving.password().await;

    let prepared = match mode {
        save::Mode::Append => saving.prepare_append(plan).await,
        save::Mode::Rewrite => saving.prepare_rewrite(plan, password.clone()).await,
    }
    // `refused_by` rather than `refused`, so `changed` is carried rather than
    // re-derived. That field is what lets the window offer Reload for a file
    // that moved under the reader and withhold it for a refusal reloading would
    // not help; deciding it again at this end would mean matching on the
    // message to answer it.
    .map_err(SaveFailure::refused_by)?;

    // Past this line every failure is an `after_close`: the reader's document is
    // being taken apart, and the honest thing to report is that they have to
    // open the file again rather than a message that reads like a refusal.
    //
    // The model first, for the reason `close_document` gives --- document
    // numbers are reused, and a journal left under a handle the service is free
    // to hand to another file is one document's edits applied to another's
    // pages.
    saving.close_model();
    let closed = saving.close_document().await;

    // Attempted whether or not the close was acknowledged. The model is gone
    // either way, so the reader is reopening either way, and a rename that the
    // mapping really did block reports that itself --- which is a better message
    // than one this end guesses from a close reply.
    let landed = land(prepared, password).await?;

    landed.map_err(|why| with_close_note(why, closed))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{save_over, Saving};
    use crate::docmodel::{MarkKind, PageSource, Quad};
    use crate::edits::{EditState, PageView, Plan, PlannedMark};
    use crate::save::Refusal;
    use crate::testutil::TempDir;
    use crate::SaveFailure;

    /// What each seam did, in the order it was asked.
    ///
    /// A shared log rather than a flag per call, because the claim being made is
    /// about *order* and a set of booleans cannot hold one. The landing pushes
    /// into the same log, which is what lets one assertion span the whole
    /// sequence.
    type Log = Arc<Mutex<Vec<String>>>;

    /// The running application, as a recorder.
    struct Fake {
        log: Log,
        state: Result<EditState, String>,
        plan: Result<Plan, String>,
        password: Option<String>,
        /// What each writer answers. Only the arm the mode picks is asked.
        prepare: Result<&'static str, Refusal>,
        closed: Result<(), String>,
    }

    impl Fake {
        fn new(log: &Log) -> Fake {
            Fake {
                log: Arc::clone(log),
                state: Ok(state(true)),
                plan: Ok(marks_only()),
                password: None,
                prepare: Ok("prepared"),
                closed: Ok(()),
            }
        }

        fn say(&self, what: &str) {
            self.log.lock().expect("the log").push(what.to_string());
        }
    }

    impl Saving for Fake {
        type Prepared = &'static str;

        fn state(&self) -> Result<EditState, String> {
            self.say("state");
            self.state.clone()
        }

        fn plan(&self) -> Result<Plan, String> {
            self.say("plan");
            self.plan.clone()
        }

        fn password(&self) -> impl std::future::Future<Output = Option<String>> + Send {
            self.say("password");
            let answer = self.password.clone();
            async move { answer }
        }

        fn prepare_append(
            &self,
            _plan: Plan,
        ) -> impl std::future::Future<Output = Result<&'static str, Refusal>> + Send {
            self.say("prepare_append");
            let answer = self.prepare.clone();
            async move { answer }
        }

        fn prepare_rewrite(
            &self,
            _plan: Plan,
            password: Option<String>,
        ) -> impl std::future::Future<Output = Result<&'static str, Refusal>> + Send {
            self.say(&format!(
                "prepare_rewrite({})",
                password.unwrap_or_default()
            ));
            let answer = self.prepare.clone();
            async move { answer }
        }

        fn close_model(&self) {
            self.say("close_model");
        }

        fn close_document(&self) -> impl std::future::Future<Output = Result<(), String>> + Send {
            self.say("close_document");
            let answer = self.closed.clone();
            async move { answer }
        }
    }

    /// A model with or without unsaved changes, and nothing else in it.
    fn state(dirty: bool) -> EditState {
        EditState {
            pages: Vec::new(),
            can_undo: false,
            can_redo: false,
            marks: Vec::new(),
            redactions: Vec::new(),
            notes: Vec::new(),
            discards: Vec::new(),
            dirty,
        }
    }

    /// A one-page plan carrying a highlight and nothing else: appendable.
    fn marks_only() -> Plan {
        Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: PageSource::Baseline(0),
                turns: 0,
                crop: None,
            }],
            redactions: Vec::new(),
            notes: Vec::new(),
            discards: Vec::new(),
            marks: vec![PlannedMark {
                kind: MarkKind::Highlight,
                stamp: None,
                reply_to: None,
                at: 0,
                quads: vec![Quad {
                    left: 10.0,
                    top: 10.0,
                    right: 20.0,
                    bottom: 20.0,
                }],
                strokes: Vec::new(),
                color: [1.0, 0.9, 0.2],
                width: 1.0,
                author: "tpdf".to_string(),
                note: String::new(),
                made: "D:20260831000000Z".to_string(),
            }],
        }
    }

    /// The same plan with a page deleted, which no append can write.
    fn a_page_short() -> Plan {
        Plan {
            baseline: 2,
            ..marks_only()
        }
    }

    /// A file for `mode_for_source` to measure, small enough for an append.
    ///
    /// `tag` is the caller's own, because [`TempDir`] names its directory after
    /// it and the process --- two tests sharing a tag delete each other's
    /// scratch, which `docs/TRAPS.md` records as having cost twelve isolated
    /// passing runs of the test that lost.
    fn a_small_file(tag: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new(tag);
        let path = dir.join("source.pdf");
        std::fs::write(&path, b"%PDF-1.7\n").expect("write the source");
        (dir, path)
    }

    /// Runs the order to completion on the current thread.
    ///
    /// A runtime is not a dependency of this module and the sequence awaits
    /// nothing that blocks, so a few lines are cheaper than one --- every future
    /// the fake returns is ready the first time it is polled.
    ///
    /// **Bounded, and the bound is the whole reason it is not a bare `loop`.**
    /// Nothing here wakes anything, so a future that ever answered `Pending`
    /// would spin for ever, and `docs/TRAPS.md` records what that costs: a check
    /// whose failure mode is a wait cannot fail, and a hung test binary reports
    /// no red at all. A panic says which test and says it in a second.
    fn run<F: std::future::Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        for _ in 0..1000 {
            if let std::task::Poll::Ready(answer) = future.as_mut().poll(&mut context) {
                return answer;
            }
        }
        panic!("the save order never finished --- a seam answered Pending and nothing wakes it");
    }

    /// Nothing that lands, and it records that it was asked.
    fn landing(log: &Log) -> impl FnOnce(&'static str, Option<String>) -> LandsWith + Send + '_ {
        move |prepared, password| {
            log.lock().expect("the log").push(format!(
                "land({prepared}, {})",
                password.unwrap_or_default()
            ));
            LandsWith(Ok(Ok(())))
        }
    }

    /// A landing's answer, ready the moment it is polled.
    struct LandsWith(Result<Result<(), SaveFailure>, SaveFailure>);

    impl std::future::Future for LandsWith {
        type Output = Result<Result<(), SaveFailure>, SaveFailure>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            // Moved out rather than cloned, and the value left behind is a
            // failure naming the mistake: a `save_over` that awaited the landing
            // twice would report "polled twice" rather than quietly succeeding.
            // `Pin::get_mut` needs no `unsafe` because this future holds a plain
            // value and is therefore `Unpin`.
            let taken = std::mem::replace(
                &mut self.get_mut().0,
                Err(SaveFailure::after_close("the landing was polled twice")),
            );
            std::task::Poll::Ready(taken)
        }
    }

    fn entries(log: &Log) -> Vec<String> {
        log.lock().expect("the log").clone()
    }

    /// A document with nothing to save is refused, and nothing is touched.
    ///
    /// The refusal is the model's own answer rather than the palette's, and the
    /// half that matters is what the log does *not* contain: a save that got as
    /// far as closing the document would have spent the reader's journal to
    /// tell them there was nothing in it.
    #[test]
    fn a_clean_document_is_refused_before_anything_is_touched() {
        let log: Log = Arc::default();
        let saving = Fake {
            state: Ok(state(false)),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-clean");

        let why = run(save_over(&saving, &source, landing(&log)))
            .expect_err("a clean document must not be saved");

        assert_eq!(
            why.message,
            "there is nothing to save --- this document has no unsaved changes"
        );
        assert!(
            !why.reopen,
            "nothing was closed, so nothing is to be reopened"
        );
        assert_eq!(
            entries(&log),
            vec!["state".to_string()],
            "a refusal this early must not ask the model for a plan or close anything"
        );
    }

    /// A plan that only adds marks is prepared by the append.
    #[test]
    fn a_plan_that_only_adds_marks_is_prepared_as_an_append() {
        let log: Log = Arc::default();
        let saving = Fake::new(&log);
        let (_dir, source) = a_small_file("save-order-append");

        run(save_over(&saving, &source, landing(&log))).expect("the save lands");

        assert!(
            entries(&log).contains(&"prepare_append".to_string()),
            "a marks-only plan over a small file is an append: {:?}",
            entries(&log)
        );
    }

    /// A plan that changes the pages is prepared by the rewrite.
    ///
    /// The discriminating half: the two plans differ in nothing but the page
    /// count, so a mode split that answered the same for both would pass the
    /// test above and fail this one.
    #[test]
    fn a_plan_that_changes_the_pages_is_prepared_as_a_rewrite() {
        let log: Log = Arc::default();
        let saving = Fake {
            plan: Ok(a_page_short()),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-rewrite");

        run(save_over(&saving, &source, landing(&log))).expect("the save lands");

        let entries = entries(&log);
        assert!(
            entries
                .iter()
                .any(|step| step.starts_with("prepare_rewrite")),
            "a plan a page short cannot be appended: {entries:?}"
        );
    }

    /// A source that cannot be measured takes the rewrite, appendable or not.
    ///
    /// The plan here is the appendable one, so the only thing that can send it
    /// down the rewrite arm is the file --- which is what pins the split to
    /// `save::mode_for_source` rather than to `Plan::is_appendable`. Those two
    /// agree on every plan a small file carries, so without this the size bound
    /// and the unmeasurable-file guard are both invisible from here.
    #[test]
    fn a_source_that_cannot_be_measured_takes_the_rewrite() {
        let log: Log = Arc::default();
        let saving = Fake::new(&log);
        let gone = std::path::Path::new("/tpdf-no-such-file-to-measure.pdf");

        run(save_over(&saving, gone, landing(&log))).expect("the save lands");

        let entries = entries(&log);
        assert!(
            entries
                .iter()
                .any(|step| step.starts_with("prepare_rewrite")),
            "a file that cannot be measured must take the arm with no memory bound: {entries:?}"
        );
    }

    /// The whole sequence, in order.
    ///
    /// Prepare, then close the model, then close the document, then land. Each
    /// adjacency is a rule with a reason: preparing first is what lets a refusal
    /// arrive while the reader still has their document, the model goes before
    /// the service because document numbers are reused, and the landing goes
    /// after the close because a rename over a mapped file succeeds on macOS and
    /// serves the old inode for ever.
    #[test]
    fn the_document_is_closed_after_the_save_is_prepared_and_before_it_lands() {
        let log: Log = Arc::default();
        let saving = Fake {
            plan: Ok(a_page_short()),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-order");

        run(save_over(&saving, &source, landing(&log))).expect("the save lands");

        assert_eq!(
            entries(&log),
            vec![
                "state".to_string(),
                "plan".to_string(),
                "password".to_string(),
                "prepare_rewrite()".to_string(),
                "close_model".to_string(),
                "close_document".to_string(),
                "land(prepared, )".to_string(),
            ]
        );
    }

    /// A writer's refusal leaves the reader's document open.
    ///
    /// `changed` is carried from the refusal rather than decided again here,
    /// which is the field the window reads to decide whether Reload is an answer.
    #[test]
    fn a_save_refused_before_the_close_leaves_the_document_open() {
        let log: Log = Arc::default();
        let saving = Fake {
            prepare: Err(Refusal::changed("the file changed under you")),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-refused");

        let why = run(save_over(&saving, &source, landing(&log)))
            .expect_err("a refused writer must not report a save");

        assert_eq!(why.message, "the file changed under you");
        assert!(
            !why.reopen,
            "nothing was closed, so the document is still open"
        );
        assert!(why.changed, "the window offers Reload off this field");
        let entries = entries(&log);
        assert!(
            !entries.iter().any(|step| step.starts_with("close")),
            "a refusal before the close must not close anything: {entries:?}"
        );
    }

    /// A landing that fails after the close tells the reader to open the file.
    ///
    /// This is the verify-then-rename arm refusing: the staged file is thrown
    /// away and nothing was renamed, and it is still `reopen` --- because the
    /// close two steps above has already spent the reader's model and journal.
    #[test]
    fn a_landing_that_fails_after_the_close_says_the_document_must_be_reopened() {
        let log: Log = Arc::default();
        let saving = Fake::new(&log);
        let (_dir, source) = a_small_file("save-order-landing");

        let why = run(save_over(&saving, &source, |_, _| {
            LandsWith(Ok(Err(SaveFailure::after_close_by(Refusal::changed(
                "the file moved between the staging and the rename",
            )))))
        }))
        .expect_err("a landing that failed is not a save");

        assert_eq!(
            why.message,
            "the file moved between the staging and the rename"
        );
        assert!(why.reopen, "the model is gone, whatever became of the file");
        assert!(
            why.changed,
            "the refusal's own field survives the composition"
        );
    }

    /// A close that did not go cleanly is named in the failure the reader sees.
    ///
    /// The two facts arrive from different places --- one from the service, one
    /// from the landing --- and the reader needs both in one sentence, because
    /// what they do next depends on the second half.
    #[test]
    fn a_failed_close_is_named_in_a_failure_that_happened_after_it() {
        let log: Log = Arc::default();
        let saving = Fake {
            closed: Err("the render thread stopped".to_string()),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-closefail");

        let why = run(save_over(&saving, &source, |_, _| {
            LandsWith(Ok(Err(SaveFailure::after_close("the rename failed"))))
        }))
        .expect_err("a landing that failed is not a save");

        assert_eq!(
            why.message,
            "the rename failed --- and the document did not close cleanly: the render thread \
             stopped"
        );
    }

    /// A close that went cleanly adds nothing, and a save that lands says nothing.
    ///
    /// The control for the test above: without it, a note appended
    /// unconditionally would pass that one and every ordinary save would end
    /// with a sentence about nothing having gone wrong.
    #[test]
    fn a_clean_close_leaves_a_landing_failure_as_it_was() {
        let log: Log = Arc::default();
        let saving = Fake::new(&log);
        let (_dir, source) = a_small_file("save-order-closeclean");

        let why = run(save_over(&saving, &source, |_, _| {
            LandsWith(Ok(Err(SaveFailure::after_close("the rename failed"))))
        }))
        .expect_err("a landing that failed is not a save");

        assert_eq!(why.message, "the rename failed");
    }

    /// The pool itself failing is reported without a close note.
    ///
    /// **Pinning what the command did rather than what looks tidier.** A panic
    /// in the closure or a runtime shutting down is the outer result, and it
    /// leaves through its own path; the close note is added only to a failure
    /// the landing produced. That asymmetry is worth a test because it is
    /// invisible in the code --- one `?` and one `map_err` --- and because
    /// changing it would change a sentence a reader acts on.
    #[test]
    fn the_pool_failing_is_reported_without_a_close_note() {
        let log: Log = Arc::default();
        let saving = Fake {
            closed: Err("the render thread stopped".to_string()),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-poolfail");

        let why = run(save_over(&saving, &source, |_, _| {
            LandsWith(Err(SaveFailure::after_close("the save did not finish")))
        }))
        .expect_err("a pool that failed is not a save");

        assert_eq!(why.message, "the save did not finish");
        assert!(why.reopen);
    }

    /// The reader's key is asked once and reaches both the rewrite and the landing.
    ///
    /// Two consumers, one ask. The rewrite needs it to re-encrypt what it
    /// writes; the append's read-back needs it because `lopdf` parses no objects
    /// at all without one, and a read-back that parsed nothing would count zero
    /// pages against the two it expects and roll a correct save back.
    #[test]
    fn the_password_is_asked_once_and_reaches_the_writer_and_the_landing() {
        let log: Log = Arc::default();
        let saving = Fake {
            plan: Ok(a_page_short()),
            password: Some("open sesame".to_string()),
            ..Fake::new(&log)
        };
        let (_dir, source) = a_small_file("save-order-password");

        run(save_over(&saving, &source, landing(&log))).expect("the save lands");

        let entries = entries(&log);
        assert_eq!(
            entries.iter().filter(|step| *step == "password").count(),
            1,
            "asked once: {entries:?}"
        );
        assert!(
            entries.contains(&"prepare_rewrite(open sesame)".to_string()),
            "the rewrite re-encrypts with it: {entries:?}"
        );
        assert!(
            entries.contains(&"land(prepared, open sesame)".to_string()),
            "the read-back parses with it: {entries:?}"
        );
    }
}
