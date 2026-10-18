//! Text written onto every page: page numbers, headers, footers and watermarks.
//!
//! One line of text, the same on every page it is written to except for two
//! fields, `{page}` and `{pages}`. A line Helvetica with `/WinAnsiEncoding` can
//! draw is set in it --- one of the fourteen fonts every reader is required to
//! have, so there is nothing to embed. Any other line is set in subsets of
//! the bundled Noto Sans and Noto Sans CJK SC, chosen per character and
//! embedded once for the whole write (`textedit::AddedFont`). It is placed in one of nine positions on the page as the
//! reader sees it, so a page carrying `/Rotate 90` gets its number at the bottom
//! of what is displayed, upright, rather than down the side of the sheet.
//!
//! ## Drawn over the page, inside a balanced `q`/`Q`
//!
//! The text goes **after** the page's own content, so it is drawn on top: a
//! watermark under the content would be hidden by any page with an opaque
//! background, which a scan always has. Drawn after, it would inherit whatever
//! graphics state the content leaves behind --- a transformed matrix, a clip,
//! an unclosed `q`. So the content is wrapped: a stream holding `q` goes in
//! front of it, and the text's own stream begins with one `Q` for that and one
//! more for every `q` the content opened and never closed, counted by decoding
//! it. A content stream that cannot be decoded within
//! `redact::MAX_CONTENT_BYTES` is taken to be balanced, which is what nearly
//! every writer produces.
//!
//! ## Where it runs in a save
//!
//! After the page surgery, so `{page}` and `{pages}` count the pages of the
//! file being written, in its order; and after the redactions, which address a
//! page's content by the position of its streams and would be pointed at the
//! wrong ones by a stream added in front. See `save::rewrite`.
//!
//! ## What it refuses
//!
//! A control character, and text with a character neither Helvetica nor the
//! bundled fonts has a glyph for: Arabic, Hebrew, the Indic scripts and Thai
//! need shaping tpdf does not do, and writing them anyway would put different
//! words on the page. A field name that
//! is neither `{page}` nor `{pages}`, which is a typing mistake rather than text
//! anybody wants printed with braces. And values outside the bounds below.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::pagetree::{self, DisplayedPage};
use crate::save::Upright;

/// Where on the page, as the reader sees it, the text is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Position {
    /// Every position, in reading order: the names the command line takes.
    pub const ALL: [Position; 9] = [
        Position::TopLeft,
        Position::TopCenter,
        Position::TopRight,
        Position::MiddleLeft,
        Position::Center,
        Position::MiddleRight,
        Position::BottomLeft,
        Position::BottomCenter,
        Position::BottomRight,
    ];

    /// The name the command line and the JSON use.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Position::TopLeft => "top-left",
            Position::TopCenter => "top-center",
            Position::TopRight => "top-right",
            Position::MiddleLeft => "middle-left",
            Position::Center => "center",
            Position::MiddleRight => "middle-right",
            Position::BottomLeft => "bottom-left",
            Position::BottomCenter => "bottom-center",
            Position::BottomRight => "bottom-right",
        }
    }

    /// The position a name stands for.
    #[must_use]
    pub fn named(name: &str) -> Option<Position> {
        Position::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Across: 0 left, 1 centre, 2 right.
    fn column(self) -> u8 {
        match self {
            Position::TopLeft | Position::MiddleLeft | Position::BottomLeft => 0,
            Position::TopCenter | Position::Center | Position::BottomCenter => 1,
            _ => 2,
        }
    }

    /// Down: 0 top, 1 middle, 2 bottom.
    fn row(self) -> u8 {
        match self {
            Position::TopLeft | Position::TopCenter | Position::TopRight => 0,
            Position::MiddleLeft | Position::Center | Position::MiddleRight => 1,
            _ => 2,
        }
    }
}

/// One line of text to write on pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddedText {
    /// The words, with `{page}` and `{pages}` where the page's number and the
    /// count go.
    pub text: String,
    pub position: Position,
    /// Font size in points.
    pub size: f32,
    /// Red, green and blue, each 0 to 1.
    pub color: [f32; 3],
    /// 1 opaque; a watermark is usually about 0.3.
    pub opacity: f32,
    /// Degrees, counter-clockwise as the reader sees the page, about the
    /// text's own centre.
    pub angle: f32,
    /// Distance from the page's edges, in points, for every position but the
    /// centre of an edge's middle.
    pub margin: f32,
    /// What `{page}` is on the first page written to.
    pub first_number: u32,
    /// The pages to write on, zero-based in the written file's order, or
    /// `None` for every page. `{page}` counts these, and `{pages}` is how
    /// many there are, so a cover left out is not counted.
    pub pages: Option<Vec<u32>>,
}

impl AddedText {
    /// A page number at the bottom centre: `Page 1 of 3`, black, 10 pt.
    #[must_use]
    pub fn page_numbers() -> Self {
        AddedText {
            text: "Page {page} of {pages}".into(),
            position: Position::BottomCenter,
            size: 10.0,
            color: [0.0, 0.0, 0.0],
            opacity: 1.0,
            angle: 0.0,
            margin: MARGIN,
            first_number: 1,
            pages: None,
        }
    }

    /// A watermark across the middle: grey, 60 pt, a third opaque, at 45 degrees.
    #[must_use]
    pub fn watermark(text: &str) -> Self {
        AddedText {
            text: text.into(),
            position: Position::Center,
            size: 60.0,
            color: [0.5, 0.5, 0.5],
            opacity: 0.3,
            angle: 45.0,
            margin: MARGIN,
            first_number: 1,
            pages: None,
        }
    }
}

