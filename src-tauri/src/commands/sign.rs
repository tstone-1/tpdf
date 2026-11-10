//! Signing with a certificate the reader already has: Phase 6 step 2.
//!
//! Three commands. [`sign_identities`] lists what the OS store holds; nothing is
//! parsed but the reader's own certificates. [`sign_preview`] draws what a
//! visible signature would look like, by the code that signs, before anything is
//! signed. [`sign_document`] runs the whole
//! split `docs/PLAN.md` §9 decided: the **worker** builds the revision with an
//! empty hole (`sign_prepare.rs`), this **app process** reads the file, checks
//! the worker's numbers against it, has the OS sign, splices the value in and
//! refuses unless its own verifier calls the result intact (`sign_cms.rs`), the
//! copy is written (`save::write_signed`), and a **worker** holding the written
//! file reports every signature in it --- which is the answer the reader is
//! shown. What happens after the OS has signed is `sign_finish.rs`, which the
//! command line runs too.
//!
//! ## A timestamp, and the signature kept while the reader decides
//!
//! When the reader asked for a timestamp, this process asks the authority for
//! one after the OS has signed and before anything is written (`tsa.rs`). If
//! that fails, **nothing is written** and the answer is [`Signing::unstamped`]:
//! the reason, and a number for the made signature, which is kept here in
//! [`Pending`]. The reader then chooses --- try again ([`sign_resume`] with the
//! authority), sign without a timestamp ([`sign_resume`] with none), or cancel
//! ([`sign_discard`]) --- and **the OS is not asked for the key again** for any
//! of them: a second PIN on a smart card, because a server was down, would be
//! tpdf's cost made the reader's. Signing without one is always the reader's
//! explicit second choice, never what a failure turns into.
//!
//! ## Long-term validation data, and the same promise
//!
//! When the reader also asked for long-term validation data (PAdES B-LT, only
//! ever together with a timestamp), this process gathers it after the
//! timestamped signature is sealed and a worker appends it (`longterm.rs`).
//! If that fails, nothing is written and the **sealed, timestamped** signature
//! is what [`Pending`] keeps: trying again gathers again, and signing without
//! the data writes the B-T file --- neither asks the key or the timestamp
//! authority again. The one failure with no second choice is a certificate
//! authority saying a certificate is revoked: nothing is kept and nothing is
//! written, with or without the data.

use std::path::{Path, PathBuf};

use super::{await_reply, outside_of, reply_channel};
use crate::docmodel::PageSource;
use crate::render::RenderService;
use crate::sign_finish::{asking_the_world, finish, now, Asking, Stage, Stopped};
use crate::{edits, keystore, save, sign_cms, sign_prepare};

/// Where the reader placed a visible signature, as the frontend sends it.
///
/// `page` is the model's page id --- what every page-level command carries ---
/// and `rect` the viewer's display-space rectangle on it. The name the
/// appearance draws is **not** here: it is read from the certificate, in this
/// process, so the words under the signature are the ones the certificate says.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Placement {
    /// The page's id, `PageView::id`.
    pub page: u64,
    /// `[left, top, right, bottom]`, points, in the page's display space.
    pub rect: [f32; 4],
    /// The image the reader chose for it, when they chose one.
    pub image: Option<crate::signature::Image>,
    /// Which lines it shows, and the reason and location. Defaulted, so a
    /// placement sent before the reader could choose means what it meant then.
    #[serde(default)]
    pub options: sign_prepare::Options,
}

impl Placement {
    /// What the worker is sent for this placement: the file's page for the
    /// page id, and the name read from the certificate. The reader's choices
    /// travel as they came; the worker checks them.
    fn visible(self, page: u32, name: String) -> sign_prepare::Visible {
        sign_prepare::Visible {
            page,
            rect: self.rect,
            name,
            image: self.image,
            options: self.options,
        }
    }
}

/// The file's page number for a page id, in a plan with nothing unsaved.
///
/// # Errors
///
/// The id is not a page of the document, or it names a page tpdf made or
/// imported --- which cannot happen once unsaved edits are refused, and is
/// refused rather than guessed at if it ever does.
pub fn baseline_page(plan: &edits::Plan, id: u64) -> Result<u32, String> {
    match plan
        .pages
        .iter()
        .find(|view| view.id == id)
        .map(|view| &view.source)
    {
        Some(PageSource::Baseline(page)) => Ok(*page),
        Some(_) => Err(said!(
            "the page chosen for the signature is not a page of the file on disk \
             --- save the document and sign again"
        )),
        None => Err(said!(
            "the page chosen for the signature is no longer in the document"
        )),
    }
}

/// A signature made and not written, while the reader chooses what to do about
/// the timestamp that did not come.
///
/// **One at a time.** A new signing replaces whatever was held, and a held one
/// is only ever reached by the number it was handed out with, so an answer to
/// an old question cannot write a newer signature. It holds the whole file as
/// read (up to `save::APPEND_MAX_BYTES`) and is dropped by the reader's choice,
/// by the next signing, or when the application quits.
#[derive(Default)]
pub struct Pending(parking_lot::Mutex<Option<Held>>);

/// What [`Pending`] holds: the signature, and where it was to be written.
struct Held {
    number: u64,
    stage: Stage,
    source: PathBuf,
    out: PathBuf,
}

/// What a held signature is waiting for, as the window reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Waiting {
    /// The timestamp.
    Timestamp,
    /// The long-term validation data, with the timestamp already in it.
    LongTerm,
}

