//! Adding long-term validation data to a document that is already signed.
//!
//! One command, [`add_validation_data`], and the tail it shares with
//! `tpdf long-term` ([`finish`], in `validation_data.rs`). The steps are `longterm::existing`'s: a
//! **worker** reads the file's signatures and hands their values over, this
//! **app process** --- which holds the network and parses no document ---
//! asks the certificate authorities and the timestamp authority the reader
//! chose, a worker appends the `/DSS` and builds the document timestamp's
//! revision, the copy is written (`save::write_signed`), and a **worker**
//! holding the written file reports every signature in it. What the reader is
//! told is that last reading.
//!
//! **No key is used and nothing is held.** A signing keeps the signature the
//! OS made while the reader decides about a timestamp that did not come,
//! because asking for the key again is a cost; here there is nothing made
//! that a second try could not make again, so a refusal is a sentence and
//! the command is run again.
//!
//! **The signed-save warning is not shown**, for the reason signing shows
//! none: this appends revisions and writes no byte of the earlier ones, and
//! refuses unless every signature the document held reads afterwards as it
//! did before.

use std::path::{Path, PathBuf};

use super::outside_of;
use crate::render::RenderService;
use crate::validation_data::{
    asking_the_world, finish, name_of, now, refuse_same_file, refuse_too_large, refuse_unsaved,
};
use crate::{edits, save};

/// Adds long-term validation data for every signature of the open document,
/// and an archive timestamp from `timestamp` over the whole, and writes the
/// result to `path`. Answers the sentence the reader is shown.
///
/// **The original file is not modified**: `path` is a new file, and naming
/// the original is refused. Refused too, before anything is asked of
/// anybody: a document with unsaved edits, and a timestamp authority that is
/// not an `http` or `https` address without credentials --- judged here,
/// because the webview is not trusted to have kept to that.
///
/// # Errors
///
/// Every refusal as a sentence, which says whether anything was written.
#[tauri::command]
pub async fn add_validation_data(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    timestamp: String,
) -> Result<String, String> {
    let authority = crate::tsa::authority(&timestamp).map_err(|why| why.sentence(""))?;
    refuse_unsaved(edits.state(doc)?.dirty)?;
    let opened_as = edits.plan(doc)?.opened_as.ok_or_else(|| {
        said!(
            "tpdf could not record what this file looked like when it was opened, so it cannot \
             tell that what it adds to is that file --- reopen it and try again"
        )
    })?;
    let len = std::fs::metadata(&source)
        .map_err(|e| said!("could not measure {source}: {e}"))?
        .len();
    refuse_too_large(len)?;
    // Before anybody is asked: `write_signed` refuses the original's own
    // name too, but only once the certificate authorities have answered and
    // the timestamp authority has issued a token for a copy never written.
    // (An output that exists is not refused here: the save panel asked.)
    refuse_same_file(Path::new(&source), Path::new(&path))?;

    let checking = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(source);
        let out = PathBuf::from(path);
        // The bytes the reader opened, read once: every revision is built
        // against exactly these, and the survey a worker makes is of them.
        let original = save::read_to_sign(&source, &opened_as).map_err(|why| {
            if why.changed {
                said!(
                    "the file on disk is not the one that was opened, so nothing was added to it \
                     --- reopen it and try again"
                )
            } else {
                why.message
            }
        })?;
        asking_the_world(crate::trust::Anchors::System, &authority, |asking| {
            finish(
                &original,
                &out,
                now(),
                checking.as_ref(),
                asking,
                &|bytes| save::write_signed(&source, &out, bytes).map_err(|why| why.message),
            )
        })
        .map(|finished| finished.summary)
        .map_err(|stopped| stopped.sentence(&name_of(&out)))
    })
    .await
    .map_err(|e| said!("adding the validation data did not run: {e}"))?
}