/// The default distance from the page's edges: half an inch.
pub const MARGIN: f32 = 36.0;
/// The longest text, in characters.
pub const MAX_CHARS: usize = 200;
/// The smallest and the largest font size, in points.
pub const SIZES: std::ops::RangeInclusive<f32> = 4.0..=300.0;
/// The largest margin, in points.
pub const MAX_MARGIN: f32 = 300.0;
/// The largest first number.
pub const MAX_FIRST: u32 = 1_000_000;
/// The most lines one write takes.
pub const MAX_LINES: usize = 16;

/// Helvetica's capital height as a fraction of the font size: the height that
/// is centred when the text is in a middle row, and that keeps a top line's
/// capitals inside the margin.
const CAP_HEIGHT: f64 = 0.718;
/// Helvetica's descender as a fraction of the font size, kept inside a bottom
/// margin.
const DESCENT: f64 = 0.207;
/// The resource names, before a suffix that keeps them clear of the page's own:
/// Helvetica's, then Noto Sans's and Noto Sans CJK's, by font slot; and the
/// graphics states' prefix.
const FONT_NAMES: [&str; 3] = ["TpdfText", "TpdfTextU", "TpdfTextC"];
const STATE_NAME: &str = "TpdfTextGS";

/// The first reason `added` cannot be written, as one sentence.
///
/// # Errors
///
/// Empty text, text past [`MAX_CHARS`], a control character, a character
/// neither Helvetica nor the bundled fonts can draw, a field other than `{page}` and `{pages}`,
/// or a size, colour, opacity, angle, margin or first number out of range.
pub fn check(added: &AddedText) -> Result<(), String> {
    let text = &added.text;
    if text.trim().is_empty() {
        return Err("the text to add is empty".into());
    }
    if text.chars().count() > MAX_CHARS {
        return Err(format!(
            "the text to add is longer than {MAX_CHARS} characters"
        ));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("the text {text:?} has a control character in it"));
    }
    if !crate::textbox::encodable(text) && !crate::textedit::AddedFont::covers(text) {
        return Err(format!(
            "the text {text:?} has characters tpdf has no font for: it writes Latin, Greek, \
             Cyrillic, Chinese, Japanese and Korean, and not scripts that join or reorder \
             their letters, such as Arabic, Hebrew, Hindi or Thai"
        ));
    }
    let mut rest = text.as_str();
    while let Some(open) = rest.find('{') {
        let after = &rest[open..];
        let Some(close) = after.find('}') else {
            return Err(format!(
                "the text {text:?} opens a field with `{{` and does not close it"
            ));
        };
        let field = &after[..=close];
        if field != "{page}" && field != "{pages}" {
            return Err(format!(
                "the text {text:?} names the field {field}, and the fields are {{page}} and \
                 {{pages}}"
            ));
        }
        rest = &after[close + 1..];
    }
    if !SIZES.contains(&added.size) {
        return Err(format!(
            "a font size of {} points is outside {} to {}",
            added.size,
            SIZES.start(),
            SIZES.end()
        ));
    }
    if !added.color.iter().all(|c| (0.0..=1.0).contains(c)) {
        return Err("a colour is red, green and blue, each from 0 to 1".into());
    }
    if !(added.opacity > 0.0 && added.opacity <= 1.0) {
        return Err("the opacity is more than 0 and at most 1".into());
    }
    if !(added.angle.is_finite() && added.angle.abs() <= 360.0) {
        return Err("the angle is in degrees, from -360 to 360".into());
    }
    if !(0.0..=MAX_MARGIN).contains(&added.margin) {
        return Err(format!("the margin is from 0 to {MAX_MARGIN} points"));
    }
    if added.first_number > MAX_FIRST {
        return Err(format!("the first number is at most {MAX_FIRST}"));
    }
    Ok(())
}

/// `text` with its fields filled in.
#[must_use]
pub fn filled(text: &str, page: u32, pages: u32) -> String {
    text.replace("{pages}", &pages.to_string())
        .replace("{page}", &page.to_string())
}

