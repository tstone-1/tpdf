//! The whole redaction, end to end, on a real file.
//!
//! `redact_probe` proves the primitive: given ordinals, the show operators go.
//! This proves the **path** --- a rectangle a reader dragged becomes a plan
//! against PDFium's own object list, becomes ordinals in a save plan, becomes a
//! written file, and the words are not in it. Every step between the drag and
//! the file is here except the dialog and the Tauri command's own glue.
//!
//! ## Two readers, and the control is the point
//!
//! The needle must be **gone** and a word on another line must **survive**, and
//! both are asserted through two independent readers: `verify::scan` over the
//! bytes, and PDFium re-extracting the written file. A scan that finds nothing
//! because it cannot look is the failure this repository has recorded from
//! several directions, and a survivor it can still find is what says it can.
//!
//! Route B removes the whole operation, so the survivor is on a **different
//! line**: a word beside the target goes with it, and asserting otherwise would
//! fail for a redaction that is working correctly.
//!
//! ## Usage
//!
//!     cargo run --release --example redact-apply-probe [-- --library DIR]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpdf_lib::docmodel::PageSource;
use tpdf_lib::document::OpenDocument;
use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::{outline, progressive, redact, render, save, text, verify};

/// The fixture, what to remove, and what has to survive.
const FILE: &str = "text-base14.pdf";
const REMOVE: &str = "4711-0815";
/// On another line, so route B's collateral does not take it.
const KEEP: &str = "Sphinx of black quartz";

/// The carrier fixture, and the two markers that tell its annotations apart.
///
/// `text-marked.pdf` carries the same line four times --- see
/// `testdata/make_text_pdf.py`. Two of those copies are annotations, and the
/// whole point of the pair is that a check can say **which** one went: one sits
/// squarely over the target line, the other nowhere near it. Asking whether the
/// secret is still somewhere in the file cannot distinguish them, which is the
/// mistake `redact-probe`'s own carrier check made until 2026-08-27.
const CARRIER_FILE: &str = "text-marked.pdf";
/// In the annotation over the target line. Must go.
const ANNOT_OVER: &str = "ANNOT-OVER";
/// In the annotation away from every line. Must stay --- a reader's other
/// comments are not theirs to lose, and a rule that took every annotation on the
/// page would pass the first check perfectly.
const ANNOT_AWAY: &str = "ANNOT-AWAY";
/// In the structure element owning the redacted line's `/MCID`. Must go.
const STRUCT_OVER: &str = "STRUCT-CARRIER";
/// In the element above it, which restates what was removed. Must go.
const STRUCT_ANCESTOR: &str = "STRUCT-ANCESTOR";
/// In the element owning a line nobody redacted. Must stay --- a rule that
/// stripped the whole tree would pass both checks above.
const STRUCT_OTHER: &str = "STRUCT-OTHER";
/// `/Alt` and `/E` on the same two elements, added 2026-09-02.
///
/// `redact.rs`'s `SHADOW_TEXT` is `/ActualText`, `/Alt` and `/E`, and until this
/// pair existed **only the first had a fixture any redaction probe opened**. The
/// other two were exercised by hand-built Rust dictionaries alone, so the loader
/// had never once produced them --- the gap `docs/PLAN.md` Phase 3 names, and the
/// reason the array could have lost two thirds of its entries with nothing here
/// going red.
///
/// The names deliberately do not begin with `STRUCT-CARRIER` or `STRUCT-OTHER`.
/// `verify::scan` matches by substring, so a marker named `STRUCT-CARRIER-ALT`
/// would keep the `STRUCT-CARRIER` assertion green by surviving --- *a check name
/// that is a prefix of another cannot be aimed at*, which `docs/TRAPS.md` already
/// records from a different direction.
const STRUCT_ALT_GONE: &str = "STRUCT-ALT-GONE";
/// `/E` on the element owning the redacted line's `/MCID`. Must go.
const STRUCT_E_GONE: &str = "STRUCT-E-GONE";
/// `/Alt` on the element for the untouched line. Must stay.
const STRUCT_ALT_KEPT: &str = "STRUCT-ALT-KEPT";
/// `/E` on the element for the untouched line. Must stay.
const STRUCT_E_KEPT: &str = "STRUCT-E-KEPT";
/// The outline entry before the carrier. Must stay --- and it is the one that
/// catches a removal that drops the object without splicing the chain, since
/// that leaves it with no `/Next` and the entry after it unreachable.
const OUTLINE_BEFORE: &str = "OUTLINE-BEFORE";
/// Under the carrier, with a title that matches nothing. Must go anyway: a
/// section's subsections belong to the section.
const OUTLINE_CHILD: &str = "OUTLINE-CHILD";
/// After the carrier. Must stay, and reaching it at all is the check.
const OUTLINE_AFTER: &str = "OUTLINE-AFTER";

/// In `/Info /Producer`, and nowhere else in the file. Must go: a redaction
/// In `/Info /Producer`, and nowhere else in the file. Must go: a redaction
/// takes the document's own description of itself, `/Info` and XMP alike.
///
/// The producer string rather than the title, because the title *is* the secret
/// and its going would be indistinguishable from the page's own copy going.
const INFO_MARKER: &str = "tpdf spike 0.3 fixture";

