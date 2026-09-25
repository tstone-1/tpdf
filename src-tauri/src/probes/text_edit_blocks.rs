//! What a paragraph model would have to work with, on an unchanged document.
//!
//! `text-edit-probe --blocks <source.pdf> [--agree-every=N]` prints one JSON report and
//! writes nothing. Wrapping an edit onto a new line needs three things the editor's
//! scanner does not have: which runs are lines of one block, what the leading between
//! them is, and what is below the block's last line. This measures the evidence for all
//! three, per editable run with visible text on each page (at most 128 pages):
//!
//! - **Why a longer edit is still refused.** The same three growth trials the length
//!   survey uses (`grow10`, `grow25`, `grow50`: characters appended until the text is at
//!   least that much longer, taken only from the run's own characters), in the one mode
//!   the editor actually sends -- `app`, the layout of `defaultTextLayout` with `grow`
//!   set. The verdict string names what stopped it, which is what separates a refusal
//!   wrapping could serve (the text would leave the page) from one it could not. The
//!   `patch` and `widened` modes of `--growth` are deliberately absent: neither is a
//!   layout the editor sends, and the ladder is most of that instrument's cost.
//! - **What the document states.** `tagged` per page, and per run the MCID, its owning
//!   structure element and that element's role. Read here **independently of**
//!   `textedit::tagging`, which keeps one tag *name* per MCID and drops the element that
//!   owns it -- so on a tagged page it can say *this text is in a paragraph* and cannot
//!   say *these two runs are in the same paragraph*, which is the question a block model
//!   asks. The probe walks the parent tree itself and keeps the owner.
//! - **What the page paints.** One render per page, at one pixel per point where the tile
//!   buffer allows it, reduced to `ink_below`: the clear distance in page points from the
//!   run's hit rectangle straight down to the first pixel that is not the page's own
//!   background, within the run's own horizontal span. A render is the only complete
//!   answer here -- `Inspection::graphics` says itself that it is not -- and it covers
//!   the figures, rules and annotation appearances no object list the editor keeps does.
//!
//! Acceptance is decided by `textedit::write`, in this process, on the parsed document,
//! exactly as `--growth` decides it: a refused write changes nothing, an accepted one is
//! undone by restoring the page dictionary and dropping the objects it added, and each
//! page's runs are rescanned afterwards to prove the restore. `--agree-every=N` sends
//! every Nth trial through the contained worker as well and counts any verdict that
//! differs. Parsing the document in this process is acceptable for a measurement over
//! files whose digests were checked first; it is why this is a probe and never an
//! application path, where every parse belongs to a sandboxed worker.
//!
//! What it does not measure, and none of these is a gap a reader can close by squinting
//! at the numbers:
//!
//! - **It defines no block.** It emits the signals a block rule would read -- baselines,
//!   left edges, font, size, the clear space between lines -- and never a verdict about
//!   which runs form one. The rule, and the measurement of how often it agrees with the
//!   tags, live in `scripts/textedit_blocks.py`, so that the instrument cannot be the
//!   evidence for its own rule.
//! - **The MCID map can be unavailable.** It comes from this probe's own decode of the
//!   page content, which must address the same operators the scanner does; where it does
//!   not, the page reports `mcid_map` as a reason and no run on it carries an MCID. A
//!   guess would be worse than an absence.
//! - **`ink_below` is a lower bound on the free space and an upper bound on nothing.** It
//!   is measured from the run's hit rectangle, which is a full em box rather than the ink
//!   inside it, and it stops at the first non-background pixel whatever drew it. A page
//!   whose background is not one colour reports an `ink_fraction` high enough to see.
//! - Runs that are not axis-aligned on an upright page carry no geometry here at all.
use super::*;
use std::collections::BTreeMap;
use std::path::Path;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde_json::{json, Value};