/// What `sign_document` and `sign_resume` answer: exactly one of the two is
/// set. A struct of two options rather than an enum because it is a reply
/// payload, and `replies.rs` pins a payload by one sample with every key set.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Signing {
    /// Written and read back.
    pub signed: Option<sign_cms::Signed>,
    /// The timestamp asked for did not come, and nothing was written.
    pub unstamped: Option<Unstamped>,
}

/// A signing whose timestamp, or whose long-term validation data, did not
/// come.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Unstamped {
    /// Why, as a sentence.
    pub why: String,
    /// The kept signature, for [`sign_resume`] and [`sign_discard`].
    pub pending: u64,
    /// Which of the two did not come, which decides what the reader is asked.
    pub stage: Waiting,
}

impl Pending {
    fn keep(&self, stage: Stage, source: PathBuf, out: PathBuf) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        *self.0.lock() = Some(Held {
            number,
            stage,
            source,
            out,
        });
        number
    }

    /// Drops whatever is held: a new signing has begun, and an answer to the
    /// old question has nothing left to write.
    fn clear(&self) {
        drop(self.0.lock().take());
    }

    /// What the held signature, if any, is waiting for.
    fn waiting(&self) -> Option<Waiting> {
        self.0.lock().as_ref().map(|held| match held.stage {
            Stage::Made(_) => Waiting::Timestamp,
            Stage::Sealed { .. } => Waiting::LongTerm,
        })
    }

    fn take(&self, number: u64) -> Option<Held> {
        let mut held = self.0.lock();
        if held.as_ref().is_some_and(|h| h.number == number) {
            held.take()
        } else {
            None
        }
    }

    /// [`Pending::take`], once `judge` has accepted the stage held under
    /// `number`. A stage it refuses stays held.
    ///
    /// # Errors
    ///
    /// `number` is not the signature held, or `judge`'s refusal.
    fn take_judged(
        &self,
        number: u64,
        judge: impl FnOnce(&Stage) -> Result<(), String>,
    ) -> Result<Held, String> {
        let mut held = self.0.lock();
        match held.as_ref() {
            Some(found) if found.number == number => judge(&found.stage)?,
            _ => {
                return Err(said!(
                    "this signature is no longer held --- sign the document again"
                ))
            }
        }
        Ok(held.take().expect("held: matched under this lock"))
    }
}

/// The authority the frontend named, judged again here: the webview is not
/// trusted to have kept to `http` and `https`.
fn authority_of(timestamp: Option<&str>) -> Result<Option<url::Url>, String> {
    timestamp
        .map(|text| crate::tsa::authority(text).map_err(|why| why.sentence("")))
        .transpose()
}

/// [`finish`] for the window: the signed copy reported, or --- when the
/// timestamp or the long-term data did not come --- the signature kept in
/// `pending` and the reason, so the reader chooses and the key is not asked
/// for again.
///
/// # Errors
///
/// A refusal with nothing kept: the seal, a revoked certificate, the write,
/// or a written copy whose timestamp or long-term data does not read back as
/// it was written ([`ReadBack::sentence`]).
#[allow(clippy::too_many_arguments)]
fn conclude(
    stage: Stage,
    authority: Option<&url::Url>,
    long_term: bool,
    source: PathBuf,
    out: PathBuf,
    checking: &dyn crate::save::Verifier,
    pending: &Pending,
    asking: Asking<'_>,
) -> Result<Signing, String> {
    let waiting = |why: String, number: u64, stage: Waiting| Signing {
        signed: None,
        unstamped: Some(Unstamped {
            why,
            pending: number,
            stage,
        }),
    };
    let finished = finish(
        stage,
        authority,
        long_term,
        &out,
        now(),
        checking,
        asking,
        &|bytes| save::write_signed(&source, &out, bytes).map_err(|why| why.message),
    );
    match finished {
        Ok(finished) => {
            let name = out.file_name().map_or_else(
                || out.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            if let Some(why) = finished.read_back.sentence(&name, &finished.field) {
                return Err(why);
            }
            Ok(Signing {
                signed: Some(sign_cms::report(
                    out.display().to_string(),
                    finished.field,
                    finished.signatures,
                    finished.ours,
                )),
                unstamped: None,
            })
        }
        Err(Stopped::Unstamped { made, why }) => {
            let host = authority
                .and_then(url::Url::host_str)
                .unwrap_or_default()
                .to_string();
            let number = pending.keep(Stage::Made(*made), source, out);
            Ok(waiting(why.sentence(&host), number, Waiting::Timestamp))
        }
        // A revoked certificate: nothing is kept to be written without the
        // data, because that would be the same revoked signature.
        Err(Stopped::Unextended { why, .. }) if why.revoked() => {
            Err(said!("{why} --- nothing was written", why = why.sentence()))
        }
        Err(Stopped::Unextended { sealed, why }) => {
            let number = pending.keep(*sealed, source, out);
            Ok(waiting(why.sentence(), number, Waiting::LongTerm))
        }
        Err(Stopped::Refused(why) | Stopped::Unwritten(why) | Stopped::Unread(why)) => Err(why),
    }
}

/// The long-term choice, judged against the timestamp: long-term data is
/// offered only with one, and the webview is not trusted to have kept to that.
fn long_term_of(long_term: bool, authority: Option<&url::Url>) -> Result<bool, String> {
    if long_term && authority.is_none() {
        return Err(crate::longterm::Refusal::NoTimestamp.sentence());
    }
    Ok(long_term)
}

/// The subject name of the certificate `identity` names, read from the store
/// in this process: the only source of the name a visible signature draws.
///
/// Asks the OS for the certificate only; the key is not touched.
async fn signer_name(identity: &str, at: u64) -> Result<String, String> {
    let wanted = identity.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let identity = keystore::find(&wanted)?;
        sign_cms::usable(&identity.certificate, at).map(|offer| offer.subject)
    })
    .await
    .map_err(|e| said!("the certificate search did not run: {e}"))?
}