/// Writes every line onto its pages.
///
/// # Errors
///
/// Anything [`check`] refuses, more than [`MAX_LINES`] lines, a page the
/// document does not have, or a page that is not a dictionary. Nothing is
/// written when any line is refused.
pub fn write(doc: &mut Document, lines: &[AddedText]) -> Result<(), String> {
    if lines.is_empty() {
        return Ok(());
    }
    if lines.len() > MAX_LINES {
        return Err(format!(
            "{} lines of text to add, and one write takes {MAX_LINES}",
            lines.len()
        ));
    }
    let pages = pagetree::ordered_pages(doc);
    let count = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    // Checked, and every page resolved, before anything is added.
    let mut planned: Vec<(ObjectId, usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        check(line)?;
        let chosen: Vec<u32> = match &line.pages {
            None => (0..count).collect(),
            Some(asked) => {
                if let Some(past) = asked.iter().find(|p| **p >= count) {
                    return Err(format!(
                        "the text is to go on page {}, and the document has {count}",
                        past + 1
                    ));
                }
                let mut sorted = asked.clone();
                sorted.sort_unstable();
                sorted.dedup();
                sorted
            }
        };
        let total = line.first_number.saturating_sub(1) + u32::try_from(chosen.len()).unwrap_or(0);
        // **Two page numbers can be one page object**, which a page tree may
        // list twice. Each would then carry both lines, drawn over each other,
        // so that is refused rather than written. `docs/TRAPS.md` has the shape.
        let mut seen: std::collections::HashMap<ObjectId, u32> = std::collections::HashMap::new();
        for page in &chosen {
            if let Some(first) = seen.insert(pages[*page as usize], *page) {
                return Err(format!(
                    "pages {} and {} are one page in this file, listed twice, so a line cannot \
                     be written on each",
                    first + 1,
                    page + 1
                ));
            }
        }
        for (nth, page) in chosen.iter().enumerate() {
            let number = line.first_number + u32::try_from(nth).unwrap_or(0);
            planned.push((
                pages[*page as usize],
                index,
                filled(&line.text, number, total),
            ));
        }
    }
    if planned.is_empty() {
        return Ok(());
    }

    // Helvetica for what it can draw, one embedded subset for the rest, each
    // added only when a line uses it.
    let beyond: Vec<&str> = planned
        .iter()
        .map(|(_, _, text)| text.as_str())
        .filter(|text| !crate::textbox::encodable(text))
        .collect();
    // The font objects by slot: Helvetica, then the embedded subsets.
    let mut ids: [Option<ObjectId>; 3] = [None; 3];
    let embedded = if beyond.is_empty() {
        None
    } else {
        let font = crate::textedit::AddedFont::for_texts(&beyond)?;
        let [sans, cjk] = font.install(doc)?;
        ids[1] = sans;
        ids[2] = cjk;
        Some(font)
    };
    if beyond.len() < planned.len() {
        ids[0] = Some(doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        }));
    }
    let states: Vec<ObjectId> = lines
        .iter()
        .map(|line| {
            doc.add_object(dictionary! {
                "Type" => "ExtGState",
                "ca" => Object::Real(line.opacity),
                "CA" => Object::Real(line.opacity),
            })
        })
        .collect();

    // One page at a time, every line for it in one stream.
    let mut by_page: std::collections::BTreeMap<ObjectId, Vec<(usize, String)>> =
        std::collections::BTreeMap::new();
    for (page, index, text) in planned {
        by_page.entry(page).or_default().push((index, text));
    }
    for (page, list) in by_page {
        let shown = pagetree::displayed_page(doc, page);
        let open = unclosed(doc, page);
        let (font_names, state_names) = names(doc, page, states.len());
        let mut used = [false; 3];
        let mut body = "Q\n".repeat(open + 1);
        for (index, text) in &list {
            let face = match &embedded {
                Some(font) if !crate::textbox::encodable(text) => Face::Embedded(font),
                _ => Face::Helvetica,
            };
            let (drawn, slots) = show(
                shown,
                &lines[*index],
                text,
                &face,
                &font_names,
                &state_names[*index],
            )?;
            for slot in slots {
                used[slot] = true;
            }
            body.push_str(&drawn);
        }
        let mut fonts: Vec<(ObjectId, &str)> = Vec::new();
        for (slot, name) in font_names.iter().enumerate() {
            if used[slot] {
                let id = ids[slot].ok_or("no font was made for a line of text")?;
                fonts.push((id, name.as_str()));
            }
        }
        attach(doc, page, &fonts, &states, &state_names, body)?;
    }
    Ok(())
}

/// How many `q` the page's content leaves open: `q` less `Q`, at least zero.
fn unclosed(doc: &Document, page: ObjectId) -> usize {
    let Ok(data) = doc.get_page_content_with_limit(page, crate::redact::MAX_CONTENT_BYTES) else {
        return 0;
    };
    let Ok(content) = lopdf::content::Content::decode(&data) else {
        return 0;
    };
    let mut depth: i64 = 0;
    for op in &content.operations {
        match op.operator.as_str() {
            "q" => depth += 1,
            "Q" => depth -= 1,
            _ => {}
        }
    }
    usize::try_from(depth.max(0)).unwrap_or(0)
}

/// The font a line is set in.
enum Face<'a> {
    Helvetica,
    Embedded(&'a crate::textedit::AddedFont),
}

impl Face<'_> {
    fn advance(&self, text: &str, size: f64) -> Result<f64, String> {
        match self {
            Face::Helvetica => Ok(crate::textbox::advance(text, size)),
            Face::Embedded(font) => font.advance(text, size),
        }
    }

    /// The text as `(font slot, hex)` pieces, one per change of font.
    fn pieces(&self, text: &str) -> Result<Vec<(usize, String)>, String> {
        match self {
            Face::Helvetica => Ok(vec![(0, crate::save::winansi_hex(text))]),
            Face::Embedded(font) => Ok(font
                .pieces(text)?
                .into_iter()
                .map(|piece| (1 + piece.font, piece.hex))
                .collect()),
        }
    }

    fn cap_height(&self) -> f64 {
        match self {
            Face::Helvetica => CAP_HEIGHT,
            Face::Embedded(font) => font.cap_height(),
        }
    }

    fn descent(&self) -> f64 {
        match self {
            Face::Helvetica => DESCENT,
            Face::Embedded(font) => font.descent(),
        }
    }
}

