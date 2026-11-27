//! Local round trip without printing document text. Requests are JSON objects
//! {"page":0,"contains":"SYNTHETIC","replacement":"EDITED"} in an array.
//! Each substring must identify exactly one run. Output is a new directory.
//! With "replace_match":true, replace only that substring within the run.
use super::*;
use std::path::Path;

#[derive(serde::Deserialize)]
struct Edit {
    page: u32,
    #[serde(default)]
    contains: String,
    #[serde(default)]
    operator: Option<u32>,
    replacement: String,
    #[serde(default)]
    replace_match: bool,
    #[serde(default)]
    layout: Option<textedit::Layout>,
}

fn pixels(worker: &mut Worker, request: &Request) -> Result<Vec<u8>, String> {
    let answer = worker.call(request)?;
    if !answer.ok {
        return Err(answer.error);
    }
    Ok(worker.tile.as_slice()[..answer.bytes].to_vec())
}

pub(super) fn run(source: &Path, requests: &Path, directory: &Path) -> Result<(), String> {
    let edits: Vec<Edit> =
        serde_json::from_slice(&std::fs::read(requests).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if edits.is_empty() || edits.len() > 128 {
        return Err("expected 1 to 128 edits".into());
    }
    let fingerprint = tpdf_lib::fingerprint::Fingerprint::of(source)?;
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(source, &library)?;
    let reply = worker.call(&Request::Open {
        lazy_geometry: false,
    })?;
    let Some(Reply::Open {
        pages, page_count, ..
    }) = reply.reply.filter(|_| reply.ok)
    else {
        return Err("could not open probe input".into());
    };
    if pages.len() != page_count || page_count > 128 {
        return Err("unsupported page count".into());
    }
    let mut changes = Vec::new();
    let mut boxes = Vec::new();
    let mut deletions = Vec::new();
    for (index, edit) in edits.into_iter().enumerate() {
        let mapped = runs(&mut worker, edit.page)?;
        let matches: Vec<_> = mapped
            .runs
            .iter()
            .filter(|run| {
                edit.operator.map_or(
                    !edit.contains.is_empty() && run.text.contains(&edit.contains),
                    |operator| run.operator == operator,
                )
            })
            .collect();
        let [run] = matches.as_slice() else {
            return Err(format!(
                "request {index}: expected one matching run; found {} (word matches: {:?})",
                matches.len(),
                edit.contains
                    .split_whitespace()
                    .map(|word| mapped
                        .runs
                        .iter()
                        .filter(|run| run.text.contains(word))
                        .count())
                    .collect::<Vec<_>>()
            ));
        };
        boxes.push((edit.page, run.display_rect));
        let replacement = if edit.replace_match {
            if edit.contains.is_empty() || run.text.matches(&edit.contains).count() != 1 {
                return Err("substring replacement requires exactly one occurrence".into());
            }
            run.text.replacen(&edit.contains, &edit.replacement, 1)
        } else {
            edit.replacement
        };
        if replacement.is_empty() && !run.text.trim().is_empty() {
            deletions.push((edit.page, run.operator, run.display_rect));
        }
        changes.push(textedit::Change {
            layout: edit.layout,
            page: edit.page,
            revision: mapped.revision,
            operator: run.operator,
            original: run.text.clone(),
            replacement,
        });
        if changes.last().is_some_and(|change| change.layout.is_some()) {
            let result = worker.call(&Request::TextRuns {
                page: edit.page,
                changes: changes.clone(),
            })?;
            let Some(Reply::TextRuns(result)) = result.reply.filter(|_| result.ok) else {
                return Err(result.error);
            };
            let preview = result.preview.ok_or("missing layout preview")?;
            if !preview.png.starts_with(&[137, 80, 78, 71]) {
                return Err("invalid preview PNG".into());
            }
            // The extent rather than the box: since 26.9.15 a draft may push the
            // rest of its line along, and those runs are pixels the edit is
            // meant to change. The extent is the box together with exactly the
            // runs it pushed, so everything else on the page -- every other
            // line included -- is still required to be identical.
            boxes.push((edit.page, preview.extent));
        }
    }
    let plan = Plan {
        field_edits: Vec::new(),
        tab_order: false,
        baseline: page_count as u32,
        source_moved: false,
        opened_as: Some(fingerprint.clone()),
        pages: (0..page_count)
            .map(|index| PageView {
                id: index as u64 + 1,
                source: PageSource::Baseline(index as u32),
                turns: 0,
                crop: None,
            })
            .collect(),
        forms: vec![],
        marks: vec![],
        notes: vec![],
        discards: vec![],
        sources: Vec::new(),
        redactions: vec![],
        // Every request file names pages of the document the probe opened.
        text_edits: changes.into_iter().map(textedit::Edit::opened).collect(),
        text_layers: Vec::new(),
        added_text: Vec::new(),
        protection: Default::default(),
        compress: Default::default(),
        new_fields: Vec::new(),
    };
    let mut previews = Vec::new();
    for (page, size) in pages.iter().enumerate() {
        if size.width_pt > 4096. || size.height_pt > 4096. {
            return Err("page exceeds round-trip raster limit".into());
        }
        let width = size.width_pt.ceil() as u16;
        let height = size.height_pt.ceil() as u16;
        let request = Request::Tile {
            annotations: true,
            printing: false,
            rid: page as u64 + 1,
            page: page as u32,
            scale: 1.,
            turns: 0,
            invert: false,
            x: 0,
            y: 0,
            width,
            height,
            png: false,
            crop: None,
        };
        let before = pixels(&mut worker, &request)?;
        let preview = pixels(
            &mut worker,
            &Request::TextView {
                changes: plan
                    .text_edits
                    .iter()
                    .map(|edit| edit.change.clone())
                    .collect(),
                fields: Vec::new(),
                request: Box::new(request.clone()),
            },
        )?;
        let edited = plan
            .text_edits
            .iter()
            .any(|edit| edit.change.page == page as u32);
        if before.len() != preview.len() || (before == preview) == edited {
            return Err("preview changed the wrong pages or no pixels".into());
        }
        // A change elsewhere on this page cannot prove that the word at the
        // requested location disappeared, even if that word occurs twice.
        for (_, operator, rect) in deletions.iter().filter(|(p, _, _)| *p == page as u32) {
            if !changed_in(&before, &preview, width, height, *rect) {
                return Err(format!(
                    "page {page}, operator {operator}: deletion changed no pixels at its target"
                ));
            }
        }
        for (i, (a, b)) in before
            .chunks_exact(4)
            .zip(preview.chunks_exact(4))
            .enumerate()
        {
            if a != b {
                let x = (i % width as usize) as f32;
                let y = (i / width as usize) as f32;
                if !boxes.iter().any(|(p, r)| {
                    *p == page as u32
                        && x >= r[0] - 2.
                        && x <= r[2] + 2.
                        && y >= r[1] - 2.
                        && y <= r[3] + 2.
                }) {
                    return Err("pixels changed outside edited text envelopes".into());
                }
            }
        }
        if pixels(&mut worker, &request)? != before {
            return Err("preview mutated its baseline".into());
        }
        previews.push((request, preview));
    }
    std::fs::create_dir(directory).map_err(|e| e.to_string())?;
    let output = directory.join("edited.pdf");
    let mut out = File::create_new(&output).map_err(|e| e.to_string())?;
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    let len = input.metadata().map_err(|e| e.to_string())?.len() as usize;
    let count = save::InWorker::at(library.clone())
        .write(
            &mut input,
            len,
            &mut out,
            &plan,
            save::Job::Save,
            None,
            None,
        )
        .map_err(|e| e.message)?;
    if out.metadata().map_err(|e| e.to_string())?.len() != count as u64 {
        return Err("saved length mismatch".into());
    }
    drop(out);
    let mut saved = Worker::spawn(&output, &library)?;
    for (request, preview) in previews {
        if pixels(&mut saved, &request)? != preview {
            return Err("saved pixels differ from preview".into());
        }
    }
    let edited_pages: std::collections::BTreeSet<_> = plan
        .text_edits
        .iter()
        .map(|edit| edit.change.page)
        .collect();
    for page in edited_pages {
        let baseline = runs(&mut worker, page)?;
        let reopened = runs(&mut saved, page)?;
        let expected = baseline.runs.iter().map(|run| {
            plan.text_edits
                .iter()
                .find(|edit| edit.change.page == page && edit.change.operator == run.operator)
                .map_or(run.text.as_str(), |edit| edit.change.replacement.as_str())
        });
        if !same_page_text(expected, reopened.runs.iter().map(|run| run.text.as_str())) {
            return Err(format!(
                "page {page}: reopened text differs at an edited or surrounding run"
            ));
        }
        // A patch fits into a single show and preserves its whitespace exactly.
        for edit in plan
            .text_edits
            .iter()
            .map(|edit| &edit.change)
            .filter(|change| change.page == page && change.layout.is_none())
        {
            if !reopened.runs.iter().any(|run| run.text == edit.replacement) {
                return Err(format!("page {page}: exact patch missing after reopening"));
            }
        }
    }
    if tpdf_lib::fingerprint::Fingerprint::of(source)? != fingerprint {
        return Err("input changed".into());
    }
    println!("[PASS] {} edits; {} pages; preview/save pixels agree; adjacent pixels and original input unchanged", plan.text_edits.len(), page_count);
    Ok(())
}

fn changed_in(before: &[u8], after: &[u8], width: u16, height: u16, rect: [f32; 4]) -> bool {
    let [left, top, right, bottom] = rect;
    let x0 = (left.floor() as usize).min(width.into());
    let x1 = (right.ceil() as usize).min(width.into());
    let y0 = (top.floor() as usize).min(height.into());
    let y1 = (bottom.ceil() as usize).min(height.into());
    x0 < x1
        && (y0..y1).any(|y| {
            let row = y * usize::from(width);
            let range = 4 * (row + x0)..4 * (row + x1);
            before[range.clone()] != after[range]
        })
}

// Wrapping can split a show and represent spaces by positioning. Compare the
// complete sequence in content order, including the text on both sides of an edit.
fn same_page_text<'a, 'b>(
    expected: impl Iterator<Item = &'a str>,
    saved: impl Iterator<Item = &'b str>,
) -> bool {
    expected
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .eq(saved.flat_map(str::chars).filter(|c| !c.is_whitespace()))
}

#[cfg(test)]
mod tests {
    use super::{changed_in, same_page_text};

    #[test]
    fn changing_the_second_location_does_not_prove_the_first_was_erased() {
        let before = [0; 24];
        let mut after = before;
        after[20] = 255;
        assert!(changed_in(&before, &after, 3, 2, [2., 1., 3., 2.]));
        assert!(!changed_in(&before, &after, 3, 2, [0., 1., 2., 2.]));
    }

    #[test]
    fn a_second_edit_cannot_mask_a_dropped_deletion_or_a_misplaced_replacement() {
        let expected = ["", "middle", "new words"];
        assert!(same_page_text(
            expected.into_iter(),
            ["middle", "new", "words"].into_iter()
        ));
        assert!(!same_page_text(
            expected.into_iter(),
            ["old", "middle", "new words"].into_iter()
        ));
        assert!(!same_page_text(
            expected.into_iter(),
            ["new words", "middle"].into_iter()
        ));
    }
}
