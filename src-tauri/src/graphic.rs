//! *Copy a graphic* and *Save a graphic*: the pictures and drawings that lie
//! wholly inside a region of a page, on their own.
//!
//! *Copy a region as a picture* renders everything in a rectangle, the page's
//! white included, at the zoom the reader has. A logo is usually wanted
//! without that: alone, on a transparent background, sharp at any size, and as
//! the file it was if it is a picture. So the reader drags around it, and the
//! page objects whose bounds lie inside the rectangle are what is exported:
//!
//! - **PNG**: those objects drawn alone, nothing behind them, at
//!   [`GRAPHIC_DPI`]; a region holding one picture and nothing else gives that
//!   picture at its own pixel size, with its transparency mask.
//! - **Original**: one picture as stored, a JPEG as its own bytes.
//! - **SVG**: the drawing as vector paths, with its colours, transparency and
//!   stroke settings, and pictures inside it embedded as PNG. Text and
//!   gradients are refused rather than drawn as something else.
//!
//! A drawing a word processor placed (Word turns an EMF logo into one Form
//! XObject) is one top-level object, and a logo drawn as loose paths is many:
//! the rectangle takes both, where a click could only take one path.
//!
//! The work happens in the document's worker, on a second PDFium document over
//! the same bytes (`DocumentGraph::reopen`), because the objects outside the
//! selection are taken off its page before it is drawn (`pagecopy.rs` has the
//! same reason and the same rule never to destroy a removed object).

use serde::{Deserialize, Serialize};

/// The resolution a drawing is rendered at, unless the picture would pass
/// [`MAX_PIXELS`]: enough to print a logo sharp at its size on the page.
pub const GRAPHIC_DPI: f64 = 600.0;

/// The most pixels a rendered graphic may have. Four bytes a pixel must fit
/// the 16 MiB tile mapping with room for a PNG that compresses nothing.
pub const MAX_PIXELS: f64 = 3_500_000.0;

/// Points of slack when deciding that an object lies inside the region: a
/// rectangle dragged tight around a logo's edge should still take it.
pub const SLACK_PT: f64 = 2.0;

/// What is asked of the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicFormat {
    /// Only what the region holds: the other three answers' options.
    Describe,
    /// The picture or drawing under the region's centre, which is where the
    /// reader clicked: its index on the page and its rectangle, for the window
    /// to outline it. See [`hit`].
    Hit,
    Png,
    Original,
    Svg,
}

/// What the region holds, and what was made of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphicFound {
    /// Top-level page objects inside the region.
    pub objects: u32,
    /// `jpg` or `png`: the region holds one picture and nothing else, and this
    /// is the form it is stored in. `None` for anything else.
    pub original: Option<String>,
    /// Why no SVG can be made, or `None` when one can.
    pub no_svg: Option<String>,
    /// The PNG's size in pixels.
    pub width: u32,
    pub height: u32,
    /// The file ending of the bytes in the tile mapping; empty for `Describe`.
    pub extension: String,
    /// For `Hit`: the index among the page's objects of the one clicked, which
    /// a later request names in [`GraphicRequest::object`]. `None` when the
    /// click found nothing.
    #[serde(default)]
    pub object: Option<u32>,
    /// For `Hit`: that object's rectangle in the request's tile pixels, left,
    /// top, right, bottom.
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
}

/// The largest share of the page a clicked object may cover. A white
/// rectangle under the whole page, which word processors write, would
/// otherwise be what every click on a margin selects.
pub const HIT_SHARE: f64 = 0.6;

/// Which of `objects`, given in page order with their kind's text flag and
/// bounds, a click at `point` selects: the topmost one that is not text,
/// whose bounds hold the point with [`SLACK_PT`] around them, and that covers
/// less than [`HIT_SHARE`] of `page_area`.
pub fn hit(objects: &[(bool, Bounds)], point: (f64, f64), page_area: f64) -> Option<usize> {
    let (x, y) = point;
    objects.iter().rposition(|&(text, at)| {
        let area = (at[2] - at[0]) * (at[3] - at[1]);
        !text
            && area > 0.0
            && area < HIT_SHARE * page_area
            && x >= at[0] - SLACK_PT
            && x <= at[2] + SLACK_PT
            && y >= at[1] - SLACK_PT
            && y <= at[3] + SLACK_PT
    })
}

/// A rectangle in page space, points: left, bottom, right, top.
pub type Bounds = [f64; 4];

/// Whether `object` lies inside `region`, with [`SLACK_PT`] around it.
pub fn inside(object: Bounds, region: Bounds) -> bool {
    object[0] >= region[0] - SLACK_PT
        && object[1] >= region[1] - SLACK_PT
        && object[2] <= region[2] + SLACK_PT
        && object[3] <= region[3] + SLACK_PT
        && object[2] > object[0]
        && object[3] > object[1]
}

