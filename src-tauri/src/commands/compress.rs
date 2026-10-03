//! Save a smaller copy, and say first what one would come to.
//!
//! `compress_estimate` asks the worker that holds the document what a smaller
//! copy would weigh and for one part of one page before and after; nothing is
//! written. `compress_copy` is `save_copy` with one thing changed: the plan
//! says how the copy is made smaller (`compress.rs`). The open document is not
//! touched, and unsaved changes go into the copy as they do for any copy.

use std::path::Path;

use base64::Engine as _;

use super::{await_reply, outside_of, password_for, reply_channel};
use crate::compress::{Compress, Pictures, Sample};
use crate::render::RenderService;
use crate::{edits, save};

/// What the reader chose: pictures as they are, or shrunk like this.
///
/// # Errors
///
/// Settings outside what is offered, in `Pictures::checked`'s sentence.
pub(crate) fn way(pictures: Option<Pictures>) -> Result<Compress, String> {
    Ok(match pictures {
        Some(pictures) => Compress::Pictures(pictures.checked()?),
        None => Compress::Lossless,
    })
}

/// One part of one page, before and after, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleView {
    /// The width of both pictures, in pixels.
    pub width: u32,
    /// The height of both.
    pub height: u32,
    /// The part as the document draws it: a PNG as a `data:` URL.
    pub before: String,
    /// The same part as the smaller copy draws it.
    pub after: String,
    /// The page it is from, counted from 1.
    pub page: u32,
    /// How far both are enlarged, in percent.
    pub zoom_percent: u32,
    /// The resolution of that page's most reduced picture before.
    pub dpi_before: u32,
    /// And after.
    pub dpi_after: u32,
}

/// What a smaller copy would come to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shrinkage {
    /// The size of the file the document was opened from, in bytes.
    pub bytes_before: u64,
    /// The size the copy would have.
    pub bytes_after: u64,
    /// Pictures the pages draw.
    pub pictures: usize,
    /// Of those, the ones that would be stored smaller.
    pub pictures_changed: usize,
    /// One part of one page before and after, when a picture changes.
    pub sample: Option<SampleView>,
}

fn data_url(rgba: &[u8], width: u32, height: u32) -> Result<String, String> {
    let png = crate::render::encode_png(rgba, width, height)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// A sample as two pictures the webview can show.
///
/// # Errors
///
/// A picture that will not encode.
pub(crate) fn view(sample: &Sample) -> Result<SampleView, String> {
    Ok(SampleView {
        width: sample.width,
        height: sample.height,
        before: data_url(&sample.before_rgba(), sample.width, sample.height)?,
        after: data_url(&sample.after_rgba(), sample.width, sample.height)?,
        page: sample.page,
        zoom_percent: sample.zoom_percent,
        dpi_before: sample.dpi_before,
        dpi_after: sample.dpi_after,
    })
}

/// Says what a smaller copy of the file `doc` was opened from would come to.
///
/// Of the file as it is on disk: a change not yet saved is not in the figure.
#[tauri::command]
pub async fn compress_estimate(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    pictures: Option<Pictures>,
) -> Result<Shrinkage, String> {
    let compress = way(pictures)?;
    let bytes_before = std::fs::metadata(&source)
        .map(|data| data.len())
        .map_err(|why| format!("{source}: {why}"))?;
    let (reply, rx) = reply_channel();
    service.shrink(doc, compress, reply);
    let estimate: crate::compress::Estimate = await_reply("compress_estimate", rx).await?;
    Ok(Shrinkage {
        bytes_before,
        bytes_after: estimate.bytes_after,
        pictures: estimate.done.pictures,
        pictures_changed: estimate.done.pictures_changed,
        sample: estimate.sample.as_ref().map(view).transpose()?,
    })
}

/// Writes the working document to `path`, made smaller as `pictures` says: as
/// they are when it is absent.
#[tauri::command]
pub async fn compress_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    pictures: Option<Pictures>,
) -> Result<save::Copied, String> {
    let mut plan = edits.plan(doc)?;
    plan.compress = way(pictures)?;
    let opened_with = password_for(&service, doc, "compress_copy").await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compress::Preset;

    #[test]
    fn no_settings_is_lossless_and_settings_are_checked() {
        assert_eq!(way(None), Ok(Compress::Lossless));
        let balanced = Pictures::from(Preset::Balanced);
        assert_eq!(way(Some(balanced)), Ok(Compress::Pictures(balanced)));
        assert!(way(Some(Pictures { dpi: 5, ..balanced })).is_err());
        assert!(way(Some(Pictures {
            quality: 0,
            ..balanced
        }))
        .is_err());
    }

    #[test]
    fn a_sample_reaches_the_window_as_two_pictures_and_in_its_spelling() {
        let sample = Sample {
            width: 2,
            height: 1,
            before: vec![255, 0, 0, 0, 255, 0],
            after: vec![0, 0, 255, 9, 9, 9],
            page: 3,
            zoom_percent: 200,
            dpi_before: 300,
            dpi_after: 110,
        };
        let shown = view(&sample).expect("encodes");
        assert!(shown.before.starts_with("data:image/png;base64,iVBOR"));
        assert!(shown.after.starts_with("data:image/png;base64,iVBOR"));
        assert_ne!(shown.before, shown.after);
        let sent = serde_json::to_value(Shrinkage {
            bytes_before: 10,
            bytes_after: 4,
            pictures: 2,
            pictures_changed: 1,
            sample: Some(shown),
        })
        .expect("serialises");
        assert_eq!(sent["bytesBefore"], 10);
        assert_eq!(sent["picturesChanged"], 1);
        assert_eq!(sent["sample"]["zoomPercent"], 200);
        assert_eq!(sent["sample"]["dpiBefore"], 300);
    }
}