/// The form field whose `/V` is the redacted line's own account number.
///
/// Its widget is at the far corner of the page, so the annotation pass leaves
/// it: the value rule is the only thing that can take this field.
const FIELD_CARRIER: &str = "FIELD-CARRIER";
/// The widget under it, which has to come with the field it belongs to.
const FIELD_WIDGET: &str = "WIDGET-UNDER-CARRIER";
/// A field holding somebody else's answer. The form's over-removal control.
const FIELD_KEEP: &str = "FIELD-KEEP";

fn main() -> ExitCode {
    let library = std::env::args()
        .skip_while(|a| a != "--library")
        .nth(1)
        .map_or_else(default_library, PathBuf::from);
    if std::env::args().any(|a| a == "--survey") {
        return match survey(&library) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("[FAIL] {e}");
                ExitCode::from(2)
            }
        };
    }
    match run(&library) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            ExitCode::from(2)
        }
    }
}

fn run(library: &Path) -> Result<bool, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repo root")?
        .to_path_buf();
    let source = root.join("testdata").join(FILE);
    if !source.exists() {
        println!(
            "[SKIP] {} is not built; run scripts/make_fixtures.sh",
            source.display()
        );
        return Ok(true);
    }
    let bindings = bind(library)?;
    let document = OpenDocument::open(bindings, &source, None).map_err(|why| why.reason)?;

    // The rectangle a reader would have dragged. Built from the character boxes
    // rather than typed in, so the probe is about the redaction and not about
    // whether somebody transcribed a coordinate correctly.
    let region = {
        let page = document.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        box_of(&extracted, REMOVE).ok_or_else(|| format!("{REMOVE} is not on page 1 of {FILE}"))?
    };
    println!(
        "[..] region {:?} around {REMOVE:?} in the file's display space",
        region.map(|v| (v * 10.0).round() / 10.0)
    );

    let plans = render::redaction_plans_of(&document, 0, &[region])?;
    let plan = plans.first().ok_or("no plan came back for one region")?;
    let mut ok = true;
    ok &= check(
        "the plan names at least one show operator",
        !plan.shows.is_empty(),
    );
    // **Asserted as present, not as absent**, and that is what this fixture is
    // for: `make_text_pdf.py` draws four unrelated non-text objects and one of
    // them is a path across the region. So the plan is deliberately
    // *incomplete*, which is the ordinary case rather than the exotic one --- a
    // rule under a line of text is what almost every real document has. The
    // removal still happens and the file still cannot be called clean, which is
    // the whole shape of `redact::Applied`.
    ok &= check(
        "the region covers a path, so the plan reports it and is not complete",
        plan.unhandled.iter().any(|object| object.kind == "path"),
    );
    for object in &plan.unhandled {
        println!("    unhandled: {}", object.sentence());
    }
    ok &= check(
        &format!("what it would take contains {REMOVE:?}"),
        plan.taking.contains(REMOVE),
    );
    println!("    it would take {:?}", plan.taking.trim());

    let out = std::env::temp_dir().join("tpdf-redact-apply-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let count = document.page_count();
    save::write_copy(&source, &plan_for(count, plan), &out, None, &save::Here)
        .map_err(|why| why.message)?;
    let bytes = std::fs::read(&out).map_err(|why| why.to_string())?;
    println!("[..] wrote {} bytes to {}", bytes.len(), out.display());

    // Reader one: the bytes. The control is `KEEP` --- a scan that reports the
    // needle absent and the survivor absent has told you nothing at all.
    let report = verify::scan(&bytes, &[REMOVE.to_string(), KEEP.to_string()], None);
    ok &= check(
        &format!("the byte scan no longer finds {REMOVE:?}"),
        !report.found.contains(REMOVE),
    );
    ok &= check(
        &format!("and still finds {KEEP:?}, so it can see this file at all"),
        report.found.contains(KEEP),
    );

    // Reader two: PDFium, which shares no code with the scan and reads the
    // written file rather than the buffer that produced it.
    let after = OpenDocument::open(bindings, &out, None).map_err(|why| why.reason)?;
    let read_back = {
        let page = after.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        as_string(&extracted)
    };
    ok &= check(
        &format!("PDFium no longer extracts {REMOVE:?}"),
        !read_back.contains(REMOVE),
    );
    ok &= check(
        &format!("and still extracts {KEEP:?}"),
        read_back.contains(KEEP),
    );

    ok &= annotations(bindings, &root)?;
    ok &= in_place(bindings, &source)?;
    ok &= forms(bindings, &root)?;
    ok &= images(bindings, &root)?;

    println!(
        "{}",
        if ok {
            "[OK] the region was removed, and both readers agree"
        } else {
            "[FAIL] see above"
        }
    );
    Ok(ok)
}