/// The smallest rectangle around all of `boxes`.
pub fn union(boxes: &[Bounds]) -> Option<Bounds> {
    boxes.iter().copied().reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

/// Pixels a point is rendered at: [`GRAPHIC_DPI`], lowered so that a
/// `width_pt` by `height_pt` graphic stays within [`MAX_PIXELS`].
pub fn scale_for(width_pt: f64, height_pt: f64) -> f64 {
    let wanted = GRAPHIC_DPI / 72.0;
    let area = (width_pt * height_pt).max(1e-6);
    wanted.min((MAX_PIXELS / area).sqrt())
}

/// The size a stored picture is exported at: its own, unless that passes
/// [`MAX_PIXELS`], and then scaled down to it with its proportions kept.
pub fn picture_size(width: u32, height: u32) -> (u32, u32) {
    let area = f64::from(width) * f64::from(height);
    if area <= MAX_PIXELS {
        return (width, height);
    }
    let k = (MAX_PIXELS / area).sqrt();
    (
        ((f64::from(width) * k).floor() as u32).max(1),
        ((f64::from(height) * k).floor() as u32).max(1),
    )
}

/// Standard base64 with padding, for a picture inside an SVG.
pub fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A PDF matrix `[a b c d e f]`: a point goes to `(a x + c y + e, b x + d y + f)`.
pub type Matrix = [f64; 6];

/// The identity matrix.
pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `first`, then `then`.
pub fn concat(first: Matrix, then: Matrix) -> Matrix {
    let [a, b, c, d, e, f] = first;
    let [p, q, r, s, t, u] = then;
    [
        a * p + b * r,
        a * q + b * s,
        c * p + d * r,
        c * q + d * s,
        e * p + f * r + t,
        e * q + f * s + u,
    ]
}

/// One step of a path, in the path's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

/// How a path is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    NonZero,
    EvenOdd,
}

/// How a path is stroked, in its own space.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub rgba: [u8; 4],
    pub width: f64,
    /// 0 butt, 1 round, 2 projecting square, as PDF numbers them.
    pub cap: i32,
    /// 0 miter, 1 round, 2 bevel.
    pub join: i32,
    pub dash: Vec<f64>,
    pub phase: f64,
}

/// One path of the drawing, with the matrix that takes it to page space.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub steps: Vec<Step>,
    pub matrix: Matrix,
    pub fill: Option<(Rule, [u8; 4])>,
    pub stroke: Option<Stroke>,
}

/// An SVG document of a drawing whose page-space bounds are `bounds`.
///
/// The outer group turns page space (y up, from the page's corner) into the
/// SVG's (y down, from the drawing's corner); each path keeps its own points
/// and carries its matrix, so a stroke scales as the PDF scales it.
pub struct Svg {
    bounds: Bounds,
    body: String,
}

impl Svg {
    pub fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            body: String::new(),
        }
    }

    /// Adds one path. A path with neither fill nor stroke draws nothing and
    /// is left out.
    pub fn shape(&mut self, shape: &Shape) {
        if (shape.fill.is_none() && shape.stroke.is_none()) || shape.steps.is_empty() {
            return;
        }
        let mut d = String::new();
        for step in &shape.steps {
            let piece = match *step {
                Step::Move(x, y) => format!("M{} {}", num(x), num(y)),
                Step::Line(x, y) => format!("L{} {}", num(x), num(y)),
                Step::Curve(a, b, c, e, x, y) => format!(
                    "C{} {} {} {} {} {}",
                    num(a),
                    num(b),
                    num(c),
                    num(e),
                    num(x),
                    num(y)
                ),
                Step::Close => "Z".to_owned(),
            };
            d.push_str(&piece);
        }
        let [a, b, c, e, f, g] = shape.matrix;
        let mut attributes = format!(
            "d=\"{d}\" transform=\"matrix({} {} {} {} {} {})\"",
            num(a),
            num(b),
            num(c),
            num(e),
            num(f),
            num(g)
        );
        match shape.fill {
            Some((rule, rgba)) => {
                attributes.push_str(&format!(" fill=\"{}\"", colour(rgba)));
                if rgba[3] != 255 {
                    attributes.push_str(&format!(
                        " fill-opacity=\"{}\"",
                        num(f64::from(rgba[3]) / 255.0)
                    ));
                }
                if rule == Rule::EvenOdd {
                    attributes.push_str(" fill-rule=\"evenodd\"");
                }
            }
            None => attributes.push_str(" fill=\"none\""),
        }
        if let Some(stroke) = &shape.stroke {
            attributes.push_str(&format!(
                " stroke=\"{}\" stroke-width=\"{}\"",
                colour(stroke.rgba),
                num(stroke.width.max(0.0))
            ));
            if stroke.rgba[3] != 255 {
                attributes.push_str(&format!(
                    " stroke-opacity=\"{}\"",
                    num(f64::from(stroke.rgba[3]) / 255.0)
                ));
            }
            let cap = ["butt", "round", "square"]
                .get(stroke.cap as usize)
                .copied()
                .unwrap_or("butt");
            let join = ["miter", "round", "bevel"]
                .get(stroke.join as usize)
                .copied()
                .unwrap_or("miter");
            attributes.push_str(&format!(
                " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\""
            ));
            if !stroke.dash.is_empty() && stroke.dash.iter().any(|v| *v > 0.0) {
                let dash: Vec<String> = stroke.dash.iter().map(|v| num(*v)).collect();
                attributes.push_str(&format!(
                    " stroke-dasharray=\"{}\" stroke-dashoffset=\"{}\"",
                    dash.join(" "),
                    num(stroke.phase)
                ));
            }
        }
        self.body.push_str(&format!("<path {attributes}/>\n"));
    }

    /// Adds a picture: `png` covers the unit square that `matrix` takes to page
    /// space, as a PDF image does.
    pub fn picture(&mut self, png: &[u8], matrix: Matrix) {
        // An image's unit square has its first row at the top: flip it into
        // the square before the image's own matrix applies.
        let placed = concat([1.0, 0.0, 0.0, -1.0, 0.0, 1.0], matrix);
        let [a, b, c, d, e, f] = placed;
        self.body.push_str(&format!(
            "<image width=\"1\" height=\"1\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {})\" href=\"data:image/png;base64,{}\"/>\n",
            num(a),
            num(b),
            num(c),
            num(d),
            num(e),
            num(f),
            base64(png)
        ));
    }

    /// The finished document.
    pub fn finish(self) -> String {
        let [left, bottom, right, top] = self.bounds;
        let (width, height) = (right - left, top - bottom);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}pt\" height=\"{h}pt\" viewBox=\"0 0 {w} {h}\">\n<g transform=\"matrix(1 0 0 -1 {x} {y})\">\n{body}</g>\n</svg>\n",
            w = num(width),
            h = num(height),
            x = num(-left),
            y = num(top),
            body = self.body
        )
    }
}

