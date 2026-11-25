//! Save a copy with a password, or without the one the document has.
//!
//! `save_copy` with one thing changed: the plan says what the copy's password
//! is (`protect.rs`). The open document is not touched, and unsaved changes go
//! into the copy as they do for any copy.

use std::path::Path;

use super::{answered, outside_of, password_for};
use crate::protect::{self, Protection};
use crate::render::RenderService;
use crate::{edits, save};

/// `plan` with the password the reader asked for: `new`, or none.
///
/// # Errors
///
/// A new password that cannot be one. Refused here, before a worker is
/// started, in the sentence the worker would give.
pub(crate) fn asked(mut plan: edits::Plan, new: Option<String>) -> Result<edits::Plan, String> {
    plan.protection = match new {
        Some(new) => {
            protect::acceptable(&new)?;
            Protection::Set(new)
        }
        None => Protection::Remove,
    };
    Ok(plan)
}

/// Validate the new password while the two fields are still editable.
#[tauri::command]
pub fn validate_new_password(password: String) -> Result<(), String> {
    protect::acceptable(&password)
}

/// Reopen the staged copy through the same reader the desktop uses.
fn verify_copy(
    service: &RenderService,
    staged: &Path,
    password: Option<&str>,
    expected: usize,
) -> Result<(), save::Refusal> {
    let open = |key| super::open_temporary(service, staged, key, "protect_copy");
    let close = |id| {
        answered("protect_copy", |reply| service.close(id, reply)).map_err(save::Refusal::from)
    };
    if password.is_some() {
        match open(None) {
            Err(e) if e.locked => (),
            Err(e) => return Err(e.reason.into()),
            Ok(info) => {
                close(info.id)?;
                return Err(said!(
                    "The copy tpdf built opens without the password, so it was not written."
                )
                .into());
            }
        }
    }
    let info = open(password.map(str::to_owned)).map_err(|e| save::Refusal::from(e.reason))?;
    close(info.id)?;
    if info.page_count != expected {
        return Err(said!(
            "The page count changed from {pages} to {actual}.",
            pages = expected,
            actual = info.page_count
        )
        .into());
    }
    Ok(())
}

/// Writes the working document to `path` with `password` as its password, or
/// with none when `password` is absent.
///
/// The password that opened the document is the one the app already holds;
/// removing a password is refused for a document that opened without one.
#[tauri::command]
pub async fn protect_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    password: Option<String>,
) -> Result<save::Copied, String> {
    let plan = asked(edits.plan(doc)?, password.clone())?;
    let expected = plan.pages.len();
    let opened_with = password_for(&service, doc, "protect_copy").await;
    let writing = outside_of(&app, service.backend());
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save::write_verified_copy(
            Path::new(&source),
            &plan,
            Path::new(&path),
            opened_with.as_deref(),
            &*writing,
            &|staged| verify_copy(&service, staged, password.as_deref(), expected),
        )
    })
    .await
    .map_err(|e| said!("the save did not run: {e}"))?
    .map_err(|why| why.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> edits::Plan {
        edits::Plan {
            field_edits: Vec::new(),
            tab_order: false,
            source_moved: false,
            opened_as: None,
            baseline: 1,
            pages: Vec::new(),
            redactions: Vec::new(),
            notes: Vec::new(),
            discards: Vec::new(),
            sources: Vec::new(),
            forms: Vec::new(),
            text_edits: Vec::new(),
            text_layers: Vec::new(),
            added_text: Vec::new(),
            protection: Protection::Keep,
            compress: Default::default(),
            new_fields: Vec::new(),
            marks: Vec::new(),
        }
    }

    #[test]
    fn a_password_is_set_and_none_removes_it() {
        assert_eq!(
            asked(plan(), Some("s3cret".into())).unwrap().protection,
            Protection::Set("s3cret".into())
        );
        assert_eq!(asked(plan(), None).unwrap().protection, Protection::Remove);
    }

    #[test]
    fn an_empty_password_is_refused_rather_than_read_as_a_removal() {
        let why = asked(plan(), Some(String::new())).unwrap_err();
        assert!(why.contains("empty"), "{why}");
    }
}
