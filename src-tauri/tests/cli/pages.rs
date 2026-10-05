//! The shipped CLI, real workers, and PDFs with distinct page content, sizes,
//! inherited boxes and rotations. Inspect the saved object graph independently
//! of the CLI's reports so dropping/reordering a page cannot pass on a count.
use super::{scratch, tool, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

pub(super) fn fixture(path: &Path, names: &[&str]) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 40 100 Td ({name}) Tj ET").into_bytes(),
        ));
        let w = 200 + i as i64 * 100;
        let h = 500 + i as i64 * 100;
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree,
            "MediaBox"=>vec![0.into(),0.into(),(w+20).into(),(h+40).into()],
            "CropBox"=>vec![10.into(),20.into(),(w+10).into(),(h+20).into()],
            "Rotate"=>(i as i64 * 90), "Contents"=>content,
            "Resources"=>dictionary! {"Font"=>dictionary! {"F1"=>font}},
        })));
    }
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>names.len() as i64}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).expect("fixture");
}

fn s(path: &Path) -> String {
    path.display().to_string()
}
pub(super) fn names(path: &Path) -> Vec<String> {
    let doc = Document::load(path).expect("written PDF");
    doc.get_pages()
        .keys()
        .map(|p| doc.extract_text(&[*p]).expect("text").trim().to_string())
        .collect()
}
fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).expect("exactly one JSON document")
}

