//! Save a copy with page numbers, a header, a footer or a watermark.
//!
//! `save_copy` with one thing added: the lines of text the plan writes on the
//! copy's pages (`addedtext.rs`). The open document is not touched, and unsaved
//! changes go into the copy as they do for any copy.

use std::path::Path;

use super::{answered, outside_of, password_for};
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
    let mut plan = edits.plan(doc)?;
    let count = u32::try_from(plan.pages.len()).map_err(|e| e.to_string())?;
    let expected = addedtext::plan_lines(count, &added)?;
    plan.added_text = added;
    let opened_with = password_for(&service, doc, "add_text_copy").await;
    let writing = outside_of(&app, service.backend());
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save::write_checked_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            opened_with.as_deref(),
            &*writing,
            &|staged| {
                let opened =
                    super::open_temporary(&service, staged, opened_with.clone(), "add_text_copy")
                        .map_err(|e| save::Refusal::from(e.reason))?;
                let checked = if opened.page_count == count as usize {
                    addedtext::verify_lines(&expected, |page| {
                        answered("add_text_copy", |reply| {
                            service.text(opened.id, page, None, reply)
                        })
                    })
                } else {
                    Err(said!("the staged copy has a different number of pages"))
                };
                let closed = answered("add_text_copy", |reply| service.close(opened.id, reply));
                checked.and(closed).map_err(save::Refusal::from)
            },
        )
        .map(|()| save::Copied { changed: false })
    })
    .await
    .map_err(|e| said!("the save did not run: {e}"))?
    .map_err(|why| why.message)
}
