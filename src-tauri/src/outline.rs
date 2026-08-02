//! The document outline --- what a PDF calls bookmarks and a reader calls a
//! table of contents.
//!
//! ## This is a hostile tree, and PDFium says so itself
//!
//! `FPDFBookmark_GetNextSibling`'s own documentation reads: *"the caller is
//! responsible for handling circular bookmark references, as may arise from
//! malformed documents."* That is not a hypothetical being guarded against on
//! principle --- it is the library handing the problem over, and a walker
//! written the obvious way (`while let Some(next) = sibling(node)`) hangs the
//! render thread forever on the first file that has one. `testdata/make_outline_pdf.py`
//! builds two of them; `qpdf --check` independently calls the result *"loop
//! detected in /Outlines tree"*.
//!
//! So the walk carries three independent bounds, and the reason there are three
//! is that each catches something the others do not:
//!
//! - **A visited set** stops a cycle at its first repeat, which is what keeps
//!   the *rest* of the outline reachable. Aborting the level instead would lose
//!   every entry after the loop.
//! - **A depth bound** stops a chain that is deep without ever repeating ---
//!   nothing in the visited set fires on 200 distinct nested nodes.
//! - **An item budget** stops everything else, including the case where the
//!   visited set is defeated because PDFium hands back a fresh pointer for a
//!   node we have already seen. The set is the mechanism we expect to work; the
//!   budget is what makes termination not depend on that expectation.
//!
//! Whatever any of them cut is *counted and reported* ([`Limits`]) rather than
//! silently dropped. A truncated outline shown as if it were complete is the
//! same class of failure as a leak scanner reporting clean on a carrier it
//! could not decode.
//!
//! One honest cost of the visited set being global rather than per-path: a
//! document that legitimately shares one subtree between two parents renders it
//! under the first parent only, and the sharing is counted as a cycle. No
//! producer does this, and the alternative --- a per-path set --- catches an
//! ancestor loop but not a *sibling* loop, which is the shape the fixture and
//! the qpdf warning are about.
//!
//! ## Actions are refused, not followed
//!
//! An outline entry can carry an action instead of a destination, and the
//! action can be `/Launch`, `/URI` or `/GoToR`. `docs/THREAT-MODEL.md` disables
//! launch actions by default; this is where an outline click would otherwise
//! reach one. Only `/GoTo` --- a destination inside this document --- is
//! resolved. The rest become [`Target::Refused`] carrying *which* kind it was,
//! so the sidebar can say why an entry does nothing rather than looking broken.
//!
//! ## Titles are attacker-controlled strings
//!
//! They arrive as UTF-16LE from `FPDFBookmark_GetTitle`, and nothing about the
//! format stops one being 50,000 characters of newlines. [`decode_title`] and
//! [`sanitize_title`] are pure and unit-tested here, because the interesting
//! inputs --- an unpaired surrogate, an odd byte count, an embedded NUL --- are
//! ones a fixture cannot reliably deliver through PDFium's own string parsing.

use std::collections::{HashMap, HashSet};
use std::os::raw::{c_int, c_ulong, c_void};

use pdfium_render::prelude::*;

use crate::progressive::{Bindings, RawDocument};

/// Deepest nesting the walk will descend.
///
/// Real outlines are rarely past six. The bound is here for the 200-level chain
/// in the hostile fixture, and for the ancestor loop that would otherwise
/// descend forever if the visited set ever failed to fire.
const MAX_DEPTH: usize = 32;

/// Most entries the walk will produce.
///
/// The list is rendered as plain DOM rather than virtualized (see
/// `src/lib/sidebar.ts`), so this is simultaneously a termination bound and the
/// row count the sidebar has to stay responsive at. Bounding the input is the
/// honest version of a lazy renderer that does not exist.
const MAX_ITEMS: usize = 10_000;

/// Longest title kept, in characters.
const MAX_TITLE_CHARS: usize = 300;