/// The same removal pointed at the reader's own file, staged and committed.
///
/// `docs/PLAN.md` §6 step 3 as that section states it, where everything above is
/// the copy that shipped first. What is new is not the removal --- it is the
/// same `apply_redactions` --- but the **write**: stage a sibling, check the
/// source has not moved under it, rename over it, and read back the path the
/// reader now has rather than the one that was written.
///
/// **Its own file, copied out of `testdata/` first.** This is the one phase that
/// destroys what it is pointed at, and pointing it at a fixture would leave
/// every later run of every other probe reading a redacted `text-base14.pdf`.
/// The copy is also what makes the fingerprint honest: `stage_in_place` refuses
/// a plan with no `opened_as`, so this takes one of the copy the way `edits.rs`
/// takes one of the file a reader opened.
///
/// **Four checks and two of them are controls.** The needle must be gone from
/// the reader's own path; `KEEP` must still be there, or a scan that cannot look
/// would pass the first; the file must still open in PDFium, because a rename
/// that landed a half-written sibling would satisfy both of those and leave the
/// reader nothing; and the staged sibling must be gone, since a temporary file
/// left beside a redacted document holds the unredacted bytes.
fn in_place(
    bindings: &'static dyn pdfium_render::prelude::PdfiumLibraryBindings,
    fixture: &Path,
) -> Result<bool, String> {
    let mine = std::env::temp_dir().join("tpdf-redact-in-place-probe.pdf");
    let _ = std::fs::remove_file(&mine);
    std::fs::copy(fixture, &mine)
        .map_err(|why| format!("could not make a file of my own: {why}"))?;

    let document = OpenDocument::open(bindings, &mine, None).map_err(|why| why.reason)?;
    let count = document.page_count();
    let region = {
        let page = document.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        box_of(&extracted, REMOVE)
            .ok_or_else(|| format!("{REMOVE} is not on page 1 of the copy"))?
    };
    let plans = render::redaction_plans_of(&document, 0, &[region])?;
    let plan = plans.first().ok_or("no plan came back for one region")?;
    let mut planned = plan_for(count, plan);
    planned.opened_as = Some(tpdf_lib::fingerprint::Fingerprint::of(&mine)?);

    // The document has to be closed before the rename, which is the ordering
    // `save_document` exists to get right: a rename over a mapped file succeeds
    // on macOS and leaves the mapping serving the inode that is no longer at
    // that path, and Windows refuses it outright while a section is open.
    let staged =
        save::stage_in_place(&mine, &planned, None, &save::Here).map_err(|why| why.message)?;
    let sibling = staged.path.clone();
    drop(document);
    save::verify_before_commit(&staged, &mine).map_err(|why| why.message)?;
    save::commit_in_place(&staged.path, &mine)?;

    let bytes = std::fs::read(&mine).map_err(|why| why.to_string())?;
    println!(
        "[..] redacted {} in place, {} bytes",
        mine.display(),
        bytes.len()
    );
    let report = verify::scan(&bytes, &[REMOVE.to_string(), KEEP.to_string()], None);
    let mut ok = check(
        &format!("in place: the reader's own file no longer holds {REMOVE:?}"),
        !report.found.contains(REMOVE),
    );
    ok &= check(
        &format!("in place: and still holds {KEEP:?}, so the scan can see it"),
        report.found.contains(KEEP),
    );
    // A rename that landed something unreadable would pass both of those, since
    // neither of them parses. This is the check that says the reader has a
    // document rather than a file with the right bytes missing from it.
    let after = OpenDocument::open(bindings, &mine, None).map_err(|why| why.reason)?;
    ok &= check(
        "in place: it still opens, with every page it had",
        after.page_count() == count,
    );
    ok &= check(
        "in place: the staged sibling is gone, and it held the unredacted bytes",
        !sibling.exists(),
    );
    drop(after);
    let _ = std::fs::remove_file(&mine);
    Ok(ok)
}