/// A number as SVG takes it: at most four decimals, no trailing zeros.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

fn colour(rgba: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2])
}

/// The region and the format, read in the worker against the page as the
/// tile that showed it was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GraphicRequest {
    pub page: u32,
    #[serde(default)]
    pub crop: Option<[f32; 4]>,
    #[serde(default)]
    pub turns: u8,
    /// Pixels a point of the tile the region was dragged on.
    pub scale: f32,
    /// The region in that tile's pixels: x, y, width, height.
    pub rect: [i32; 4],
    pub format: GraphicFormat,
    /// One object by its index on the page, as a `Hit` answered it, in place
    /// of the objects inside `rect`.
    #[serde(default)]
    pub object: Option<u32>,
}

pub use engine::export;

mod engine {
    use std::ffi::c_void;
    use std::os::raw::{c_int, c_ulong};

    use pdfium_render::prelude::*;

    use super::{
        concat, hit, inside, scale_for, union, Bounds, GraphicFormat, GraphicFound, GraphicRequest,
        Matrix, Rule, Shape, Step, Stroke, Svg, IDENTITY,
    };
    use crate::document::OpenDocument;
    use crate::progressive::{Bindings, RawPage};

    const TEXT: c_int = 1;
    const PATH: c_int = 2;
    const IMAGE: c_int = 3;
    const SHADING: c_int = 4;
    const FORM: c_int = 5;
    const BGRA: c_int = 4;
    /// `FPDF_REVERSE_BYTE_ORDER`: RGBA out of a BGRA bitmap.
    const REVERSE: c_int = 0x10;
    const SEGMENT_LINE: c_int = 0;
    const SEGMENT_BEZIER: c_int = 1;
    const SEGMENT_MOVE: c_int = 2;