/// Draws what a visible signature would look like, before anything is signed.
///
/// `size` is `[width, height]` in points --- the panel asks for one
/// representative shape before the reader has placed anything, and the signing
/// lays out again for the rectangle actually placed. The name is read from the
/// certificate here, as [`sign_document`] reads it, so the preview names whom
/// the signature will. The picture is drawn in `doc`'s worker by the function
/// that signs (`sign_prepare::preview`); nothing of the document is read and
/// nothing is written.
#[tauri::command]
pub async fn sign_preview(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    identity: String,
    size: [f32; 2],
    image: Option<crate::signature::Image>,
    options: sign_prepare::Options,
) -> Result<sign_prepare::Preview, String> {
    let at = now();
    let name = signer_name(&identity, at).await?;
    let visible = sign_prepare::Visible {
        page: 0,
        rect: [0.0, 0.0, size[0], size[1]],
        name,
        image,
        options,
    };
    let (reply, rx) = reply_channel();
    service.signature_preview(doc, at, visible, reply);
    await_reply("sign_preview", rx).await
}

/// The certificates in the reader's store that have a key, sorted into the
/// ones that may sign now and the ones that may not.
///
/// On the blocking pool: the store is searched synchronously, and a smart-card
/// reader can take a noticeable moment to answer.
#[tauri::command]
pub async fn sign_identities() -> Result<sign_cms::Choices, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let found: Vec<(String, Vec<u8>)> = keystore::identities()?
            .into_iter()
            .map(|identity| (identity.id(), identity.certificate))
            .collect();
        Ok(sign_cms::choices(&found, now()))
    })
    .await
    .map_err(|e| said!("the certificate search did not run: {e}"))?
}

/// Linux: remembers the certificate file at `path` for signing (`certfiles.rs`),
/// once its password, asked in the native dialog (`pin_prompt.rs`) so it never
/// reaches the webview, opens it and it holds an ECDSA key. `None` when the
/// reader cancelled the password. Off Linux, a refusal that says where such a
/// file goes instead.
#[tauri::command]
pub async fn sign_add_certificate_file(path: String) -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            use secrecy::ExposeSecret as _;
            let path = std::path::PathBuf::from(path);
            let store = crate::certfiles::store().ok_or_else(|| {
                said!("There is no home folder to keep the list of certificate files in.")
            })?;
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let Some(password) = crate::pkcs11::ask(crate::pkcs11::Asked::Password(&name))? else {
                return Ok(None);
            };
            let entry = crate::certfiles::add(&store, &path, password.expose_secret())?;
            let certificate = entry.certificate();
            let subject = sign_cms::usable(&certificate, now())
                .map_or_else(|_| sign_cms::named(&certificate), |offer| offer.subject);
            Ok(Some(said!(
                "{subject} can sign now: choose it when you sign a document."
            )))
        })
        .await
        .map_err(|e| said!("adding the certificate file did not run: {e}"))?
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        let _ = path;
        Err(said!(
            "Certificate files are added to tpdf on Linux; on macOS and Windows, import the \
             file into your keychain or certificate store, and tpdf finds it there."
        ))
    }
}

/// Signs the open document with `identity` and writes the result to `path`.
///
/// **The original file is not modified**: `path` is a new file, and naming the
/// original is refused. Refused too, before anything is asked of the OS: a
/// document with unsaved edits, since a signature is over the file and the
/// reader is looking at the file plus their edits.
///
/// The OS may show its own prompt while signing --- keychain access, a smart
/// card's PIN. That is the OS asking, and tpdf never sees the answer.
///
/// `timestamp` is the authority the reader chose for this signing, or `None`
/// for none --- in which case no request is made to anybody. It is judged here
/// before anything else is asked. `long_term` asks for long-term validation
/// data as well, and is refused without a timestamp.
// Eleven because a Tauri command's arguments are its IPC shape: three are the
// states Tauri injects, and bundling the eight the frontend sends into a struct
// would change `ipc.ts`'s mirror for no reader's benefit.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn sign_document(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    identity: String,
    path: String,
    placement: Option<Placement>,
    timestamp: Option<String>,
    long_term: Option<bool>,
    field: Option<String>,
    lock: Option<bool>,
) -> Result<Signing, String> {
    let authority = authority_of(timestamp.as_deref())?;
    let long_term = long_term_of(long_term.unwrap_or(false), authority.as_ref())?;
    {
        use tauri::Manager as _;
        app.state::<Pending>().clear();
    }
    sign_cms::refuse_unsaved(edits.state(doc)?.dirty)?;
    let opened_as = edits.plan(doc)?.opened_as.ok_or_else(|| {
        said!(
            "tpdf could not record what this file looked like when it was opened, so it \
             cannot tell that what it signs is that file --- reopen it and sign again"
        )
    })?;

    let len = std::fs::metadata(&source)
        .map_err(|e| said!("could not measure {source}: {e}"))?
        .len();
    sign_cms::refuse_too_large(len)?;

    // The worker's half, first: if the document cannot be signed at all ---
    // encrypted, certified against any change --- the reader hears that before
    // the OS asks them for anything.
    //
    // A visible signature needs the signer's name first, to draw it. It is read
    // from the certificate the store holds, which asks the OS for nothing but
    // the certificate --- the key is not touched until the value is made.
    let at = now();
    let visible = match placement {
        None => None,
        Some(placement) => {
            let page = baseline_page(&edits.plan(doc)?, placement.page)?;
            let name = signer_name(&identity, at).await?;
            Some(placement.visible(page, name))
        }
    };
    let (reply, rx) = reply_channel();
    // An empty signature field the document has, when the reader chose one:
    // the worker finds it by this name and signs it, and a placement beside
    // it is drawn in the field's rectangle whatever rectangle it names.
    let notes = crate::sign_prepare::Notes {
        field: field.unwrap_or_default(),
        lock: lock.unwrap_or(false),
        ..Default::default()
    };
    service.prepare_signature(doc, at, visible, notes, reply);
    let unsigned = await_reply("sign_document", rx).await?;

    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager as _;
        let original =
            save::read_to_sign(Path::new(&source), &opened_as).map_err(|why| why.message)?;
        let identity = keystore::find(&identity)?;
        let made = sign_cms::sign(
            original,
            unsigned,
            at,
            &identity.certificate,
            &identity.chain,
            &keystore::Counted {
                key: &identity,
                count: &keystore::KEY_REQUESTS,
            },
        )?;
        asking_the_world(crate::trust::Anchors::System, |asking| {
            conclude(
                Stage::Made(made),
                authority.as_ref(),
                long_term,
                PathBuf::from(source),
                PathBuf::from(path),
                checking.as_ref(),
                &app.state::<Pending>(),
                asking,
            )
        })
    })
    .await
    .map_err(|e| said!("the signing did not run: {e}"))?
}