/// The resource names for each font slot and each line's graphics state,
/// clear of any the page already uses.
fn names(doc: &Document, page: ObjectId, states: usize) -> (Vec<String>, Vec<String>) {
    let resources = resources_of(doc, page);
    let taken = |key: &[u8], name: &str| {
        resources
            .get(key)
            .ok()
            .map(|found| dictionary_of(doc, found))
            .is_some_and(|dict| dict.has(name.as_bytes()))
    };
    let fonts = FONT_NAMES
        .iter()
        .map(|base| {
            let mut suffix = 0_u32;
            loop {
                let name = if suffix == 0 {
                    (*base).to_string()
                } else {
                    format!("{base}{suffix}")
                };
                if !taken(b"Font", &name) {
                    break name;
                }
                suffix += 1;
            }
        })
        .collect();
    let mut out = Vec::with_capacity(states);
    let mut n = 0_u32;
    while out.len() < states {
        let name = format!("{STATE_NAME}{n}");
        if !taken(b"ExtGState", &name) {
            out.push(name);
        }
        n += 1;
    }
    (fonts, out)
}

/// The operators that draw one line on a page displayed as `shown`, and the
/// font slots they use.
fn show(
    shown: DisplayedPage,
    line: &AddedText,
    text: &str,
    face: &Face<'_>,
    fonts: &[String],
    state: &str,
) -> Result<(String, Vec<usize>), String> {
    let (width, height) = (f64::from(shown.width), f64::from(shown.height));
    let size = f64::from(line.size);
    let margin = f64::from(line.margin);
    let advance = face.advance(text, size)?;
    let (cap_height, descent) = (face.cap_height(), face.descent());

    // The anchor, in the displayed page's space: points from its top-left
    // corner, y down. A middle row puts the capitals' middle on the anchor.
    let x = match line.position.column() {
        0 => margin,
        1 => width / 2.0,
        _ => width - margin,
    };
    let y = match line.position.row() {
        0 => margin,
        1 => height / 2.0,
        _ => height - margin,
    };
    // Where the text starts along its own baseline, and how far the baseline
    // sits below the anchor, before the turn.
    let along = match line.position.column() {
        0 => 0.0,
        1 => -advance / 2.0,
        _ => -advance,
    };
    let below = match line.position.row() {
        0 => size * cap_height,
        1 => size * cap_height / 2.0,
        _ => -size * descent,
    };

    // The displayed page as an upright box over the page's own space.
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    let quad = crate::text::from_device(
        shown.turns,
        shown.width,
        shown.height,
        [0.0, 0.0, shown.width, shown.height],
    );
    let upright = Upright::of(
        shown.turns,
        [quad[0] + ox, quad[1] + oy, quad[2] + ox, quad[3] + oy],
    );
    let origin = upright.at(0.0, 0.0);
    let to = |p: (f64, f64)| (p.0 - origin.0, p.1 - origin.1);
    let right = to(upright.at(1.0, 0.0));
    let down = to(upright.at(0.0, 1.0));

    // The text's direction and its up, turned by the angle the reader sees.
    let (sin, cos) = f64::from(line.angle).to_radians().sin_cos();
    let along_dir = (cos * right.0 - sin * down.0, cos * right.1 - sin * down.1);
    let up_dir = (-sin * right.0 - cos * down.0, -sin * right.1 - cos * down.1);
    let anchor = upright.at(x, y);
    let start = (
        anchor.0 + along * along_dir.0 - below * up_dir.0,
        anchor.1 + along * along_dir.1 - below * up_dir.1,
    );
    let number = |v: f64| {
        let v = if v.abs() < 1e-9 { 0.0 } else { v };
        format!("{v:.4}")
    };
    let [r, g, b] = line.color;
    // Marked as pagination, not content (ISO 32000-1 §14.8.2.2), so a screen
    // reader and a tagged-PDF check skip it; a line across the middle of the
    // page has no header or footer subtype.
    let artifact = match line.position.row() {
        0 => "<</Type /Pagination /Subtype /Header>>",
        1 => "<</Type /Pagination>>",
        _ => "<</Type /Pagination /Subtype /Footer>>",
    };
    let pieces = face.pieces(text)?;
    let shows: String = pieces
        .iter()
        .map(|(slot, hex)| format!("/{} {} Tf\n<{hex}> Tj\n", fonts[*slot], number(size)))
        .collect();
    let drawn = format!(
        "/Artifact {artifact} BDC\nq\n/{state} gs\n{r} {g} {b} rg\nBT\n{} {} {} {} {} {} Tm\n{shows}ET\nQ\nEMC\n",
        number(along_dir.0),
        number(along_dir.1),
        number(up_dir.0),
        number(up_dir.1),
        number(start.0),
        number(start.1),
    );
    Ok((drawn, pieces.into_iter().map(|(slot, _)| slot).collect()))
}

