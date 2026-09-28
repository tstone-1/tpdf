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
//! shown.
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

use std::path::{Path, PathBuf};

use super::{await_reply, outside_of, reply_channel};
use crate::docmodel::PageSource;
use crate::render::RenderService;
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
        Some(_) => Err(
            "the page chosen for the signature is not a page of the file on disk \
                        --- save the document and sign again"
                .into(),
        ),
        None => Err("the page chosen for the signature is no longer in the document".into()),
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
    made: sign_cms::Made,
    source: PathBuf,
    out: PathBuf,
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

/// A signing whose timestamp did not come.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Unstamped {
    /// Why, as a sentence.
    pub why: String,
    /// The kept signature, for [`sign_resume`] and [`sign_discard`].
    pub pending: u64,
}

impl Pending {
    fn keep(&self, made: sign_cms::Made, source: PathBuf, out: PathBuf) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        *self.0.lock() = Some(Held {
            number,
            made,
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

    fn take(&self, number: u64) -> Option<Held> {
        let mut held = self.0.lock();
        if held.as_ref().is_some_and(|h| h.number == number) {
            held.take()
        } else {
            None
        }
    }
}

/// The authority the frontend named, judged again here: the webview is not
/// trusted to have kept to `http` and `https`.
fn authority_of(timestamp: Option<&str>) -> Result<Option<url::Url>, String> {
    timestamp
        .map(|text| crate::tsa::authority(text).map_err(|why| why.sentence("")))
        .transpose()
}

/// The rest of a signing, from a made signature: the timestamp when one was
/// asked for, the seal, the write and the read-back --- or, when the timestamp
/// did not come, the signature kept and the reason. On a blocking thread: the
/// request, the write and the read-back all wait.
fn conclude(
    made: sign_cms::Made,
    authority: Option<&url::Url>,
    source: PathBuf,
    out: PathBuf,
    checking: &dyn crate::save::Outside,
    pending: &Pending,
) -> Result<Signing, String> {
    let stamped = match crate::tsa::stamp(&made, authority, |url, value| {
        crate::tsa::ask_blocking(url, value, &crate::tsa::LIMITS)
    }) {
        Ok(stamped) => stamped,
        Err(why) => {
            let host = authority
                .and_then(url::Url::host_str)
                .unwrap_or_default()
                .to_string();
            let number = pending.keep(made, source, out);
            return Ok(Signing {
                signed: None,
                unstamped: Some(Unstamped {
                    why: why.sentence(&host),
                    pending: number,
                }),
            });
        }
    };
    let field = made.field.clone();
    let bytes = made.seal(stamped)?;
    save::write_signed(&source, &out, &bytes).map_err(|why| why.message)?;

    // Read back by a worker, through the handle of the file just written:
    // the verdict the reader is shown is the one the properties dialog would
    // give, computed where every other parse of the file happens.
    let mut written = std::fs::File::open(&out)
        .map_err(|e| format!("the signed file was written and could not be reopened: {e}"))?;
    let found = checking
        .signatures(&mut written, bytes.len())
        .map_err(|e| format!("the signed file was written and could not be checked: {e}"))?;
    Ok(Signing {
        signed: Some(sign_cms::report(out.display().to_string(), field, found)),
        unstamped: None,
    })
}

/// Seconds since the epoch, now.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
    .map_err(|e| format!("the certificate search did not run: {e}"))?
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
    .map_err(|e| format!("the certificate search did not run: {e}"))?
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
/// before anything else is asked.
// Nine because a Tauri command's arguments are its IPC shape: three are the
// states Tauri injects, and bundling the six the frontend sends into a struct
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
) -> Result<Signing, String> {
    let authority = authority_of(timestamp.as_deref())?;
    {
        use tauri::Manager as _;
        app.state::<Pending>().clear();
    }
    sign_cms::refuse_unsaved(edits.state(doc)?.dirty)?;
    let opened_as = edits.plan(doc)?.opened_as.ok_or_else(|| {
        "tpdf could not record what this file looked like when it was opened, so it \
         cannot tell that what it signs is that file --- reopen it and sign again"
            .to_string()
    })?;

    let len = std::fs::metadata(&source)
        .map_err(|e| format!("could not measure {source}: {e}"))?
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
    service.prepare_signature(doc, at, visible, reply);
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
            &identity,
        )?;
        conclude(
            made,
            authority.as_ref(),
            PathBuf::from(source),
            PathBuf::from(path),
            checking.as_ref(),
            &app.state::<Pending>(),
        )
    })
    .await
    .map_err(|e| format!("the signing did not run: {e}"))?
}

/// Finishes a signing whose timestamp did not come, as the reader chose: with
/// `timestamp` to try that authority again, or `None` to write it without one.
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
) -> Result<Signing, String> {
    let authority = authority_of(timestamp.as_deref())?;
    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager as _;
        let held = app.state::<Pending>();
        let Held {
            made, source, out, ..
        } = held
            .take(pending)
            .ok_or("this signature is no longer held --- sign the document again".to_string())?;
        conclude(
            made,
            authority.as_ref(),
            source,
            out,
            checking.as_ref(),
            &held,
        )
    })
    .await
    .map_err(|e| format!("the signing did not run: {e}"))?
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
            }
        );

        let older = r#"{"page":7,"rect":[20,30,170,90],"image":null}"#;
        let placement: Placement = serde_json::from_str(older).expect("the older shape");
        assert_eq!(
            placement.visible(1, "A. Signer".into()).options,
            sign_prepare::Options::default()
        );
    }
}
