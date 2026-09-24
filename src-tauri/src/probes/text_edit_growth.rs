//! Replacement length against refusal, on an unchanged document.
//!
//! `text-edit-probe --growth <source.pdf> [--agree-every=N]` prints one JSON report and
//! writes nothing. For every editable run with visible text on each page (at most 128
//! pages) it asks whether a replacement would be accepted, for:
//!
//! - `identity`: the run's own text, unchanged, in the editor's box (`app` only; the
//!   byte-patch writer refuses an unchanged replacement by rule), which says whether
//!   the box can hold what is already there;
//! - `control`: the same length, two adjacent distinct characters swapped (or one
//!   replaced by another character of the run), separating "any edit refused" from
//!   "a longer edit refused";
//! - `shrink25`: the last quarter of the characters removed;
//! - `grow10`, `grow25`, `grow50`: characters appended until the text is at least that
//!   much longer, taken only from the run's own characters (its text, then a space if
//!   the run has one), so a missing glyph cannot be the reason.
//!
//! Each trial runs in up to three modes. `app` sends the layout the editor sends when a
//! reader types (`defaultTextLayout` in `src/lib/textlayout.ts`: the box is the run's own
//! advance, and `grow` set, so the box follows the typed text as far as the room after
//! the run allows). `patch` sends no layout, which is the byte-patch writer. `widened`
//! (growth only) is a box a reader sized: `grow` cleared and the width widened along a 5%
//! ladder until the refusal is no longer about the box's own width. That mode is
//! deliberately unchanged by growth, so its column stays comparable across the change.
//!
//! Acceptance is decided by `textedit::write`, in this process, on the parsed document:
//! the same function the worker's `TextRuns` request runs before it serialises and
//! renders a preview. A refused write changes nothing; an accepted one is undone by
//! restoring the page dictionary and dropping the objects it added, and each page's runs
//! are rescanned afterwards to prove the restore. `--agree-every=N` sends every Nth
//! trial through the contained worker as well and counts any verdict or message that
//! differs, so the in-process shortcut is measured rather than assumed.
//! Parsing the document in this process is acceptable for a measurement over files whose
//! digests were checked first; it is why this is a probe and never an application path,
//! where every parse belongs to a sandboxed worker.
//!
//! `--growth-request <source.pdf> <page> <operator> <trial> <width|app> [grow]` prints the
//! one-element request array `--roundtrip` takes for that trial, with the box at
//! `width`; the array contains the replacement text and the run's own `original`,
//! which `--roundtrip` ignores and `scripts/text_wrap_check.py --compare` counts
//! glyphs with, so write it to an ignored file.
//! A trailing `grow` sets the flag the editor sets for a box a reader has not sized,
//! which is how a round trip exercises a box that follows the text.
//!
//! What it does not measure: whether an accepted edit saves (see `--roundtrip`), how a
//! real reader would phrase a longer replacement, or where a text column ends. The
//! geometry it records (`gap_right`, `extent_right`) is the hit rectangles of the other
//! discovered runs, which omits read-only text, graphics and form fields.
use super::*;
use std::path::Path;

use lopdf::{Object, ObjectId};
use serde_json::{json, Value};

const GROWTHS: [(&str, f64); 3] = [("grow10", 0.10), ("grow25", 0.25), ("grow50", 0.50)];
const LADDER_STEP: f64 = 1.05;
const LADDER_STEPS: usize = 24;

/// The layout `defaultTextLayout` builds, in the same arithmetic. `grow` is set,
/// as it is for a reader who has not touched the width control, so `app` is what
/// the editor actually sends; `widened` clears it, being a box a reader set.
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

/// Same length, still a change: swap the first adjacent pair of distinct visible
/// characters, or failing that replace one character with another from the run.
fn control(text: &str) -> Option<String> {
    let mut chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        let (a, b) = (chars[i], chars[i + 1]);
        if a != b && !a.is_whitespace() && !b.is_whitespace() {
            chars.swap(i, i + 1);
            return Some(chars.into_iter().collect());
        }
    }
    let visible: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace())
        .collect();
    let first = *visible.first()?;
    let other = visible.iter().copied().find(|c| *c != first)?;
    let at = chars.iter().position(|c| *c == first)?;
    chars[at] = other;
    Some(chars.into_iter().collect())
}

