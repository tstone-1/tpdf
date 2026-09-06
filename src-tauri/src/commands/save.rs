//! Saving: in place, as a copy, and the three that write more than one file.
//!
//! The sequence itself is `save_order.rs`; what is here is the seam it runs
//! against ([`LiveSave`]) and the commands that build one. That split is what
//! lets the ordering be tested at all --- nothing can call the body of a
//! `#[tauri::command]`.

use std::path::Path;

use super::{await_reply, outside_of, password_for, reply_channel};
use crate::render::RenderService;
use crate::{edits, save, save_order, SaveFailure};

/// The render service and the working model, as a save over the source needs them.
///
/// Every method here is a round trip `save_order.rs` cannot make itself, and
/// each one is the code that used to sit inline in [`save_document`]. What is
/// gained by their living behind a trait is that the *order* they are called in
/// is now a thing a test can assert: `save_order::save_over` is an ordinary
/// function, and the seam it takes can be a recorder.
///
/// It holds borrows rather than clones because it lives no longer than the
/// command that builds it.
struct LiveSave<'a> {
    app: &'a tauri::AppHandle,
    service: &'a RenderService,
    edits: &'a edits::Edits,
    doc: u32,
    source: String,
}

impl save_order::Saving for LiveSave<'_> {
    /// Both writers' answers, which the landing below is the only reader of.
    type Prepared = Prepared;

    fn state(&self) -> Result<edits::EditState, String> {
        self.edits.state(self.doc)
    }

    fn plan(&self) -> Result<edits::Plan, String> {
        self.edits.plan(self.doc)
    }

    fn password(&self) -> impl std::future::Future<Output = Option<String>> + Send {
        password_for(self.service, self.doc, "save_document")
    }

    /// **The append's parse happens in the worker**, which is the one difference
    /// between the two writers and the reason they are not one `spawn_blocking`.
    /// `save::append_update` is a pure function of the document's bytes and the
    /// plan, and those bytes are the attacker's --- so it runs in the process
    /// that already holds this document under a sandbox, a deadline and a
    /// restart, and that has already parsed it with `lopdf` for its comments,
    /// links and properties. What comes back is bytes and two numbers.
    ///
    /// Every decision about the file stays out here: `append_ready` measures and
    /// fingerprints it before the request, and `save::appended` refuses an answer
    /// built against a different length.
    ///
    /// The pool failing and the render thread stopping both arrive as a plain
    /// `Refusal`, which carries `changed: false` --- the same `SaveFailure` the
    /// two of them produced when they were spelled out separately, and one
    /// classification rather than three ways of writing it.
    fn prepare_append(
        &self,
        plan: edits::Plan,
    ) -> impl std::future::Future<Output = Result<Prepared, save::Refusal>> + Send {
        let checking = self.source.clone();
        let asking = plan.clone();
        async move {
            let ready = tauri::async_runtime::spawn_blocking(move || {
                save::append_ready(Path::new(&checking), &asking)
            })
            .await
            .map_err(|e| save::Refusal::from(format!("the save did not run: {e}")))??;

            let (reply, rx) = reply_channel();
            self.service.append(self.doc, plan, reply);
            let update = await_reply("save_document", rx)
                .await
                .map_err(save::Refusal::from)?;
            save::appended(ready, update).map(Prepared::Append)
        }
    }

    /// **The rewrite's parse happens in the worker too, since 2026-08-28**, and
    /// it is the same argument [`LiveSave::prepare_append`] makes:
    /// `save::rewrite_update` is a pure function of the document's bytes and the
    /// plan, and those bytes are the attacker's. What took longer is where the
    /// answer goes --- an append's is kilobytes and fits in a reply, a rewrite's
    /// is the whole document --- so the worker is handed the staging file's own
    /// descriptor and writes down it. `docs/THREAT-MODEL.md` residual risk 18.
    fn prepare_rewrite(
        &self,
        plan: edits::Plan,
        password: Option<String>,
    ) -> impl std::future::Future<Output = Result<Prepared, save::Refusal>> + Send {
        let staging = self.source.clone();
        let writing = outside_of(self.app, self.service.backend());
        async move {
            tauri::async_runtime::spawn_blocking(move || {
                save::stage_in_place(Path::new(&staging), &plan, password.as_deref(), &*writing)
            })
            .await
            .map_err(|e| save::Refusal::from(format!("the save did not run: {e}")))?
            .map(Prepared::Rewrite)
        }
    }

    fn close_model(&self) {
        self.edits.close(self.doc);
    }

    /// The send happens now and the wait is what is awaited, which is the order
    /// the command had: nothing between the two, so a caller cannot leave the
    /// service holding a document it was told to give back.
    fn close_document(&self) -> impl std::future::Future<Output = Result<(), String>> + Send {
        let (reply, rx) = reply_channel();
        self.service.close(self.doc, reply);
        async move { await_reply("save_document", rx).await }
    }
}

