//! Reading a document rather than changing it: text, search, and the panels.
//!
//! Two routes, and which one a command takes is a measurement rather than a
//! preference. Text and search go through the render service, because the
//! answer is PDFium's. Comments, links, properties and the character mapping
//! are read from the object graph, because the question is about the whole
//! document and a page load per page costs more than the one parse the file
//! already needs.

use super::{await_reply, reply_channel};
use crate::render::RenderService;
use crate::webopen::{Registry, Source};
use crate::{annots, docinfo, encoding, links, opener, outline, search, text};

/// Extracts one page's characters and their positions.
///
/// Selection, search and the accessibility tree all read this, and they read
/// the same one deliberately --- three extractions would disagree in ways no
/// test catches, each being self-consistent. Cached on the frontend rather than
/// here: what a page's text costs to *re-request* is an IPC round trip, and what
/// it costs to re-extract is measured in `examples/text_probe.rs`.
#[tauri::command]
pub async fn page_text(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<text::PageText, String> {
    let (reply, rx) = reply_channel();
    service.text(doc, page, crop, reply);
    await_reply("page_text", rx).await
}

/// Finds a query in one page, or in a short run of them.
///
/// **Not the whole document in one call**, because the render thread is FIFO and
/// a scan of 775 pages would sit in front of every tile --- see
/// `RenderService::search`. The caller walks the document in runs and stops
/// asking when it wants to cancel; a run is what makes the walk cost one round
/// trip per sixteen pages instead of one per page, and short enough that a
/// reader scrolling during a search still gets tiles.
///
/// `pages` is that run, in walk order, and `page` is its first entry. Omitted or
/// empty is the single-page request this was before runs existed, and the reply
/// is byte-identical to the one it gave then --- the rest of a run's answers
/// arrive in `PageMatches::more`, which is skipped when empty. **The worker may
/// answer fewer pages than were asked for**: a reply is bounded, so the caller
/// reads how many came back and continues from there rather than assuming.
///
/// `carry` is the previous page's tail, handed back by the previous call. It is
/// how a phrase that runs over a page break is found without either side
/// holding two pages at once --- see `search::Carry`. Inside a run the worker
/// chains it itself, between listed pages that are neighbours.
#[tauri::command]
pub async fn search_page(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    pages: Option<Vec<u32>>,
    query: String,
    options: search::Options,
    carry: Option<search::Carry>,
) -> Result<search::PageMatches, String> {
    let (reply, rx) = reply_channel();
    // One list, never empty: an omitted `pages` is the run of one that `page`
    // names. A caller that sends both is taken at its `pages`, because a run's
    // first entry is the page the reply is about and two statements of that
    // could disagree.
    let run = pages
        .filter(|pages| !pages.is_empty())
        .unwrap_or_else(|| vec![page]);
    service.search(doc, run, query, options, carry, reply);
    await_reply("search_page", rx).await
}

/// Reads a document's outline --- its bookmarks --- as a bounded tree.
///
/// Bounded is the operative word: the outline of a malformed document can be
/// infinite, and PDFium documents that it is our job to notice. See
/// `outline.rs`.
#[tauri::command]
pub async fn document_outline(
    service: tauri::State<'_, RenderService>,
    web: tauri::State<'_, Registry>,
    doc: u32,
) -> Result<outline::Outline, String> {
    let (reply, rx) = reply_channel();
    service.outline(doc, reply);
    let mut answer: outline::Outline = await_reply("document_outline", rx).await?;
    web.adopt(doc, Source::Outline, &mut answer.urls);
    Ok(answer)
}

/// Reads every comment in a document --- notes, highlights, replies.
///
/// Document-level rather than per page, because the answer comes from one
/// `lopdf` parse of the whole file: asking per page would repeat that parse
/// once per page to return a slice of the same list. Lazy for the reason
/// `document_mapping` is --- it is off the startup path, and a reader who never
/// opens the comments panel never pays for it.
///
/// A failure is an error rather than an empty list. "This document has no
/// comments" and "this document could not be read" are different things to tell
/// a reader, and the frontend shows them differently. See `annots.rs`.
#[tauri::command]
pub async fn document_comments(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<annots::Comments, String> {
    let (reply, rx) = reply_channel();
    service.comments(doc, reply);
    await_reply("document_comments", rx).await
}

/// Reads every link in a document --- the rectangles a reader clicks.
///
/// Document-level for the same reason `document_comments` is, and asked for
/// once just after first paint rather than on demand: nothing opens a panel
/// before clicking a cross-reference, so a lazy version would mean the first
/// click on any document goes nowhere.
///
/// A failure is an error rather than an empty list. A document whose links
/// could not be read is one whose cross-references silently do nothing, which
/// is worth telling a reader rather than leaving them to click.
#[tauri::command]
pub async fn document_links(
    service: tauri::State<'_, RenderService>,
    web: tauri::State<'_, Registry>,
    doc: u32,
) -> Result<links::Links, String> {
    let (reply, rx) = reply_channel();
    service.links(doc, reply);
    let mut answer: links::Links = await_reply("document_links", rx).await?;
    // Before the answer is returned, and that ordering is the whole guarantee:
    // `adopt` **drains** the addresses, so what serializes to the webview is an
    // empty list rather than every URL in the document. See `webopen.rs`.
    web.adopt(doc, Source::Links, &mut answer.urls);
    Ok(answer)
}

/// Opens a web link the reader has confirmed.
///
/// **Takes a token, never an address.** The URL is not a parameter and cannot
/// be: it lives in `webopen::Registry`, which the webview has no route to, and
/// `token` is an index into the scan named by `source`. So the widest thing a
/// caller can ask for is "open the link that was at position N of this
/// document's links", which is an address the document already contained ---
/// and `docs/THREAT-MODEL.md` §T9.1 is the worked-out version of why that
/// matters.
///
/// **The confirmation is the frontend's and is not enforced here**, which is
/// worth stating rather than leaving to be discovered: a script in the webview
/// can reach this command without showing anybody a dialog. That is residual
/// risk 7's shape and not a new hole --- the same script can already print and
/// save --- and it is why the *allowlist* is on this side of the boundary
/// rather than beside the dialog.
///
/// Blocking, on a pool thread. `ShellExecuteW` starting a cold browser takes
/// long enough to be worth keeping off the async runtime's threads, and the
/// macOS arm is a window-server round trip.
#[tauri::command]
pub async fn open_web_link(
    web: tauri::State<'_, Registry>,
    doc: u32,
    source: Source,
    token: u32,
) -> Result<(), String> {
    let Some(address) = web.address(doc, source, token) else {
        // One message for every way a token can name nothing --- see
        // `Registry::address`. A reader who clicks a link in a document that
        // has just been closed and one who meets an entry the boundary check
        // rejected are in the same position: there is nothing to open, and the
        // difference between the two is not theirs to act on.
        return Err("this link is no longer available".into());
    };
    tauri::async_runtime::spawn_blocking(move || opener::open(&address))
        .await
        .map_err(|e| format!("could not reach the system opener: {e}"))?
}

/// Reads what a document says about itself: properties, encryption, signatures.
///
/// Document-level like `document_comments`, and asked for only when a reader
/// opens the dialog --- it is the one `lopdf` parse nothing on the reading path
/// ever needs, so a reader who never asks never pays for it.
///
/// A failure is an error rather than an empty readout. `crate::docinfo` reports
/// what it could not read through its own limits, so an error here means the
/// document could not be parsed at all, which is worth saying.
#[tauri::command]
pub async fn document_properties(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<docinfo::Properties, String> {
    let (reply, rx) = reply_channel();
    service.properties(doc, reply);
    await_reply("document_properties", rx).await
}

/// Reports, per page, whether the text means anything or PDFium is guessing.
///
/// A CID font with no `/ToUnicode` makes PDFium read glyph ids as character
/// codes, so a page comes back with text of the right length, in the right
/// places, that means nothing --- and the reader searching for a word they can
/// plainly see is told there are no matches. `encoding.rs` has the rule.
///
/// **Asked for lazily, and the cost is measured rather than assumed.** 0.1 ms on
/// a small document, 5.8 ms on the 775-page one, 11.9 ms on the 337 MB scan ---
/// `lopdf` reads the xref and object headers, not every stream, so this tracks
/// object count and not file size. Cheap, and still not free: warm startup has
/// ~25 ms of margin against its target, so this is deliberately kept off the
/// critical path rather than done at open. Cached for the document's lifetime.
///
/// In practice the frontend asks on the first frame after open, from the
/// accessibility layer, rather than only after a fruitless search --- a
/// screen-reader user may never search and is the reader least able to tell that
/// what they are being read is nonsense.
#[tauri::command]
pub async fn document_mapping(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<Vec<encoding::PageMapping>, String> {
    let (reply, rx) = reply_channel();
    service.mapping(doc, reply);
    await_reply("document_mapping", rx).await
}

/// Reads the form through the document worker; the coordinator parses no bytes.
#[tauri::command]
pub async fn document_form(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<crate::forms::Form, String> {
    let (reply, rx) = reply_channel();
    service.form(doc, reply);
    await_reply("document_form", rx).await
}

/// Validates a shared field answer before recording it in the document journal.
#[tauri::command]
pub async fn form_fill(
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, crate::edits::Edits>,
    doc: u32,
    object: (u32, u16),
    value: crate::forms::Value,
) -> Result<crate::edits::EditState, String> {
    let (reply, rx) = reply_channel();
    service.form(doc, reply);
    let form: crate::forms::Form = await_reply("form_fill", rx).await?;
    let widgets: Vec<_> = form.widgets.iter().filter(|w| w.object == object).collect();
    if widgets.is_empty() {
        return Err("This field is no longer available".into());
    }
    for widget in widgets {
        crate::forms::validate(widget, &value)?;
    }
    edits.fill(doc, object, value)
}