/// The original followed by at least `ceil(n * growth)` characters (at least one),
/// drawn by cycling through the run's own text and one space if it contains a space.
/// Never ends in whitespace, which the layout would not measure.
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

fn shrunk(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let keep = chars.len() - ((chars.len() as f64 * 0.25).ceil() as usize);
    let out: String = chars[..keep.max(1)].iter().collect();
    (out != text && !out.trim().is_empty()).then_some(out)
}

/// A refusal about the box's own width, which widening the box can answer. A grown
/// box says "no room for more text" instead and widening it by hand cannot help, so
/// that message deliberately does not belong here -- the ladder would climb past the
/// neighbour it was stopped by.
fn box_width(reason: &str) -> bool {
    reason.contains("exceeds the box width") || reason.contains("ink exceeds the box")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.)
}

/// Equal discovery, allowing floats to differ in the last places.
fn same_runs(a: &textedit::PageRuns, b: &textedit::PageRuns) -> bool {
    a.revision == b.revision
        && a.runs.len() == b.runs.len()
        && a.runs.iter().zip(&b.runs).all(|(x, y)| {
            x.operator == y.operator
                && x.text == y.text
                && x.font == y.font
                && x.minimum_height.is_some() == y.minimum_height.is_some()
                && close(x.size, y.size)
                && close(x.advance, y.advance)
                && x.matrix.iter().zip(&y.matrix).all(|(p, q)| close(*p, *q))
                && x.display_rect
                    .iter()
                    .zip(&y.display_rect)
                    .all(|(p, q)| close(f64::from(*p), f64::from(*q)))
        })
}

struct Page {
    id: ObjectId,
    dictionary: Object,
}

