//! An unchanged external fixture, separate from the generated two-line cases.
//! text-edit-probe --w3c-dummy <downloaded-dummy.pdf> <new-output-directory>
//! Source URL/digest live in testdata/textedit-public-corpus.json. The sentence
//! edits as one line (file -> fill); the input is never rewritten for admission.
use super::*;

pub(super) fn run(source: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
    let fingerprint = tpdf_lib::fingerprint::Fingerprint::of(source)?;
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(tpdf_lib::PDFIUM_SUBDIR);
    let mut worker = Worker::spawn(source, &library)?;
    let opened = worker.call(&Request::Open {
        lazy_geometry: false,
    })?;
    if !matches!(opened.reply, Some(Reply::Open { page_count: 1, .. })) || !opened.ok {
        return Err("expected the one-page W3C fixture".into());
    }
    let mapped = runs(&mut worker, 0)?;
    if mapped
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        != ["Dummy PDF file"]
    {
        return Err("unexpected W3C source text fragments".into());
    }
    let mut plan = Plan {
        baseline: 1,
        opened_as: Some(fingerprint.clone()),
        pages: vec![PageView {
            id: 1,
            source: PageSource::Baseline(0),
            turns: 0,
            crop: None,
        }],
        forms: vec![],
        marks: vec![],
        notes: vec![],
        discards: vec![],
        sources: Vec::new(),
        redactions: vec![],
        text_edits: vec![textedit::Edit::opened(textedit::Change {
            layout: None,
            page: 0,
            revision: mapped.revision.clone(),
            operator: mapped.runs[0].operator,
            original: "Dummy PDF file".into(),
            replacement: "Dummy PDF fill".into(),
        })],
    };
    let tile = Request::Tile {
        rid: 91,
        page: 0,
        scale: 1.,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: 596,
        height: 842,
        png: false,
        crop: None,
    };
    let pixels = |worker: &mut Worker, request: &Request| -> Result<Vec<u8>, String> {
        let response = worker.call(request)?;
        if !response.ok {
            return Err(response.error);
        }
        Ok(worker.tile.as_slice()[..response.bytes].to_vec())
    };
    let before = pixels(&mut worker, &tile)?;
    let preview = pixels(
        &mut worker,
        &Request::TextView {
            changes: plan
                .text_edits
                .iter()
                .map(|edit| edit.change.clone())
                .collect(),
            request: Box::new(tile.clone()),
        },
    )?;
    if preview == before || preview.len() != before.len() {
        return Err("preview did not change pixels".into());
    }
    // Fixed independently from the fixture's 56.8pt line origin and
    // 758.1pt baseline. Do not trust the editor's own hit rectangle as the oracle.
    for (i, (a, b)) in before
        .chunks_exact(4)
        .zip(preview.chunks_exact(4))
        .enumerate()
    {
        if a != b && !((55..184).contains(&(i % 596)) && (68..87).contains(&(i / 596))) {
            return Err("preview changed pixels outside the edited line".into());
        }
    }
    if pixels(&mut worker, &tile)? != before || runs(&mut worker, 0)?.runs != mapped.runs {
        return Err("preview altered the baseline".into());
    }
    std::fs::create_dir(dir).map_err(|e| e.to_string())?;
    let saved_path = dir.join("synthetic-after.pdf");
    // Names match the existing independent readback tools; these bytes are the
    // downloaded public fixture, not a generated or normalized substitute.
    std::fs::copy(source, dir.join("synthetic-before.pdf")).map_err(|e| e.to_string())?;
    let writer = save::InWorker::at(library.clone());
    let write = |plan: &Plan, out: &mut File| -> Result<usize, String> {
        let mut input = File::open(source).map_err(|e| e.to_string())?;
        let len = input.metadata().map_err(|e| e.to_string())?.len() as usize;
        writer
            .write(&mut input, len, out, plan, save::Job::Save, None, None)
            .map_err(|e| e.message)
    };
    let mut out = File::create_new(&saved_path).map_err(|e| e.to_string())?;
    let count = write(&plan, &mut out)?;
    if out.metadata().map_err(|e| e.to_string())?.len() != count as u64 {
        return Err("wrong saved byte count".into());
    }
    drop(out);
    let mut saved = Worker::spawn(&saved_path, &library)?;
    let after = runs(&mut saved, 0)?;
    if after
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        != "Dummy PDF fill"
        || pixels(&mut saved, &tile)? != preview
    {
        return Err("saved content disagrees with the preview or changed adjacent text".into());
    }
    for (index, invalid) in ["l".repeat(80), "B".into()].into_iter().enumerate() {
        plan.text_edits[0].change.replacement = invalid;
        let mut out = File::create_new(dir.join(format!("refused-{index}.pdf")))
            .map_err(|e| e.to_string())?;
        if write(&plan, &mut out).is_ok() || out.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err("invalid replacement accepted or wrote output".into());
        }
    }
    if tpdf_lib::fingerprint::Fingerprint::of(source)? != fingerprint {
        return Err("original input changed".into());
    }
    println!("[PASS] unchanged W3C discovery; contained preview/save agree; adjacent text and source preserved; invalid edits write nothing");
    Ok(())
}