const GROWTHS: [(&str, f64); 3] = [("grow10", 0.10), ("grow25", 0.25), ("grow50", 0.50)];
/// The render must fit the worker's shared tile buffer, which holds 2048 x 2048 pixels.
const MAX_RENDER_PIXELS: f64 = 2048. * 2048.;
/// A pixel counts as ink when a channel differs from the page's own background by more
/// than this. An anti-aliased glyph edge reaches the background within a few units.
const INK_TOLERANCE: i32 = 12;

/// The layout `defaultTextLayout` builds, in the same arithmetic as
/// `text_edit_growth::app_layout` and for the same reason: `app` is what the editor
/// sends, so a verdict taken under any other layout is about a box no reader has.
fn app_layout(run: &textedit::Run) -> textedit::Layout {
    let x = run.matrix[0].hypot(run.matrix[1]);
    let y = run.matrix[2].hypot(run.matrix[3]);
    let round = |value: f64| (value * 1000.).ceil() / 1000.;
    let source_size = run.size * y;
    let size = round(source_size);
    let height = (source_size * 1.25).max(run.minimum_height.unwrap_or(0.)) * size / source_size;
    textedit::Layout {
        width: round(run.advance * x).max(0.1),
        height: round(height).max(0.1),
        size,
        wrap: false,
        font: textedit::EditFont::Auto,
        grow: true,
    }
}

/// The original followed by at least `ceil(n * growth)` characters, drawn by cycling
/// through the run's own text and one space if it contains one, so a missing glyph
/// cannot be the reason a trial is refused. Identical to `--growth`'s, deliberately:
/// the two instruments have to be talking about the same trials.
fn grown(text: &str, growth: f64) -> String {
    let n = text.chars().count();
    let wanted = ((n as f64 * growth).ceil() as usize).max(1);
    let mut source: Vec<char> = text.chars().collect();
    if text.contains(' ') {
        source.push(' ');
    }
    let mut out = String::from(text);
    let mut added = 0;
    let mut i = 0;
    while added < wanted || out.ends_with(char::is_whitespace) {
        out.push(source[i % source.len()]);
        added += 1;
        i += 1;
    }
    out
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.)
}

/// Equal discovery, allowing floats to differ in the last places, as `--growth` does:
/// the worker's reply crosses a JSON channel whose parser does not round-trip every f64.
fn same_runs(a: &textedit::PageRuns, b: &textedit::PageRuns) -> bool {
    a.revision == b.revision
        && a.runs.len() == b.runs.len()
        && a.runs.iter().zip(&b.runs).all(|(x, y)| {
            x.operator == y.operator
                && x.text == y.text
                && x.font == y.font
                && close(x.size, y.size)
                && close(x.advance, y.advance)
                && x.matrix.iter().zip(&y.matrix).all(|(p, q)| close(*p, *q))
                && x.display_rect
                    .iter()
                    .zip(&y.display_rect)
                    .all(|(p, q)| close(f64::from(*p), f64::from(*q)))
        })
}

/// This probe's own indirect-reference walk, bounded.
///
/// `encoding::resolve` is the application's and is crate-private; an example reaches
/// neither it nor the editor's parse, which is the point -- the structure tree below is
/// read here independently of the module whose answer it is a control on.
fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    let mut current = object;
    for _ in 0..32 {
        let Object::Reference(id) = current else {
            return current;
        };
        match doc.get_object(*id) {
            Ok(next) => current = next,
            Err(_) => return current,
        }
    }
    current
}

struct Page {
    id: ObjectId,
    dictionary: Object,
}