    /// The graphic in `request`'s region: what it is, and with any format but
    /// `Describe`, its bytes.
    pub fn export(
        document: &OpenDocument,
        request: &GraphicRequest,
    ) -> Result<(Vec<u8>, GraphicFound), String> {
        let bindings = document.pdfium().bindings();
        let copy = document.graph().reopen(bindings)?;
        let pages = copy.page_count() as usize;
        let page = copy.page_cropped(request.page, request.crop, &|index| {
            document.graph().sheet(index, pages)
        })?;
        let region = region_pt(bindings, &page, request);
        // SAFETY: every object handle below belongs to `page`, which outlives
        // this function's use of them; removed objects are never destroyed.
        unsafe {
            let handle = page.handle();
            let mut listed = Vec::new();
            for index in 0..bindings.FPDFPage_CountObjects(handle).max(0) {
                let object = bindings.FPDFPage_GetObject(handle, index);
                if object.is_null() {
                    continue;
                }
                if let Some(at) = bounds(bindings, object) {
                    listed.push((index as u32, object, at));
                }
            }
            let picked: Vec<usize> = if let Some(wanted) = request.object {
                let at = listed.iter().position(|(index, ..)| *index == wanted);
                let Some(at) = at else {
                    return Err(said!("the selected graphic is no longer on the page"));
                };
                vec![at]
            } else if request.format == GraphicFormat::Hit {
                let kinds: Vec<(bool, Bounds)> = listed
                    .iter()
                    .map(|&(_, object, at)| (bindings.FPDFPageObj_GetType(object) == TEXT, at))
                    .collect();
                let centre = ((region[0] + region[2]) / 2.0, (region[1] + region[3]) / 2.0);
                let area = f64::from(page.width_pt()) * f64::from(page.height_pt());
                match hit(&kinds, centre, area) {
                    Some(at) => vec![at],
                    None => {
                        return Ok((
                            Vec::new(),
                            GraphicFound {
                                objects: 0,
                                original: None,
                                no_svg: None,
                                width: 0,
                                height: 0,
                                extension: String::new(),
                                object: None,
                                rect: None,
                            },
                        ))
                    }
                }
            } else {
                (0..listed.len())
                    .filter(|&at| inside(listed[at].2, region))
                    .collect()
            };
            let chosen: Vec<FPDF_PAGEOBJECT> = picked.iter().map(|&at| listed[at].1).collect();
            let boxes: Vec<Bounds> = picked.iter().map(|&at| listed[at].2).collect();
            let Some(around) = union(&boxes) else {
                return Err(said!(
                    "no picture or drawing lies wholly inside the region; drag around all of it"
                ));
            };
            let single = (chosen.len() == 1 && bindings.FPDFPageObj_GetType(chosen[0]) == IMAGE)
                .then_some(chosen[0]);
            let original = match single {
                Some(image) => Some(if stored_jpeg(bindings, &copy, &page, image) {
                    "jpg"
                } else {
                    "png"
                }),
                None => None,
            };
            let mut problems = Vec::new();
            for &object in &chosen {
                cannot_draw(bindings, object, &mut problems);
            }
            let no_svg = problems.first().map(|problem| match problem {
                Unsupported::Text => {
                    said!("it holds text, which the SVG cannot draw yet; save it as PNG")
                }
                Unsupported::Gradient => {
                    said!("it holds a gradient, which the SVG cannot draw yet; save it as PNG")
                }
            });
            let scale = scale_for(around[2] - around[0], around[3] - around[1]);
            let (width, height) = match single {
                Some(image) => {
                    native_size(bindings, image).map_or((0, 0), |(w, h)| super::picture_size(w, h))
                }
                None => (
                    ((around[2] - around[0]) * scale).round().max(1.0) as u32,
                    ((around[3] - around[1]) * scale).round().max(1.0) as u32,
                ),
            };
            let mut found = GraphicFound {
                objects: chosen.len() as u32,
                original: original.map(str::to_owned),
                no_svg,
                width,
                height,
                extension: String::new(),
                object: None,
                rect: None,
            };
            if request.format == GraphicFormat::Hit {
                found.object = picked.first().map(|&at| listed[at].0);
                found.rect = Some(to_tile(bindings, &page, request, around));
            }
            let bytes = match (request.format, single) {
                (GraphicFormat::Describe | GraphicFormat::Hit, _) => Vec::new(),
                (GraphicFormat::Original, Some(image)) if original == Some("jpg") => {
                    found.extension = "jpg".to_owned();
                    raw_data(bindings, image)?
                }
                (GraphicFormat::Original | GraphicFormat::Png, Some(image)) => {
                    found.extension = "png".to_owned();
                    native_png(bindings, &copy, &page, image)?
                }
                (GraphicFormat::Original, None) => {
                    return Err(said!(
                        "the region holds more than one picture, or a drawing"
                    ))
                }
                (GraphicFormat::Png, None) => {
                    found.extension = "png".to_owned();
                    for index in (0..bindings.FPDFPage_CountObjects(handle).max(0)).rev() {
                        let object = bindings.FPDFPage_GetObject(handle, index);
                        if !object.is_null() && !chosen.contains(&object) {
                            // Not destroyed: see the module note.
                            bindings.FPDFPage_RemoveObject(handle, object);
                        }
                    }
                    alone_png(bindings, &page, around, scale, width, height)?
                }
                (GraphicFormat::Svg, _) => {
                    if let Some(why) = &found.no_svg {
                        return Err(why.clone());
                    }
                    found.extension = "svg".to_owned();
                    let mut svg = Svg::new(around);
                    for &object in &chosen {
                        draw(bindings, &copy, &page, object, IDENTITY, &mut svg)?;
                    }
                    svg.finish().into_bytes()
                }
            };
            Ok((bytes, found))
        }
    }

    /// `bounds` in the request's tile pixels: left, top, right, bottom.
    fn to_tile(
        bindings: Bindings,
        page: &RawPage<'_>,
        request: &GraphicRequest,
        bounds: Bounds,
    ) -> [f64; 4] {
        // Sixteen times the tile, because the answer is in whole pixels and
        // the outline should not move by one when the zoom does.
        const FINE: f32 = 16.0;
        let width = (page.width_pt() * request.scale * FINE).round() as c_int;
        let height = (page.height_pt() * request.scale * FINE).round() as c_int;
        let (size_x, size_y) = if request.turns % 2 == 0 {
            (width, height)
        } else {
            (height, width)
        };
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (x, y) in [
            (bounds[0], bounds[1]),
            (bounds[2], bounds[1]),
            (bounds[0], bounds[3]),
            (bounds[2], bounds[3]),
        ] {
            let (mut dx, mut dy) = (0 as c_int, 0 as c_int);
            // SAFETY: the page is live; the out-pointers are locals.
            let ok = unsafe {
                bindings.FPDF_PageToDevice(
                    page.handle(),
                    0,
                    0,
                    size_x,
                    size_y,
                    c_int::from(request.turns % 4),
                    x,
                    y,
                    &mut dx,
                    &mut dy,
                )
            };
            if ok != 0 {
                xs.push(f64::from(dx) / f64::from(FINE));
                ys.push(f64::from(dy) / f64::from(FINE));
            }
        }
        let low = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let high = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        [low(&xs), low(&ys), high(&xs), high(&ys)]
    }