/// The annotation carrier: a comment over the words goes, the other one stays.
///
/// `docs/PLAN.md` §6's *Annotations* row, on the path a reader takes. A sticky
/// note anchored on the text being redacted quotes it --- that is what a comment
/// about a passage is --- and every reader goes on displaying it after the
/// drawing is gone. Both annotations here are hidden (`/F 2`), which is the case
/// no visual check can reach.
///
/// **Three assertions and the middle one is the control.** The marker over the
/// line must go; the marker away from it must stay; and the secret itself must
/// still be found, because `/Info /Title` and the surviving annotation both hold
/// it and this command does not touch either. If that last one ever flips, the
/// document-level carriers are being cleared too and this probe needs rewriting
/// rather than celebrating.
fn annotations(
    bindings: &'static dyn pdfium_render::prelude::PdfiumLibraryBindings,
    root: &Path,
) -> Result<bool, String> {
    let source = root.join("testdata").join(CARRIER_FILE);
    if !source.exists() {
        println!("[SKIP] {CARRIER_FILE} is not built");
        return Ok(true);
    }
    let document = OpenDocument::open(bindings, &source, None).map_err(|why| why.reason)?;
    let region = {
        let page = document.page(0).map_err(|e| e.to_string())?;
        let extracted = text::extract(&page).map_err(|e| e.to_string())?;
        box_of(&extracted, REMOVE)
            .ok_or_else(|| format!("{REMOVE} is not on page 1 of {CARRIER_FILE}"))?
    };
    let plans = render::redaction_plans_of(&document, 0, &[region])?;
    let plan = plans.first().ok_or("no plan came back for one region")?;

    let markers = [
        ANNOT_OVER.to_string(),
        ANNOT_AWAY.to_string(),
        STRUCT_OVER.to_string(),
        STRUCT_ANCESTOR.to_string(),
        STRUCT_OTHER.to_string(),
        STRUCT_ALT_GONE.to_string(),
        STRUCT_E_GONE.to_string(),
        STRUCT_ALT_KEPT.to_string(),
        STRUCT_E_KEPT.to_string(),
        INFO_MARKER.to_string(),
        FIELD_CARRIER.to_string(),
        FIELD_WIDGET.to_string(),
        FIELD_KEEP.to_string(),
    ];
    let before = std::fs::read(&source).map_err(|why| why.to_string())?;
    let seen = verify::scan(&before, &markers, None);
    // The control for the controls: every marker has to be in the file to begin
    // with, or no assertion below can fail.
    let mut ok = check(
        "the fixture carries every carrier marker to begin with",
        markers.iter().all(|marker| seen.found.contains(marker)),
    );

    let out = std::env::temp_dir().join("tpdf-redact-annots-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let count = document.page_count();
    save::write_copy(&source, &plan_for(count, plan), &out, None, &save::Here)
        .map_err(|why| why.message)?;
    let bytes = std::fs::read(&out).map_err(|why| why.to_string())?;
    println!("[..] wrote {} bytes to {}", bytes.len(), out.display());

    let mut wanted: Vec<String> = markers.to_vec();
    wanted.push(REMOVE.to_string());
    let report = verify::scan(&bytes, &wanted, None);
    ok &= check(
        &format!("the annotation over the words is gone ({ANNOT_OVER})"),
        !report.found.contains(ANNOT_OVER),
    );
    ok &= check(
        &format!("the annotation away from them is not ({ANNOT_AWAY})"),
        report.found.contains(ANNOT_AWAY),
    );
    ok &= check(
        &format!("the structure element holding the line is gone ({STRUCT_OVER})"),
        !report.found.contains(STRUCT_OVER),
    );
    ok &= check(
        &format!("so is the element above it ({STRUCT_ANCESTOR})"),
        !report.found.contains(STRUCT_ANCESTOR),
    );
    ok &= check(
        &format!("and the element for the untouched line is not ({STRUCT_OTHER})"),
        report.found.contains(STRUCT_OTHER),
    );
    // The other two thirds of `SHADOW_TEXT`, in both directions. `/ActualText`
    // above could pass on its own with `/Alt` and `/E` never read at all, which
    // is what it did until this fixture carried them.
    ok &= check(
        &format!("its /Alt went with it ({STRUCT_ALT_GONE})"),
        !report.found.contains(STRUCT_ALT_GONE),
    );
    ok &= check(
        &format!("and its /E ({STRUCT_E_GONE})"),
        !report.found.contains(STRUCT_E_GONE),
    );
    ok &= check(
        &format!("while the untouched line keeps its /Alt ({STRUCT_ALT_KEPT})"),
        report.found.contains(STRUCT_ALT_KEPT),
    );
    ok &= check(
        &format!("and its /E ({STRUCT_E_KEPT})"),
        report.found.contains(STRUCT_E_KEPT),
    );
    ok &= check(
        &format!("the document's own description of itself is gone ({INFO_MARKER})"),
        !report.found.contains(INFO_MARKER),
    );
    ok &= check(
        &format!("and {REMOVE:?} is still in the file, which the surviving annotation keeps"),
        report.found.contains(REMOVE),
    );

    // **The form.** Both widgets are hidden, so nothing on the page draws
    // either answer --- which is the leak rather than a convenience: a byte scan
    // finds what no reader can see. The carrier's widget is at the far corner,
    // so the annotation pass leaves it and the *value* rule is the only thing
    // that can take the field; the widget then has to come with it, or the page
    // keeps an annotation whose `/Parent` is gone.
    ok &= check(
        &format!("the field holding what went is gone ({FIELD_CARRIER})"),
        !report.found.contains(FIELD_CARRIER),
    );
    ok &= check(
        &format!("and the widget under it came with it ({FIELD_WIDGET})"),
        !report.found.contains(FIELD_WIDGET),
    );
    ok &= check(
        &format!("the field holding somebody else's answer is not ({FIELD_KEEP})"),
        report.found.contains(FIELD_KEEP),
    );

    // **The outline, read through PDFium rather than out of the bytes.** Every
    // check above asks whether a marker is anywhere in the file, which cannot
    // answer the question this carrier poses: an entry is *reachable* or it is
    // not, and an entry that is still an object but has been spliced out of the
    // chain is neither present nor absent by a byte scan. So this walks it the
    // way the sidebar does.
    //
    // Which is also the point: `outline::read` is what feeds the panel, so a
    // title it still returns is a title a reader still sees in tpdf, in the file
    // that was supposed to have lost it.
    let after = OpenDocument::open(bindings, &out, None).map_err(|why| why.reason)?;
    let titles: Vec<String> = outline::read(&after)
        .items
        .iter()
        .flat_map(flatten)
        .collect();
    println!("[..] the redacted file's outline reads {titles:?}");
    ok &= check(
        "the outline entry naming the removed line is gone",
        !titles.iter().any(|title| REMOVE.contains(title.as_str())),
    );
    ok &= check(
        &format!("and so is what hung under it ({OUTLINE_CHILD})"),
        !titles.iter().any(|title| title == OUTLINE_CHILD),
    );
    // The two controls, and the second is the one this phase exists for. A
    // removal that drops the entry without splicing takes `/Next` off
    // OUTLINE-BEFORE, so a walk reaches it and stops --- OUTLINE-AFTER is then
    // still an object in the file and invisible to every reader.
    ok &= check(
        &format!("the entry before it survives ({OUTLINE_BEFORE})"),
        titles.iter().any(|title| title == OUTLINE_BEFORE),
    );
    ok &= check(
        &format!("and the one after it is still REACHABLE ({OUTLINE_AFTER})"),
        titles.iter().any(|title| title == OUTLINE_AFTER),
    );
    Ok(ok)
}

/// How often the correspondence guard would refuse, across the whole corpus.
///
/// **The one number that decides whether this feature works on real files.**
/// `redact::remove_shows` refuses when the show operators `lopdf` decodes
/// disagree with the text objects PDFium counted, because nothing connects the
/// two lists but order and a mis-addressed removal deletes the wrong words while
/// reporting success. Spike 0.3 measured 4:4 on four fixtures built for it and
/// said plainly that a `TJ` split across objects or a Form XObject contributing
/// from another stream breaks it.
///
/// What that spike could not say is how often it breaks. This walks every page
/// of every fixture in `testdata/` and counts, so *"the guard may refuse on real
/// documents"* becomes a fraction instead of a worry. It asserts nothing --- a
/// page that disagrees is a fact about the corpus, not a defect --- and prints
/// the pages that do, because those are the ones worth reading.
fn survey(library: &Path) -> Result<bool, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repo root")?
        .to_path_buf();
    let bindings = bind(library)?;
    let mut agree = 0usize;
    let mut differ = 0usize;
    let mut unreadable = 0usize;
    let mut files = 0usize;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("testdata"))
        .map_err(|why| why.to_string())?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();

    for path in paths {
        let Ok(document) = OpenDocument::open(bindings, &path, None) else {
            // An encrypted or malformed fixture. Counted rather than skipped
            // silently: a survey that quietly drops what it cannot open reports
            // a cleaner corpus than it measured.
            unreadable += 1;
            continue;
        };
        let Ok(mut lopdf) = lopdf::Document::load(&path) else {
            unreadable += 1;
            continue;
        };
        files += 1;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let pages: Vec<lopdf::ObjectId> = lopdf.get_pages().values().copied().collect();
        for (at, page_id) in pages.iter().enumerate() {
            let Ok(page) = document.page(at as u32) else {
                continue;
            };
            let Ok(objects) = tpdf_lib::objects::read(&page) else {
                continue;
            };
            let shows = show_operators(&mut lopdf, *page_id);
            if shows == objects.text.len() {
                agree += 1;
            } else {
                differ += 1;
                println!(
                    "    {name} page {}: {} show operator(s), {} text object(s)",
                    at + 1,
                    shows,
                    objects.text.len()
                );
            }
        }
    }

    let total = agree + differ;
    println!("[..] {files} readable file(s), {unreadable} not opened; {total} page(s) compared");
    println!(
        "[..] {agree} agree, {differ} differ --- a removal is refused on {:.1}% of pages",
        if total == 0 {
            0.0
        } else {
            100.0 * differ as f64 / total as f64
        }
    );
    Ok(true)
}