/// Run `textedit::write` and put the document back exactly as it was.
fn trial(doc: &mut Document, page: &Page, change: textedit::Change) -> Result<(), String> {
    let max_id = doc.max_id;
    // A wrap rewrites the rectangle of each link it moves, which is an object
    // the document already had: dropping the new objects and restoring the
    // page does not put those back, and the next trial would meet the links
    // where the last accepted one left them.
    let annotations: Vec<(ObjectId, Object)> = {
        let list = doc
            .get_dictionary(page.id)
            .and_then(|dict| dict.get(b"Annots"));
        let items = match list {
            Ok(Object::Reference(id)) => doc.get_object(*id).and_then(Object::as_array),
            Ok(list) => list.as_array(),
            Err(error) => Err(error),
        };
        items
            .map(|items| items.iter().filter_map(|e| e.as_reference().ok()).collect())
            .unwrap_or_else(|_| Vec::new())
            .into_iter()
            .filter_map(|id| doc.get_object(id).ok().map(|object| (id, object.clone())))
            .collect()
    };
    let result = textedit::write(doc, &[change]);
    if result.is_ok() {
        doc.objects.retain(|id, _| id.0 <= max_id);
        doc.objects.insert(page.id, page.dictionary.clone());
        doc.objects.extend(annotations);
        doc.max_id = max_id;
    }
    result
}

fn verdict(result: &Result<(), String>) -> Value {
    match result {
        Ok(()) => Value::String("ok".into()),
        Err(reason) => Value::String(reason.clone()),
    }
}

struct Agreement {
    every: usize,
    seen: usize,
    checked: usize,
    disagreements: Vec<Value>,
}