/// Writes the working document over the file the reader opened.
///
/// **The sequence is in `save_order.rs`**, which is where its reasons are
/// written and where its tests are. This is the adapter: it puts the render
/// service and the model behind [`LiveSave`], and it holds the landing --- the
/// one step that cannot move, because choosing where the read-back parses needs
/// the app handle and because `scripts/check_writers.py` reads the terminal
/// writers a command can reach out of the command's own body.
///
/// On the blocking pool for the parse and the serialisation, for the reason
/// [`save_copy`] gives. The close and the rename are not: one is a channel
/// round trip and the other is a rename in a directory that has just been
/// written to.
#[tauri::command]
pub async fn save_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    source: String,
) -> Result<(), SaveFailure> {
    let live = LiveSave {
        app: &app,
        service: &service,
        edits: &edits,
        doc,
        source: source.clone(),
    };
    // The landing owns everything it needs, because it outlives the borrows
    // above: it runs after the seam has been asked its last question. Reading
    // `backend()` here rather than inside it changes nothing about the answer
    // --- it is a field --- and it is what keeps the `service` borrow out of a
    // closure that has to be `'static` to cross onto the blocking pool.
    let backend = service.backend();
    let landing_app = app.clone();
    let landing_source = source.clone();

    save_order::save_over(
        &live,
        Path::new(&source),
        move |prepared, password| async move {
            // **Who re-reads the file the append writes**, chosen the same way the
            // render backend is and for the same reason: the previous revision of
            // that file is the document the reader opened, so the parse belongs in a
            // sandboxed child wherever there can be one. A platform with none still
            // saves --- refusing would make it useless rather than uncontained,
            // which is the rule `Backend::default_here` already follows --- and it
            // is not silent: `render::UNSANDBOXED_MARK` is what keeps the two runs
            // distinguishable.
            //
            // Built here rather than inside `save::append_in_place`, because
            // choosing it needs the app handle and that function is reachable from
            // `cargo test`, where there is none.
            let reread = outside_of(&landing_app, backend);

            // **On the blocking pool, and this whole match had been on the async
            // runtime until 2026-08-23.** Every arm below does file work that
            // blocks: the rewrite takes its last look at the source in
            // `verify_before_commit` and then renames, and the append writes, waits
            // for the platter, reads the whole file back and *parses it with
            // `lopdf`*. That last one is a parse of attacker-derived bytes --- the
            // previous revision is the document the reader opened --- so it belongs
            // where the other three coordinator-side parses already are.
            // `docs/THREAT-MODEL.md` residual risk 17 said the append had moved into
            // the worker; its *preparation* had, and this is the half that had not.
            //
            // This said the rewrite "hashes every byte of it in
            // `verify_before_commit`" until 2026-08-31, which was never true and made
            // the arms sound alike. That function compares length and modification
            // time --- the digest of every byte ran earlier, in `save::rewrite_ready`,
            // before staging. So the rewrite arm is here for its rename rather than
            // for its size, and the append arm is the one that is proportional to
            // the reader's document.
            tauri::async_runtime::spawn_blocking(move || match prepared {
                Prepared::Rewrite(staged) => {
                    // One more look before the rename, closing the window the
                    // staging opens. What it compares and why it compares against
                    // staging rather than against the open is on the function, where
                    // a test can reach it.
                    //
                    // `after_close`, and this is worth stating because the comment
                    // here said the opposite until 2026-08-19 while the code did what
                    // it does now. Nothing has been renamed, so it is tempting to
                    // call this a refusal that costs nothing --- but the close has
                    // already happened, so the reader's model and their journal are
                    // gone. `refused` would tell them their document is still open
                    // when it is not, which is the one thing that flag decides.
                    save::verify_before_commit(&staged, Path::new(&landing_source))
                        .map_err(SaveFailure::after_close_by)?;
                    save::commit_in_place(&staged.path, Path::new(&landing_source))
                        .map_err(SaveFailure::after_close)
                }
                // No second look of its own because it takes its own, and it has to
                // be its own: `verify_before_commit` compares against a *path*, and
                // an append writes through a *handle*. `append_in_place` opens the
                // file, asks `Appended::verified` the same length-and-timestamp
                // question through that handle, writes, reads back and rolls back
                // through it, and finally checks that the pathname still names it.
                //
                // This comment claimed the opposite until 2026-08-22 --- that
                // comparing a length alone was "a sharper answer" than comparing a
                // length and a timestamp --- and the code agreed with it. It is the
                // wrong way round, `fingerprint.rs` says so in its own header, and
                // `docs/TRAPS.md` has had *Equal length is not no change* since
                // before either was written.
                Prepared::Append(appended) => save::append_in_place(
                    &appended,
                    Path::new(&landing_source),
                    password.as_deref(),
                    &*reread,
                )
                .map_err(SaveFailure::after_close),
            })
            .await
            // The pool itself failing --- a panic in the closure, or a runtime
            // shutting down. `after_close` for the same reason every arm above is:
            // the document is gone whatever happened to the file. It leaves as the
            // outer result, which is what keeps the close note off it; see
            // `save_order::save_over`.
            .map_err(|e| SaveFailure::after_close(format!("the save did not finish: {e}")))
        },
    )
    .await
}

