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
        baseline: page_count as u32,
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
    };
    let mut previews = Vec::new();
    for (page, size) in pages.iter().enumerate() {
        if size.width_pt > 4096. || size.height_pt > 4096. {
            return Err("page exceeds round-trip raster limit".into());
        }
        let width = size.width_pt.ceil() as u16;
        let height = size.height_pt.ceil() as u16;
        let request = Request::Tile {
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
    for change in plan.text_edits.iter().map(|edit| &edit.change) {
        let reopened = runs(&mut saved, change.page)?;
        let found = if change.layout.as_ref().is_some_and(|layout| layout.wrap) {
            reopened
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .contains(&change.replacement.replace('\n', ""))
        } else {
            reopened
                .runs
                .iter()
                .any(|run| run.text == change.replacement)
        };
        if !found {
            return Err("replacement missing after reopening".into());
        }
    }
    if tpdf_lib::fingerprint::Fingerprint::of(source)? != fingerprint {
        return Err("input changed".into());
    }
    println!("[PASS] {} edits; {} pages; preview/save pixels agree; adjacent pixels and original input unchanged", plan.text_edits.len(), page_count);
    Ok(())
}