/// Wraps the page's content in `q` and its new `Q`s, puts `body` after it, and
/// adds the font and the graphics states to the page's resources.
fn attach(
    doc: &mut Document,
    page: ObjectId,
    used: &[(ObjectId, &str)],
    states: &[ObjectId],
    state_names: &[String],
    body: String,
) -> Result<(), String> {
    // The effective resources, copied onto the page so that adding to them
    // changes this page and no other --- `textlayer.rs`'s rule.
    let mut resources = resources_of(doc, page);
    let mut fonts = match resources.get(b"Font") {
        Ok(found) => dictionary_of(doc, found),
        Err(_) => Dictionary::new(),
    };
    for (font, name) in used {
        fonts.set(name.as_bytes().to_vec(), Object::Reference(*font));
    }
    resources.set("Font", Object::Dictionary(fonts));
    let mut graphics = match resources.get(b"ExtGState") {
        Ok(found) => dictionary_of(doc, found),
        Err(_) => Dictionary::new(),
    };
    for (id, name) in states.iter().zip(state_names) {
        graphics.set(name.as_bytes().to_vec(), Object::Reference(*id));
    }
    resources.set("ExtGState", Object::Dictionary(graphics));

    let before = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let mut stream = Stream::new(Dictionary::new(), body.into_bytes());
    let _ = stream.compress();
    let after = doc.add_object(stream);

    let existing = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
        .get(b"Contents")
        .ok()
        .cloned();
    let mut contents = vec![Object::Reference(before)];
    match existing {
        Some(Object::Array(parts)) => contents.extend(parts),
        Some(Object::Reference(id)) => match doc.get_object(id) {
            Ok(Object::Array(parts)) => contents.extend(parts.iter().cloned()),
            _ => contents.push(Object::Reference(id)),
        },
        _ => {}
    }
    contents.push(Object::Reference(after));

    let dict = doc
        .get_object_mut(page)
        .and_then(Object::as_dict_mut)
        .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?;
    dict.set("Contents", Object::Array(contents));
    dict.set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// The page's effective resources, which it may inherit.
fn resources_of(doc: &Document, page: ObjectId) -> Dictionary {
    match pagetree::inherited(doc, page, b"Resources") {
        Some(found) => dictionary_of(doc, &found),
        None => Dictionary::new(),
    }
}

/// A dictionary, read through one reference, or an empty one.
fn dictionary_of(doc: &Document, object: &Object) -> Dictionary {
    let direct = match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    };
    direct
        .and_then(|found| found.as_dict().ok())
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document of `pages` empty A4 pages, each turned `turns` quarters, its
    /// content `content`.
    fn document(pages: usize, turns: i64, content: &[u8]) -> Document {
        let mut doc = Document::with_version("1.7");
        let tree = doc.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..pages {
            let stream = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => tree,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Rotate" => turns * 90,
                "Contents" => stream,
                "Resources" => dictionary! {},
            })));
        }
        let count = i64::try_from(kids.len()).unwrap();
        doc.objects.insert(
            tree,
            dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => count}.into(),
        );
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => tree});
        doc.trailer.set("Root", catalog);
        doc
    }

    /// Every operation of page `n`'s content, decoded.
    fn operations(doc: &Document, n: usize) -> Vec<lopdf::content::Operation> {
        let page = pagetree::ordered_pages(doc)[n];
        let data = doc.get_page_content(page);
        lopdf::content::Content::decode(&data)
            .expect("decodes")
            .operations
    }

    /// The `Tj` strings of page `n`, decoded from WinAnsi.
    fn shown(doc: &Document, n: usize) -> Vec<String> {
        operations(doc, n)
            .into_iter()
            .filter(|op| op.operator == "Tj")
            .filter_map(|op| op.operands.first().cloned())
            .filter_map(|o| {
                o.as_str()
                    .ok()
                    .map(|b| b.iter().map(|c| char::from(*c)).collect())
            })
            .collect()
    }

    /// The `Tm` of page `n`'s first line: six numbers.
    fn matrix(doc: &Document, n: usize) -> [f64; 6] {
        let op = operations(doc, n)
            .into_iter()
            .find(|op| op.operator == "Tm")
            .expect("a Tm");
        let v: Vec<f64> = op
            .operands
            .iter()
            .map(|o| o.as_float().map(f64::from).expect("a number"))
            .collect();
        v.try_into().expect("six")
    }

    #[test]
    fn page_numbers_count_the_pages_written_to() {
        let mut doc = document(3, 0, b"");
        write(&mut doc, &[AddedText::page_numbers()]).expect("writes");
        assert_eq!(shown(&doc, 0), ["Page 1 of 3"]);
        assert_eq!(shown(&doc, 2), ["Page 3 of 3"]);
    }

    #[test]
    fn a_page_left_out_is_not_counted_and_the_first_number_is_taken() {
        let mut doc = document(4, 0, b"");
        let mut line = AddedText::page_numbers();
        line.pages = Some(vec![3, 1, 2, 2]);
        line.first_number = 5;
        write(&mut doc, &[line]).expect("writes");
        assert!(shown(&doc, 0).is_empty());
        assert_eq!(shown(&doc, 1), ["Page 5 of 7"]);
        assert_eq!(shown(&doc, 3), ["Page 7 of 7"]);
    }

    #[test]
    fn the_bottom_centre_is_centred_and_above_the_margin() {
        let mut doc = document(1, 0, b"");
        write(&mut doc, &[AddedText::page_numbers()]).expect("writes");
        let m = matrix(&doc, 0);
        let width = crate::textbox::advance("Page 1 of 1", 10.0);
        assert_eq!(&m[..4], &[1.0, 0.0, 0.0, 1.0]);
        assert!((m[4] - (595.0 - width) / 2.0).abs() < 1e-3, "{m:?}");
        assert!((m[5] - (36.0 + 10.0 * DESCENT)).abs() < 1e-3, "{m:?}");
    }

    #[test]
    fn the_other_corners_and_sides_sit_inside_the_margin() {
        let at = |position: Position| {
            let mut doc = document(1, 0, b"");
            let line = AddedText {
                position,
                ..AddedText::page_numbers()
            };
            write(&mut doc, &[line]).expect("writes");
            matrix(&doc, 0)
        };
        let width = crate::textbox::advance("Page 1 of 1", 10.0);
        // The right column ends at the margin, the top row's capitals hang
        // from it.
        let m = at(Position::TopRight);
        assert!((m[4] - (595.0 - 36.0 - width)).abs() < 1e-3, "{m:?}");
        assert!(
            (m[5] - (842.0 - 36.0 - 10.0 * CAP_HEIGHT)).abs() < 1e-3,
            "{m:?}"
        );
        // The left column starts at the margin, the middle row's capitals are
        // centred on the page's middle.
        let m = at(Position::MiddleLeft);
        assert!((m[4] - 36.0).abs() < 1e-3, "{m:?}");
        assert!(
            (m[5] - (421.0 - 10.0 * CAP_HEIGHT / 2.0)).abs() < 1e-3,
            "{m:?}"
        );
    }

    #[test]
    fn a_turned_page_gets_its_number_at_the_bottom_it_displays() {
        // `/Rotate 90` turns the sheet clockwise, so the displayed bottom is
        // the sheet's right edge, and the text runs up the sheet so that it
        // reads left to right on screen, its capitals towards the sheet's left.
        let mut doc = document(1, 1, b"");
        write(&mut doc, &[AddedText::page_numbers()]).expect("writes");
        let m = matrix(&doc, 0);
        assert_eq!(&m[..4], &[0.0, 1.0, -1.0, 0.0], "{m:?}");
        assert!(
            (m[4] - (595.0 - 36.0 - 10.0 * DESCENT)).abs() < 1e-3,
            "{m:?}"
        );
        let width = crate::textbox::advance("Page 1 of 1", 10.0);
        assert!((m[5] - (842.0 - width) / 2.0).abs() < 1e-3, "{m:?}");
    }

    #[test]
    fn a_watermark_is_turned_about_the_middle_and_translucent() {
        let mut doc = document(1, 0, b"");
        write(&mut doc, &[AddedText::watermark("DRAFT")]).expect("writes");
        let m = matrix(&doc, 0);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (m[0] - half).abs() < 1e-3 && (m[1] - half).abs() < 1e-3,
            "{m:?}"
        );
        // The middle of the text's baseline-to-capital box is the page's middle.
        let width = crate::textbox::advance("DRAFT", 60.0);
        let mid_x = m[4] + m[0] * width / 2.0 + m[2] * 60.0 * CAP_HEIGHT / 2.0;
        let mid_y = m[5] + m[1] * width / 2.0 + m[3] * 60.0 * CAP_HEIGHT / 2.0;
        assert!(
            (mid_x - 297.5).abs() < 1e-2 && (mid_y - 421.0).abs() < 1e-2,
            "{mid_x} {mid_y}"
        );
        let page = pagetree::ordered_pages(&doc)[0];
        let resources = resources_of(&doc, page);
        let states = dictionary_of(&doc, resources.get(b"ExtGState").expect("states"));
        let (_, state) = states.iter().next().expect("one");
        let state = dictionary_of(&doc, state);
        assert_eq!(state.get(b"ca").and_then(Object::as_float).ok(), Some(0.3));
    }

    #[test]
    fn the_text_is_drawn_after_the_content_with_every_open_q_closed() {
        let mut doc = document(1, 0, b"q q 2 0 0 2 0 0 cm");
        write(&mut doc, &[AddedText::page_numbers()]).expect("writes");
        let ops: Vec<String> = operations(&doc, 0)
            .into_iter()
            .map(|op| op.operator)
            .collect();
        let at = ops.iter().position(|op| op == "BT").expect("BT");
        // Our q in front, the content's two, three Q, then our own q.
        assert_eq!(ops[0], "q");
        assert_eq!(&ops[4..7], &["Q", "Q", "Q"], "{ops:?}");
        let depth = ops[..at].iter().fold(0_i32, |d, op| match op.as_str() {
            "q" => d + 1,
            "Q" => d - 1,
            _ => d,
        });
        assert_eq!(depth, 1, "only the text's own q is open at BT: {ops:?}");
    }

    #[test]
    fn each_line_is_a_pagination_artifact_with_its_row_as_subtype() {
        let subtype = |position: Position| {
            let mut doc = document(1, 0, b"");
            let line = AddedText {
                position,
                ..AddedText::page_numbers()
            };
            write(&mut doc, &[line]).expect("writes");
            let ops = operations(&doc, 0);
            let at = ops.iter().position(|op| op.operator == "BDC").expect("BDC");
            let bt = ops.iter().position(|op| op.operator == "BT").expect("BT");
            let emc = ops.iter().position(|op| op.operator == "EMC").expect("EMC");
            assert!(at < bt && bt < emc, "the text is inside the sequence");
            assert_eq!(ops[at].operands[0].as_name().ok(), Some(&b"Artifact"[..]));
            let properties = ops[at].operands[1].as_dict().expect("inline properties");
            assert_eq!(
                properties.get(b"Type").and_then(Object::as_name).ok(),
                Some(&b"Pagination"[..])
            );
            properties
                .get(b"Subtype")
                .and_then(Object::as_name)
                .ok()
                .map(<[u8]>::to_vec)
        };
        assert_eq!(subtype(Position::TopLeft), Some(b"Header".to_vec()));
        assert_eq!(subtype(Position::Center), None);
        assert_eq!(subtype(Position::BottomRight), Some(b"Footer".to_vec()));
    }

    #[test]
    fn a_name_the_page_uses_is_not_taken() {
        let mut doc = document(1, 0, b"");
        let page = pagetree::ordered_pages(&doc)[0];
        let existing = doc.add_object(dictionary! {"Type" => "Font"});
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .unwrap()
            .set(
                "Resources",
                dictionary! {"Font" => dictionary! {FONT_NAMES[0] => existing}},
            );
        write(&mut doc, &[AddedText::page_numbers()]).expect("writes");
        let fonts = dictionary_of(&doc, resources_of(&doc, page).get(b"Font").unwrap());
        assert_eq!(
            fonts.get(FONT_NAMES[0].as_bytes()).unwrap(),
            &Object::Reference(existing)
        );
        assert!(fonts.has(format!("{}1", FONT_NAMES[0]).as_bytes()));
    }

    #[test]
    fn what_cannot_be_written_is_refused_before_anything_is() {
        let refused = |change: &dyn Fn(&mut AddedText), said: &str| {
            let mut doc = document(2, 0, b"");
            let objects = doc.objects.len();
            let mut line = AddedText::page_numbers();
            change(&mut line);
            let why = write(&mut doc, &[line]).expect_err(said);
            assert!(why.contains(said), "{why}");
            assert_eq!(doc.objects.len(), objects, "nothing added for {said}");
        };
        refused(
            &|l| l.text = "Seite {seite}".into(),
            "names the field {seite}",
        );
        refused(&|l| l.text = "Page {page".into(), "does not close it");
        refused(&|l| l.text = "עמוד {page}".into(), "has no font for");
        refused(&|l| l.text = "Page\u{7}{page}".into(), "control character");
        refused(&|l| l.text = "  ".into(), "is empty");
        refused(&|l| l.size = 1.0, "font size");
        refused(&|l| l.opacity = 0.0, "opacity");
        refused(&|l| l.color = [0.0, 1.5, 0.0], "a colour is");
        refused(&|l| l.angle = 361.0, "the angle is in degrees");
        refused(&|l| l.margin = MAX_MARGIN + 1.0, "the margin is from");
        refused(
            &|l| l.first_number = MAX_FIRST + 1,
            "the first number is at most",
        );
        refused(
            &|l| l.text = "x".repeat(MAX_CHARS + 1),
            "longer than 200 characters",
        );
        refused(
            &|l| l.pages = Some(vec![2]),
            "page 3, and the document has 2",
        );
    }

    #[test]
    fn german_text_is_written_as_winansi_bytes() {
        let mut doc = document(1, 0, b"");
        let mut line = AddedText::page_numbers();
        line.text = "Seite {page} von {pages} – Entwurf für Prüfung".replace('–', "-");
        write(&mut doc, &[line]).expect("writes");
        assert_eq!(shown(&doc, 0), ["Seite 1 von 1 - Entwurf für Prüfung"]);
    }

    /// Page `n`'s font resource `name`, resolved, if it has one.
    fn font_named(doc: &Document, n: usize, name: &str) -> Option<Dictionary> {
        let page = pagetree::ordered_pages(doc)[n];
        let fonts = dictionary_of(doc, resources_of(doc, page).get(b"Font").ok()?);
        Some(dictionary_of(doc, fonts.get(name.as_bytes()).ok()?))
    }

    /// Each line of page `n` as a reader extracts it: every `Tj` between `BT`
    /// and `ET` read through the font its `Tf` set, a Type0 font by its
    /// `/ToUnicode` map and Helvetica as WinAnsi.
    fn read_back(doc: &Document, n: usize) -> Vec<String> {
        let page = pagetree::ordered_pages(doc)[n];
        let fonts = dictionary_of(doc, resources_of(doc, page).get(b"Font").unwrap());
        let table = |name: &[u8]| -> Option<std::collections::HashMap<u32, String>> {
            let font = dictionary_of(doc, fonts.get(name).ok()?);
            let Object::Reference(id) = font.get(b"ToUnicode").ok()? else {
                return None;
            };
            let map = doc.get_object(*id).ok()?.as_stream().ok()?.content.clone();
            let map = String::from_utf8(map).ok()?;
            let mut out = std::collections::HashMap::new();
            for line in map.lines().filter(|l| l.starts_with('<')) {
                let mut parts = line.split_whitespace();
                let code = parts.next()?.trim_matches(['<', '>']);
                let target = parts.next()?.trim_matches(['<', '>']);
                let units: Vec<u16> = (0..target.len())
                    .step_by(4)
                    .map(|at| u16::from_str_radix(&target[at..at + 4], 16).unwrap())
                    .collect();
                out.insert(
                    u32::from_str_radix(code, 16).ok()?,
                    String::from_utf16(&units).ok()?,
                );
            }
            Some(out)
        };
        let mut current = None;
        let mut line = String::new();
        let mut out = Vec::new();
        for op in operations(doc, n) {
            match op.operator.as_str() {
                "Tf" => current = table(op.operands[0].as_name().unwrap()),
                "Tj" => {
                    let bytes = op.operands[0].as_str().unwrap();
                    match &current {
                        Some(map) => {
                            for pair in bytes.chunks(2) {
                                line.push_str(
                                    &map[&(u32::from(pair[0]) << 8 | u32::from(pair[1]))],
                                );
                            }
                        }
                        None => line.extend(bytes.iter().map(|b| char::from(*b))),
                    }
                }
                "ET" => out.push(std::mem::take(&mut line)),
                _ => {}
            }
        }
        out
    }

    /// The `BaseFont` of page `n`'s font resource `name`, and its embedded
    /// program's length.
    fn program_of(doc: &Document, n: usize, name: &str) -> (String, usize) {
        let font = font_named(doc, n, name).expect("the font");
        let base =
            String::from_utf8(font.get(b"BaseFont").unwrap().as_name().unwrap().to_vec()).unwrap();
        let resolve = |o: &Object| match o {
            Object::Reference(id) => doc.get_object(*id).unwrap().clone(),
            other => other.clone(),
        };
        assert_eq!(
            font.get(b"Encoding").and_then(Object::as_name).ok(),
            Some(&b"Identity-H"[..])
        );
        let child = resolve(&font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]);
        let descriptor = resolve(child.as_dict().unwrap().get(b"FontDescriptor").unwrap());
        let program = resolve(descriptor.as_dict().unwrap().get(b"FontFile2").unwrap());
        (base, program.as_stream().unwrap().content.len())
    }

    #[test]
    fn greek_is_set_in_a_subset_of_noto_sans_that_extracts_as_typed() {
        let mut doc = document(2, 0, b"");
        let mut line = AddedText::page_numbers();
        line.text = "Σελίδα {page} από {pages}".into();
        write(&mut doc, &[line]).expect("writes");
        assert_eq!(read_back(&doc, 1), ["Σελίδα 2 από 2"]);
        let (base, bytes) = program_of(&doc, 0, FONT_NAMES[1]);
        assert!(base.ends_with("+NotoSans"), "{base}");
        // The whole of Noto Sans is 431 KB.
        assert!(bytes < 60_000, "a subset, {bytes} bytes");
        // No Helvetica and no CJK font where no character is set in them.
        assert!(font_named(&doc, 0, FONT_NAMES[0]).is_none());
        assert!(font_named(&doc, 0, FONT_NAMES[2]).is_none());
        // The second page's number is centred by the embedded font's widths.
        let width = crate::textedit::AddedFont::for_texts(&["Σελίδα 2 από 2"])
            .unwrap()
            .advance("Σελίδα 2 από 2", 10.0)
            .unwrap();
        assert!((matrix(&doc, 1)[4] - (595.0 - width) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn chinese_is_set_in_noto_sans_cjk_and_its_latin_in_noto_sans() {
        let mut doc = document(1, 0, b"");
        write(&mut doc, &[AddedText::watermark("草稿 DRAFT")]).expect("writes");
        assert_eq!(read_back(&doc, 0), ["草稿 DRAFT"]);
        let (base, bytes) = program_of(&doc, 0, FONT_NAMES[2]);
        assert!(base.ends_with("+NotoSansCJKSC"), "{base}");
        // The whole of Noto Sans CJK SC is 13.6 MB.
        assert!(bytes < 60_000, "a subset, {bytes} bytes");
        let (base, _) = program_of(&doc, 0, FONT_NAMES[1]);
        assert!(base.ends_with("+NotoSans"), "{base}");
    }

    #[test]
    fn a_line_neither_font_holds_alone_switches_font_where_its_characters_do() {
        // Noto Sans CJK SC has no accented Greek, and Noto Sans no ideographs.
        let mut doc = document(1, 0, b"");
        let mut line = AddedText::page_numbers();
        line.text = "Σελίδα {page} · 第{page}页".into();
        write(&mut doc, &[line]).expect("writes");
        assert_eq!(read_back(&doc, 0), ["Σελίδα 1 · 第1页"]);
        let fonts: Vec<String> = operations(&doc, 0)
            .into_iter()
            .filter(|op| op.operator == "Tf")
            .map(|op| String::from_utf8(op.operands[0].as_name().unwrap().to_vec()).unwrap())
            .collect();
        let [_, sans, cjk] = FONT_NAMES;
        assert_eq!(fonts, [sans, cjk, sans, cjk]);
    }

    #[test]
    fn a_latin_line_stays_in_helvetica_beside_a_greek_one() {
        let mut doc = document(1, 0, b"");
        let greek = AddedText::watermark("ΠΡΟΣΧΕΔΙΟ");
        write(&mut doc, &[AddedText::page_numbers(), greek]).expect("writes");
        let helvetica = font_named(&doc, 0, FONT_NAMES[0]).expect("Helvetica");
        assert_eq!(
            helvetica.get(b"BaseFont").and_then(Object::as_name).ok(),
            Some(&b"Helvetica"[..])
        );
        assert_eq!(read_back(&doc, 0), ["Page 1 of 1", "ΠΡΟΣΧΕΔΙΟ"]);
    }

    #[test]
    fn a_page_listed_twice_is_refused_rather_than_numbered_twice() {
        let mut doc = document(1, 0, b"");
        let tree = doc
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let kid = pagetree::ordered_pages(&doc)[0];
        let node = doc
            .get_object_mut(tree)
            .and_then(Object::as_dict_mut)
            .unwrap();
        node.set("Kids", vec![Object::Reference(kid), Object::Reference(kid)]);
        node.set("Count", 2);
        // The control: the tree really lists the one page twice.
        assert_eq!(pagetree::ordered_pages(&doc), vec![kid, kid]);
        let objects = doc.objects.len();
        let why = write(&mut doc, &[AddedText::page_numbers()]).expect_err("refused");
        assert!(why.contains("pages 1 and 2 are one page"), "{why}");
        assert_eq!(doc.objects.len(), objects);
    }

    #[test]
    fn positions_round_trip_by_name() {
        for position in Position::ALL {
            assert_eq!(Position::named(position.name()), Some(position));
            let json = serde_json::to_string(&position).unwrap();
            assert_eq!(json, format!("\"{}\"", position.name()));
        }
        assert_eq!(Position::named("middle"), None);
    }
}