/// How many text-showing operators one page's content stream holds.
///
/// The same count `redact::remove_shows` derives, and derived here through the
/// same decode rather than through a second rule: what is being measured is
/// whether the two *populations* agree, so a private notion of what counts as a
/// show operator would be measuring this file's opinion instead.
/// Text inside a Form XObject: removed where it is marked, refused where it is shared.
///
/// PDFium enumerates a form as **one** page object, so `remove_shows` cannot
/// address the text inside it --- `docs/PLAN.md` §6 measured that carrier at
/// 9,310 of 154,095 realistic regions across 41 real documents, three times the
/// image count. `remove_form_shows` reaches it by descending one level and
/// rewriting the form's own stream.
///
/// **Its own fixture, and the fixture is most of the design.** Every other file
/// in `testdata/` carrying `/Subtype /Form` carries it as an annotation
/// appearance stream, which is a different thing in a different place.
/// `form-xobject.pdf` places its forms with a translating matrix, so a bounds
/// convention error cannot pass; gives one form two lines, so "removed the right
/// one" can be told from "removed everything"; nests a form inside a form, so
/// the one-level limit has a subject; and draws one form twice, so the refusal
/// does.
fn forms(
    bindings: &'static dyn pdfium_render::prelude::PdfiumLibraryBindings,
    root: &Path,
) -> Result<bool, String> {
    const FORM_FILE: &str = "form-xobject.pdf";
    /// Inside a form, and marked. Must go.
    const IN_FORM: &str = "REDACT ME: account 4711-0815 inside a form";
    /// Inside the **same** form and not marked. Must stay --- without it, a
    /// removal that emptied the whole stream would pass every other check here.
    const BESIDE_IT: &str = "Keep this line, it is in the same form";
    /// Drawn by the page itself. Must stay: a removal inside a form must not
    /// reach the page's own content.
    const ON_THE_PAGE: &str = "Sphinx of black quartz";
    /// Inside a form that is drawn twice. Must stay, because the removal refuses.
    const SHARED: &str = "This form is drawn twice";
    /// Page 3, at one end of a form whose other child is 300 points away.
    const FAR_TEXT: &str = "Text at one end of the form";

    let source = root.join("testdata").join(FORM_FILE);
    if !source.exists() {
        println!("[SKIP] {} is not built", source.display());
        return Ok(true);
    }
    let document = OpenDocument::open(bindings, &source, None).map_err(|why| why.reason)?;
    let count = document.page_count();
    let mut ok = true;

    // --- page 1: the ordinary case -----------------------------------------
    let page = document.page(0).map_err(|e| e.to_string())?;
    let objects = tpdf_lib::objects::read(&page).map_err(|e| e.to_string())?;
    let Some(form) = objects.forms.first() else {
        println!("[FAIL] page 1 of {FORM_FILE} has no form XObject on it");
        return Ok(false);
    };
    ok &= check(
        "the descent reads what a form draws",
        form.text.iter().any(|t| t.draws.trim() == IN_FORM),
    );
    // The nested form: one level down is reachable, two is not, and the second
    // has to be *reported* or a region over it would be certified.
    let nested = objects
        .forms
        .iter()
        .any(|f| f.unreachable.iter().any(|u| u.kind == "form"));
    ok &= check(
        "a form inside a form is reported rather than followed",
        nested,
    );

    let Some(target) = form.text.iter().find(|t| t.draws.trim() == IN_FORM) else {
        println!("[FAIL] {IN_FORM:?} is not inside a form on page 1");
        return Ok(false);
    };
    let r = target.bounds;
    let region = [r[0] - 1.0, r[1] - 1.0, r[2] + 1.0, r[3] + 1.0];
    let plan = redact::covered(&objects.all, &objects.forms, region);
    ok &= check(
        "the region names the form's line and nothing on the page",
        plan.form_shows.len() == 1 && plan.shows.is_empty(),
    );

    let out = std::env::temp_dir().join("tpdf-redact-form-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let planned = form_plan(0, count, &objects, &plan, region);
    save::write_copy(&source, &planned, &out, None, &save::Here).map_err(|why| why.message)?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let holds = |needle: &str| {
        bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    };
    ok &= check(
        "the marked line is gone from inside the form",
        !holds(IN_FORM),
    );
    ok &= check(
        "the line beside it in the same form is still there",
        holds(BESIDE_IT),
    );
    ok &= check("the page's own text is untouched", holds(ON_THE_PAGE));
    let _ = std::fs::remove_file(&out);

    // --- page 2: the shared form -------------------------------------------
    let page = document.page(1).map_err(|e| e.to_string())?;
    let shared = tpdf_lib::objects::read(&page).map_err(|e| e.to_string())?;
    let Some(first) = shared.forms.first().and_then(|f| f.text.first()) else {
        println!("[FAIL] page 2 of {FORM_FILE} has no text inside a form");
        return Ok(false);
    };
    // What the refusal below is refusing. Without it a fixture that stopped
    // holding this line would still produce a refusal --- of an empty plan ---
    // and the check would read as a pass.
    ok &= check(
        "the shared form holds the line the refusal is about",
        first.draws.trim() == SHARED,
    );
    let r = first.bounds;
    let region = [r[0] - 1.0, r[1] - 1.0, r[2] + 1.0, r[3] + 1.0];
    let plan = redact::covered(&shared.all, &shared.forms, region);
    let planned = form_plan(1, count, &shared, &plan, region);
    let out = std::env::temp_dir().join("tpdf-redact-form-shared-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let refused = save::write_copy(&source, &planned, &out, None, &save::Here);
    ok &= check(
        "a form the document draws twice is refused",
        refused
            .as_ref()
            .err()
            .is_some_and(|why| why.message.contains("draws 2 time(s)")),
    );
    // A refusal that still wrote the file would be the worst outcome of the
    // three: the reader is told nothing happened and has a copy that says
    // otherwise. **Nothing weaker than "there is no file"**, and the version
    // that shipped for ten minutes was `!out.exists() || <the file still holds
    // the text>` --- which is satisfied by every outcome there is, including the
    // one it was written to catch.
    ok &= check("the refusal wrote no file", !out.exists());
    let _ = std::fs::remove_file(&out);

    // --- page 3: a child at the other end of the form ----------------------
    // The discrimination every other page here is missing. On pages 1 and 2 the
    // unreachable child sits on top of the form's text, so "report a child the
    // region covers" and "report every child of a form the region touches" give
    // the same answer and no check could tell them apart. Measured over 40 real
    // documents, the second rule accounted for 56% of every refusal --- a form
    // is routinely a letterhead, and a region over one line inside one reported
    // every picture in it.
    let page = document.page(2).map_err(|e| e.to_string())?;
    let far = tpdf_lib::objects::read(&page).map_err(|e| e.to_string())?;
    let Some(form) = far.forms.first() else {
        println!("[FAIL] page 3 of {FORM_FILE} has no form XObject on it");
        return Ok(false);
    };
    let Some(line) = form.text.iter().find(|t| t.draws.trim() == FAR_TEXT) else {
        println!("[FAIL] page 3 of {FORM_FILE} does not hold {FAR_TEXT:?} inside a form");
        return Ok(false);
    };
    let Some(path) = form.unreachable.iter().find(|u| u.kind == "path") else {
        println!("[FAIL] page 3's form holds no path for the region to miss");
        return Ok(false);
    };
    // The fixture's own discrimination, asserted rather than assumed: two
    // children that overlapped would make both checks below pass on either rule.
    ok &= check(
        "the form's two children are far enough apart to tell the rules apart",
        !redact::overlaps(line.bounds, path.bounds),
    );

    let r = line.bounds;
    let over_text = [r[0] - 1.0, r[1] - 1.0, r[2] + 1.0, r[3] + 1.0];
    let plan = redact::covered(&far.all, &far.forms, over_text);
    ok &= check(
        "a region over the form's text does not report the path at the other end",
        plan.form_shows.len() == 1 && plan.unhandled.is_empty(),
    );
    ok &= check("that region is therefore complete", plan.is_complete());

    let r = path.bounds;
    let over_path = [r[0] + 1.0, r[1] + 1.0, r[2] - 1.0, r[3] - 1.0];
    let plan = redact::covered(&far.all, &far.forms, over_path);
    ok &= check(
        "a region over the path itself still reports it",
        plan.unhandled.iter().any(|u| u.kind == "path") && plan.form_shows.is_empty(),
    );
    Ok(ok)
}

/// A region over a picture removes the picture, and its bytes leave the file.
///
/// **Two claims, and only the second is a redaction.** Deleting the `Do` stops
/// the page drawing the image; the stream is still an object, still reachable
/// from the page's resources, and every byte of the picture is still there for
/// anyone who opens the file with something other than a viewer. So this greps
/// the written bytes for the image's own pixels rather than asking what the page
/// draws --- which is why `image-region.pdf` stores them **uncompressed**, as one
/// repeated marker per image. A compressed stream could not be searched for, and
/// a check that could not tell the two claims apart would pass on either.
fn images(
    bindings: &'static dyn pdfium_render::prelude::PdfiumLibraryBindings,
    root: &Path,
) -> Result<bool, String> {
    const IMAGE_FILE: &str = "image-region.pdf";
    /// The marked picture's pixels. Must leave the file.
    const MARKED: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
    /// The other picture on the same page, not marked. Must stay --- without it,
    /// a removal that dropped every image would pass every other check here.
    const BESIDE_IT: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];
    const ON_THE_PAGE: &str = "Sphinx of black quartz";

    let source = root.join("testdata").join(IMAGE_FILE);
    if !source.exists() {
        println!("[SKIP] {} is not built", source.display());
        return Ok(true);
    }
    let document = OpenDocument::open(bindings, &source, None).map_err(|why| why.reason)?;
    let count = document.page_count();
    let mut ok = true;

    let page = document.page(0).map_err(|e| e.to_string())?;
    let objects = tpdf_lib::objects::read(&page).map_err(|e| e.to_string())?;
    let Some((at, first)) = objects
        .all
        .iter()
        .enumerate()
        .find(|(_, object)| object.kind == "image")
    else {
        println!("[FAIL] page 1 of {IMAGE_FILE} draws no image");
        return Ok(false);
    };
    let _ = at;
    let r = first.bounds;
    let region = [r[0] + 1.0, r[1] + 1.0, r[2] - 1.0, r[3] - 1.0];
    let plan = redact::covered(&objects.all, &objects.forms, region);
    ok &= check(
        "a region over a picture names it rather than refusing",
        plan.images == vec![0] && plan.unhandled.is_empty(),
    );

    let out = std::env::temp_dir().join("tpdf-redact-image-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let planned = image_plan(0, count, &objects, &plan, region);
    save::write_copy(&source, &planned, &out, None, &save::Here).map_err(|why| why.message)?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let holds = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    ok &= check(
        "the marked picture's own pixels are gone from the file",
        !holds(&MARKED),
    );
    ok &= check(
        "the other picture on the page is still there",
        holds(&BESIDE_IT),
    );
    ok &= check(
        "the page's own text is untouched",
        bytes
            .windows(ON_THE_PAGE.len())
            .any(|w| w == ON_THE_PAGE.as_bytes()),
    );
    // Reopening is what says the file is still a PDF rather than merely shorter:
    // a rewrite that dropped an object something still pointed at would satisfy
    // both checks above.
    ok &= check(
        "and the copy still opens with both its pages",
        OpenDocument::open(bindings, &out, None)
            .map(|d| d.page_count())
            .unwrap_or(0)
            == 2,
    );
    let _ = std::fs::remove_file(&out);

    // --- page 2: the picture drawn twice ------------------------------------
    let page = document.page(1).map_err(|e| e.to_string())?;
    let twice = tpdf_lib::objects::read(&page).map_err(|e| e.to_string())?;
    let Some(first) = twice.all.iter().find(|object| object.kind == "image") else {
        println!("[FAIL] page 2 of {IMAGE_FILE} draws no image");
        return Ok(false);
    };
    let r = first.bounds;
    let region = [r[0] + 1.0, r[1] + 1.0, r[2] - 1.0, r[3] - 1.0];
    let plan = redact::covered(&twice.all, &twice.forms, region);
    ok &= check(
        "the shared picture is named before the refusal is asked for",
        !plan.images.is_empty(),
    );
    let planned = image_plan(1, count, &twice, &plan, region);
    let out = std::env::temp_dir().join("tpdf-redact-image-shared-probe.pdf");
    let _ = std::fs::remove_file(&out);
    let refused = save::write_copy(&source, &planned, &out, None, &save::Here);
    ok &= check(
        "a picture the document draws twice is refused",
        refused
            .as_ref()
            .err()
            .is_some_and(|why| why.message.contains("drawn 2 time(s)")),
    );
    ok &= check("the refusal wrote no file", !out.exists());
    let _ = std::fs::remove_file(&out);
    Ok(ok)
}

/// A plan removing exactly the images `plan` names from one page.
fn image_plan(
    page: u32,
    count: u32,
    objects: &tpdf_lib::objects::PageObjects,
    plan: &redact::Plan,
    region: [f32; 4],
) -> Plan {
    let mut planned = form_plan(page, count, objects, plan, region);
    if let Some(redaction) = planned.redactions.first_mut() {
        redaction.images = plan.images.clone();
        redaction.image_objects = objects
            .all
            .iter()
            .filter(|object| object.kind == "image")
            .count();
    }
    planned
}

/// A plan removing exactly what `plan` names from one page.
fn form_plan(
    page: u32,
    count: u32,
    objects: &tpdf_lib::objects::PageObjects,
    plan: &redact::Plan,
    region: [f32; 4],
) -> Plan {
    Plan {
        opened_as: None,
        // Taken from the document rather than written down. It was a literal 2
        // until `form-xobject.pdf` gained a third page, and the save then
        // refused with *"the document on disk has 3 page(s) and the edits were
        // made against 2"* --- a guard doing its job about a fixture constant
        // this file had copied. `docs/TRAPS.md`: a new corpus has to satisfy the
        // sample points every existing check hardcodes.
        baseline: count,
        pages: (0..count)
            .map(|at| PageView {
                id: u64::from(at) + 1,
                source: PageSource::Baseline(at),
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: page,
            shows: plan.shows.clone(),
            text_objects: objects.text.len(),
            areas: vec![region],
            taking: Vec::new(),
            form_shows: plan.form_shows.clone(),
            form_text_objects: objects
                .forms
                .iter()
                .map(|form| (form.at, form.text.len()))
                .collect(),
            images: Vec::new(),
            image_objects: 0,
        }],
        notes: Vec::new(),
        discards: Vec::new(),
    }
}

fn show_operators(doc: &mut lopdf::Document, page: lopdf::ObjectId) -> usize {
    let Ok(data) = doc.get_page_content_with_limit(page, redact::MAX_CONTENT_BYTES) else {
        return usize::MAX;
    };
    let Ok(content) = lopdf::content::Content::decode(&data) else {
        return usize::MAX;
    };
    content
        .operations
        .iter()
        .filter(|operation| matches!(operation.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
        .count()
}

/// The save plan the redact command builds: the file as it is, plus the removal.
///
/// Every page kept, unturned and uncropped, no marks --- which is a document a
/// reader has not edited at all. That is deliberate: it is the plan for which
/// `Plan::is_identity` would otherwise answer *true* and hand the file over
/// unchanged, so this probe is also the check that a redaction stops it.
fn plan_for(pages: u32, region: &redact::RegionPlan) -> Plan {
    Plan {
        baseline: pages,
        opened_as: None,
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at),
                source: PageSource::Baseline(at),
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: vec![PlannedRedaction {
            source: 0,
            shows: region.shows.clone(),
            text_objects: region.text_objects,
            areas: vec![region.area],
            // What `lib.rs` carries from the same field, so the probe drives
            // the outline carrier through the same input the command does.
            taking: vec![region.taking.trim().to_string()],
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }],
        notes: Vec::new(),
        discards: Vec::new(),
    }
}

/// An outline item's title and every title beneath it.
///
/// Depth-first and flat, because what this phase asks is *which titles does a
/// reader still see* --- and the shape of the tree is not part of that question.
fn flatten(item: &outline::OutlineItem) -> Vec<String> {
    let mut out = vec![item.title.clone()];
    for child in &item.children {
        out.extend(flatten(child));
    }
    out
}

/// The characters of a page as one string, in the file's own order.
///
/// Deliberately not the *reading* order: what this probe asks is whether a word
/// is still in the page at all, and reordering it could only make the answer
/// harder to read.
fn as_string(page: &text::PageText) -> String {
    page.codes
        .iter()
        .filter_map(|code| char::from_u32(*code))
        .collect()
}

/// The box around a needle's characters, in the page's display space.
fn box_of(page: &text::PageText, needle: &str) -> Option<[f32; 4]> {
    let text = as_string(page);
    let at = text.find(needle)?;
    // Character indices, not byte offsets: the fixture is ASCII, and counting
    // characters is what stays right when it is not.
    let from = text[..at].chars().count();
    let to = from + needle.chars().count();
    let mut found: Option<[f32; 4]> = None;
    for index in from..to {
        let base = index * 4;
        let quad = [
            *page.boxes.get(base)?,
            *page.boxes.get(base + 1)?,
            *page.boxes.get(base + 2)?,
            *page.boxes.get(base + 3)?,
        ];
        found = Some(match found {
            None => quad,
            Some(so_far) => [
                so_far[0].min(quad[0]),
                so_far[1].min(quad[1]),
                so_far[2].max(quad[2]),
                so_far[3].max(quad[3]),
            ],
        });
    }
    found
}

fn check(what: &str, held: bool) -> bool {
    println!("{} {what}", if held { "[OK]  " } else { "[FAIL]" });
    held
}

fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

fn default_library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join("vendor")
        .join("pdfium")
        .join(if cfg!(windows) { "bin" } else { "lib" })
}