    /// The region as page space, from the tile's pixels.
    fn region_pt(bindings: Bindings, page: &RawPage<'_>, request: &GraphicRequest) -> Bounds {
        let width = (page.width_pt() * request.scale).round() as c_int;
        let height = (page.height_pt() * request.scale).round() as c_int;
        let (size_x, size_y) = if request.turns % 2 == 0 {
            (width, height)
        } else {
            (height, width)
        };
        let [x, y, w, h] = request.rect;
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (dx, dy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
            let (mut px, mut py) = (0f64, 0f64);
            // SAFETY: the page is live; the out-pointers are locals.
            let ok = unsafe {
                bindings.FPDF_DeviceToPage(
                    page.handle(),
                    0,
                    0,
                    size_x,
                    size_y,
                    c_int::from(request.turns % 4),
                    dx,
                    dy,
                    &mut px,
                    &mut py,
                )
            };
            if ok != 0 {
                xs.push(px);
                ys.push(py);
            }
        }
        let low = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let high = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        [low(&xs), low(&ys), high(&xs), high(&ys)]
    }

    /// An object's bounds in page space, when PDFium reports finite ones.
    unsafe fn bounds(bindings: Bindings, object: FPDF_PAGEOBJECT) -> Option<Bounds> {
        let (mut l, mut b, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
        if bindings.FPDFPageObj_GetBounds(object, &mut l, &mut b, &mut r, &mut t) == 0 {
            return None;
        }
        let at = [l, b, r, t].map(f64::from);
        (at.iter().all(|v| v.is_finite()) && at[0] <= at[2] && at[1] <= at[3]).then_some(at)
    }

    unsafe fn matrix(bindings: Bindings, object: FPDF_PAGEOBJECT) -> Matrix {
        let mut m = FS_MATRIX {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        if bindings.FPDFPageObj_GetMatrix(object, &mut m) == 0 {
            return IDENTITY;
        }
        [m.a, m.b, m.c, m.d, m.e, m.f].map(f64::from)
    }

    /// What the SVG cannot draw.
    enum Unsupported {
        Text,
        Gradient,
    }

    /// What in `object`, at any depth, the SVG cannot draw.
    unsafe fn cannot_draw(
        bindings: Bindings,
        object: FPDF_PAGEOBJECT,
        problems: &mut Vec<Unsupported>,
    ) {
        match bindings.FPDFPageObj_GetType(object) {
            TEXT => problems.push(Unsupported::Text),
            SHADING => problems.push(Unsupported::Gradient),
            FORM => {
                for index in 0..bindings.FPDFFormObj_CountObjects(object).max(0) {
                    let child = bindings.FPDFFormObj_GetObject(object, index as c_ulong);
                    if !child.is_null() {
                        cannot_draw(bindings, child, problems);
                    }
                }
            }
            _ => {}
        }
    }

    /// Adds `object` and, for a form, what it holds, to `svg`.
    unsafe fn draw(
        bindings: Bindings,
        document: &crate::progressive::RawDocument,
        page: &RawPage<'_>,
        object: FPDF_PAGEOBJECT,
        parent: Matrix,
        svg: &mut Svg,
    ) -> Result<(), String> {
        let own = concat(matrix(bindings, object), parent);
        match bindings.FPDFPageObj_GetType(object) {
            PATH => svg.shape(&shape(bindings, object, own)),
            FORM => {
                for index in 0..bindings.FPDFFormObj_CountObjects(object).max(0) {
                    let child = bindings.FPDFFormObj_GetObject(object, index as c_ulong);
                    if !child.is_null() {
                        draw(bindings, document, page, child, own, svg)?;
                    }
                }
            }
            IMAGE => svg.picture(&native_png(bindings, document, page, object)?, own),
            _ => {}
        }
        Ok(())
    }

    unsafe fn shape(bindings: Bindings, object: FPDF_PAGEOBJECT, matrix: Matrix) -> Shape {
        let mut steps = Vec::new();
        let mut curve: Vec<(f64, f64)> = Vec::new();
        for index in 0..bindings.FPDFPath_CountSegments(object).max(0) {
            let segment = bindings.FPDFPath_GetPathSegment(object, index);
            if segment.is_null() {
                continue;
            }
            let (mut x, mut y) = (0f32, 0f32);
            bindings.FPDFPathSegment_GetPoint(segment, &mut x, &mut y);
            let (x, y) = (f64::from(x), f64::from(y));
            match bindings.FPDFPathSegment_GetType(segment) {
                SEGMENT_MOVE => steps.push(Step::Move(x, y)),
                SEGMENT_LINE => steps.push(Step::Line(x, y)),
                SEGMENT_BEZIER => {
                    curve.push((x, y));
                    if curve.len() == 3 {
                        let [(a, b), (c, d), (e, f)] = [curve[0], curve[1], curve[2]];
                        steps.push(Step::Curve(a, b, c, d, e, f));
                        curve.clear();
                    }
                }
                _ => {}
            }
            if bindings.FPDFPathSegment_GetClose(segment) != 0 {
                steps.push(Step::Close);
            }
        }
        let (mut mode, mut stroked) = (0 as c_int, 0);
        bindings.FPDFPath_GetDrawMode(object, &mut mode, &mut stroked);
        let fill = match mode {
            1 => Some(Rule::EvenOdd),
            2 => Some(Rule::NonZero),
            _ => None,
        }
        .map(|rule| (rule, colour(bindings, object, false)));
        let stroke = (stroked != 0).then(|| {
            let mut width = 1f32;
            bindings.FPDFPageObj_GetStrokeWidth(object, &mut width);
            let count = bindings.FPDFPageObj_GetDashCount(object).max(0) as usize;
            let mut dash = vec![0f32; count];
            if count > 0 {
                bindings.FPDFPageObj_GetDashArray(object, dash.as_mut_ptr(), count);
            }
            let mut phase = 0f32;
            bindings.FPDFPageObj_GetDashPhase(object, &mut phase);
            Stroke {
                rgba: colour(bindings, object, true),
                width: f64::from(width),
                cap: bindings.FPDFPageObj_GetLineCap(object),
                join: bindings.FPDFPageObj_GetLineJoin(object),
                dash: dash.into_iter().map(f64::from).collect(),
                phase: f64::from(phase),
            }
        });
        Shape {
            steps,
            matrix,
            fill,
            stroke,
        }
    }

    unsafe fn colour(bindings: Bindings, object: FPDF_PAGEOBJECT, stroke: bool) -> [u8; 4] {
        let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 255u32);
        if stroke {
            bindings.FPDFPageObj_GetStrokeColor(object, &mut r, &mut g, &mut b, &mut a);
        } else {
            bindings.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut b, &mut a);
        }
        [r, g, b, a].map(|v| v.min(255) as u8)
    }