/// Run `textedit::write` and put the document back exactly as it was.
fn trial(doc: &mut lopdf::Document, page: &Page, change: textedit::Change) -> Result<(), String> {
    let max_id = doc.max_id;
    let result = textedit::write(doc, &[change]);
    if result.is_ok() {
        doc.objects.retain(|id, _| id.0 <= max_id);
        doc.objects.insert(page.id, page.dictionary.clone());
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
        // Discovery here must be the worker's, or the trials describe other runs.
        // Floats may differ in the last place: the reply crosses the worker's JSON
        // channel, whose parser does not round-trip every f64. Trials use the
        // worker's values, as the application does.
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
        let upright = tpdf_lib::pagetree::displayed_page(&doc, id).turns == 0;
        let extent_right = mapped
            .runs
            .iter()
            .filter(|run| !run.text.trim().is_empty())
            .map(|run| run.display_rect[2])
            .fold(f32::MIN, f32::max);
        let mut runs = Vec::new();
        let mut blank = 0;
        for run in &mapped.runs {
            if run.text.trim().is_empty() {
                blank += 1;
                continue;
            }
            let base = app_layout(run);
            let change = |replacement: &str, layout: Option<textedit::Layout>| textedit::Change {
                page: index,
                revision: mapped.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: replacement.to_owned(),
                layout,
            };
            let mut record = serde_json::Map::new();
            let mut cases: Vec<(&str, Option<String>, Option<f64>)> = vec![
                ("identity", Some(run.text.clone()), None),
                ("control", control(&run.text), None),
                ("shrink25", shrunk(&run.text), None),
            ];
            for (label, growth) in GROWTHS {
                cases.push((label, Some(grown(&run.text, growth)), Some(growth)));
            }
            for (label, replacement, growth) in cases {
                let Some(replacement) = replacement else {
                    record.insert(label.into(), Value::Null);
                    continue;
                };
                let mut result = serde_json::Map::new();
                result.insert(
                    "added".into(),
                    json!(replacement.chars().count() as i64 - run.text.chars().count() as i64),
                );
                let modes: &[(&str, Option<textedit::Layout>)] = if label == "identity" {
                    &[("app", Some(base.clone()))]
                } else {
                    &[("app", Some(base.clone())), ("patch", None)]
                };
                for (mode, layout) in modes.iter().cloned() {
                    let edit = change(&replacement, layout);
                    let outcome = trial(&mut doc, &page, edit.clone());
                    agreement.check(&mut worker, &edit, &outcome, label)?;
                    result.insert(mode.into(), verdict(&outcome));
                }
                if let Some(growth) = growth {
                    let mut width = base.width * (1. + growth);
                    let mut steps = 0;
                    let outcome = loop {
                        let mut layout = base.clone();
                        layout.width = width;
                        layout.grow = false;
                        let edit = change(&replacement, Some(layout));
                        let outcome = trial(&mut doc, &page, edit.clone());
                        agreement.check(&mut worker, &edit, &outcome, label)?;
                        steps += 1;
                        match &outcome {
                            Err(reason) if box_width(reason) && steps < LADDER_STEPS => {
                                width *= LADDER_STEP;
                            }
                            _ => break outcome,
                        }
                    };
                    result.insert(
                        "widened".into(),
                        json!({"verdict": verdict(&outcome), "width": width, "steps": steps}),
                    );
                }
                record.insert(label.into(), Value::Object(result));
            }
            let axis_aligned = run.matrix[1] == 0.
                && run.matrix[2] == 0.
                && run.matrix[0] > 0.
                && run.matrix[3] > 0.
                && upright;
            // The nearest discovered run to the right sharing at least half the height.
            let rect = run.display_rect;
            let gap_right = axis_aligned
                .then(|| {
                    mapped
                        .runs
                        .iter()
                        .filter(|other| {
                            other.operator != run.operator && !other.text.trim().is_empty()
                        })
                        .filter(|other| {
                            let overlap = rect[3].min(other.display_rect[3])
                                - rect[1].max(other.display_rect[1]);
                            let height = (rect[3] - rect[1])
                                .min(other.display_rect[3] - other.display_rect[1]);
                            other.display_rect[0] >= rect[2] - 0.5 && overlap >= height * 0.5
                        })
                        .map(|other| other.display_rect[0] - rect[2])
                        .fold(None, |best: Option<f32>, gap| {
                            Some(best.map_or(gap, |b| b.min(gap)))
                        })
                })
                .flatten();
            record.insert("operator".into(), json!(run.operator));
            record.insert("chars".into(), json!(run.text.chars().count()));
            record.insert("axis_aligned".into(), json!(axis_aligned));
            record.insert("width".into(), json!(base.width));
            record.insert("right".into(), json!(rect[2]));
            record.insert("gap_right".into(), json!(gap_right));
            record.insert("extent_right".into(), json!(extent_right));
            runs.push(Value::Object(record));
        }
        // Every accepted trial was undone; prove the page is what was discovered.
        let after = textedit::scan(&doc, index)?;
        if !same_runs(&after, &mapped) {
            return Err(format!("page {index}: a trial was not undone"));
        }
        pages.push(
            json!({"page": index, "status": "editable", "runs": mapped.runs.len(),
            "blank": blank, "tried": runs}),
        );
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

/// A `--roundtrip` request reproducing one widened trial, for checking that an
/// accepted verdict is an edit that saves and reads back.
pub(super) fn request(
    source: &Path,
    page: u32,
    operator: u32,
    label: &str,
    width: Option<f64>,
    grow: bool,
) -> Result<(), String> {
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(source, &library)?;
    let mapped = runs(&mut worker, page)?;
    let run = mapped
        .runs
        .iter()
        .find(|run| run.operator == operator)
        .ok_or("no run with that operator")?;
    let replacement = match label {
        "control" => control(&run.text),
        "shrink25" => shrunk(&run.text),
        _ => GROWTHS
            .iter()
            .find(|(name, _)| *name == label)
            .map(|(_, growth)| grown(&run.text, *growth)),
    }
    .ok_or("no such trial for this run")?;
    let mut layout = app_layout(run);
    if let Some(width) = width {
        layout.width = width;
    }
    layout.grow = grow;
    println!(
        "{}",
        json!([{"page": page, "operator": operator, "original": run.text, "replacement": replacement, "layout": layout}])
    );
    Ok(())
}
