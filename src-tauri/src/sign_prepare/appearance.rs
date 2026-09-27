//! What a visible signature looks like, written inside the revision it signs.
//!
//! **The appearance is covered by the signature**, and that is the reason it is
//! built here rather than added afterwards. The widget's `/AP`, its form, the
//! image and the font are objects of the same incremental revision as the
//! signature dictionary, so they sit inside the `/ByteRange` and the digest the
//! app process signs. An appearance appended in a later revision would be a
//! change *after* the signature, which is what a verifier's difference analysis
//! exists to flag.
//!
//! ## What it draws
//!
//! Three lines of Helvetica --- *Digitally signed by*, the certificate's subject
//! name, and the signing time in UTC, which is `/M` read back so the two cannot
//! disagree --- and, when the reader has a saved visual signature (Phase 4's
//! store), that image beside them: on the left of a wide rectangle, above the
//! words in a tall one. Text alone when there is no saved image.
//!
//! Everything is sized to fit the rectangle rather than clipped by it. The type
//! is as large as the widest line and the three lines together allow, up to
//! [`MAX_SIZE`], and is placed so that Helvetica's whole font bounding box ---
//! not only the letters of these lines --- stays inside. A clipping path of the
//! rectangle is written as well, and `/BBox` clips again; those are the floor,
//! not the layout.
//!
//! ## Characters outside WinAnsi are refused, not substituted
//!
//! The words are set in a standard font with `/WinAnsiEncoding`, the choice
//! `textbox.rs` makes for a text box and for the same reason: no font file to
//! embed or subset. A certificate's name is somebody else's text and may be
//! Cyrillic, Greek or Chinese. Drawing it with substitutes --- `?` or a blank ---
//! would put a *different name* on a signature, under the signer's own
//! signature, which is worse than no appearance. So such a name is refused with a
//! message that says the invisible signature still works; the text box makes the
//! same refusal for the same characters.
//!
//! ## It turns with its page
//!
//! A page carrying `/Rotate` is displayed turned, and the appearance is drawn in
//! the reader's frame through [`crate::save::Upright`], the mapping every mark's
//! appearance uses --- one rule for "upright as displayed", not a second copy
//! of it (`docs/TRAPS.md`, *A mark's rectangle survives a quarter turn and
//! everything drawn inside it does not*).

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use crate::pagetree::DisplayedPage;
use crate::save::{winansi_hex, Upright, TEXT_FONT};
use crate::textbox;

/// A visible signature: where the reader put it and what it shows.
///
/// **Crosses the worker boundary** inside `Request::PrepareSignature`. Every
/// field is checked by [`check`] and [`place`] before anything is built,
/// because the worker is where the document is parsed and the rectangle is the
/// document's geometry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Visible {
    /// The page, zero-based, in the file as it is on disk. Signing refuses a
    /// document with unsaved edits, so the file's order is the reader's.
    pub page: u32,
    /// `[left, top, right, bottom]` in points, in the page's **display** space
    /// --- after `/Rotate`, from the displayed box's top-left corner --- which
    /// is the space every rectangle the viewer hands back is measured in.
    pub rect: [f32; 4],
    /// The signer's name as the certificate gives it.
    pub name: String,
    /// The reader's saved visual signature, when there is one.
    pub image: Option<crate::signature::Image>,
}

/// The smallest side a visible signature may have, in points.
///
/// A third of an inch. With an image beside the words, half of the box less
/// the insets is what the words get, and at eight points --- the first value
/// tried --- that was nothing at all: the layout test found a box with no room
/// for text. At this size the three lines are small and present. It is stated
/// once, here, and not copied into the frontend: the viewer's drag separates a
/// click from a drag and no more, and a rectangle below this is refused with
/// this number in the message rather than by a second copy of it that could
/// drift.
pub const MIN_SIDE: f64 = 24.0;

/// The longest name drawn, in characters.
///
/// A common name is at most 64 by X.520; the fallback when a certificate has
/// none is the whole distinguished name, which is longer. This is a bound on
/// work, not a layout rule: a long name is drawn smaller, not cut.
pub const MAX_NAME_CHARS: usize = 256;

/// The largest the words are set, in points.
///
/// A large rectangle is a large mark, not a request for headline type; ten is
/// a little under body text and reads as a caption to the image beside it.
pub const MAX_SIZE: f64 = 10.0;

/// Helvetica's font bounding box, in em: the highest any glyph reaches above
/// its baseline (`Aring` and friends), and the lowest below it.
///
/// From the font's own metrics (`FontBBox [-166 -225 1000 931]`). The lines
/// below are placed so that the whole box fits, which is what makes "inside the
/// rectangle" true of every Latin-1 name rather than of the names tried.
const ASCENT: f64 = 0.931;
const DESCENT: f64 = 0.225;