    /// The picture's own pixel size.
    unsafe fn native_size(bindings: Bindings, image: FPDF_PAGEOBJECT) -> Option<(u32, u32)> {
        let (mut w, mut h) = (0u32, 0u32);
        (bindings.FPDFImageObj_GetImagePixelSize(image, &mut w, &mut h) != 0 && w > 0 && h > 0)
            .then_some((w, h))
    }

    /// The picture with its mask applied, as RGBA, from PDFium's rendering of it.
    unsafe fn native_rgba(
        bindings: Bindings,
        document: &crate::progressive::RawDocument,
        page: &RawPage<'_>,
        image: FPDF_PAGEOBJECT,
    ) -> Result<(Vec<u8>, u32, u32), String> {
        // PDFium draws the picture at one pixel a point of its matrix, which
        // is its size on the page: 4 x 2 pixels shown 80 x 40 points wide
        // came back 80 x 40 (measured with the fixture in `tests/cli`). So the
        // matrix is set to the picture's own size, upright, for the call and
        // put back after; the page is the private copy either way.
        let (w, h) =
            native_size(bindings, image).ok_or_else(|| said!("the picture has no size"))?;
        let (w, h) = super::picture_size(w, h);
        let mut kept = FS_MATRIX {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        bindings.FPDFPageObj_GetMatrix(image, &mut kept);
        let own = FS_MATRIX {
            a: w as f32,
            b: 0.0,
            c: 0.0,
            d: h as f32,
            e: kept.e,
            f: kept.f,
        };
        bindings.FPDFPageObj_SetMatrix(image, &own);
        let bitmap =
            bindings.FPDFImageObj_GetRenderedBitmap(document.handle(), page.handle(), image);
        bindings.FPDFPageObj_SetMatrix(image, &kept);
        if bitmap.is_null() {
            return Err(said!("the picture could not be read"));
        }
        let width = bindings.FPDFBitmap_GetWidth(bitmap).max(0) as usize;
        let height = bindings.FPDFBitmap_GetHeight(bitmap).max(0) as usize;
        let stride = bindings.FPDFBitmap_GetStride(bitmap).max(0) as usize;
        let format = bindings.FPDFBitmap_GetFormat(bitmap);
        let buffer = bindings.FPDFBitmap_GetBuffer(bitmap) as *const u8;
        let channels = match format {
            1 => 1,
            2 => 3,
            3 | 4 => 4,
            _ => 0,
        };
        if buffer.is_null()
            || channels == 0
            || width == 0
            || height == 0
            || stride < width * channels
        {
            bindings.FPDFBitmap_Destroy(bitmap);
            return Err(said!("the picture came back in a form tpdf does not read"));
        }
        let source = std::slice::from_raw_parts(buffer, stride * height);
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in source.chunks(stride).take(height) {
            for pixel in row[..width * channels].chunks(channels) {
                match format {
                    1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
                    2 => rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]),
                    3 => rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]),
                    _ => rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]),
                }
            }
        }
        bindings.FPDFBitmap_Destroy(bitmap);
        Ok((rgba, width as u32, height as u32))
    }

    unsafe fn native_png(
        bindings: Bindings,
        document: &crate::progressive::RawDocument,
        page: &RawPage<'_>,
        image: FPDF_PAGEOBJECT,
    ) -> Result<Vec<u8>, String> {
        let (rgba, width, height) = native_rgba(bindings, document, page, image)?;
        crate::render::encode_png(&rgba, width, height)
    }

    /// Whether the picture is a JPEG stored as one, with nothing a JPEG cannot
    /// carry: one `DCTDecode` filter, and no pixel its mask makes transparent.
    unsafe fn stored_jpeg(
        bindings: Bindings,
        document: &crate::progressive::RawDocument,
        page: &RawPage<'_>,
        image: FPDF_PAGEOBJECT,
    ) -> bool {
        if bindings.FPDFImageObj_GetImageFilterCount(image) != 1 {
            return false;
        }
        let mut name = [0u8; 32];
        let length = bindings.FPDFImageObj_GetImageFilter(
            image,
            0,
            name.as_mut_ptr() as *mut c_void,
            name.len() as c_ulong,
        ) as usize;
        if name.get(..length.saturating_sub(1)) != Some(b"DCTDecode".as_slice()) {
            return false;
        }
        native_rgba(bindings, document, page, image)
            .is_ok_and(|(rgba, _, _)| rgba.chunks(4).all(|p| p[3] == 255))
    }

    /// The picture's stream as stored, filters not undone.
    unsafe fn raw_data(bindings: Bindings, image: FPDF_PAGEOBJECT) -> Result<Vec<u8>, String> {
        let length = bindings.FPDFImageObj_GetImageDataRaw(image, std::ptr::null_mut(), 0) as usize;
        if length == 0 || length > crate::worker::TILE_CAPACITY {
            return Err(said!("the picture's data could not be read"));
        }
        let mut bytes = vec![0u8; length];
        bindings.FPDFImageObj_GetImageDataRaw(
            image,
            bytes.as_mut_ptr() as *mut c_void,
            length as c_ulong,
        );
        Ok(bytes)
    }

    /// What is left on the page, drawn alone on a transparent bitmap: the
    /// region `around` at `scale` pixels a point, `width` by `height`.
    unsafe fn alone_png(
        bindings: Bindings,
        page: &RawPage<'_>,
        around: Bounds,
        scale: f64,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        // Where the graphic's top-left corner lands with the page drawn one
        // pixel a point, which is the space PDFium's matrix render starts in.
        let size_x = page.width_pt().round() as c_int;
        let size_y = page.height_pt().round() as c_int;
        let mut corners = Vec::new();
        for (x, y) in [
            (around[0], around[3]),
            (around[2], around[1]),
            (around[0], around[1]),
            (around[2], around[3]),
        ] {
            let (mut dx, mut dy) = (0f64, 0f64);
            let (mut ix, mut iy) = (0 as c_int, 0 as c_int);
            if bindings.FPDF_PageToDevice(
                page.handle(),
                0,
                0,
                size_x * 64,
                size_y * 64,
                0,
                x,
                y,
                &mut ix,
                &mut iy,
            ) != 0
            {
                dx = f64::from(ix) / 64.0;
                dy = f64::from(iy) / 64.0;
            }
            corners.push((dx, dy));
        }
        let left = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let top = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let (w, h) = (width as usize, height as usize);
        let mut buffer = vec![0u8; w * h * 4];
        let bitmap = bindings.FPDFBitmap_CreateEx(
            width as c_int,
            height as c_int,
            BGRA,
            buffer.as_mut_ptr() as *mut c_void,
            (w * 4) as c_int,
        );
        if bitmap.is_null() {
            return Err(said!("a {width} x {height} picture could not be made"));
        }
        let matrix = FS_MATRIX {
            a: scale as f32,
            b: 0.0,
            c: 0.0,
            d: scale as f32,
            e: (-left * scale) as f32,
            f: (-top * scale) as f32,
        };
        let clip = FS_RECTF {
            left: 0.0,
            top: 0.0,
            right: width as f32,
            bottom: height as f32,
        };
        bindings.FPDF_RenderPageBitmapWithMatrix(bitmap, page.handle(), &matrix, &clip, REVERSE);
        bindings.FPDFBitmap_Destroy(bitmap);
        crate::render::encode_png(&buffer, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_counts_as_inside_with_slack_and_never_when_empty() {
        let region = [100.0, 100.0, 200.0, 200.0];
        assert!(inside([101.0, 101.0, 199.0, 199.0], region));
        assert!(
            inside([99.0, 99.0, 201.0, 201.0], region),
            "within the slack"
        );
        assert!(
            !inside([90.0, 120.0, 150.0, 150.0], region),
            "sticks out on the left"
        );
        assert!(!inside([150.0, 150.0, 150.0, 160.0], region), "no width");
    }

    #[test]
    fn a_click_takes_the_topmost_graphic_under_it_and_never_text_or_the_page() {
        let page = 595.0 * 842.0;
        let background = (false, [0.0, 0.0, 595.0, 842.0]);
        let logo = (false, [50.0, 760.0, 290.0, 805.0]);
        let mark = (false, [60.0, 770.0, 80.0, 790.0]);
        let words = (true, [55.0, 765.0, 200.0, 800.0]);
        // Mutation: `position` for `rposition` -> the logo under the mark.
        assert_eq!(hit(&[background, logo, mark], (70.0, 780.0), page), Some(2));
        assert_eq!(
            hit(&[background, logo, mark], (200.0, 780.0), page),
            Some(1)
        );
        assert_eq!(
            hit(&[background, logo, words], (100.0, 780.0), page),
            Some(1),
            "text is passed over"
        );
        assert_eq!(
            hit(&[background, logo], (300.0, 100.0), page),
            None,
            "the page white is not a graphic"
        );
        assert_eq!(
            hit(&[logo], (291.0, 780.0), page),
            Some(0),
            "within the slack"
        );
        assert_eq!(hit(&[logo], (295.0, 780.0), page), None);
    }

    #[test]
    fn the_union_holds_every_box_and_none_is_none() {
        assert_eq!(union(&[]), None);
        assert_eq!(
            union(&[[0.0, 5.0, 10.0, 6.0], [-2.0, 1.0, 3.0, 9.0]]),
            Some([-2.0, 1.0, 10.0, 9.0])
        );
    }

    #[test]
    fn the_scale_is_600_dpi_until_the_pixel_budget_lowers_it() {
        assert!((scale_for(216.0, 58.0) - GRAPHIC_DPI / 72.0).abs() < 1e-9);
        let big = scale_for(595.0, 842.0);
        assert!(big < GRAPHIC_DPI / 72.0);
        assert!((595.0 * big) * (842.0 * big) <= MAX_PIXELS + 1.0);
    }

    #[test]
    fn a_picture_keeps_its_size_until_the_pixel_budget_and_then_its_proportions() {
        assert_eq!(picture_size(157, 51), (157, 51));
        let (w, h) = picture_size(6000, 4000);
        assert!(f64::from(w) * f64::from(h) <= MAX_PIXELS, "{w} x {h}");
        assert!(
            (f64::from(w) / f64::from(h) - 1.5).abs() < 0.01,
            "{w} x {h}"
        );
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), output, "{input:?}");
        }
    }

    #[test]
    fn concat_applies_the_first_matrix_first() {
        let scale = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        let shift = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];
        assert_eq!(concat(scale, shift), [2.0, 0.0, 0.0, 2.0, 10.0, 0.0]);
        assert_eq!(concat(shift, scale), [2.0, 0.0, 0.0, 2.0, 20.0, 0.0]);
    }

    #[test]
    fn the_svg_flips_page_space_and_keeps_each_paths_matrix_and_paint() {
        let mut svg = Svg::new([100.0, 700.0, 160.0, 720.0]);
        svg.shape(&Shape {
            steps: vec![
                Step::Move(0.0, 0.0),
                Step::Line(10.0, 0.0),
                Step::Curve(10.0, 5.0, 5.0, 10.0, 0.0, 10.0),
                Step::Close,
            ],
            matrix: [1.0, 0.0, 0.0, 1.0, 110.0, 705.0],
            fill: Some((Rule::EvenOdd, [0x1f, 0x40, 0x80, 128])),
            stroke: Some(Stroke {
                rgba: [255, 0, 0, 255],
                width: 0.5,
                cap: 1,
                join: 2,
                dash: vec![3.0, 1.0],
                phase: 0.0,
            }),
        });
        // Neither fill nor stroke: nothing is written for it.
        svg.shape(&Shape {
            steps: vec![Step::Move(0.0, 0.0), Step::Line(1.0, 1.0)],
            matrix: IDENTITY,
            fill: None,
            stroke: None,
        });
        let text = svg.finish();
        assert!(
            text.contains("width=\"60pt\" height=\"20pt\" viewBox=\"0 0 60 20\""),
            "{text}"
        );
        assert!(
            text.contains("<g transform=\"matrix(1 0 0 -1 -100 720)\">"),
            "{text}"
        );
        assert!(text.contains("d=\"M0 0L10 0C10 5 5 10 0 10Z\""), "{text}");
        assert!(
            text.contains("transform=\"matrix(1 0 0 1 110 705)\""),
            "{text}"
        );
        assert!(
            text.contains("fill=\"#1f4080\" fill-opacity=\"0.502\" fill-rule=\"evenodd\""),
            "{text}"
        );
        assert!(
            text.contains("stroke=\"#ff0000\" stroke-width=\"0.5\""),
            "{text}"
        );
        assert!(
            text.contains("stroke-linecap=\"round\" stroke-linejoin=\"bevel\""),
            "{text}"
        );
        assert!(text.contains("stroke-dasharray=\"3 1\""), "{text}");
        assert_eq!(text.matches("<path ").count(), 1, "{text}");
    }

    #[test]
    fn a_picture_is_flipped_into_its_unit_square() {
        let mut svg = Svg::new([0.0, 0.0, 100.0, 50.0]);
        svg.picture(b"x", [100.0, 0.0, 0.0, 50.0, 0.0, 0.0]);
        let text = svg.finish();
        // Row one of the picture goes to the top of its square: y 1 -> 50.
        assert!(
            text.contains("transform=\"matrix(100 0 0 -50 0 50)\""),
            "{text}"
        );
        assert!(
            text.contains("href=\"data:image/png;base64,eA==\""),
            "{text}"
        );
    }

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.12345), "0.1235");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(f64::NAN), "0");
    }
}
