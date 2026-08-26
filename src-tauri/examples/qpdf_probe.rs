//! Does an *independent validator* accept the files tpdf actually writes?
//!
//! `docs/PLAN.md` §6 step 5 asks for a parser that did not write a rewrite to
//! re-check it. Measuring that on 2026-08-26 produced an uncomfortable answer:
//! given a rewrite whose `/Size` claims more objects than the file holds,
//! `lopdf`'s loader says *OK, 8 pages*, PDFKit says *OK, 8 pages*, and only
//! `qpdf --check` objects. **There is no validator in the tpdf process**, so the
//! shipped check (`verify::structure`) is deliberately narrow --- a header, one
//! `%%EOF`, no trailing data, a `startxref` inside the file --- and says in its
//! own doc comment that it is not cross-reference validation.
//!
//! This is where the missing half is exercised. qpdf is not a dependency and is
//! not on a hosted runner; it is on a development machine, so the class of
//! defect spike 0.4 found is caught by running this rather than at run time.
//!
//! ## What it compares, and why both directions are failures
//!
//! Every fixture goes through the **real** writer --- `save::write_copy`, the
//! same call `Save a copy`, `Extract pages` and `Split` reach --- and the output
//! is put to two readers:
//!
//! * **qpdf refuses what we passed.** The rewrite produced a structurally broken
//!   file and shipped it. This is the defect the probe exists for.
//! * **we refused what qpdf passed.** Over-refusal, which is worse than no check
//!   at all: it would refuse to save a document the reader had just edited. The
//!   first draft of a `/Size` rule did exactly this, condemning a healthy swept
//!   rewrite of `links.pdf`, and it is why this probe compares rather than just
//!   running qpdf.
//!
//! ## The two controls, and the probe is worthless without them
//!
//! A sweep that reports "nothing found" looks identical whether the oracle ran
//! or was never invoked, and `docs/TRAPS.md` has that entry more than once. So:
//!
//! * **A planted structural defect must be refused by qpdf**, reproducing spike
//!   0.4's exactly --- sweep the graph and leave `max_id` above the highest
//!   surviving object, so `/Size` overcounts. It also re-measures the gap:
//!   `verify::structure` is expected to *pass* that file, and a run where it
//!   suddenly catches it means the shipped check grew a capability this probe's
//!   own documentation denies.
//! * **A planted trailing-bytes defect must be refused by us**, so a run where
//!   `verify::structure` reports nothing at all is distinguishable from one where
//!   it was not called.
//!
//! Usage:
//!     cargo run --release --example qpdf-probe [-- --only NAME]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use lopdf::Document;
use tpdf_lib::edits::{PageView, Plan};
use tpdf_lib::{save, sweep, verify};

/// Fixtures above this are skipped: the 321 MB scan rewrites in tens of seconds
/// and says nothing about the *shape* of what the writer emits, which is the
/// question here.
const LARGEST: u64 = 8 * 1024 * 1024;

/// How many rewrites must be examined before a clean run means anything.
///
/// A fresh checkout has no fixtures at all --- they are generated, `BUILD.md`
/// says so --- and without this the probe reports success having opened nothing.
const FEWEST: usize = 20;