/// Finishes a signing whose timestamp did not come, as the reader chose: with
/// `timestamp` to try that authority again, or `None` to write it without one.
/// For one whose long-term data did not come, `long_term` gathers it again, and
/// `false` writes the timestamped signature without it; `timestamp` is not
/// asked again then, as the token is already in the signature.
/// **The OS is not asked for anything**: the signature is the one already made.
///
/// # Errors
///
/// `pending` is not the signature held --- it was cancelled, or a later
/// signing replaced it --- and everything writing it can refuse.
#[tauri::command]
pub async fn sign_resume(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    pending: u64,
    timestamp: Option<String>,
    long_term: Option<bool>,
) -> Result<Signing, String> {
    let authority = authority_of(timestamp.as_deref())?;
    let long_term = long_term.unwrap_or(false);
    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager as _;
        let held = app.state::<Pending>();
        let Held {
            stage, source, out, ..
        } = resumed(&held, pending, long_term, authority.as_ref())?;
        asking_the_world(crate::trust::Anchors::System, |asking| {
            conclude(
                stage,
                authority.as_ref(),
                long_term,
                source,
                out,
                checking.as_ref(),
                &held,
                asking,
            )
        })
    })
    .await
    .map_err(|e| said!("the signing did not run: {e}"))?
}

/// The signature held under `pending`, taken for [`sign_resume`] once the
/// reader's answer has been judged against it.
///
/// **Judged before it is taken**: a refusal leaves the signature held, so the
/// reader can answer again without the key being asked for a second time.
/// Until 2026-10-09 it was taken first, and a refused answer lost it.
///
/// # Errors
///
/// `pending` is not the signature held, or long-term data was asked for a
/// made signature with no timestamp authority to ask.
fn resumed(
    held: &Pending,
    pending: u64,
    long_term: bool,
    authority: Option<&url::Url>,
) -> Result<Held, String> {
    held.take_judged(pending, |stage| {
        // A made signature needs the timestamp for the data; a sealed one
        // already carries it, and `timestamp` is not asked again.
        if matches!(stage, Stage::Made(_)) {
            long_term_of(long_term, authority)?;
        }
        Ok(())
    })
}

/// What signing has done in this process, as the checks build reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SignRecord {
    /// How many times the OS has been asked to sign with a key:
    /// [`keystore::KEY_REQUESTS`].
    pub key_requests: u64,
    /// What the held signature is waiting for, or `None` when none is held.
    pub held: Option<Waiting>,
}

/// How many times the OS was asked for a key, and what is held.
///
/// Nothing in the shipped window asks this. The checks build's signing phase
/// does (`src/lib/signingcheck.ts`), at each step, because the two facts it
/// needs are ones the screen cannot show: that *Sign without long-term data*
/// wrote the signature already made rather than asking for the key again, and
/// that *Cancel* dropped the held one. It reveals a count and a state, and
/// nothing about a key or a document.
#[tauri::command]
pub fn sign_record(held: tauri::State<'_, Pending>) -> SignRecord {
    SignRecord {
        key_requests: keystore::KEY_REQUESTS.load(std::sync::atomic::Ordering::Relaxed),
        held: held.waiting(),
    }
}

