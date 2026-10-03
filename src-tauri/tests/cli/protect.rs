//! `tpdf protect` and `tpdf unprotect` through the shipped tool.
//!
//! PDFium, in the tool's own workers, decides whether a copy opens; `qpdf`,
//! where it is installed, reads the encryption back as a second opinion that
//! shares no code with the writer.

use std::path::Path;
use std::process::Command;

use super::{fixture, scratch, tool, Report};

const NEW: &str = "tr0ub4dor";
const OLD: &str = "swordfish";

/// The text of `path` as the tool reads it, with the password in `key`.
fn text(path: &str, key: Option<&str>) -> (i32, String) {
    let (code, out, _) = match key {
        Some(key) => tool(&["text", path, "--password-env", "KEY"], &[("KEY", key)]),
        None => tool(&["text", path], &[]),
    };
    (code, out)
}

/// `qpdf` on `path`: its exit code and both streams, or `None` without it.
fn qpdf(args: &[&str]) -> Option<(i32, String)> {
    let out = Command::new("qpdf").args(args).output().ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    ))
}

pub(super) fn sets_and_removes_a_password(report: &mut Report) {
    let (Some(plain), Some(locked), Some(open)) = (
        fixture("rotated.pdf"),
        fixture("incr-encrypted-pw.pdf"),
        fixture("incr-encrypted-open.pdf"),
    ) else {
        report.skip("protect", "testdata/ is not generated");
        return;
    };
    let dir = scratch("protect");
    let at = |name: &str| dir.join(name).display().to_string();
    let (plain, locked, open) = (
        plain.display().to_string(),
        locked.display().to_string(),
        open.display().to_string(),
    );
    let (protected, back, replaced, refused) = (
        at("protected.pdf"),
        at("back.pdf"),
        at("replaced.pdf"),
        at("refused.pdf"),
    );
    let (_, words) = text(&plain, None);

    let (code, json, stderr) = tool(
        &[
            "protect",
            &plain,
            "-o",
            &protected,
            "--new-password-env",
            "NEW",
            "--json",
        ],
        &[("NEW", NEW)],
    );
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "protect reports a copy that needs its password",
        code == 0
            && result["command"] == "protect"
            && result["protected"] == true
            && result["was_protected"] == false
            && result["pages"] == 4,
        &format!("exit {code}; {stderr}; {json}"),
    );
    let (without, _) = text(&protected, None);
    let (wrong, _) = text(&protected, Some(OLD));
    let (with, read) = text(&protected, Some(NEW));
    report.check(
        "the copy opens with the new password and with nothing else",
        without == 3 && wrong == 3 && with == 0 && !words.is_empty() && read == words,
        &format!("no password {without}; wrong {wrong}; right {with}"),
    );

    match qpdf(&["--check", &format!("--password={NEW}"), &protected]) {
        Some((code, said)) => report.check(
            "qpdf reads the copy as AES-256 with nothing to warn about",
            code == 0 && said.contains("R = 6") && said.contains("file encryption method: AESv3"),
            &format!("exit {code}; {said}"),
        ),
        None => report.skip("qpdf reads the protected copy", "qpdf is unavailable"),
    }

    let (code, json, stderr) = tool(
        &[
            "unprotect",
            &protected,
            "-o",
            &back,
            "--password-env",
            "KEY",
            "--json",
        ],
        &[("KEY", NEW)],
    );
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let (opens, read) = text(&back, None);
    report.check(
        "unprotect writes a copy that opens without a password, with the same text",
        code == 0
            && result["protected"] == false
            && result["was_protected"] == true
            && opens == 0
            && read == words
            && !std::fs::read(&back)
                .unwrap()
                .windows(8)
                .any(|w| w == b"/Encrypt"),
        &format!("exit {code}; {stderr}; {json}"),
    );
    match qpdf(&["--is-encrypted", &back]) {
        // 2 is qpdf's "not encrypted".
        Some((code, said)) => report.check(
            "qpdf says the unprotected copy is not encrypted",
            code == 2,
            &format!("exit {code}; {said}"),
        ),
        None => report.skip("qpdf reads the unprotected copy", "qpdf is unavailable"),
    }

    let (_, was) = text(&locked, Some(OLD));
    let (code, said, stderr) = tool(
        &[
            "protect",
            &locked,
            "-o",
            &replaced,
            "--new-password-env",
            "NEW",
            "--password-env",
            "KEY",
        ],
        &[("NEW", NEW), ("KEY", OLD)],
    );
    let (old, _) = text(&replaced, Some(OLD));
    let (new, read) = text(&replaced, Some(NEW));
    report.check(
        "a document that had a password gets the new one instead",
        code == 0
            && said.contains("the one it had no longer opens it")
            && old == 3
            && new == 0
            && !was.is_empty()
            && read == was,
        &format!("exit {code}; {stderr}; old {old}; new {new}"),
    );

    let long = "x".repeat(128);
    type Case<'a> = (Vec<&'a str>, Vec<(&'a str, &'a str)>, i32, &'a str);
    let cases: [Case<'_>; 7] = [
        (
            vec!["unprotect", &plain, "-o", &refused, "--password-env", "KEY"],
            vec![("KEY", OLD)],
            3,
            "has no password",
        ),
        (
            vec!["unprotect", &open, "-o", &refused, "--password-env", "KEY"],
            vec![("KEY", OLD)],
            3,
            "opens without a password",
        ),
        (
            vec![
                "unprotect",
                &locked,
                "-o",
                &refused,
                "--password-env",
                "KEY",
            ],
            vec![("KEY", NEW)],
            3,
            "did not open it",
        ),
        (
            vec![
                "protect",
                &plain,
                "-o",
                &refused,
                "--new-password-env",
                "NEW",
            ],
            vec![("NEW", "")],
            2,
            "empty",
        ),
        (
            vec![
                "protect",
                &plain,
                "-o",
                &refused,
                "--new-password-env",
                "NEW",
            ],
            vec![("NEW", &long)],
            2,
            "too long",
        ),
        (
            vec![
                "protect",
                &plain,
                "-o",
                &refused,
                "--new-password-env",
                "TPDF_UNSET_NAME",
            ],
            vec![],
            2,
            "no such environment variable",
        ),
        (
            vec![
                "protect",
                &locked,
                "-o",
                &refused,
                "--new-password-env",
                "NEW",
            ],
            vec![("NEW", NEW)],
            3,
            "--password-env",
        ),
    ];
    for (args, env, status, sentence) in cases {
        let (code, _, stderr) = tool(&args, &env);
        report.check(
            "a refused protect or unprotect says why and writes nothing",
            code == status && stderr.contains(sentence) && !Path::new(&refused).exists(),
            &format!("{args:?}: exit {code}; {stderr}"),
        );
    }
    report.check(
        "protect leaves no staging directories",
        std::fs::read_dir(&dir).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tpdf-cli-")
        }),
        "a staging directory was left beside the output",
    );
}