/// Largest title buffer that will be allocated, in bytes.
///
/// `FPDFBookmark_GetTitle` reports the length it needs and writes nothing if
/// the buffer is smaller, so an oversized title cannot be read *partly* --- the
/// choice is to allocate what it asks for or to decline. A title beyond this is
/// declined and marked clipped rather than allowed to size an allocation from
/// the file.
const MAX_TITLE_BYTES: usize = 1 << 20;

/// Action types from `fpdf_doc.h`.
mod action {
    use std::os::raw::c_ulong;

    pub const GOTO: c_ulong = 1;
    pub const REMOTEGOTO: c_ulong = 2;
    pub const URI: c_ulong = 3;
    pub const LAUNCH: c_ulong = 4;
    pub const EMBEDDEDGOTO: c_ulong = 5;
}

/// Where an outline entry points, or why it points nowhere.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// A page in this document, and how far down it, in points from the page's
    /// top edge. `top_pt` is absent for destinations like `/Fit` that name no
    /// coordinate.
    Page { page: u32, top_pt: Option<f32> },
    /// A destination that resolves to no page this document has.
    Broken,
    /// An action tpdf declines to follow. `action` names which kind.
    Refused { action: String },
    /// No destination and no action at all --- a heading that is only a heading.
    None,
}

/// One entry, and everything under it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OutlineItem {
    pub title: String,
    /// Whether the producer marked this subtree open, from the sign of
    /// `FPDFBookmark_GetCount`.
    pub open: bool,
    pub target: Target,
    pub children: Vec<OutlineItem>,
}

/// What the bounds cut off, so the UI can say the outline is incomplete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// Entries skipped because they had already been visited.
    pub cycles: usize,
    /// Subtrees dropped at [`MAX_DEPTH`].
    pub too_deep: usize,
    /// Whether [`MAX_ITEMS`] was reached, so the tree is cut short.
    pub over_budget: bool,
    /// Titles shortened, either clipped to [`MAX_TITLE_CHARS`] or declined at
    /// [`MAX_TITLE_BYTES`].
    pub titles_clipped: usize,
}

impl Limits {
    /// Whether anything was cut. The UI shows a warning on exactly this.
    pub fn any(&self) -> bool {
        self.cycles > 0 || self.too_deep > 0 || self.over_budget || self.titles_clipped > 0
    }
}

/// A document's outline.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Outline {
    pub items: Vec<OutlineItem>,
    /// Entries produced, at every depth. Zero means the document has no outline.
    pub total: usize,
    pub limits: Limits,
    /// Time spent walking, in milliseconds.
    pub walk_ms: f64,
}

/// State threaded through the walk. Separate from [`Outline`] because the
/// visited set and the page-height cache are not part of the answer.
struct Walk<'doc> {
    bindings: Bindings,
    document: FPDF_DOCUMENT,
    page_count: u32,
    seen: HashSet<usize>,
    sizes: HashMap<u32, (f32, f32)>,
    /// Quarter-turns per page, filled only for pages a destination names with
    /// coordinates. See [`Walk::turns_of`].
    turns: HashMap<u32, u8>,
    limits: Limits,
    total: usize,
    _borrow: &'doc RawDocument,
}