/// A save that has been prepared, by whichever writer the plan chose.
///
/// The two carry different things --- a rewrite carries a path to rename, an
/// append carries bytes and the length they go after --- and an enum is what
/// keeps the caller from having to know which fields mean anything. See
/// `save::Mode`.
enum Prepared {
    Rewrite(save::Staged),
    Append(save::Appended),
}

/// Writes the working document to a new file.
///
/// `source` comes from the frontend, which is what [`super::print::print_document`] does and
/// for the same reason: the render service holds the document, and the path is
/// the frontend's own record of what it asked to open.
///
/// On the blocking pool, unlike every command that only waits on the render
/// service: this parses the whole
/// document with `lopdf` and serialises it, which on the 337 MB scan is not work
/// to do on a runtime worker. Read the argument in [`super::print::print_document`]
/// --- it is the same one, and the two commands are the two members of this
/// repository that genuinely belong there.
#[tauri::command]
pub async fn save_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
) -> Result<save::Copied, String> {
    // Read out of the model *before* the move onto the pool. The state is behind
    // a mutex that is not held across an await anywhere in this file, and taking
    // a `State` handle into a `spawn_blocking` closure would need it to outlive
    // the command.
    let plan = edits.plan(doc)?;
    // **A copy is written even from a source that changed**, and this comment
    // said the opposite until 2026-08-19: it described the copy as refused "in
    // the same words" and named opening the file again as the way out. That was
    // a dead end wearing a helpful sentence. Save a copy IS the fallback the
    // in-place refusal points at, and reopening is exactly what spends the edits
    // the copy exists to keep -- so a reader whose file changed had nowhere at
    // all to put their work.
    //
    // What comes back says whether the source had changed, because a copy built
    // from a document that is no longer the one on screen is a fact the reader
    // has to be told rather than a failure. `save.rs`'s `OnChange` carries the
    // argument, including what still refuses: a changed file that also changed
    // shape is caught by the page-count guard whichever path asks.
    let password = password_for(&service, doc, "save_copy").await;
    // **Who parses the reader's document**, chosen the same way `save_document`
    // chooses it and for the same reason: the bytes are the attacker's, so the
    // parse belongs in a sandboxed child wherever there can be one. Built here
    // rather than inside `save::write_copy` because choosing it needs the app
    // handle, and that function is reachable from `cargo test`, where there is
    // none.
    let writing = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            password.as_deref(),
            &*writing,
        )
    })
    .await
    .map_err(|e| format!("the save did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes a subset of the working document's pages to a new file.
///
/// Everything [`save_copy`] does, over a selection rather than the whole
/// document, and it shares that command's whole write path --- so the three
/// refusals `save.rs` states (encrypted source, a page count that disagrees with
/// the baseline, writing over the source) apply here unchanged and are not
/// restated.
///
/// `slots` are positions in the **current** order, deduplicated and ascending;
/// `edits::Edits::plan_subset` refuses anything else rather than normalising it,
/// so a defect on the way here is a message and not a file with pages in an
/// order nobody asked for.
///
/// On the blocking pool for the same reason as [`save_copy`], which is the
/// reason it does not simply call it: the plan has to be read out of the model
/// before the move onto the pool, and the only difference between the two
/// commands is which plan.
#[tauri::command]
pub async fn extract_pages(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    slots: Vec<u32>,
) -> Result<save::Copied, String> {
    let plan = edits.plan_subset(doc, &slots)?;
    // Same outcome as `save_copy` and for the same reason: an extract is a copy
    // of some of the pages, so it is written from a changed source too, and the
    // reader is told the same way.
    let password = password_for(&service, doc, "extract_pages").await;
    // `save_copy`'s, and the same object for the same reason: an extract is a
    // copy of some of the pages, so it takes the same writer.
    let writing = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            password.as_deref(),
            &*writing,
        )
    })
    .await
    .map_err(|e| format!("the extract did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes the working document's pages to several new files, one per group.
///
/// [`extract_pages`] repeated, which is what the plan said it would be: each
/// group becomes its own plan and its own `write_copy`-shaped write, so the
/// three refusals `save.rs` states apply to every file and are not restated.
/// `save::write_split` adds one more that only a split needs --- no destination
/// may already exist --- and its doc comment carries the reason.
///
/// **Changes nothing about the open document.** No command is journalled and
/// there is nothing to undo, which is [`extract_pages`]' and [`merge_documents`]'
/// property: all three read the document and write elsewhere.
///
/// Every plan is built **before** the move onto the pool, for [`save_copy`]'s
/// reason and one more: a group that names an unknown slot must refuse before
/// any file is written, not after the first two are on disk.
///
/// `groups` are positions in the current order, each deduplicated and ascending,
/// which `edits::Edits::plan_subset` enforces per group. Nothing here checks
/// that the groups *partition* the document: `parseSplitPoints` builds them and
/// a caller sending overlapping groups gets overlapping files, which is a
/// stranger request than it is a dangerous one.
#[tauri::command]
pub async fn split_document(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    groups: Vec<Vec<u32>>,
) -> Result<save::Split, String> {
    let plans = groups
        .iter()
        .map(|slots| edits.plan_subset(doc, slots))
        .collect::<Result<Vec<_>, String>>()?;
    let password = password_for(&service, doc, "split_document").await;
    // One writer for every part, and it is asked once per file --- see
    // `save::write_split`, which opens the source once and hands the same handle
    // to each.
    let writing = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        save::write_split(
            Path::new(&source),
            &plans,
            Path::new(&path),
            password.as_deref(),
            &*writing,
        )
    })
    .await
    .map_err(|e| format!("the split did not run: {e}"))?
    .map_err(|why| why.message)
}

