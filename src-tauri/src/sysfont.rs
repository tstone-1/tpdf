//! Finding an installed font by exact PostScript name, in the **app process**.
//!
//! The one place tpdf reads a font file the reader has installed. A worker
//! cannot: it has no filesystem authority (`docs/THREAT-MODEL.md` §T6.26), so
//! when automatic font mode wants an installed copy of a document's font the
//! worker answers with the name (`textedit::Preview::wants`), this module asks
//! the operating system where that font is and reads the file, and the bytes go
//! back to the worker inside the next request. **Nothing here parses a font**:
//! CoreText or DirectWrite names the file, and every check on what is in it is
//! the worker's (`textedit::fonts::installed::accept`) --- including that the
//! face really carries the name, because a lookup that misses can answer with
//! a substitute.
//!
//! The name comes from the document, so a document can make this process read
//! the file of any font the reader has installed. What that reaches is the
//! worker, which has no network, and a subset of the font inside the edited
//! document the reader chose to save; §T6.26 records it.
use std::io::Read;
use std::path::PathBuf;

use crate::textedit::{Change, EditFont, Installed, PageRuns};

/// An installed font file, and the face in it when the platform names one.
#[derive(Debug)]
pub(crate) struct Found {
    pub bytes: Vec<u8>,
    pub index: Option<u32>,
}

/// The installed font whose PostScript name is exactly `name`, read whole, or
/// `None` when there is none, it is larger than the worker accepts, or the
/// name cannot be a PostScript name.
pub(crate) fn find(name: &str) -> Option<Found> {
    find_with(name, locate)
}

/// [`find`] over a given lookup, which is the seam the tests use in place of
/// the operating system.
pub(crate) fn find_with(
    name: &str,
    locate: impl FnOnce(&str) -> Option<(PathBuf, Option<u32>)>,
) -> Option<Found> {
    if !crate::textedit::valid_postscript_name(name) {
        return None;
    }
    let (path, index) = locate(name)?;
    let file = std::fs::File::open(path).ok()?;
    let limit = crate::textedit::MAX_INSTALLED as u64;
    // Bounded by the read rather than by the file's length beforehand, which
    // a file growing in between would outrun: one byte past the bound is
    // enough to refuse it.
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(Found { bytes, index })
}

/// The draft a text request previews, if automatic mode asked for an
/// installed font it was not given and the lookup finds one: supplied with
/// the file, so the caller asks the worker again. Every request from the
/// webview is first stripped of anything it named itself ([`sanitise`]), so
/// the bytes a worker sees here are always ones this process read.
pub(crate) fn supply(
    draft: &mut Change,
    runs: &PageRuns,
    lookup: impl FnOnce(&str) -> Option<Found>,
) -> bool {
    let Some(name) = runs.preview.as_ref().and_then(|p| p.wants.as_deref()) else {
        return false;
    };
    let Some(layout) = draft.layout.as_mut() else {
        return false;
    };
    if layout.font != EditFont::Auto || layout.installed.is_some() {
        return false;
    }
    let Some(found) = lookup(name) else {
        return false;
    };
    layout.installed = Some(Installed {
        name: name.to_owned(),
        index: found.index,
        program: found.bytes,
    });
    true
}

/// A text request's batch: the page's pending replacements less any earlier
/// one for the draft's run, with the draft last when it changes anything, and
/// whether it did. The draft is stripped of any installed font the webview
/// named ([`sanitise`]); the only one a worker is ever handed is one this
/// process read or a worker built.
pub(crate) fn batch(mut pending: Vec<Change>, mut draft: Change) -> (Vec<Change>, bool) {
    sanitise(&mut draft);
    pending.retain(|old| (old.page, old.operator) != (draft.page, draft.operator));
    let changes = draft.replacement != draft.original || draft.layout.is_some();
    if changes {
        pending.push(draft);
    }
    (pending, changes)
}

/// Asks for a page's runs with `ask`, and when automatic font mode wants an
/// installed copy of the draft's font (the last change), looks it up and asks
/// once more with the file. Returns the runs with the app-only preview fields
/// taken off, and the installed font's subset the draft was set in, if any.
pub(crate) async fn ask_with_installed<A, F>(
    mut changes: Vec<Change>,
    ask: A,
    lookup: impl FnOnce(&str) -> Option<Found>,
) -> Result<(PageRuns, Option<Installed>), String>
where
    A: Fn(Vec<Change>) -> F,
    F: std::future::Future<Output = Result<PageRuns, String>>,
{
    let mut runs = ask(changes.clone()).await?;
    if let Some(draft) = changes.last_mut() {
        if supply(draft, &runs, lookup) {
            runs = ask(changes).await?;
        }
    }
    let installed = take(&mut runs);
    Ok((runs, installed))
}

/// The replacement `text_replace` journals, once a worker has accepted it:
/// the webview's change, set in the subset the worker built when it used an
/// installed font. **The journal keeps that subset and never the file**, and
/// every later request that carries the change --- a tile, an outline, the
/// save --- puts it through the same checks again (`textedit::Installed`).
pub(crate) async fn replacement<A, F>(
    change: Change,
    pending: Vec<Change>,
    ask: A,
    lookup: impl FnOnce(&str) -> Option<Found>,
) -> Result<Change, String>
where
    A: Fn(Vec<Change>) -> F,
    F: std::future::Future<Output = Result<PageRuns, String>>,
{
    let mut change = change;
    let (pending, drafted) = batch(pending, change.clone());
    let (_, installed) = ask_with_installed(pending, ask, lookup).await?;
    // A change that restores the original is not in the batch, and is
    // journalled as it came: the model takes it as removing the edit.
    if let (true, Some(layout)) = (drafted, change.layout.as_mut()) {
        layout.installed = installed;
    }
    Ok(change)
}