/// Drops a signature whose timestamp did not come: the reader cancelled.
/// Nothing was written, and nothing is.
#[tauri::command]
pub fn sign_discard(held: tauri::State<'_, Pending>, pending: u64) {
    drop(held.take(pending));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign_finish::tests::{
        asking, finished, scratch, sealed, test_archive, test_vouch, Rewording,
    };
    use crate::sign_finish::ReadBack;

    /// A placement as `signing.ts` sends it reaches the worker with the
    /// reader's choices, and one sent without them means the three lines a
    /// visible signature drew before there was a choice.
    #[test]
    fn a_placement_carries_the_readers_choices_to_the_worker() {
        let sent = r#"{"page":7,"rect":[20,30,170,90],"image":null,
            "options":{"label":false,"name":true,"date":false,
                       "reason":"Geprüft","location":"Köln"}}"#;
        let placement: Placement = serde_json::from_str(sent).expect("the frontend's shape");
        assert_eq!(placement.page, 7);
        let visible = placement.visible(1, "A. Signer".into());
        assert_eq!(visible.page, 1);
        assert_eq!(visible.name, "A. Signer");
        assert_eq!(visible.rect, [20.0, 30.0, 170.0, 90.0]);
        assert_eq!(
            visible.options,
            sign_prepare::Options {
                label: false,
                name: true,
                date: false,
                reason: "Geprüft".into(),
                location: "Köln".into(),
                hide_reason: false,
                hide_location: false,
                text: Vec::new(),
                date_format: String::new(),
            }
        );

        let older = r#"{"page":7,"rect":[20,30,170,90],"image":null}"#;
        let placement: Placement = serde_json::from_str(older).expect("the older shape");
        assert_eq!(
            placement.visible(1, "A. Signer".into()).options,
            sign_prepare::Options::default()
        );
    }

    /// Long-term data that did not come: nothing written, the sealed
    /// signature held, and signing without the data writes the timestamped
    /// signature --- with nothing asked of the key or the authority, since
    /// the stage holds neither a key nor a request.
    #[test]
    fn long_term_data_that_did_not_come_is_held_and_can_be_left_out() {
        use crate::integrity::test_tsa::{Pki, Plan, Serve};
        let pki = Pki::start(Plan {
            signer_ocsp: None,
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let (source, out) = scratch("held");
        let pending = Pending::default();
        let answer = asking(test_vouch, |asking| {
            conclude(
                sealed(&pki),
                None,
                true,
                source.clone(),
                out.clone(),
                &crate::save::Here,
                &pending,
                asking,
            )
        })
        .expect("an answer");
        let waiting = answer.unstamped.expect("held");
        assert_eq!(waiting.stage, Waiting::LongTerm);
        assert!(
            waiting.why.contains("A timestamp alone works"),
            "{}",
            waiting.why
        );
        assert!(answer.signed.is_none() && !out.exists(), "nothing written");
        // What `sign_record` tells the window's signing phase.
        assert_eq!(pending.waiting(), Some(Waiting::LongTerm));

        let Held {
            stage, source, out, ..
        } = pending.take(waiting.pending).expect("held");
        assert_eq!(pending.waiting(), None, "taken, nothing is held");
        let answer = asking(test_vouch, |asking| {
            conclude(
                stage,
                None,
                false,
                source,
                out.clone(),
                &crate::save::Here,
                &pending,
                asking,
            )
        })
        .expect("written");
        let signed = answer.signed.expect("signed");
        let ours = signed.signatures.iter().find(|s| s.ours).expect("ours");
        assert!(ours.timestamp.is_some(), "the timestamp was kept");
        assert_eq!(
            ours.revocation.as_ref().map(|r| r.standing),
            Some(crate::revocation::Status::None)
        );
        assert!(out.exists());
    }

    #[test]
    fn long_term_data_that_came_is_written_and_read_back_good() {
        use crate::integrity::test_tsa::{Pki, Plan, Serve};
        let pki = Pki::start(Plan {
            signer_ocsp: Some(Serve::Good),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let (source, out) = scratch("good");
        let answer = asking(test_vouch, |asking| {
            conclude(
                sealed(&pki),
                None,
                true,
                source,
                out,
                &crate::save::Here,
                &Pending::default(),
                asking,
            )
        })
        .expect("written");
        let signed = answer.signed.expect("signed");
        let ours = signed.signatures.iter().find(|s| s.ours).expect("ours");
        assert_eq!(
            ours.revocation.as_ref().map(|r| r.standing),
            Some(crate::revocation::Status::Good)
        );
    }

    /// A revoked certificate: refused outright, and nothing held that could
    /// be written without the data.
    #[test]
    fn a_revoked_certificate_is_refused_and_nothing_is_held() {
        use crate::integrity::test_tsa::{Pki, Plan, Serve};
        let pki = Pki::start(Plan {
            signer_ocsp: Some(Serve::Revoked),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let (source, out) = scratch("revoked");
        let pending = Pending::default();
        let why = match asking(test_vouch, |asking| {
            conclude(
                sealed(&pki),
                None,
                true,
                source,
                out.clone(),
                &crate::save::Here,
                &pending,
                asking,
            )
        }) {
            Err(why) => why,
            Ok(answer) => panic!("not refused: {answer:?}"),
        };
        assert!(
            why.contains("revoked") && why.contains("nothing was written"),
            "{why}"
        );
        assert!(pending.0.lock().is_none(), "a revoked signature was kept");
        assert!(!out.exists());
    }

    /// Long-term data asks for itself whether the authority is trusted, before
    /// it asks the certificate authorities for anything: from a sealed stage
    /// whose token nobody vouches for, nothing is fetched and nothing written.
    ///
    /// The stage here is built by the test. `finish` vouches for a fresh token
    /// before it seals one, so the window never holds a sealed stage like this
    /// one and never offers *Sign without long-term data* for it
    /// (`a_long_term_signing_does_not_seal_a_token_from_an_authority_nobody_vouches_for`).
    /// What this test holds is the second check, in `longterm::extend`, which
    /// stays the one in front of the fetching.
    #[test]
    fn an_untrusted_authority_is_held_and_nothing_is_fetched() {
        use crate::integrity::test_tsa::{Pki, Plan, Serve};
        fn untrusted(token: &[u8], now: u64) -> crate::longterm::Vouched {
            crate::longterm::vouched_under(token, now, crate::trust::Anchors::Only(&[]))
        }
        let pki = Pki::start(Plan {
            signer_ocsp: Some(Serve::Good),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let (source, out) = scratch("untrusted");
        let pending = Pending::default();
        let answer = asking(untrusted, |asking| {
            conclude(
                sealed(&pki),
                None,
                true,
                source,
                out.clone(),
                &crate::save::Here,
                &pending,
                asking,
            )
        })
        .expect("an answer");
        let waiting = answer.unstamped.expect("held");
        assert_eq!(waiting.stage, Waiting::LongTerm);
        assert!(
            waiting.why.contains("is not trusted by this computer"),
            "{}",
            waiting.why
        );
        assert!(answer.signed.is_none() && !out.exists(), "nothing written");
        assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    }

    #[test]
    fn long_term_data_is_refused_without_a_timestamp() {
        assert!(long_term_of(true, None).is_err());
        assert_eq!(long_term_of(false, None), Ok(false));
        let url = url::Url::parse("http://127.0.0.1/").expect("a URL");
        assert_eq!(long_term_of(true, Some(&url)), Ok(true));
    }

    // ------------------------------------------- one tail, two entry points

    /// The steps a signing took after the OS had signed, each with whether
    /// the signed copy was on disk when it ran.
    type Steps = std::sync::Mutex<Vec<String>>;

    /// A worker that records what it is asked, and answers as `save::Here`.
    struct Recording<'a> {
        steps: &'a Steps,
        out: PathBuf,
    }

    impl Recording<'_> {
        fn note(&self, step: &str) {
            let written = if self.out.exists() {
                "written"
            } else {
                "not written"
            };
            self.steps
                .lock()
                .expect("the record")
                .push(format!("{step} ({written})"));
        }
    }

    impl crate::save::Verifier for Recording<'_> {
        fn scan(
            &self,
            _: &mut std::fs::File,
            _: usize,
            _: &[String],
            _: Option<&str>,
        ) -> Result<crate::verify::Report, String> {
            Err("not asked".into())
        }

        fn signatures(
            &self,
            file: &mut std::fs::File,
            len: usize,
        ) -> Result<Vec<crate::docinfo::Signature>, String> {
            self.note("read back");
            crate::save::Here.signatures(file, len)
        }

        fn validation(
            &self,
            signed: &[u8],
            gathered: &crate::sign_dss::Gathered,
        ) -> Result<crate::sign_dss::Extended, String> {
            self.note("validation data appended");
            crate::save::Here.validation(signed, gathered)
        }

        fn document_timestamp(&self, signed: &[u8]) -> Result<sign_prepare::Unsigned, String> {
            self.note("archive timestamp prepared");
            crate::save::Here.document_timestamp(signed)
        }

        fn survey(&self, signed: &[u8]) -> Result<crate::sign_dss::Survey, String> {
            self.note("signatures surveyed");
            crate::save::Here.survey(signed)
        }
    }

    /// The window and the command line run one tail: from the same made
    /// signature, asking the same authorities, each takes the same steps in
    /// the same order --- the timestamp, the authority vouched for before the
    /// seal and again before the fetching, the revocation data, the two revisions a worker builds, the archive
    /// timestamp, and only then the write, read back by a worker --- and each
    /// ends with the same read-back verdict.
    #[test]
    fn the_window_and_the_command_line_take_the_same_steps_in_the_same_order() {
        use crate::integrity::test_tsa::{mint, Imprint, Pki, Plan, Serve};
        use crate::sign_cms::testkeys::{plain_pdf, Soft};
        let pki = Pki::start(Plan {
            signer_ocsp: Some(Serve::Good),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let authority = url::Url::parse("http://127.0.0.1:9/").expect("a URL");
        let made = || {
            let at = now();
            let original = plain_pdf();
            let unsigned = sign_prepare::prepare(original.clone(), at, None).expect("prepared");
            sign_cms::sign(
                original,
                unsigned,
                at,
                &pki.signer.certificate,
                &pki.chain,
                &Soft::p256(pki.signer.seed),
            )
            .expect("made")
        };
        // Each seam records itself and answers as the fake PKI does.
        let recorded = |steps: &Steps, out: &Path, run: &mut dyn FnMut(Asking<'_>)| {
            let note = |step: String| {
                let written = if out.exists() {
                    "written"
                } else {
                    "not written"
                };
                steps
                    .lock()
                    .expect("the record")
                    .push(format!("{step} ({written})"));
            };
            run(Asking {
                stamp: &|_, value| {
                    note("timestamp".into());
                    Ok(mint(
                        Imprint::Sha256,
                        &Imprint::Sha256.digest(value),
                        None,
                        now(),
                        &pki.tsa,
                    ))
                },
                os_chain: &|_, _| Vec::new(),
                vouch: &|token, now| {
                    note("authority vouched for".into());
                    test_vouch(token, now)
                },
                fetch: &mut |url, body, limits| {
                    note(format!("fetched {}", url.path()));
                    crate::longterm::fetch_blocking(url, body, limits)
                },
                archive: &mut |url, pieces| {
                    note("archive timestamp".into());
                    test_archive(url, pieces)
                },
                any_authority: false,
            });
        };

        let (source, out) = scratch("order-window");
        let window = Steps::default();
        recorded(&window, &out, &mut |asking| {
            let answer = conclude(
                Stage::Made(made()),
                Some(&authority),
                true,
                source.clone(),
                out.clone(),
                &Recording {
                    steps: &window,
                    out: out.clone(),
                },
                &Pending::default(),
                asking,
            )
            .expect("written");
            assert!(answer.signed.is_some(), "{answer:?}");
        });

        let (source, out) = scratch("order-tool");
        let tool = Steps::default();
        let line: Vec<String> = [
            source.to_str().expect("a path"),
            "-o",
            out.to_str().expect("a path"),
            "--identity",
            "anybody",
            "--timestamp",
            authority.as_str(),
            "--long-term",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let sign = crate::cli::sign::parse(&line).expect("a line");
        recorded(&tool, &out, &mut |asking| {
            let finished = crate::cli::sign::concluded(
                &sign,
                made(),
                now(),
                &Recording {
                    steps: &tool,
                    out: out.clone(),
                },
                asking,
            )
            .unwrap_or_else(|why| panic!("{}", why.message));
            assert_eq!(finished.read_back, ReadBack::Holds);
        });

        let window = window.into_inner().expect("the record");
        assert_eq!(
            window,
            [
                "timestamp (not written)",
                // Before the seal, and again by the long-term data before it
                // fetches: the second answer plans the chain it fetches for.
                "authority vouched for (not written)",
                "authority vouched for (not written)",
                "fetched /ocsp/signer (not written)",
                "fetched /ocsp/authority (not written)",
                "validation data appended (not written)",
                "archive timestamp prepared (not written)",
                "archive timestamp (not written)",
                "authority vouched for (not written)",
                "read back (written)",
            ]
        );
        assert_eq!(tool.into_inner().expect("the record"), window);
    }

    /// The window's long-term signing, from a made signature whose timestamp
    /// comes from an authority nobody vouches for: held as a signature still
    /// waiting for its *timestamp*, never as a sealed one waiting for its
    /// long-term data, because *Sign without long-term data* writes a sealed
    /// stage as it is. Nothing is fetched and nothing is written, and the
    /// answer that writes a held stage without long-term data writes no token
    /// from that authority either.
    #[test]
    fn a_long_term_signing_does_not_seal_a_token_from_an_authority_nobody_vouches_for() {
        use crate::integrity::test_tsa::{mint, Imprint, Pki, Plan, Serve};
        use crate::sign_cms::testkeys::{plain_pdf, Soft};
        let pki = Pki::start(Plan {
            signer_ocsp: Some(Serve::Good),
            authority_ocsp: Some(Serve::Good),
            ..Plan::default()
        });
        let authority = url::Url::parse("http://127.0.0.1:9/").expect("a URL");
        let at = now();
        let original = plain_pdf();
        let unsigned = sign_prepare::prepare(original.clone(), at, None).expect("prepared");
        let made = sign_cms::sign(
            original,
            unsigned,
            at,
            &pki.signer.certificate,
            &pki.chain,
            &Soft::p256(pki.signer.seed),
        )
        .expect("made");
        let signing = |stage: Stage, long_term: bool, source: PathBuf, out: PathBuf, held| {
            let mut fetch = crate::longterm::fetch_blocking;
            let mut archive = test_archive;
            conclude(
                stage,
                Some(&authority),
                long_term,
                source,
                out,
                &crate::save::Here,
                held,
                Asking {
                    stamp: &|_, value| {
                        Ok(mint(
                            Imprint::Sha256,
                            &Imprint::Sha256.digest(value),
                            None,
                            now(),
                            &pki.tsa,
                        ))
                    },
                    os_chain: &|_, _| Vec::new(),
                    vouch: &|token, now| {
                        crate::longterm::vouched_under(token, now, crate::trust::Anchors::Only(&[]))
                    },
                    fetch: &mut fetch,
                    archive: &mut archive,
                    any_authority: false,
                },
            )
        };
        let (source, out) = scratch("long-term-unvouched");
        let pending = Pending::default();
        let answer =
            signing(Stage::Made(made), true, source, out.clone(), &pending).expect("an answer");
        let waiting = answer.unstamped.expect("held");
        assert_eq!(waiting.stage, Waiting::Timestamp, "{}", waiting.why);
        assert!(
            waiting.why.contains("is not trusted by this computer"),
            "{}",
            waiting.why
        );
        assert_eq!(pending.waiting(), Some(Waiting::Timestamp));
        assert!(answer.signed.is_none() && !out.exists(), "nothing written");
        assert!(pki.paths().is_empty(), "{:?}", pki.paths());

        // *Sign without long-term data*, as `sign_resume` runs it on what is
        // held: the same authority is asked again and is refused again.
        let Held {
            stage, source, out, ..
        } = resumed(&pending, waiting.pending, false, Some(&authority)).expect("held");
        let answer = signing(stage, false, source, out.clone(), &pending).expect("an answer");
        assert_eq!(
            answer.unstamped.map(|u| u.stage),
            Some(Waiting::Timestamp),
            "an unvouched token was written"
        );
        assert!(!out.exists(), "nothing written");
    }

    /// A worker that reads the written file as `save::Here` does, and then
    /// loses each signature's timestamp: a copy that does not read back as it
    /// was written.
    struct Forgetful;

    impl crate::save::Verifier for Forgetful {
        fn scan(
            &self,
            _: &mut std::fs::File,
            _: usize,
            _: &[String],
            _: Option<&str>,
        ) -> Result<crate::verify::Report, String> {
            Err("not asked".into())
        }

        fn signatures(
            &self,
            file: &mut std::fs::File,
            len: usize,
        ) -> Result<Vec<crate::docinfo::Signature>, String> {
            let mut found = crate::save::Here.signatures(file, len)?;
            for signature in &mut found {
                signature.timestamp = None;
            }
            Ok(found)
        }

        fn validation(
            &self,
            signed: &[u8],
            gathered: &crate::sign_dss::Gathered,
        ) -> Result<crate::sign_dss::Extended, String> {
            crate::save::Here.validation(signed, gathered)
        }

        fn document_timestamp(&self, signed: &[u8]) -> Result<sign_prepare::Unsigned, String> {
            crate::save::Here.document_timestamp(signed)
        }

        fn survey(&self, signed: &[u8]) -> Result<crate::sign_dss::Survey, String> {
            crate::save::Here.survey(signed)
        }
    }

    /// The window holds a written copy to the command line's rule: a
    /// timestamped signing whose timestamp is not in what a worker reads back
    /// is a refusal that names the copy, not a signing reported as done. Until
    /// 2026-10-05 the window's sentence looked at the signature's verdict
    /// alone.
    #[test]
    fn the_window_refuses_a_copy_whose_timestamp_did_not_read_back() {
        use crate::integrity::test_tsa::{Pki, Plan};
        let pki = Pki::start(Plan::default());
        let written = |name: &str, checking: &dyn crate::save::Verifier| {
            let (source, out) = scratch(name);
            let answer = asking(test_vouch, |asking| {
                conclude(
                    sealed(&pki),
                    None,
                    false,
                    source,
                    out.clone(),
                    checking,
                    &Pending::default(),
                    asking,
                )
            });
            (answer, out)
        };
        let (answer, out) = written("unstamped", &Forgetful);
        let why = match answer {
            Err(why) => why,
            Ok(answer) => panic!("reported as signed: {answer:?}"),
        };
        assert_eq!(
            why,
            "signed.pdf was written, but reading it back did not find the timestamp of the \
             new signature Signature1 intact. Do not rely on that copy."
        );
        assert!(out.exists(), "the copy the sentence names is there");
        // The control: the same signing, read back as it was written.
        let (answer, _) = written("stamped", &crate::save::Here);
        let signed = answer.expect("written").signed.expect("signed");
        assert!(signed
            .signatures
            .iter()
            .any(|s| s.ours && s.timestamp.is_some()));
    }

    /// The hashing budget is one for the document and the new signature is
    /// reached last, so on a large file with several signatures it is the one
    /// left unchecked. That copy is correct --- the seal checked this
    /// signature on a budget of its own --- and until 2026-10-09 it was
    /// reported as one not to rely on.
    #[test]
    fn a_new_signature_the_read_back_had_no_budget_for_is_not_a_failed_copy() {
        use crate::integrity::{Verdict, Why};
        fn starved(mut found: Vec<crate::docinfo::Signature>) -> Vec<crate::docinfo::Signature> {
            let verdict = found[0].integrity.as_mut().expect("a verdict");
            verdict.verdict = Verdict::Unchecked;
            verdict.why = Some(Why::Budget);
            found
        }
        let done = finished("starved", &Rewording(starved));
        assert_eq!(done.read_back, ReadBack::Unchecked);

        // The window: a signing reported, not an error, whose new signature
        // carries the verdict `afterSigning` words.
        use crate::integrity::test_tsa::{Pki, Plan};
        let pki = Pki::start(Plan::default());
        let (source, out) = scratch("starved-window");
        let answer = asking(test_vouch, |asking| {
            conclude(
                sealed(&pki),
                None,
                false,
                source,
                out.clone(),
                &Rewording(starved),
                &Pending::default(),
                asking,
            )
        })
        .expect("reported, not refused");
        let signed = answer.signed.expect("signed");
        let ours = signed.signatures.iter().find(|s| s.ours).expect("ours");
        assert_eq!(
            ours.integrity.as_ref().map(|i| (i.verdict, i.why)),
            Some((Verdict::Unchecked, Some(Why::Budget)))
        );
        assert!(out.exists());
    }

    /// An answer `sign_resume` refuses leaves the signature held: the reader
    /// answers again and the key is not asked for a second time. Until
    /// 2026-10-09 the signature was taken before the answer was judged.
    #[test]
    fn an_answer_that_is_refused_leaves_the_signature_held() {
        use crate::integrity::test_tsa::{Pki, Plan};
        use crate::sign_cms::testkeys::{plain_pdf, Soft};
        let pki = Pki::start(Plan::default());
        let at = now();
        let original = plain_pdf();
        let unsigned = sign_prepare::prepare(original.clone(), at, None).expect("prepared");
        let made = sign_cms::sign(
            original,
            unsigned,
            at,
            &pki.signer.certificate,
            &pki.chain,
            &Soft::p256(pki.signer.seed),
        )
        .expect("made");
        let pending = Pending::default();
        let (source, out) = scratch("refused-answer");
        let number = pending.keep(Stage::Made(made), source, out);

        // Long-term data with no timestamp authority: refused.
        let why = match resumed(&pending, number, true, None) {
            Err(why) => why,
            Ok(_) => panic!("taken"),
        };
        assert_eq!(why, crate::longterm::Refusal::NoTimestamp.sentence());
        assert_eq!(pending.waiting(), Some(Waiting::Timestamp), "still held");
        // A number that is not the held one takes nothing either.
        assert!(resumed(&pending, number + 1, false, None).is_err());
        assert_eq!(pending.waiting(), Some(Waiting::Timestamp));
        // The answer that is accepted takes it.
        assert!(resumed(&pending, number, false, None).is_ok());
        assert_eq!(pending.waiting(), None);
        assert!(
            resumed(&pending, number, false, None).is_err(),
            "taken once"
        );
    }
}