/// Reads a document's outline.
pub fn read(document: &RawDocument) -> Outline {
    let started = std::time::Instant::now();
    let mut walk = Walk {
        bindings: document.bindings(),
        document: document.handle(),
        page_count: document.page_count(),
        seen: HashSet::new(),
        sizes: HashMap::new(),
        turns: HashMap::new(),
        limits: Limits::default(),
        total: 0,
        _borrow: document,
    };

    // SAFETY: a null bookmark asks for the first top-level entry, which is what
    // `FPDFBookmark_GetFirstChild` documents.
    let first = unsafe {
        walk.bindings
            .FPDFBookmark_GetFirstChild(walk.document, std::ptr::null_mut())
    };
    let items = walk.siblings(first, 0);

    Outline {
        items,
        total: walk.total,
        limits: walk.limits,
        walk_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

impl Walk<'_> {
    /// Walks a sibling chain and everything under it.
    ///
    /// Stops at the first repeat rather than abandoning the level, so a cycle
    /// costs the entries inside the loop and not the ones after it.
    fn siblings(&mut self, first: FPDF_BOOKMARK, depth: usize) -> Vec<OutlineItem> {
        let mut items = Vec::new();
        let mut node = first;

        while !node.is_null() {
            if self.total >= MAX_ITEMS {
                self.limits.over_budget = true;
                break;
            }
            if !self.seen.insert(node as usize) {
                self.limits.cycles += 1;
                break;
            }

            self.total += 1;
            items.push(self.item(node, depth));

            // SAFETY: `node` is a live bookmark handle owned by the document,
            // which outlives this walk.
            node = unsafe {
                self.bindings
                    .FPDFBookmark_GetNextSibling(self.document, node)
            };
        }

        items
    }

    /// Builds one entry, descending into it unless the depth bound says not to.
    fn item(&mut self, node: FPDF_BOOKMARK, depth: usize) -> OutlineItem {
        let raw = decode_title(&self.title(node));
        let (title, clipped) = sanitize_title(&raw, MAX_TITLE_CHARS);
        if clipped {
            self.limits.titles_clipped += 1;
        }

        // SAFETY: as above. A positive count means the producer wants the
        // subtree open; the magnitude is the visible-descendant count and is
        // not used here.
        let count = unsafe { self.bindings.FPDFBookmark_GetCount(node) };

        // SAFETY: as above.
        let first = unsafe {
            self.bindings
                .FPDFBookmark_GetFirstChild(self.document, node)
        };
        let children = if first.is_null() {
            Vec::new()
        } else if depth + 1 >= MAX_DEPTH {
            self.limits.too_deep += 1;
            Vec::new()
        } else {
            self.siblings(first, depth + 1)
        };

        OutlineItem {
            title,
            open: count > 0,
            target: self.target(node),
            children,
        }
    }

    /// Reads a title, in bytes, as PDFium hands it over.
    fn title(&mut self, node: FPDF_BOOKMARK) -> Vec<u8> {
        // SAFETY: the documented two-call form --- a null buffer asks for the
        // length in bytes, including the terminating UTF-16 NUL.
        let needed = unsafe {
            self.bindings
                .FPDFBookmark_GetTitle(node, std::ptr::null_mut(), 0)
        } as usize;

        if needed <= 2 {
            return Vec::new();
        }
        if needed > MAX_TITLE_BYTES {
            // Declined rather than truncated: PDFium writes nothing at all when
            // the buffer is short, so there is no prefix to be had.
            self.limits.titles_clipped += 1;
            return Vec::new();
        }

        let mut buffer = vec![0u8; needed];
        // SAFETY: `buffer` is `needed` writable bytes, which is exactly what the
        // call above asked for.
        let written = unsafe {
            self.bindings.FPDFBookmark_GetTitle(
                node,
                buffer.as_mut_ptr() as *mut c_void,
                needed as c_ulong,
            )
        } as usize;
        buffer.truncate(written.min(needed));
        buffer
    }

    /// Resolves an entry's destination, or records why it has none.
    ///
    /// **The action is read first, and that ordering is load-bearing.**
    /// `FPDFBookmark_GetDest` is not the narrow accessor its name suggests: when
    /// a bookmark has no `/Dest` it silently falls back to the bookmark's
    /// *action* and returns that action's `/D` array, **without checking the
    /// action's type**. So a `/GoToR` entry --- "open other.pdf at page 1" ---
    /// comes back from it as a perfectly ordinary destination, which then
    /// resolves against *this* document and jumps to an unrelated page of the
    /// file the reader already has open. Measured on the hostile fixture, which
    /// reported `page 1` where it should have reported a refusal.
    ///
    /// Asking for the action first removes the fallback's only opportunity to
    /// fire: it is reached only when there is no action, and a bookmark with no
    /// action is one whose `/Dest` is the whole story. It also matches PDF
    /// 32000-1 §12.3.3, which says `/Dest` shall not be present when `/A` is.
    fn target(&mut self, node: FPDF_BOOKMARK) -> Target {
        // SAFETY: live bookmark handle; null means the entry has no /A.
        let action = unsafe { self.bindings.FPDFBookmark_GetAction(node) };

        let dest = if action.is_null() {
            // SAFETY: as above. With no action there is nothing for PDFium's
            // fallback to reach, so this returns /Dest or nothing.
            unsafe { self.bindings.FPDFBookmark_GetDest(self.document, node) }
        } else {
            // SAFETY: as above.
            let kind = unsafe { self.bindings.FPDFAction_GetType(action) };
            match kind {
                // SAFETY: the type check is what makes this call legal ---
                // `FPDFAction_GetDest` documents that the action must be a
                // GoTo or RemoteGoto, and only the first is ours to follow.
                action::GOTO => unsafe { self.bindings.FPDFAction_GetDest(self.document, action) },
                action::REMOTEGOTO => return refused("remote"),
                action::URI => return refused("uri"),
                action::LAUNCH => return refused("launch"),
                action::EMBEDDEDGOTO => return refused("embedded"),
                _ => return refused("unsupported"),
            }
        };

        if dest.is_null() {
            // An action that claimed to be a GoTo and carried no destination is
            // broken; a bookmark with neither is simply a heading.
            return if action.is_null() {
                Target::None
            } else {
                Target::Broken
            };
        }

        // SAFETY: `dest` is non-null here and belongs to the document.
        let index = unsafe { self.bindings.FPDFDest_GetDestPageIndex(self.document, dest) };
        if index < 0 || index as u32 >= self.page_count {
            return Target::Broken;
        }
        let page = index as u32;

        Target::Page {
            page,
            top_pt: self.top_of(dest, page),
        }
    }

    /// The `/XYZ` y coordinate of a destination, flipped into device space.
    ///
    /// PDFium reports it in page space --- y upwards from the bottom-left ---
    /// and every consumer downstream works from the page's top edge. Getting it
    /// backwards does not look like a bug: it still scrolls to the right *page*,
    /// just to the mirror image of the right place on it.
    ///
    /// It is the same conversion `text.rs` does for character boxes and it goes
    /// through the same function, which is the point: a destination on a page
    /// carrying `/Rotate 90` is reported in the page's own **unrotated** space
    /// while `FPDF_GetPageSizeByIndexF` reports the **displayed** size --- so the
    /// naive flip is wrong there in exactly the way it was wrong for character
    /// boxes, and a second implementation of the turn would be a second place to
    /// get it wrong.
    ///
    /// Note what that costs: for a quarter or three-quarter turn the display's
    /// vertical axis is the page's *horizontal* one, so a destination that names
    /// no x cannot be placed at all and this returns `None` --- the page's top,
    /// which is what a destination with no coordinate means anyway.
    fn top_of(&mut self, dest: FPDF_DEST, page: u32) -> Option<f32> {
        let (mut has_x, mut has_y, mut has_zoom) = (0, 0, 0);
        let (mut x, mut y, mut zoom) = (0f32, 0f32, 0f32);

        // SAFETY: six writable out-parameters, all live for the call.
        let ok = unsafe {
            self.bindings.FPDFDest_GetLocationInPage(
                dest,
                &mut has_x,
                &mut has_y,
                &mut has_zoom,
                &mut x,
                &mut y,
                &mut zoom,
            )
        };
        if ok == 0 || has_y == 0 {
            return None;
        }

        let (width, height) = self.size_of(page)?;
        let turns = self.turns_of(page);
        if turns % 2 == 1 && has_x == 0 {
            return None;
        }

        // A degenerate box, so the one mapping in `text.rs` places this too. Its
        // `top` is the display's vertical coordinate under every turn.
        let placed = crate::text::to_device(
            turns,
            width,
            height,
            [x as f64, y as f64, x as f64, y as f64],
        );
        // Clamped because the coordinates are read from the file: a destination
        // at y = -1e9 would otherwise scroll to a place the document does not
        // have.
        Some(placed[1].clamp(0.0, height))
    }

    /// Quarter-turns a page is displayed rotated by, cached.
    ///
    /// This is the one place in the walk that loads a page, and it is why it is
    /// asked for lazily: `FPDFPage_GetRotation` needs an `FPDF_PAGE`, while
    /// everything else here reads the page dictionary through
    /// `FPDF_GetPageSizeByIndexF` and never pays a load at all. Only a
    /// destination that actually names coordinates reaches this, and the answer
    /// is cached per page --- so an outline of headings all pointing at the top
    /// of their page still loads nothing.
    ///
    /// A page that cannot be loaded is treated as unrotated, which is what it
    /// was before this existed.
    fn turns_of(&mut self, page: u32) -> u8 {
        if let Some(&turns) = self.turns.get(&page) {
            return turns;
        }
        let turns = self
            ._borrow
            .page(page)
            .map(|loaded| loaded.quarter_turns())
            .unwrap_or(0);
        self.turns.insert(page, turns);
        turns
    }

    /// A page's displayed size in points, cached for the walk.
    ///
    /// `FPDF_GetPageSizeByIndexF` reads the page dictionary's boxes rather than
    /// loading the page, which matters because an outline can name hundreds of
    /// pages and `FPDF_LoadPage` costs 44 ms apiece on a complex one. Measured
    /// 2026-07-27: it reports the size **after** `/Rotate`, the same as
    /// `FPDF_GetPageWidthF` --- 792x612 for a rotated letter page, not 612x792.
    fn size_of(&mut self, page: u32) -> Option<(f32, f32)> {
        if let Some(&size) = self.sizes.get(&page) {
            return Some(size);
        }

        let mut size = FS_SIZEF {
            width: 0.0,
            height: 0.0,
        };
        // SAFETY: `size` is a live writable `FS_SIZEF`, and the index was
        // bounds-checked against the page count by the caller.
        let ok = unsafe {
            self.bindings
                .FPDF_GetPageSizeByIndexF(self.document, page as c_int, &mut size)
        };
        // `is_finite` as well as positive: a MediaBox read from the file can be
        // NaN, and NaN passes `> 0.0` being false either way while poisoning
        // every subtraction downstream.
        if ok == 0
            || !size.height.is_finite()
            || size.height <= 0.0
            || !size.width.is_finite()
            || size.width <= 0.0
        {
            return None;
        }

        self.sizes.insert(page, (size.width, size.height));
        Some((size.width, size.height))
    }
}

/// Builds a refusal, spelling out which action kind was declined.
fn refused(kind: &str) -> Target {
    Target::Refused {
        action: kind.to_string(),
    }
}

/// Decodes a UTF-16LE title as PDFium returns it.
///
/// Three things are deliberate. An odd trailing byte is dropped rather than
/// failing the title --- one bad byte should not cost the whole heading. An
/// unpaired surrogate becomes U+FFFD for the same reason, which is what
/// `String::from_utf16` would *not* do: it returns `Err` for the whole string,
/// so one malformed code unit anywhere would blank an otherwise readable title.
/// And the terminating NUL is dropped, along with any others: PDF strings may
/// carry embedded NULs, and a `String` containing one renders as a gap that
/// nothing downstream can see.
pub fn decode_title(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();

    char::decode_utf16(units)
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Flattens a title into something a single-line tree row can show.
///
/// Returns the title and whether it was shortened. Control characters become
/// spaces rather than being dropped, so `"Chapter\u{7}One"` stays two words;
/// runs of whitespace collapse, because a title carrying forty newlines is
/// forty blanks in a row otherwise.
pub fn sanitize_title(title: &str, limit: usize) -> (String, bool) {
    let mut out = String::new();
    let mut pending_space = false;
    let mut clipped = false;

    for ch in title.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if ch == ' ' || ch.is_whitespace() {
            // Held rather than pushed, so trailing whitespace never survives
            // and a run costs one space wherever it falls.
            pending_space = !out.is_empty();
            continue;
        }
        if out.chars().count() >= limit {
            clipped = true;
            break;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }

    (out, clipped)
}

/// Reads and decodes a title in one step. Split out so the decode is testable.
#[cfg(test)]
fn read_title(bytes: &[u8], limit: usize) -> (String, bool) {
    sanitize_title(&decode_title(bytes), limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-16LE bytes for a string, with the terminating NUL PDFium appends.
    fn utf16le(value: &str) -> Vec<u8> {
        let mut bytes: Vec<u8> = value
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    #[test]
    fn a_title_decodes_from_utf16le() {
        assert_eq!(decode_title(&utf16le("Introduction")), "Introduction");
    }

    #[test]
    fn a_title_keeps_characters_outside_latin1() {
        assert_eq!(decode_title(&utf16le("第三章 Ü")), "第三章 Ü");
    }

    #[test]
    fn a_title_keeps_astral_characters() {
        // Two UTF-16 code units for one scalar, which is the case a decoder
        // that walks units rather than pairs gets wrong.
        assert_eq!(decode_title(&utf16le("Deeper 𝄞")), "Deeper 𝄞");
        assert_eq!(decode_title(&utf16le("𝄞")).chars().count(), 1);
    }

    #[test]
    fn an_unpaired_surrogate_becomes_one_replacement_character() {
        // A lone high surrogate, then 'A'. `String::from_utf16` fails the whole
        // string here; the rest of the title has to survive.
        let bytes = [0x00, 0xd8, 0x41, 0x00, 0x00, 0x00];
        assert_eq!(decode_title(&bytes), "\u{fffd}A");
    }

    #[test]
    fn the_terminating_nul_is_dropped() {
        assert_eq!(decode_title(&utf16le("Appendix")), "Appendix");
        assert!(!decode_title(&utf16le("Appendix")).contains('\0'));
    }

    #[test]
    fn an_embedded_nul_ends_the_title() {
        // PDFium reports a byte count, not a string length, so a title with a
        // NUL in the middle arrives with content after it. Taking the whole
        // buffer would put a character in the string that no row can render.
        let mut bytes = utf16le("Visible");
        bytes.extend_from_slice(&utf16le("Hidden"));
        assert_eq!(decode_title(&bytes), "Visible");
    }

    #[test]
    fn an_odd_trailing_byte_is_dropped_not_fatal() {
        let mut bytes = utf16le("Odd");
        bytes.push(0x41);
        assert_eq!(decode_title(&bytes), "Odd");
    }

    #[test]
    fn an_empty_buffer_decodes_to_an_empty_title() {
        assert_eq!(decode_title(&[]), "");
        assert_eq!(decode_title(&[0, 0]), "");
    }

    #[test]
    fn control_characters_become_spaces() {
        // Not dropped: "Chapter" and "One" are two words and must stay two.
        assert_eq!(sanitize_title("Chapter\u{7}One", 300).0, "Chapter One");
        assert_eq!(sanitize_title("Line\nbreak", 300).0, "Line break");
    }

    #[test]
    fn whitespace_runs_collapse_and_the_edges_are_trimmed() {
        assert_eq!(sanitize_title("  a \n\t\r  b  ", 300).0, "a b");
    }

    #[test]
    fn a_long_title_is_clipped_and_says_so() {
        let (title, clipped) = sanitize_title(&"H".repeat(5000), 300);
        assert_eq!(title.chars().count(), 300);
        assert!(clipped);
    }

    #[test]
    fn a_short_title_is_not_reported_as_clipped() {
        // The control for the test above: a flag that is always set carries no
        // information, and this is what distinguishes the two.
        let (title, clipped) = sanitize_title("Introduction", 300);
        assert_eq!(title, "Introduction");
        assert!(!clipped);
    }

    #[test]
    fn the_limit_counts_characters_not_bytes() {
        // Three bytes each in UTF-8. A byte-based clip would cut this to 100
        // characters, or worse, mid-scalar.
        let (title, _) = sanitize_title(&"第".repeat(500), 300);
        assert_eq!(title.chars().count(), 300);
    }

    #[test]
    fn clipping_does_not_count_the_trailing_space_of_a_run() {
        // A title ending in whitespace at exactly the limit must not report a
        // clip: nothing readable was lost.
        let (title, clipped) = sanitize_title("abc   ", 3);
        assert_eq!(title, "abc");
        assert!(!clipped);
    }

    #[test]
    fn a_whitespace_only_title_is_empty_rather_than_blank() {
        assert_eq!(sanitize_title(" \n\t ", 300), (String::new(), false));
    }

    #[test]
    fn decoding_and_sanitizing_compose() {
        let bytes = utf16le("  第三章 \u{7} Deep  ");
        assert_eq!(read_title(&bytes, 300), ("第三章 Deep".to_string(), false));
    }

    #[test]
    fn limits_report_nothing_when_nothing_was_cut() {
        assert!(!Limits::default().any());
        assert!(Limits {
            cycles: 1,
            ..Default::default()
        }
        .any());
        assert!(Limits {
            over_budget: true,
            ..Default::default()
        }
        .any());
    }

    /// No [`Target`] variant may carry a URL, and adding one must not compile.
    ///
    /// This is the Rust half of a two-sided invariant, and the halves cannot see
    /// each other. `scripts/check_webview_sinks.py` proves the frontend has no
    /// way to turn a string into markup, a navigation or a script; that proof is
    /// *sufficient* only because no attacker-controlled URL ever reaches the
    /// frontend to be turned into anything. This is what makes that true:
    /// `/URI`, `/Launch` and `/GoToR` become [`Target::Refused`], whose `action`
    /// is one of five literals chosen here rather than anything the document
    /// said.
    ///
    /// The match below is **exhaustive and deliberately not a wildcard**. A new
    /// variant --- `Target::Uri { url: String }`, say --- is a compile error
    /// here, which is the strongest verdict available: the mistake cannot be
    /// made and then caught, it cannot be made. `AGENTS.md` records the
    /// preference for moving an impossibility into the type over guarding it at
    /// runtime, and a non-exhaustive match is that in its cheapest form.
    ///
    /// If a variant genuinely must carry a URL one day, this test failing to
    /// compile is the notice that `docs/THREAT-MODEL.md` T8 and the sinks gate
    /// both need revisiting first.
    #[test]
    fn no_target_variant_may_carry_a_url() {
        /// Every field of every variant, as the frontend would receive it.
        fn fields(target: &Target) -> Vec<String> {
            match target {
                // Numbers. A page index and an offset cannot be a URL.
                Target::Page { page, top_pt } => {
                    vec![page.to_string(), format!("{top_pt:?}")]
                }
                // Unit variants carry nothing at all.
                Target::Broken | Target::None => vec![],
                // The one string, and it is ours.
                Target::Refused { action } => vec![action.clone()],
            }
        }

        // Every refusal this module can build, by construction rather than by
        // listing them again: these are the five `refused()` call sites.
        let refusals = ["remote", "uri", "launch", "embedded", "unsupported"];
        for kind in refusals {
            let Target::Refused { action } = refused(kind) else {
                panic!("refused() must build a Refused");
            };
            assert_eq!(
                action, kind,
                "the action name must be ours, not the document's"
            );
            assert!(
                !action.contains(':') && !action.contains('/'),
                "an action name that can hold a scheme or a path is a URL in disguise: {action:?}"
            );
        }

        // And the walk never produces a target carrying anything URL-shaped.
        for target in [
            Target::Page {
                page: 3,
                top_pt: Some(1.0),
            },
            Target::Broken,
            Target::None,
            refused("uri"),
        ] {
            for field in fields(&target) {
                assert!(
                    !field.contains("://") && !field.starts_with("javascript:"),
                    "a Target field reached the frontend looking like a URL: {field:?}"
                );
            }
        }
    }
}