/// A change as it arrives from the webview, without an installed font: only
/// this process supplies one, from a file it read or a subset a worker built.
pub(crate) fn sanitise(change: &mut Change) {
    if let Some(layout) = change.layout.as_mut() {
        layout.installed = None;
    }
}

/// What a text request's reply keeps for the journal --- the subset the draft
/// was set in, if it was an installed font --- with both fields only this
/// process reads taken off before the reply goes to the webview.
pub(crate) fn take(runs: &mut PageRuns) -> Option<Installed> {
    let preview = runs.preview.as_mut()?;
    preview.wants = None;
    preview.installed.take()
}

#[cfg(target_os = "macos")]
fn locate(name: &str) -> Option<(PathBuf, Option<u32>)> {
    use objc2_core_foundation::{CFString, CFURL};
    use objc2_core_text::{kCTFontNameAttribute, kCTFontURLAttribute, CTFontDescriptor};
    // SAFETY: CoreText's documented descriptor calls, on a string and
    // attribute keys that live for the whole call; every result is checked.
    unsafe {
        let descriptor = CTFontDescriptor::with_name_and_size(&CFString::from_str(name), 12.);
        let matched = descriptor.matching_font_descriptor(None)?;
        // A name CoreText does not know is still matched, to a substitute;
        // only the exact name counts. The worker checks the face again.
        let found = matched
            .attribute(kCTFontNameAttribute)?
            .downcast::<CFString>()
            .ok()?;
        if found.to_string() != name {
            return None;
        }
        let url = matched
            .attribute(kCTFontURLAttribute)?
            .downcast::<CFURL>()
            .ok()?;
        // CoreText names the file and not the face in a collection; the
        // worker finds the face that carries the name.
        Some((url.to_file_path()?, None))
    }
}

#[cfg(windows)]
fn locate(name: &str) -> Option<(PathBuf, Option<u32>)> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::{Interface, BOOL};
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteFontFile,
        IDWriteLocalFontFileLoader, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_SIMULATIONS_NONE,
        DWRITE_INFORMATIONAL_STRING_POSTSCRIPT_NAME,
    };
    // SAFETY: DirectWrite's documented COM calls; every out-parameter is
    // initialised by the call that fills it and checked before use.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let mut collection: Option<IDWriteFontCollection> = None;
        factory
            .GetSystemFontCollection(&mut collection, false)
            .ok()?;
        let collection = collection?;
        for family in 0..collection.GetFontFamilyCount() {
            let Ok(family) = collection.GetFontFamily(family) else {
                continue;
            };
            for index in 0..family.GetFontCount() {
                let Ok(font) = family.GetFont(index) else {
                    continue;
                };
                // A simulated bold or oblique is DirectWrite drawing another
                // face, not a font file of this name.
                if font.GetSimulations() != DWRITE_FONT_SIMULATIONS_NONE {
                    continue;
                }
                let mut strings = None;
                let mut exists = BOOL(0);
                if font
                    .GetInformationalStrings(
                        DWRITE_INFORMATIONAL_STRING_POSTSCRIPT_NAME,
                        &mut strings,
                        &mut exists,
                    )
                    .is_err()
                    || !exists.as_bool()
                {
                    continue;
                }
                let Some(strings) = strings else { continue };
                let named = (0..strings.GetCount()).any(|at| {
                    let Ok(length) = strings.GetStringLength(at) else {
                        return false;
                    };
                    let mut buffer = vec![0_u16; length as usize + 1];
                    strings.GetString(at, &mut buffer).is_ok()
                        && String::from_utf16_lossy(&buffer[..length as usize]) == name
                });
                if !named {
                    continue;
                }
                let face = font.CreateFontFace().ok()?;
                let mut count = 0;
                face.GetFiles(&mut count, None).ok()?;
                if count != 1 {
                    return None;
                }
                let mut files: [Option<IDWriteFontFile>; 1] = [None];
                face.GetFiles(&mut count, Some(files.as_mut_ptr())).ok()?;
                let file = files[0].take()?;
                let loader: IDWriteLocalFontFileLoader = file.GetLoader().ok()?.cast().ok()?;
                let mut key = std::ptr::null_mut();
                let mut size = 0;
                file.GetReferenceKey(&mut key, &mut size).ok()?;
                let length = loader.GetFilePathLengthFromKey(key, size).ok()?;
                let mut path = vec![0_u16; length as usize + 1];
                loader.GetFilePathFromKey(key, size, &mut path).ok()?;
                path.truncate(length as usize);
                return Some((
                    PathBuf::from(std::ffi::OsString::from_wide(&path)),
                    Some(face.GetIndex()),
                ));
            }
        }
        None
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn locate(_: &str) -> Option<(PathBuf, Option<u32>)> {
    None
}

#[cfg(test)]
mod tests;