/// Room left beside each line, in em, for a glyph whose ink passes its advance.
const BEARING: f64 = 0.1;

/// Baseline to baseline, in em. At least `ASCENT + DESCENT`, so no two lines'
/// boxes overlap and the last one's stays inside the block.
const LEADING: f64 = 1.2;

/// The three lines a visible signature says.
///
/// `pdf_date` is the signature dictionary's `/M` exactly as written
/// (`D:YYYYMMDDHHmmSSZ`), so the time on the page is the time in the
/// dictionary by construction.
#[must_use]
pub fn words(name: &str, pdf_date: &str) -> [String; 3] {
    let digits = pdf_date.trim_start_matches("D:");
    let part = |from: usize, to: usize| digits.get(from..to).unwrap_or("??");
    [
        "Digitally signed by".to_string(),
        name.to_string(),
        format!(
            "Date: {}-{}-{} {}:{}:{} UTC",
            part(0, 4),
            part(4, 6),
            part(6, 8),
            part(8, 10),
            part(10, 12),
            part(12, 14)
        ),
    ]
}

/// Refuses a name or an image the appearance cannot honestly draw.
///
/// # Errors
///
/// The name is empty, too long, or holds a character outside the Latin-1 part
/// of WinAnsi (control characters included); or the image is not a valid
/// signature raster.
pub fn check(visible: &Visible) -> Result<(), String> {
    let name = visible.name.as_str();
    if name.trim().is_empty() {
        return Err("the certificate names nobody, so there is no name to draw".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "the certificate's name is longer than the {MAX_NAME_CHARS} characters a visible \
             signature draws --- sign without a visible appearance"
        ));
    }
    if !textbox::encodable(name) || name.chars().any(char::is_control) {
        return Err(format!(
            "the certificate's name, {name}, has characters tpdf cannot draw in a visible \
             signature yet (it draws Latin-1 only), and drawing others would put a different \
             name on the page --- sign without a visible appearance"
        ));
    }
    if visible.image.as_ref().is_some_and(|image| !image.valid()) {
        return Err("the saved signature image is damaged".into());
    }
    Ok(())
}

/// The rectangle in the page's own space, `[llx, lly, urx, ury]`, and the page's
/// quarter turns.
///
/// # Errors
///
/// A coordinate is not a finite number, the rectangle is not on its page, or a
/// side is shorter than [`MIN_SIDE`].
pub fn place(visible: &Visible, shown: DisplayedPage) -> Result<[f64; 4], String> {
    let [left, top, right, bottom] = visible.rect;
    let (width, height) = (f64::from(shown.width), f64::from(shown.height));
    // Half a point of slack, for a drag clamped to the page in display pixels
    // and converted to points with a rounding either way.
    let slack = 0.5;
    let on_page = visible.rect.iter().all(|v| v.is_finite())
        && f64::from(left) >= -slack
        && f64::from(top) >= -slack
        && f64::from(right) <= width + slack
        && f64::from(bottom) <= height + slack;
    if !on_page {
        return Err("the rectangle chosen for the signature is not on its page".into());
    }
    if f64::from(right - left) < MIN_SIDE || f64::from(bottom - top) < MIN_SIDE {
        return Err(format!(
            "the rectangle chosen for the signature is smaller than {MIN_SIDE} points a side, \
             too small to read --- sign again and drag a larger one"
        ));
    }
    let page = crate::text::from_device(shown.turns, shown.width, shown.height, visible.rect);
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    Ok([page[0] + ox, page[1] + oy, page[2] + ox, page[3] + oy])
}

/// Where each thing goes, in the reader's frame: `u` right and `v` down from the
/// rectangle's displayed top-left corner, in points.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The type size, in points.
    pub size: f64,
    /// `(u, baseline v, text)` for each line.
    pub lines: Vec<(f64, f64, String)>,
    /// `[u, v, width, height]` of the image, top-left corner first.
    pub image: Option<[f64; 4]>,
}

