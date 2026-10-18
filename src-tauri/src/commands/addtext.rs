//! Save a copy with page numbers, a header, a footer or a watermark.
//!
//! `save_copy` with one thing added: the lines of text the plan writes on the
//! copy's pages (`addedtext.rs`). The open document is not touched, and unsaved
//! changes go into the copy as they do for any copy.

use std::path::Path;

use super::{outside_of, password_for};
use crate::addedtext::{self, AddedText};
use crate::render::RenderService;
use crate::{edits, save};

/// Writes the working document to `path` with `added` on its pages.
///
/// Each line is checked here, before a worker is started, in the sentence
/// the writer would give; the writer checks again.
#[tauri::command]
pub async fn add_text_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    added: Vec<AddedText>,
) -> Result<save::Copied, String> {
    for line in &added {
        addedtext::check(line)?;
    }
    let mut plan = edits.plan(doc)?;
    plan.added_text = added;
    let opened_with = password_for(&service, doc, "add_text_copy").await;
    let writing = outside_of(&app, service.backend());
    tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            opened_with.as_deref(),
            &*writing,
        )
    })
    .await
    .map_err(|e| format!("the save did not run: {e}"))?
    .map_err(|why| why.message)
}