impl Agreement {
    fn check(
        &mut self,
        worker: &mut Worker,
        change: &textedit::Change,
        local: &Result<(), String>,
        label: &str,
    ) -> Result<(), String> {
        self.seen += 1;
        if self.every == 0 || self.seen % self.every != 0 {
            return Ok(());
        }
        self.checked += 1;
        let answer = worker.call(&Request::TextRuns {
            page: change.page,
            changes: vec![change.clone()],
        })?;
        let remote = if answer.ok { Ok(()) } else { Err(answer.error) };
        if &remote != local {
            self.disagreements.push(json!({
                "page": change.page, "operator": change.operator, "trial": label,
                "local": verdict(local), "worker": verdict(&remote),
            }));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The structure tree, read here rather than taken from `textedit::tagging`.
// ---------------------------------------------------------------------------

/// One page's MCID slots, and every element on the path from one of them to the root.
#[derive(Default)]
struct Tagged {
    /// By MCID: the owning structure element, where the parent-tree slot names one.
    owners: Vec<Option<ObjectId>>,
    /// Each element reached: its own `/S`, `/S` after the root's `/RoleMap`, and `/P`.
    ///
    /// The ancestors are here because a `Span` or `NonStruct` leaf owns the text while
    /// the block that owns *the line* is one of its ancestors, and which ancestor
    /// counts as a block is a judgement. The probe carries the chain and makes none.
    elements: BTreeMap<ObjectId, (String, String, Option<ObjectId>)>,
}

fn name_of(object: &Object) -> Option<String> {
    object
        .as_name()
        .ok()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// The value a number tree holds for `key`, or `None` (ISO 32000-1 7.9.7).
///
/// Bounded rather than trusted: a tree deeper than `depth` or wider than 4,096 kids at
/// one level answers nothing, which reports as an absent map and not as an untagged page.
fn number_tree<'a>(
    doc: &'a Document,
    node: &'a Object,
    key: i64,
    depth: usize,
) -> Option<&'a Object> {
    let dict = resolve(doc, node).as_dict().ok()?;
    if let Ok(nums) = dict.get(b"Nums") {
        if let Ok(nums) = resolve(doc, nums).as_array() {
            for pair in nums.chunks(2) {
                if let [k, value] = pair {
                    if k.as_i64().ok() == Some(key) {
                        return Some(value);
                    }
                }
            }
        }
    }
    if depth == 0 {
        return None;
    }
    let kids = resolve(doc, dict.get(b"Kids").ok()?).as_array().ok()?;
    if kids.len() > 4096 {
        return None;
    }
    for kid in kids {
        let limits = resolve(doc, kid)
            .as_dict()
            .ok()
            .and_then(|kid| kid.get(b"Limits").ok())
            .and_then(|limits| resolve(doc, limits).as_array().ok());
        if let Some([lo, hi]) = limits.map(Vec::as_slice) {
            if let (Ok(lo), Ok(hi)) = (lo.as_i64(), hi.as_i64()) {
                if !(lo..=hi).contains(&key) {
                    continue;
                }
            }
        }
        if let Some(found) = number_tree(doc, kid, key, depth - 1) {
            return Some(found);
        }
    }
    None
}

/// The page's MCID slots, from `/StructParents` into the structure root's parent tree.
///
/// `Ok(None)` is an untagged page: no structure root, or this page claims no slot in one.
fn tagged(doc: &Document, page: ObjectId) -> Result<Option<Tagged>, String> {
    let Ok(catalog) = doc.catalog() else {
        return Ok(None);
    };
    let Ok(root) = catalog.get(b"StructTreeRoot") else {
        return Ok(None);
    };
    let root = resolve(doc, root)
        .as_dict()
        .map_err(|_| "structure root is not a dictionary")?;
    let dict = doc
        .get_dictionary(page)
        .map_err(|_| "page is not a dictionary")?;
    let Ok(key) = dict.get(b"StructParents") else {
        return Ok(None);
    };
    let key = resolve(doc, key)
        .as_i64()
        .map_err(|_| "page structure key is not an integer")?;
    let tree = root
        .get(b"ParentTree")
        .map_err(|_| "structure root has no parent tree")?;
    let Some(slots) = number_tree(doc, tree, key, 16) else {
        return Ok(None);
    };
    // A page's slot is an array, one entry per MCID. A single element there would be a
    // content item's own parent, which is what an annotation's `/StructParent` holds.
    let Ok(slots) = resolve(doc, slots).as_array() else {
        return Ok(None);
    };
    let map = root
        .get(b"RoleMap")
        .ok()
        .and_then(|value| resolve(doc, value).as_dict().ok());
    let mut elements: BTreeMap<ObjectId, (String, String, Option<ObjectId>)> = BTreeMap::new();
    let mut owners = Vec::with_capacity(slots.len());
    for slot in slots {
        let Object::Reference(id) = slot else {
            owners.push(None);
            continue;
        };
        owners.push(Some(*id));
        // Every element from the owner up to the root, so that the driver can decide
        // which ancestor is the block. Bounded, and a cycle stops at the first repeat.
        let mut next = Some(*id);
        for _ in 0..32 {
            let Some(id) = next.filter(|id| !elements.contains_key(id)) else {
                break;
            };
            let Ok(element) = doc.get_dictionary(id) else {
                break;
            };
            let role = element
                .get(b"S")
                .ok()
                .map(|value| resolve(doc, value))
                .and_then(name_of)
                .unwrap_or_default();
            let mapped = map
                .and_then(|map| map.get(role.as_bytes()).ok())
                .map(|value| resolve(doc, value))
                .and_then(name_of)
                .unwrap_or_else(|| role.clone());
            let parent = match element.get(b"P") {
                Ok(Object::Reference(parent)) => Some(*parent),
                _ => None,
            };
            elements.insert(id, (role, mapped, parent));
            next = parent;
        }
    }
    Ok(Some(Tagged { owners, elements }))
}

fn showing(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

/// The MCID an operand names, directly or through the page's `/Properties`.
fn mcid_of(
    doc: &Document,
    properties: Option<&Dictionary>,
    operand: Option<&Object>,
) -> Option<usize> {
    let list = match resolve(doc, operand?) {
        Object::Dictionary(dict) => dict,
        Object::Name(name) => resolve(doc, properties?.get(name).ok()?).as_dict().ok()?,
        _ => return None,
    };
    usize::try_from(resolve(doc, list.get(b"MCID").ok()?).as_i64().ok()?).ok()
}

/// Each text-showing operation's innermost MCID, from this probe's own decode.
///
/// The editor addresses a run by its index in the decoded content, so the same index in
/// the same decode names the same operator -- which the caller checks rather than assumes.
fn marked(
    doc: &Document,
    page: ObjectId,
    content: &Content,
) -> Result<BTreeMap<u32, usize>, String> {
    let properties = doc
        .get_dictionary(page)
        .ok()
        .and_then(|dict| dict.get(b"Resources").ok())
        .and_then(|value| resolve(doc, value).as_dict().ok())
        .and_then(|res| res.get(b"Properties").ok())
        .and_then(|value| resolve(doc, value).as_dict().ok());
    let mut stack: Vec<Option<usize>> = Vec::new();
    let mut found = BTreeMap::new();
    for (index, operation) in content.operations.iter().enumerate() {
        match operation.operator.as_str() {
            "BDC" => {
                if stack.len() >= 32 {
                    return Err("marked content nests deeper than 32".into());
                }
                stack.push(mcid_of(doc, properties, operation.operands.get(1)));
            }
            "BMC" => {
                if stack.len() >= 32 {
                    return Err("marked content nests deeper than 32".into());
                }
                stack.push(None);
            }
            "EMC" => {
                if stack.pop().is_none() {
                    return Err("unmatched marked-content end".into());
                }
            }
            other if showing(other) => {
                if let Some(mcid) = stack.iter().rev().flatten().next() {
                    found.insert(index as u32, *mcid);
                }
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("marked content left open at the end of the page".into());
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// What the page paints.
// ---------------------------------------------------------------------------

/// One page rendered, reduced to a per-pixel "is this the page's own background".
struct Ink {
    /// Row-major, `width * height`, true where the pixel is not the background.
    ink: Vec<bool>,
    width: usize,
    height: usize,
    /// Pixels per page point.
    scale: f64,
    /// The share of the page that is not background, so a page whose background is not
    /// one colour can be seen rather than believed.
    fraction: f64,
}

impl Ink {
    /// The clear distance in page points straight down from `rect`'s bottom edge, within
    /// its own horizontal span, to the first pixel that is not background, and whether
    /// nothing was found before the page ran out.
    ///
    /// The scan starts one pixel below the rectangle, because a glyph's own
    /// anti-aliasing reaches the hit box's edge.
    fn below(&self, rect: [f32; 4], page_height: f64) -> (f64, bool) {
        let bottom = f64::from(rect[3]);
        let x0 = ((f64::from(rect[0]) * self.scale).floor().max(0.) as usize).min(self.width);
        let x1 = ((f64::from(rect[2]) * self.scale).ceil().max(0.) as usize).clamp(x0, self.width);
        let start = (((bottom * self.scale).ceil() as i64) + 1).max(0) as usize;
        for y in start..self.height {
            if self.ink[y * self.width + x0..y * self.width + x1]
                .iter()
                .any(|&on| on)
            {
                return ((y as f64 / self.scale - bottom).max(0.), false);
            }
        }
        ((page_height - bottom).max(0.), true)
    }
}

fn render(worker: &mut Worker, page: u32, width: f32, height: f32) -> Result<Ink, String> {
    let (w, h) = (f64::from(width), f64::from(height));
    if !(w >= 1. && h >= 1.) {
        return Err("page has no displayed size".into());
    }
    let scale = ((MAX_RENDER_PIXELS / (w * h)).sqrt().min(1.)) as f32;
    let pixels_wide = (w * f64::from(scale)).floor().max(1.) as usize;
    let pixels_high = (h * f64::from(scale)).floor().max(1.) as usize;
    let response = worker.call(&Request::Tile {
        rid: 0,
        page,
        scale,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: u16::try_from(pixels_wide).map_err(|_| "page is wider than a tile")?,
        height: u16::try_from(pixels_high).map_err(|_| "page is taller than a tile")?,
        png: false,
        crop: None,
    })?;
    if !response.ok {
        return Err(response.error);
    }
    let bytes = &worker.tile.as_slice()[..response.bytes];
    if bytes.len() != pixels_wide * pixels_high * 4 {
        return Err("the tile is not the size that was asked for".into());
    }
    // The background is whatever colour the page has most of, not an assumed white: a
    // dark or tinted page would otherwise read as ink from edge to edge.
    let mut counts: BTreeMap<[u8; 3], usize> = BTreeMap::new();
    for pixel in bytes.chunks_exact(4) {
        *counts.entry([pixel[0], pixel[1], pixel[2]]).or_default() += 1;
    }
    let paper = counts
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(colour, _)| *colour)
        .ok_or("the tile is empty")?;
    let ink: Vec<bool> = bytes
        .chunks_exact(4)
        .map(|pixel| {
            (0..3).any(|c| (i32::from(pixel[c]) - i32::from(paper[c])).abs() > INK_TOLERANCE)
        })
        .collect();
    let on = ink.iter().filter(|&&on| on).count();
    Ok(Ink {
        fraction: on as f64 / ink.len() as f64,
        ink,
        width: pixels_wide,
        height: pixels_high,
        scale: f64::from(scale),
    })
}

// ---------------------------------------------------------------------------

pub(super) fn run(source: &Path, agree_every: usize) -> Result<(), String> {
    let fingerprint = tpdf_lib::fingerprint::Fingerprint::of(source)?;
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(source, &library)?;
    let opened = worker.call(&Request::Open {
        lazy_geometry: true,
    })?;
    let Some(Reply::Open { page_count, .. }) = opened.reply.filter(|_| opened.ok) else {
        return Err("could not open the document".into());
    };
    let page_count = u32::try_from(page_count).map_err(|_| "page count exceeds u32")?;
    if page_count == 0 {
        return Err("document has no pages".into());
    }
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    let mut doc = tpdf_lib::encoding::load(&bytes, None)?;
    let ids = doc.get_pages();
    let mut agreement = Agreement {
        every: agree_every,
        seen: 0,
        checked: 0,
        disagreements: Vec::new(),
    };
    let mut pages = Vec::new();
    for index in 0..page_count.min(128) {
        let reply = worker.call(&Request::TextRuns {
            page: index,
            changes: Vec::new(),
        })?;
        let mapped = match reply.reply {
            Some(Reply::TextRuns(runs)) if reply.ok => runs,
            _ => {
                pages.push(json!({"page": index, "status": "refused", "reason": reply.error}));
                continue;
            }
        };
        // Every object as it was before this page's trials, to prove afterwards
        // that each accepted one was undone -- whatever it wrote to.
        let objects = doc.objects.clone();
        // Discovery here must be the worker's, or the trials describe other runs.
        let local = textedit::scan(&doc, index)?;
        if !same_runs(&local, &mapped) {
            return Err(format!(
                "page {index}: in-process discovery disagrees with the worker"
            ));
        }
        let id = *ids
            .get(&(index + 1))
            .ok_or("page missing from the page tree")?;
        let page = Page {
            id,
            dictionary: doc.get_object(id).map_err(|e| e.to_string())?.clone(),
        };
        let geometry = tpdf_lib::pagetree::displayed_page(&doc, id);
        let upright = geometry.turns == 0;
        let (tags, tag_status) = match tagged(&doc, id) {
            Ok(tags) => (tags, String::from("ok")),
            Err(reason) => (None, reason),
        };
        let decoded = Content::decode(&doc.get_page_content(id))
            .map_err(|error| error.to_string())
            .and_then(|content| {
                // The addresses have to be the scanner's, or an MCID is attached to
                // whatever happens to sit at that index in a different decode.
                let agrees = mapped.runs.iter().all(|run| {
                    content
                        .operations
                        .get(run.operator as usize)
                        .is_some_and(|op| showing(&op.operator))
                });
                if agrees {
                    Ok(content)
                } else {
                    Err("this decode does not address the same operators".into())
                }
            });
        let (mcids, map_status) = match (&tags, &decoded) {
            (None, _) => (BTreeMap::new(), tag_status.clone()),
            (Some(_), Err(reason)) => (BTreeMap::new(), reason.clone()),
            (Some(_), Ok(content)) => match marked(&doc, id, content) {
                Ok(found) => (found, String::from("ok")),
                Err(reason) => (BTreeMap::new(), reason),
            },
        };
        let paint = render(&mut worker, index, geometry.width, geometry.height);
        let mut runs = Vec::new();
        for run in &mapped.runs {
            if run.text.trim().is_empty() {
                continue;
            }
            let base = app_layout(run);
            let mut record = serde_json::Map::new();
            for (label, growth) in GROWTHS {
                let edit = textedit::Change {
                    page: index,
                    revision: mapped.revision.clone(),
                    operator: run.operator,
                    original: run.text.clone(),
                    replacement: grown(&run.text, growth),
                    layout: Some(base.clone()),
                };
                let outcome = trial(&mut doc, &page, edit.clone());
                agreement.check(&mut worker, &edit, &outcome, label)?;
                record.insert(label.into(), verdict(&outcome));
            }
            let axis_aligned = run.matrix[1] == 0.
                && run.matrix[2] == 0.
                && run.matrix[0] > 0.
                && run.matrix[3] > 0.
                && upright;
            let mcid = mcids.get(&run.operator).copied();
            let owner = mcid.and_then(|mcid| tags.as_ref()?.owners.get(mcid).copied().flatten());
            let role = owner.and_then(|id| tags.as_ref()?.elements.get(&id));
            record.insert("operator".into(), json!(run.operator));
            record.insert("chars".into(), json!(run.text.chars().count()));
            record.insert("axis_aligned".into(), json!(axis_aligned));
            record.insert("font".into(), json!(run.font));
            record.insert("size".into(), json!(base.size));
            record.insert("rect".into(), json!(run.display_rect));
            record.insert("mcid".into(), json!(mcid));
            record.insert(
                "element".into(),
                json!(owner.map(|(number, generation)| format!("{number} {generation}"))),
            );
            record.insert("role".into(), json!(role.map(|(_, mapped, _)| mapped)));
            record.insert("authored_role".into(), json!(role.map(|(raw, _, _)| raw)));
            if axis_aligned {
                if let Ok(paint) = &paint {
                    let (clear, edge) = paint.below(run.display_rect, f64::from(geometry.height));
                    record.insert("ink_below".into(), json!(clear));
                    record.insert("below_is_page_edge".into(), json!(edge));
                }
            }
            runs.push(Value::Object(record));
        }
        // Every accepted trial was undone; prove the page is what was discovered.
        let after = textedit::scan(&doc, index)?;
        if !same_runs(&after, &mapped) || doc.objects != objects {
            return Err(format!("page {index}: a trial was not undone"));
        }
        pages.push(json!({
            "page": index, "status": "editable", "runs": mapped.runs.len(),
            "width": geometry.width, "height": geometry.height, "turns": geometry.turns,
            "tagged": tags.is_some(), "mcid_map": map_status,
            "mcid_slots": tags.as_ref().map_or(0, |tags| tags.owners.len()),
            "elements": tags.as_ref().map(|tags| tags.elements.iter()
                .map(|((number, generation), (raw, mapped, parent))| (
                    format!("{number} {generation}"),
                    json!({"authored_role": raw, "role": mapped,
                        "parent": parent.map(|(n, g)| format!("{n} {g}"))}),
                ))
                .collect::<serde_json::Map<_, _>>()),
            "render": match &paint {
                Ok(paint) => json!({"scale": paint.scale, "ink_fraction": paint.fraction}),
                Err(reason) => json!({"failed": reason}),
            },
            "tried": runs,
        }));
    }
    if tpdf_lib::fingerprint::Fingerprint::of(source)? != fingerprint {
        return Err("input changed".into());
    }
    println!(
        "{}",
        json!({"page_count": page_count, "pages_inspected": page_count.min(128),
            "pages": pages, "agreement": {"every": agreement.every, "checked": agreement.checked,
            "disagreements": agreement.disagreements}})
    );
    Ok(())
}