/// Writes the working document followed by other files' pages, to a new file.
///
/// The open document goes in as the reader has it and the others go in as they
/// are on disk --- `save::write_merged` holds that asymmetry and the reason for
/// it. Everything [`save_copy`] refuses about the open document is refused here
/// unchanged and is not restated.
///
/// **Changes nothing about the open document.** No command is journalled, the
/// order is untouched and there is nothing to undo, which is [`extract_pages`]'s
/// property arriving from the other direction: extract reads some of one file,
/// merge reads all of several, and neither is an edit.
///
/// `others` are paths the reader chose in a file dialog. They are opened here,
/// in the coordinator, which is where every other `lopdf` parse on a save path
/// runs --- see `docs/THREAT-MODEL.md` residual risk 18, which this widens by
/// one file per merge rather than by a new kind of access.
///
/// On the blocking pool for [`save_copy`]'s reason, and rather more so: this one
/// parses every file it was given.
#[tauri::command]
pub async fn merge_documents(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    others: Vec<String>,
) -> Result<save::Merged, String> {
    // Out of the model before the move onto the pool, as `save_copy` does and
    // for the same reason.
    let plan = edits.plan(doc)?;
    let password = password_for(&service, doc, "merge_documents").await;
    // **Who parses the documents**, chosen as `save_copy` chooses it --- and here
    // it decides more than anywhere else, because a merge parses files tpdf has
    // never opened. See `save::merge_update` and
    // `worker_proto::Request::Merge`.
    let writing = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        let others: Vec<std::path::PathBuf> = others.into_iter().map(Into::into).collect();
        save::write_merged(
            Path::new(&source),
            &plan,
            &others,
            Path::new(&path),
            password.as_deref(),
            &*writing,
        )
    })
    .await
    .map_err(|e| format!("the merge did not run: {e}"))?
    .map_err(|why| why.message)
}