pub(super) fn operations(report: &mut Report) {
    let dir = scratch("pages");
    let source = dir.join("source.pdf");
    let extra = dir.join("extra.pdf");
    fixture(&source, &["Alpha", "Bravo", "Charlie"]);
    fixture(&extra, &["Delta"]);
    let original = std::fs::read(&source).expect("input");
    let input = s(&source);
    let output = dir.join("extracted.pdf");
    let (code, stdout, stderr) = tool(
        &[
            "extract",
            &input,
            "--pages",
            "3,1,1",
            "-o",
            &s(&output),
            "--json",
        ],
        &[],
    );
    report.check(
        "extract keeps selected content in document order, each once",
        code == 0
            && output.exists()
            && names(&output) == ["Alpha", "Charlie"]
            && json(&stdout)["outputs"][0]["pages"] == 2,
        &stderr,
    );

    let merged = dir.join("merged.pdf");
    let (code, stdout, stderr) = tool(
        &["merge", &s(&extra), &input, "-o", &s(&merged), "--json"],
        &[],
    );
    report.check(
        "merge preserves argument order and every page's content",
        code == 0
            && merged.exists()
            && names(&merged) == ["Delta", "Alpha", "Bravo", "Charlie"]
            && json(&stdout)["complete"] == true,
        &stderr,
    );

    let rotated = dir.join("rotated.pdf");
    let (code, _, stderr) = tool(
        &[
            "rotate",
            &input,
            "--degrees",
            "90",
            "--pages",
            "2",
            "-o",
            &s(&rotated),
            "--json",
        ],
        &[],
    );
    let turns = if rotated.exists() {
        let doc = Document::load(&rotated).unwrap();
        doc.get_pages()
            .values()
            .map(|id| {
                doc.get_dictionary(*id)
                    .unwrap()
                    .get(b"Rotate")
                    .unwrap()
                    .as_i64()
                    .unwrap()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    report.check(
        "rotate adds to existing rotation only on the selected page",
        code == 0 && turns == [0, 180, 180] && names(&rotated) == ["Alpha", "Bravo", "Charlie"],
        &stderr,
    );

    let cropped = dir.join("cropped.pdf");
    let (code, _, stderr) = tool(
        &[
            "crop",
            &input,
            "--pages",
            "2",
            "--rect",
            "10,20,100,150",
            "-o",
            &s(&cropped),
            "--json",
        ],
        &[],
    );
    let crop = if cropped.exists() {
        let doc = Document::load(&cropped).unwrap();
        doc.get_dictionary(doc.get_pages()[&2])
            .unwrap()
            .get(b"CropBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    report.check(
        "crop converts displayed coordinates on a rotated offset page",
        code == 0
            && crop == [30., 30., 180., 130.]
            && names(&cropped) == ["Alpha", "Bravo", "Charlie"],
        &format!("{stderr}; box={crop:?}"),
    );

    let prefix = dir.join("part.pdf");
    let (code, stdout, stderr) = tool(
        &["split", &input, "--every", "2", "-o", &s(&prefix), "--json"],
        &[],
    );
    let first = dir.join("part-1.pdf");
    let last = dir.join("part-2.pdf");
    report.check(
        "split writes a complete group and the short final group",
        code == 0
            && first.exists()
            && last.exists()
            && names(&first) == ["Alpha", "Bravo"]
            && names(&last) == ["Charlie"]
            && !prefix.exists()
            && json(&stdout)["outputs"]
                .as_array()
                .is_some_and(|a| a.len() == 2),
        &stderr,
    );
    let last_before = std::fs::read(&last).expect("last part");
    std::fs::remove_file(&first).unwrap();
    let (code, stdout, _) = tool(
        &["split", &input, "--every", "2", "-o", &s(&prefix), "--json"],
        &[],
    );
    report.check(
        "a collision in a later split target writes no earlier part",
        code == 3
            && !first.exists()
            && std::fs::read(&last).unwrap() == last_before
            && json(&stdout)["error"]["kind"] == "refused",
        &stdout,
    );

    let keep = std::fs::read(&output).unwrap();
    let (code, stdout, _) = tool(
        &[
            "extract",
            &input,
            "--pages",
            "2",
            "-o",
            &s(&output),
            "--json",
        ],
        &[],
    );
    report.check(
        "existing output is preserved without force and has a JSON refusal",
        code == 3
            && std::fs::read(&output).unwrap() == keep
            && json(&stdout)["error"]["exit_code"] == 3,
        &stdout,
    );
    let (code, _, stderr) = tool(
        &[
            "extract",
            &input,
            "--pages",
            "2",
            "-o",
            &s(&output),
            "--force",
            "--json",
        ],
        &[],
    );
    report.check(
        "force replaces an existing output with checked content",
        code == 0 && names(&output) == ["Bravo"],
        &stderr,
    );

    let alias = dir.join("alias.pdf");
    std::fs::hard_link(&source, &alias).unwrap();
    let (code, stdout, _) = tool(
        &[
            "rotate",
            &input,
            "--degrees",
            "180",
            "-o",
            &s(&alias),
            "--force",
            "--json",
        ],
        &[],
    );
    report.check(
        "force cannot overwrite an input through a hard-link alias",
        code == 2
            && json(&stdout)["error"]["kind"] == "usage"
            && std::fs::read(&source).unwrap() == original,
        &stdout,
    );

    for (args, expected) in [
        (vec!["extract", &input, "--pages", "4"], 3),
        (vec!["crop", &input, "--rect", "10,20,900,900"], 3),
        (vec!["rotate", &input, "--degrees", "45"], 2),
        (vec!["extract", &input, "--pages", "1-4294967295"], 2),
    ] {
        let no = dir.join("refused.pdf");
        let no_s = s(&no);
        let mut args = args;
        args.extend(["-o", &no_s, "--json"]);
        let (code, stdout, stderr) = tool(&args, &[]);
        report.check(
            &format!("page refusal {} returns JSON and no file", args.join(" ")),
            code == expected && !no.exists() && json(&stdout)["error"]["exit_code"] == expected,
            &stderr,
        );
    }
    if let Some(signed) = super::fixture("incr-signed.pdf") {
        let dest = dir.join("unsigned.pdf");
        let (code, stdout, _) = tool(
            &["merge", &input, &s(&signed), "-o", &s(&dest), "--json"],
            &[],
        );
        report.check(
            "merge checks signatures on incoming documents too",
            code == 3
                && !dest.exists()
                && json(&stdout)["error"]["message"]
                    .as_str()
                    .is_some_and(|s| s.contains("--invalidate-signatures")),
            &stdout,
        );
        let (code, stdout, stderr) = tool(
            &[
                "extract",
                &s(&signed),
                "--pages",
                "1",
                "-o",
                &s(&dest),
                "--invalidate-signatures",
                "--json",
            ],
            &[],
        );
        report.check(
            "explicit signature invalidation is reported",
            code == 0
                && dest.exists()
                && json(&stdout)["signatures_invalidated"]
                    .as_u64()
                    .is_some_and(|n| n > 0),
            &stderr,
        );
    } else {
        report.check("signed fixture exists", false, "generate incr fixtures");
    }
    let encrypted = dir.join("encrypted.pdf");
    let encryption = std::process::Command::new("qpdf")
        .args([
            "--encrypt",
            "swordfish",
            "owner-secret",
            "256",
            "--",
            &input,
            &s(&encrypted),
        ])
        .output();
    if encryption.is_ok_and(|o| o.status.success()) {
        let enc_out = dir.join("encrypted-extract.pdf");
        let (code, stdout, stderr) = tool(
            &[
                "extract",
                &s(&encrypted),
                "--pages",
                "2",
                "-o",
                &s(&enc_out),
                "--password-env",
                "TPDF_IT_PASSWORD",
                "--json",
            ],
            &[("TPDF_IT_PASSWORD", "swordfish")],
        );
        let still_encrypted = std::process::Command::new("qpdf")
            .args(["--is-encrypted", &s(&enc_out)])
            .status()
            .is_ok_and(|s| s.success());
        let (locked, _, _) = tool(&["text", &s(&enc_out), "--json"], &[]);
        let (read, text, _) = tool(
            &["text", &s(&enc_out), "--password-env", "TPDF_IT_PASSWORD"],
            &[("TPDF_IT_PASSWORD", "swordfish")],
        );
        report.check(
            "encrypted extraction preserves encryption, password and selected content",
            code == 0
                && json(&stdout)["complete"] == true
                && still_encrypted
                && locked == 3
                && read == 0
                && text.trim_end_matches(['\n', '\u{c}']) == "Bravo",
            &stderr,
        );
        let no = dir.join("encrypted-refused.pdf");
        let (code, stdout, _) = tool(
            &["merge", &input, &s(&encrypted), "-o", &s(&no), "--json"],
            &[],
        );
        report.check(
            "an encrypted additional merge input is refused with no output",
            code == 3 && !no.exists() && json(&stdout)["error"]["exit_code"] == 3,
            &stdout,
        );
        let kept = std::fs::read(&output).unwrap();
        let (code, stdout, _) = tool(
            &[
                "extract",
                &s(&encrypted),
                "--pages",
                "1",
                "-o",
                &s(&output),
                "--force",
                "--password-env",
                "TPDF_IT_PASSWORD",
                "--json",
            ],
            &[("TPDF_IT_PASSWORD", "wrong")],
        );
        report.check(
            "a wrong password with force preserves the previous output",
            code == 3
                && std::fs::read(&output).unwrap() == kept
                && json(&stdout)["error"]["exit_code"] == 3,
            &stdout,
        );
    } else {
        report.skip("encrypted page operations", "qpdf is not available");
    }
    // --force replaces a regular file and nothing else: not a directory, and
    // not whatever a link names, which the command line never did.
    let directory = dir.join("a-directory");
    std::fs::create_dir(&directory).expect("directory");
    #[allow(unused_mut)]
    let mut targets = vec![("a directory", directory)];
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"not this run's").expect("victim");
    #[cfg(unix)]
    {
        let link = dir.join("link.pdf");
        std::os::unix::fs::symlink(&victim, &link).expect("link");
        targets.push(("a link", link));
    }
    for (what, target) in &targets {
        let (code, stdout, stderr) = tool(
            &[
                "extract",
                &input,
                "--pages",
                "1",
                "-o",
                &s(target),
                "--force",
                "--json",
            ],
            &[],
        );
        report.check(
            &format!("--force does not write onto {what} (3), and what it names is as it was"),
            code == 3
                && json(&stdout)["error"]["exit_code"] == 3
                && stderr.contains("--force for a regular file")
                && std::fs::read(&victim).ok().as_deref() == Some(b"not this run's".as_slice())
                && target.symlink_metadata().is_ok_and(|data| !data.is_file()),
            &format!("exit {code}: {stderr}"),
        );
    }
    report.check(
        "page operations leave the source unchanged and no staging files",
        std::fs::read(&source).unwrap() == original
            && std::fs::read_dir(&dir).unwrap().all(|e| {
                !e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tpdf-cli-")
            }),
        "input or scratch leaked",
    );
    let _ = std::fs::remove_dir_all(dir);
}