/// Lays the words and the image out in a `width` x `height` box as displayed.
///
/// `image` is the raster's pixel size, whose proportions are kept.
#[must_use]
pub fn layout(width: f64, height: f64, image: Option<(u32, u32)>, lines: &[String]) -> Layout {
    let inset = textbox::INSET;
    // [u, v, w, h]. A wide box puts the image on the left and the words on the
    // right; a tall one puts the image above them. Half each, so neither is
    // squeezed by a long name.
    let (text, picture) = match image {
        None => ([0.0, 0.0, width, height], None),
        Some(_) if width >= height => (
            [width / 2.0, 0.0, width / 2.0, height],
            Some([0.0, 0.0, width / 2.0, height]),
        ),
        Some(_) => (
            [0.0, height / 2.0, width, height / 2.0],
            Some([0.0, 0.0, width, height / 2.0]),
        ),
    };
    let placed = image.zip(picture).and_then(|((iw, ih), [u, v, w, h])| {
        let (w, h) = (w - inset * 2.0, h - inset * 2.0);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let (iw, ih) = (f64::from(iw), f64::from(ih));
        let scale = (w / iw).min(h / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        Some([
            u + inset + (w - dw) / 2.0,
            v + inset + (h - dh) / 2.0,
            dw,
            dh,
        ])
    });

    let [tu, tv, tw, th] = text;
    let (tw, th) = (tw - inset * 2.0, th - inset * 2.0);
    let widest = lines
        .iter()
        .map(|line| textbox::advance(line, 1.0))
        .fold(0.0_f64, f64::max);
    let count = lines.len() as f64;
    let size = MAX_SIZE
        .min(tw / (widest + BEARING * 2.0))
        .min(th / (count * LEADING));
    // NaN named as well: no finite box produces one, and one must not reach a
    // content stream if something ever does.
    if size.is_nan() || size <= 0.0 || lines.is_empty() {
        return Layout {
            size: 0.0,
            lines: Vec::new(),
            image: placed,
        };
    }
    // Centred as a block, top to bottom; each line starts one bearing in.
    let top = tv + inset + (th - count * LEADING * size) / 2.0;
    let left = tu + inset + BEARING * size;
    let lines = lines
        .iter()
        .enumerate()
        .map(|(at, line)| {
            let baseline = top + (at as f64) * LEADING * size + ASCENT * size;
            (left, baseline, line.clone())
        })
        .collect();
    Layout {
        size,
        lines,
        image: placed,
    }
}

/// The ink a line of the layout can put down, `[u0, v0, u1, v1]`: its advance
/// plus a bearing each side, and Helvetica's whole box above and below.
///
/// For the tests' bound, and written beside the constants it reads so that the
/// bound and the layout cannot be about different fonts.
#[must_use]
pub fn line_extent(size: f64, u: f64, baseline: f64, text: &str) -> [f64; 4] {
    [
        u - BEARING * size,
        baseline - ASCENT * size,
        u + textbox::advance(text, size) + BEARING * size,
        baseline + DESCENT * size,
    ]
}

/// A rectangle as a PDF array: what `/Rect` and `/BBox` are both written as, so
/// the two are the same numbers rounded the same way.
#[must_use]
pub fn rect_object(rect: [f64; 4]) -> Object {
    Object::Array(rect.iter().map(|v| Object::Real(*v as f32)).collect())
}

/// Builds the appearance form for a widget whose rectangle is `rect` (page
/// space) on a page turned `turns` quarters, and adds it and what it uses to
/// `doc`.
pub fn stream(
    doc: &mut Document,
    rect: [f64; 4],
    turns: u8,
    lines: &[String],
    image: Option<&crate::signature::Image>,
) -> ObjectId {
    let seen = Upright::of(turns, rect);
    let layout = layout(
        seen.width,
        seen.height,
        image.map(|image| (image.width, image.height)),
        lines,
    );
    let [x0, y0, x1, y1] = rect;
    let mut content = format!("q {x0} {y0} {} {} re W n\n", x1 - x0, y1 - y0);
    let mut resources = Dictionary::new();
    if let (Some(image), Some([u, v, w, h])) = (image, layout.image) {
        let object = image.xobject(doc);
        resources.set("XObject", dictionary! { "Signature" => object });
        // The image's unit square onto the reader's box: its bottom-left
        // corner, then one step along its bottom edge and one up its left.
        let (ex, ey) = seen.at(u, v + h);
        let (ax, ay) = seen.at(u + w, v + h);
        let (cx, cy) = seen.at(u, v);
        content.push_str(&format!(
            "q {} {} {} {} {ex} {ey} cm /Signature Do Q\n",
            ax - ex,
            ay - ey,
            cx - ex,
            cy - ey
        ));
    }
    if !layout.lines.is_empty() {
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut fonts = Dictionary::new();
        fonts.set(TEXT_FONT, Object::Reference(font));
        resources.set("Font", Object::Dictionary(fonts));
        content.push_str(&format!("0 g BT /{TEXT_FONT} {} Tf\n", layout.size));
        for (u, v, line) in &layout.lines {
            content.push_str(&format!("{}\n", seen.text_matrix(*u, *v)));
            content.push_str(&format!("<{}> Tj\n", winansi_hex(line)));
        }
        content.push_str("ET\n");
    }
    content.push_str("Q\n");

    let mut form = Dictionary::new();
    form.set("Type", Object::Name(b"XObject".to_vec()));
    form.set("Subtype", Object::Name(b"Form".to_vec()));
    form.set("FormType", Object::Integer(1));
    form.set("BBox", rect_object(rect));
    form.set("Resources", Object::Dictionary(resources));
    doc.add_object(Stream::new(form, content.into_bytes()))
}
