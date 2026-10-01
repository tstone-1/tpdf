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
//! Up to five lines of Helvetica, each one the reader's choice ([`Options`]):
//! *Digitally signed by*, the certificate's subject name, the signing time in
//! UTC --- which is `/M` read back so the two cannot disagree --- and, when the
//! reader typed them, *Reason:* and *Location:*, which are the signature
//! dictionary's `/Reason` and `/Location` drawn. Beside them, when the reader
//! chose one, an image (Phase 4's saved visual signature, or one drawn for this
//! signing): on the left of a wide rectangle, above the words in a tall one, and
//! the whole rectangle when no line is on. **Something must be drawn**: a
//! visible signature with no image and no line is refused, because an empty
//! rectangle on a page says nothing and looks like a mistake.
//!
//! Everything is sized to fit the rectangle rather than clipped by it. The type
//! is as large as the widest line and the lines together allow, up to
//! [`MAX_SIZE`], and is placed so that Helvetica's whole font bounding box ---
//! not only the letters of these lines --- stays inside. A clipping path of the
//! rectangle is written as well, and `/BBox` clips again; those are the floor,
//! not the layout.
//!
//! ## The form is drawn at the origin, so a preview is the same bytes
//!
//! The form's `/BBox` is `[0 0 w h]` and everything inside it is drawn in that
//! box's own space; PDF 32000-1 §12.5.5 maps the box onto the widget's `/Rect`
//! wherever that is on the page. So the appearance stream depends on the
//! rectangle's **size**, the page's turns, the options, the image and the time,
//! and not on where the rectangle sits. That is what lets [`super::preview`]
//! render, before anything is signed, exactly the stream the signing writes:
//! `a_preview_draws_the_stream_the_signing_writes` compares the two byte for
//! byte at several positions.
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
    /// The image the reader chose, when they chose one.
    pub image: Option<crate::signature::Image>,
    /// Which lines are drawn, and the reason and location.
    ///
    /// Defaulted, so a request written before the reader could choose parses as
    /// the three lines it drew then and no reason or location.
    #[serde(default)]
    pub options: Options,
}

/// What the reader chose to show, beside or instead of an image.
///
/// **Crosses the worker boundary** inside [`Visible`], and from the frontend
/// inside `commands::sign::Placement`. The signer's name is not here and is not
/// editable: [`Visible::name`] is read from the certificate by the app process,
/// and `name` below only says whether it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Options {
    /// *Digitally signed by*.
    pub label: bool,
    /// The certificate's subject name.
    pub name: bool,
    /// The signing time, `/M` read back.
    pub date: bool,
    /// Why the document is signed, or blank for none. Written as `/Reason` and
    /// drawn as *Reason: ...*.
    pub reason: String,
    /// Where it was signed, or blank for none. Written as `/Location` and drawn
    /// as *Location: ...*.
    pub location: String,
    /// Write the reason and do not draw it: `sign --hide reason`, for an
    /// appearance that is an image alone. Off wherever it is not said.
    pub hide_reason: bool,
    /// The same for the location.
    pub hide_location: bool,
}

impl Default for Options {
    /// The three lines a visible signature drew before there was a choice.
    fn default() -> Self {
        Self {
            label: true,
            name: true,
            date: true,
            reason: String::new(),
            location: String::new(),
            hide_reason: false,
            hide_location: false,
        }
    }
}

impl Options {
    /// The reason, trimmed, or `None` when there is nothing in it.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        Some(self.reason.trim()).filter(|text| !text.is_empty())
    }

    /// The location, trimmed, or `None` when there is nothing in it.
    #[must_use]
    pub fn location(&self) -> Option<&str> {
        Some(self.location.trim()).filter(|text| !text.is_empty())
    }

    /// The reason as a line of the appearance: none when it is hidden.
    #[must_use]
    pub fn drawn_reason(&self) -> Option<&str> {
        self.reason().filter(|_| !self.hide_reason)
    }

    /// The location as a line of the appearance: none when it is hidden.
    #[must_use]
    pub fn drawn_location(&self) -> Option<&str> {
        self.location().filter(|_| !self.hide_location)
    }
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

/// The longest reason or location, in characters.
///
/// The same bound as [`MAX_NAME_CHARS`] and for the same reason: it bounds
/// work, and a long one is drawn smaller rather than cut.
pub const MAX_NOTE_CHARS: usize = 256;

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
pub(crate) const BEARING: f64 = 0.1;

/// Baseline to baseline, in em. At least `ASCENT + DESCENT`, so no two lines'
/// boxes overlap and the last one's stays inside the block.
pub(crate) const LEADING: f64 = 1.2;

