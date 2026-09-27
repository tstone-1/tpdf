//! Signing with a certificate the reader already has: Phase 6 step 2.
//!
//! Two commands. [`sign_identities`] lists what the OS store holds; nothing is
//! parsed but the reader's own certificates. [`sign_document`] runs the whole
//! split `docs/PLAN.md` §9 decided: the **worker** builds the revision with an
//! empty hole (`sign_prepare.rs`), this **app process** reads the file, checks
//! the worker's numbers against it, has the OS sign, splices the value in and
//! refuses unless its own verifier calls the result intact (`sign_cms.rs`), the
//! copy is written (`save::write_signed`), and a **worker** holding the written
//! file reports every signature in it --- which is the answer the reader is
//! shown.

use std::path::Path;

use super::{await_reply, outside_of, reply_channel};
use crate::render::RenderService;
use crate::{edits, keystore, save, sign_cms};

/// Seconds since the epoch, now.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
#[tauri::command]
pub async fn sign_document(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    identity: String,
    path: String,
) -> Result<sign_cms::Signed, String> {
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
    let (reply, rx) = reply_channel();
    service.prepare_signature(doc, now(), reply);
    let unsigned = await_reply("sign_document", rx).await?;

    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        let source = Path::new(&source);
        let out = Path::new(&path);
        let original = save::read_to_sign(source, &opened_as).map_err(|why| why.message)?;
        let identity = keystore::find(&identity)?;
        let field = unsigned.field.clone();
        let bytes = sign_cms::finish(
            original,
            unsigned,
            &identity.certificate,
            &identity.chain,
            &identity,
        )?;
        save::write_signed(source, out, &bytes).map_err(|why| why.message)?;

        // Read back by a worker, through the handle of the file just written:
        // the verdict the reader is shown is the one the properties dialog
        // would give, computed where every other parse of the file happens.
        let mut written = std::fs::File::open(out)
            .map_err(|e| format!("the signed file was written and could not be reopened: {e}"))?;
        let found = checking
            .signatures(&mut written, bytes.len())
            .map_err(|e| format!("the signed file was written and could not be checked: {e}"))?;
        Ok(sign_cms::report(path.clone(), field, found))
    })
    .await
    .map_err(|e| format!("the signing did not run: {e}"))?
}