fn main() -> ExitCode {
    let only = std::env::args()
        .skip_while(|arg| arg != "--only")
        .nth(1)
        .filter(|name| !name.is_empty());

    let Some(qpdf) = which_qpdf() else {
        println!(
            "[SKIP] qpdf is not on PATH, and it is the only reader that answers the question \
             this probe asks. `brew install qpdf`, then run it again. Nothing was checked."
        );
        // A skip, not a pass: the caller wanted a verdict and there is none.
        return ExitCode::from(2);
    };
    println!("[INFO] oracle: {}", qpdf.display());

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let scratch = root.join("target").join("qpdf-probe");
    let _ = std::fs::remove_dir_all(&scratch);
    if let Err(why) = std::fs::create_dir_all(&scratch) {
        println!("[FAIL] could not make a scratch directory: {why}");
        return ExitCode::FAILURE;
    }

    let mut failures = 0usize;
    let mut examined = 0usize;
    let mut refused = 0usize;

    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(root.join("testdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("pdf"))
        .collect();
    fixtures.sort();

    for fixture in &fixtures {
        let name = fixture
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if only.as_ref().is_some_and(|wanted| &name != wanted) {
            continue;
        }
        if std::fs::metadata(fixture)
            .map(|m| m.len())
            .unwrap_or(u64::MAX)
            > LARGEST
        {
            continue;
        }
        let Ok(source) = Document::load(fixture) else {
            continue;
        };
        let pages = source.get_pages().len();
        if pages == 0 {
            continue;
        }

        // Both writers a reader reaches: the identity copy, and one that drops a
        // page, which is the plan that makes `rewrite` run the mark-and-sweep.
        for (what, plan) in plans(pages as u32) {
            let out = scratch.join(format!("{name}.{what}.pdf"));
            if save::write_copy(fixture, &plan, &out).is_err() {
                // Encrypted, signed, or a shape the writer refuses by design.
                // Its refusal is another check's subject.
                refused += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&out) else {
                println!("[FAIL] {name} ({what}): the file we just wrote cannot be read");
                failures += 1;
                continue;
            };
            examined += 1;
            let ours = verify::structure(&bytes);
            let theirs = check(&qpdf, &out);
            match (ours.is_empty(), theirs.is_ok()) {
                (true, true) => {}
                (true, false) => {
                    // **Compared against the source's own verdict, not against
                    // nothing.** A rewrite faithfully carries a defect the input
                    // already had, and refusing it then says something about the
                    // fixture rather than about the writer. The first run of this
                    // probe reported `outline-hostile.pdf` for a loop in the
                    // `/Outlines` tree, which is what that fixture is *for*.
                    // Baselined lazily: qpdf is only asked about the source once
                    // its output has already been refused.
                    if check(&qpdf, fixture).is_err() {
                        println!(
                            "[INFO] {name} ({what}): qpdf refuses it, and refuses the source \
                             too -- carried forward, not introduced"
                        );
                        continue;
                    }
                    println!(
                        "[FAIL] {name} ({what}): qpdf accepts the source and refuses what tpdf \
                         wrote from it -- {}",
                        theirs.unwrap_err()
                    );
                    failures += 1;
                }
                (false, true) => {
                    println!(
                        "[FAIL] {name} ({what}): qpdf accepts it and verify::structure refuses \
                         it -- {}. Over-refusal is worse than no check: this would refuse to \
                         save a document a reader had just edited.",
                        ours.join("; ")
                    );
                    failures += 1;
                }
                (false, false) => println!(
                    "[INFO] {name} ({what}): both readers refuse it -- {}",
                    ours.join("; ")
                ),
            }
        }
    }

    println!("[INFO] {examined} rewrites checked, {refused} plans refused by the writer");
    if examined < FEWEST && only.is_none() {
        println!(
            "[FAIL] only {examined} rewrites were checked, which is too few to have tested \
             anything -- run scripts/ci_fixtures.py and the scripts BUILD.md names"
        );
        failures += 1;
    }

    failures += controls(&qpdf, &scratch, &root);

    if failures == 0 {
        println!("[OK] every rewrite tpdf wrote is accepted by qpdf, and both controls fired.");
        ExitCode::SUCCESS
    } else {
        println!("[FAIL] {failures} problem(s).");
        ExitCode::FAILURE
    }
}

/// The identity copy, and a copy that drops the last page where there is one.
fn plans(pages: u32) -> Vec<(&'static str, Plan)> {
    let mut out = vec![("whole", keeping(pages, (0..pages).collect()))];
    if pages > 1 {
        out.push(("dropped", keeping(pages, (0..pages - 1).collect())));
    }
    out
}

/// A plan over a `baseline`-page document keeping `kept`, unturned and uncropped.
fn keeping(baseline: u32, kept: Vec<u32>) -> Plan {
    Plan {
        opened_as: None,
        baseline,
        pages: kept
            .into_iter()
            .map(|source| PageView {
                id: u64::from(source) + 1,
                source,
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
    }
}

/// The two planted defects, each proving one reader is awake.
///
/// Returns how many did **not** behave. A control that does not fire is a
/// failure of the probe rather than of the code, and it is reported as loudly:
/// every verdict above rests on these.
fn controls(qpdf: &Path, scratch: &Path, root: &Path) -> usize {
    let mut wrong = 0;
    let Some(base) = ["links.pdf", "rotated.pdf", "columns.pdf"]
        .iter()
        .map(|name| root.join("testdata").join(name))
        .find(|path| path.exists())
    else {
        println!("[FAIL] no fixture to build the controls from, so nothing above is evidence");
        return 1;
    };

    // Control 1 -- spike 0.4's defect, reproduced. qpdf must refuse it, and
    // `verify::structure` is expected to miss it: that gap is what this probe
    // exists to cover, and a run where it closes on its own needs reading.
    match planted_stale_size(&base) {
        Err(why) => {
            println!("[FAIL] the stale-/Size control could not be built: {why}");
            wrong += 1;
        }
        Ok(bytes) => {
            let at = scratch.join("control-stale-size.pdf");
            let _ = std::fs::write(&at, &bytes);
            match check(qpdf, &at) {
                Ok(()) => {
                    println!(
                        "[FAIL] control: qpdf ACCEPTED a file whose /Size overcounts by 40. \
                         Either it did not run or it stopped checking, and every [OK] above \
                         means nothing."
                    );
                    wrong += 1;
                }
                Err(why) => println!("[OK] control: qpdf refuses a planted stale /Size -- {why}"),
            }
            if verify::structure(&bytes).is_empty() {
                println!(
                    "[INFO] control: verify::structure passes it, as documented -- the gap \
                     qpdf is here to cover is still exactly that gap"
                );
            } else {
                println!(
                    "[INFO] control: verify::structure now CATCHES the stale /Size. That is \
                     new, and its doc comment and docs/PLAN.md §6 both say it cannot -- read \
                     them before believing it."
                );
            }
        }
    }

    // Control 2 -- ours must fire on something. Without this a run where
    // `verify::structure` was never called reads exactly like a clean one.
    let mut trailing = std::fs::read(&base).unwrap_or_default();
    trailing.extend_from_slice(b"leftover");
    let complaints = verify::structure(&trailing);
    if complaints.is_empty() {
        println!(
            "[FAIL] control: verify::structure passed a file with 8 bytes after its %%EOF, \
             so it is not looking at anything."
        );
        wrong += 1;
    } else {
        println!(
            "[OK] control: verify::structure refuses planted trailing bytes -- {}",
            complaints.join("; ")
        );
    }
    wrong
}

/// A document whose `/Size` claims forty objects the file does not contain.
///
/// Spike 0.4's defect verbatim: sweep the graph and leave `max_id` above the
/// highest surviving object number. `docs/PLAN.md` §6 records that PDFium
/// renders the result pixel-identically to a correct file.
fn planted_stale_size(source: &Path) -> Result<Vec<u8>, String> {
    let mut doc = Document::load(source).map_err(|e| e.to_string())?;
    let reachable = sweep::reachable(&doc)?;
    doc.objects.retain(|id, _| reachable.contains(id));
    let honest = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    doc.max_id = honest + 40;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `qpdf --check`, as a verdict.
///
/// Its exit code is the answer --- 0 is clean, 3 is warnings, 2 is errors --- and
/// the *message* is what a reader of this probe needs, so the findings come back
/// with the refusal.
///
/// **Both streams are read, and the first version of this read only stdout.** It
/// reported `qpdf said nothing on stdout` for a real finding, which is a probe
/// that names a defect and cannot say what it is --- qpdf puts its banner
/// (`checking`, `PDF Version`, `File is not encrypted`) on stdout and its
/// warnings on stderr.
fn check(qpdf: &Path, path: &Path) -> Result<(), String> {
    let out = Command::new(qpdf)
        .arg("--check")
        .arg(path)
        .output()
        .map_err(|why| format!("qpdf could not be run: {why}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let findings: Vec<String> = said
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("checking")
                && !line.starts_with("PDF Version")
                && !line.starts_with("File is")
                && !line.starts_with("No syntax")
                && !line.starts_with("errors that qpdf")
        })
        .take(3)
        .map(str::to_string)
        .collect();
    let first = if findings.is_empty() {
        "qpdf said nothing on either stream".to_string()
    } else {
        findings.join(" | ")
    };
    Err(format!("exit {:?}: {first}", out.status.code()))
}

/// Where qpdf is, if it is anywhere.
fn which_qpdf() -> Option<PathBuf> {
    for dir in std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let at = dir.join(if cfg!(windows) { "qpdf.exe" } else { "qpdf" });
        if at.is_file() {
            return Some(at);
        }
    }
    None
}
