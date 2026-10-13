//! `sign --lock`: the signature says nothing may change after it, `verify`
//! reports that as `lock`, and a further signature is refused.
use super::{
    certificate, fixture, now, plain_pdf, scratch, signs, strings, tool, Report, TestStore, SUBJECT,
};
use std::path::Path;

/// The sentence after signing, for a signature that locks the document.
const LOCKS: &str = "The signature locks the document: no changes are permitted after it.";

/// `lock` of every signature `verify --json` reports for `path`.
fn locks(path: &Path) -> Vec<serde_json::Value> {
    super::verify_appendix::signatures(path)
        .iter()
        .map(|s| s["lock"].clone())
        .collect()
}

pub fn locks_the_document(report: &mut Report) {
    let now = now();
    let dir = scratch("sign-lock");
    let s = |p: &Path| p.display().to_string();
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let line = |input: &Path, out: &Path, extra: &[&str]| {
        let mut args = strings(&["sign", &s(input), "-o", &s(out), "--identity", SUBJECT]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };

    // Without --lock: the control every check below is read against.
    let open = dir.join("open.pdf");
    let (code, stdout, stderr) = signs(&line(&plain, &open, &["--json"]), &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let (_, text, _) = tool(&["verify", &s(&open)], &[]);
    report.check(
        "sign without --lock reports lock 0, and says nothing of a lock",
        code == 0
            && json["signatures"][0]["lock"] == 0
            && locks(&open) == [serde_json::json!(0)]
            && json["summary"]
                .as_str()
                .is_some_and(|said| !said.contains("locks"))
            && !text.contains("Lock:"),
        &format!("exit {code}; {stderr}; {:?}\n{text}", locks(&open)),
    );

    let locked = dir.join("locked.pdf");
    let (code, stdout, stderr) = signs(&line(&plain, &locked, &["--lock", "--json"]), &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "sign --lock reports the new signature with lock 1, and verify reads it so",
        code == 0
            && json["signatures"][0]["lock"] == 1
            && json["signatures"][0]["integrity"]["verdict"] == "intact"
            && locks(&locked) == [serde_json::json!(1)],
        &format!("exit {code}; {stderr}; {:?}", locks(&locked)),
    );
    report.check(
        "and its summary says the document is locked",
        json["summary"]
            .as_str()
            .is_some_and(|said| said.contains(LOCKS)),
        &json["summary"].to_string(),
    );
    let (code, text, _) = tool(&["verify", &s(&locked)], &[]);
    report.check(
        "verify says under the signature that it locks the document",
        code == 0
            && text.lines().any(|row| {
                row == "    Lock: the signature locks the document: no changes are permitted \
                        after it"
            }),
        &format!("exit {code}\n{text}"),
    );

    // Without --json the sentence is the whole of stdout.
    let said = dir.join("said.pdf");
    let (code, stdout, stderr) = signs(&line(&plain, &said, &["--lock"]), &store, now);
    report.check(
        "sign --lock without --json says the document is locked",
        code == 0 && stdout.contains(LOCKS) && locks(&said) == [serde_json::json!(1)],
        &format!("exit {code}; {stderr}; {stdout}"),
    );

    // A further signature on the locked document: refused, nothing written.
    for (what, extra) in [("", vec![]), (" with --lock", vec!["--lock"])] {
        let again = dir.join("again.pdf");
        let (code, _, stderr) = signs(&line(&locked, &again, &extra), &store, now);
        report.check(
            &format!("signing a locked document again{what} is refused"),
            code == 3 && stderr.contains("locked this document") && !again.exists(),
            &format!("exit {code}; {stderr}"),
        );
    }
    // Control: the same line on the document signed without a lock signs it.
    let twice = dir.join("twice.pdf");
    let (code, _, stderr) = signs(&line(&open, &twice, &[]), &store, now);
    report.check(
        "control: a document signed without a lock is signed again",
        code == 0 && locks(&twice) == [serde_json::json!(0), serde_json::json!(0)],
        &format!("exit {code}; {stderr}; {:?}", locks(&twice)),
    );

    // A certified document already says what may change: no lock beside it.
    let what = "sign --lock refuses a certified document, and signs it without --lock";
    match fixture("incr-certified-2.pdf") {
        None => report.skip(what, "incr-certified-2.pdf"),
        Some(certified) => {
            let out = dir.join("certified.pdf");
            let (code, _, stderr) = signs(&line(&certified, &out, &["--lock"]), &store, now);
            let refused = code == 3 && stderr.contains("is certified") && !out.exists();
            let (plain_code, _, plain_stderr) = signs(&line(&certified, &out, &[]), &store, now);
            report.check(
                what,
                refused && plain_code == 0 && out.exists(),
                &format!("exit {code}; {stderr}; without: exit {plain_code}; {plain_stderr}"),
            );
        }
    }
}