/// The lines a visible signature says, in the order they are drawn.
///
/// `pdf_date` is the signature dictionary's `/M` exactly as written
/// (`D:YYYYMMDDHHmmSSZ`), so the time on the page is the time in the
/// dictionary by construction. A reason or location is drawn with the words
/// that say what it is, because a bare "Berlin" under a signature could be
/// anything.
#[must_use]
pub fn words(name: &str, pdf_date: &str, options: &Options) -> Vec<String> {
    let digits = pdf_date.trim_start_matches("D:");
    let part = |from: usize, to: usize| digits.get(from..to).unwrap_or("??");
    let mut lines = Vec::with_capacity(5);
    if options.label {
        lines.push("Digitally signed by".to_string());
    }
    if options.name {
        lines.push(name.to_string());
    }
    if options.date {
        lines.push(format!(
            "Date: {}-{}-{} {}:{}:{} UTC",
            part(0, 4),
            part(4, 6),
            part(6, 8),
            part(8, 10),
            part(10, 12),
            part(12, 14)
        ));
    }
    if let Some(reason) = options.drawn_reason() {
        lines.push(format!("Reason: {reason}"));
    }
    if let Some(location) = options.drawn_location() {
        lines.push(format!("Location: {location}"));
    }
    lines
}

/// Refuses free text the appearance cannot draw, or the dictionary should not
/// carry: too long, a control character, or a character Helvetica with
/// `/WinAnsiEncoding` has no glyph for.
fn check_note(what: &str, text: Option<&str>) -> Result<(), String> {
    let Some(text) = text else { return Ok(()) };
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(format!(
            "the {what} is longer than the {MAX_NOTE_CHARS} characters a visible signature draws"
        ));
    }
    if text.chars().any(char::is_control) || !textbox::encodable(text) {
        return Err(format!(
            "the {what}, {text}, has characters tpdf cannot draw in a visible signature yet \
             (it draws Latin-1 only), and drawing others would put different words on the \
             page --- change the {what}, or leave it empty"
        ));
    }
    Ok(())
}

/// Refuses a name, a reason, a location or an image the appearance cannot
/// honestly draw, and an appearance that would draw nothing.
///
/// The name is checked only when it is drawn: a reader whose certificate names
/// them in a script tpdf cannot draw can still sign visibly with the name line
/// off, and the name is not written anywhere else by this revision.
///
/// # Errors
///
/// Nothing is drawn; the name is drawn and is empty, too long, or holds a
/// character outside the Latin-1 part of WinAnsi (control characters included);
/// the reason or location is too long or holds such a character; or the image
/// is not a valid signature raster.
pub fn check(visible: &Visible) -> Result<(), String> {
    let options = &visible.options;
    let lines = options.label
        || options.name
        || options.date
        || options.drawn_reason().is_some()
        || options.drawn_location().is_some();
    if !lines && visible.image.is_none() {
        return Err(
            "a visible signature has to show something --- choose an image or at least one \
             line of text"
                .into(),
        );
    }
    if options.name {
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
                 name on the page --- turn the name line off, or sign without a visible appearance"
            ));
        }
    }
    check_note("reason", options.reason())?;
    check_note("location", options.location())?;
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
        // No words, so the image has the whole box.
        Some(_) if lines.is_empty() => ([0.0, 0.0, 0.0, 0.0], Some([0.0, 0.0, width, height])),
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
/// the two are rounded the same way.
#[must_use]
pub fn rect_object(rect: [f64; 4]) -> Object {
    Object::Array(rect.iter().map(|v| Object::Real(*v as f32)).collect())
}

/// Builds the appearance form for a widget whose rectangle is `rect` (page
/// space) on a page turned `turns` quarters, and adds it and what it uses to
/// `doc`.
///
/// Drawn at the origin: the form's box is `[0 0 w h]`, the size of `rect`, and
/// the reader places it by `/Rect` alone (module note). `rect`'s position is
/// read for nothing but its size.
pub fn stream(
    doc: &mut Document,
    rect: [f64; 4],
    turns: u8,
    lines: &[String],
    image: Option<&crate::signature::Image>,
) -> ObjectId {
    let local = [0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]];
    let seen = Upright::of(turns, local);
    let layout = layout(
        seen.width,
        seen.height,
        image.map(|image| (image.width, image.height)),
        lines,
    );
    let [_, _, x1, y1] = local;
    let mut content = format!("q 0 0 {x1} {y1} re W n\n");
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
    form.set("BBox", rect_object(local));
    form.set("Resources", Object::Dictionary(resources));
    doc.add_object(Stream::new(form, content.into_bytes()))
}
