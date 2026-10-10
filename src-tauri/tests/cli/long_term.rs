//! `long-term`: validation data and an archive timestamp for a document that
//! is already signed, through the real worker --- the survey of the
//! signatures, the `/DSS` revision and the document timestamp are each built
//! by a sandboxed child of this process, and the written file is read back
//! by one.
//!
//! The document is signed first by `sign --timestamp`, with the fake PKI's
//! signer; nothing of that signing is kept but the file. Then `long-term`
//! is given the file as anybody's: the signer's root and the authority's are
//! the only roots it may trust, so no keychain or certificate store is read.
use super::test_tsa::{self, Pki, Plan, Serve};
use super::{
    authority_by, fixture, from_json, in_process, now, os_key, plain_pdf, read_back, scratch,
    signs_under, tool, Answer, PkiStore, Report, PKI_SUBJECT,
};
use std::path::Path;

fn s(path: &Path) -> String {
    path.display().to_string()
}

/// `plain.pdf` signed and timestamped by `pki`, written to `out`: the
/// document `long-term` is then given.
fn signed_by(pki: &Pki, plain: &Path, out: &Path, url: &str) -> (i32, String) {
    let store = PkiStore {
        certificate: pki.signer.certificate.clone(),
        chain: pki.chain.clone(),
        seed: pki.signer.seed,
        calls: std::rc::Rc::new(os_key::Calls::default()),
    };
    let args: Vec<String> = [
        "sign",
        &s(plain),
        "-o",
        &s(out),
        "--identity",
        PKI_SUBJECT,
        "--timestamp",
        url,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let root = pki.tsa.root.clone();
    let (code, _, stderr) = signs_under(
        &args,
        &store,
        now(),
        tpdf_lib::trust::Anchors::Only(std::slice::from_ref(&root)),
    );
    (code, stderr)
}

/// `long-term <input> -o <out> --timestamp <url>` in this process, trusting
/// `roots` and no others.
fn long_term(
    input: &Path,
    out: &Path,
    url: &str,
    extra: &[&str],
    roots: &[Vec<u8>],
) -> (i32, String, String) {
    // No identity is asked for: a store that would fail if it were.
    struct NoStore;
    impl tpdf_lib::cli::Store for NoStore {
        fn identities(&self) -> Result<Vec<tpdf_lib::cli::Held>, String> {
            Err("long-term asked the key store for an identity".into())
        }
        fn saved_image(&self) -> Result<Option<tpdf_lib::signature::Image>, String> {
            Err("long-term asked for the saved signature image".into())
        }
    }
    let mut args: Vec<String> = ["long-term", &s(input), "-o", &s(out), "--timestamp", url]
        .iter()
        .map(ToString::to_string)
        .collect();
    args.extend(extra.iter().map(ToString::to_string));
    signs_under(
        &args,
        &NoStore,
        now(),
        tpdf_lib::trust::Anchors::Only(roots),
    )
}

pub(super) fn adds_to_a_signed_document(report: &mut Report) {
    let dir = scratch("long-term-existing");
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let good = Plan {
        signer_ocsp: Some(Serve::Good),
        authority_ocsp: Some(Serve::Good),
        ..Plan::default()
    };

    // A document somebody signed and timestamped, with no validation data.
    let pki = Pki::start(good);
    let url = authority_by(Answer::Token(test_tsa::Faults::default()), pki.tsa.clone());
    let signed = dir.join("signed.pdf");
    let (code, stderr) = signed_by(&pki, &plain, &signed, &url);
    report.check(
        "the fixture: a signed and timestamped document, nothing asked of the PKI",
        code == 0 && signed.exists() && pki.paths().is_empty(),
        &format!("exit {code}, asked {:?}: {stderr}", pki.paths()),
    );
    let before = std::fs::read(&signed).unwrap_or_default();
    let both = [pki.root.certificate.clone(), pki.tsa.root.clone()];

    // Sound: both revisions through real workers, read back by the tool.
    let kept = dir.join("kept.pdf");
    let (code, stdout, stderr) = long_term(&signed, &kept, &url, &["--json"], &both);
    report.check(
        "long-term on a signed document: exits 0",
        code == 0,
        &format!("exit {code}: {stderr}"),
    );
    let said: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "long-term --json: one document naming what was covered and the archive",
        said["schema"] == 1
            && said["command"] == "long-term"
            && said["covered"] == serde_json::json!(["Signature1"])
            && said["archive"] == "Signature2"
            && said["signatures"].as_array().map(Vec::len) == Some(2)
            && said["summary"]
                .as_str()
                .is_some_and(|text| text.contains("saved the result to kept.pdf")),
        &stdout,
    );
    let written = std::fs::read(&kept).unwrap_or_default();
    report.check(
        "long-term appends: the copy begins with the original, which is unchanged",
        written.len() > before.len()
            && written[..before.len()] == before[..]
            && std::fs::read(&signed).unwrap_or_default() == before,
        &format!("{} bytes after {}", written.len(), before.len()),
    );
    match read_back(&kept) {
        Err(why) => report.check("long-term: the built tool reads the copy", false, &why),
        Ok((_, json)) => {
            let signature = &json["files"][0]["signatures"][0];
            report.check(
                "long-term: the earlier signature reads intact, its certificates good",
                signature["document_timestamp"] == false
                    && signature["integrity"]["verdict"] == "intact"
                    && signature["revocation"]["standing"] == "good"
                    && signature["timestamp"]["integrity"]["verdict"] == "intact"
                    && signature["timestamp"]["revocation"]["standing"] == "good",
                &signature.to_string(),
            );
            let archive = &json["files"][0]["signatures"][1];
            report.check(
                "long-term: an archive timestamp over the whole file follows, intact",
                json["files"][0]["signatures"].as_array().map(Vec::len) == Some(2)
                    && archive["document_timestamp"] == true
                    && archive["covers_whole_file"] == true
                    && archive["timestamp"]["integrity"]["verdict"] == "intact",
                &archive.to_string(),
            );
            report.check(
                "long-term: the tool reads what the in-process reader reads",
                from_json(&json["files"][0]) == in_process(&kept),
                &format!(
                    "{:?} / {:?}",
                    from_json(&json["files"][0]),
                    in_process(&kept)
                ),
            );
        }
    }
    report.check(
        "long-term: the PKI was asked about the signer and the authority, once each",
        pki.paths() == ["/ocsp/signer", "/ocsp/authority"],
        &format!("{:?}", pki.paths()),
    );

    // Again, on its own result: a further archive, and the first one's
    // authority covered.
    let again = dir.join("kept-again.pdf");
    let (code, stdout, stderr) = long_term(&kept, &again, &url, &["--json"], &both);
    let said: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "long-term on its own result: a further archive over the earlier one",
        code == 0
            && said["covered"] == serde_json::json!(["Signature1", "Signature2"])
            && said["archive"] == "Signature3"
            && said["summary"].as_str().is_some_and(|text| {
                text.contains("the signature Signature1 and the timestamp Signature2")
            }),
        &format!("exit {code}: {stdout}{stderr}"),
    );

    // An existing output is kept unless --force says otherwise.
    let (code, _, stderr) = long_term(&signed, &kept, &url, &[], &both);
    report.check(
        "long-term: an output that exists is exit 3 and is left as it was",
        code == 3
            && stderr.contains("already exists")
            && std::fs::read(&kept).unwrap_or_default() == written,
        &format!("exit {code}: {stderr}"),
    );

    // And --force replaces it, with the copy this run made.
    std::fs::write(&again, b"something else").expect("written");
    let (code, _, stderr) = long_term(&kept, &again, &url, &["--force"], &both);
    let replaced = std::fs::read(&again).unwrap_or_default();
    report.check(
        "long-term --force: an output that exists is replaced by the new copy",
        code == 0 && replaced.len() > written.len() && replaced[..written.len()] == written[..],
        &format!("exit {code}, {} bytes: {stderr}", replaced.len()),
    );

    // The refusals: each exit 3, nothing written, nothing asked of the PKI.
    let pki = Pki::start(good);
    let url = authority_by(Answer::Token(test_tsa::Faults::default()), pki.tsa.clone());
    let signed = dir.join("signed-2.pdf");
    let (code, stderr) = signed_by(&pki, &plain, &signed, &url);
    report.check("the second fixture is signed", code == 0, &stderr);
    let both = [pki.root.certificate.clone(), pki.tsa.root.clone()];
    let refused = |report: &mut Report, what: &str, input: &Path, roots: &[Vec<u8>], says: &str| {
        let out = dir.join(format!("refused-{}.pdf", what.replace(' ', "-")));
        let (code, stdout, stderr) = long_term(input, &out, &url, &[], roots);
        report.check(
            &format!("long-term refuses {what}: exit 3, nothing written or fetched, and says so"),
            code == 3
                && !out.exists()
                && stdout.is_empty()
                && pki.paths().is_empty()
                && stderr.contains(says)
                && stderr.contains("nothing was written"),
            &format!(
                "exit {code}, exists {}, asked {:?}: {stderr}",
                out.exists(),
                pki.paths()
            ),
        );
    };
    refused(
        report,
        "a document with no signature",
        &plain,
        &both,
        "has no signature",
    );
    refused(
        report,
        "a signer this computer does not trust",
        &signed,
        std::slice::from_ref(&pki.tsa.root),
        "is not trusted by this computer",
    );
    let mut altered = std::fs::read(&signed).unwrap_or_default();
    if let Some(at) = altered.windows(8).position(|w| w == b"MediaBox") {
        altered[at] = b'm';
    }
    let broken = dir.join("altered.pdf");
    std::fs::write(&broken, altered).expect("written");
    refused(
        report,
        "a signature that does not verify",
        &broken,
        &both,
        "does not verify",
    );
    for (name, what, says) in [
        (
            "incr-encrypted-open.pdf",
            "an encrypted document",
            "is encrypted",
        ),
        // One that needs a password: a worker started without it answers
        // that it is locked, which is this refusal and not a worker failing.
        (
            "incr-encrypted-pw.pdf",
            "a document that needs a password",
            "is encrypted",
        ),
        (
            "incr-certified-1.pdf",
            "a certification with no changes permitted",
            "no changes permitted",
        ),
    ] {
        match fixture(name) {
            Some(path) => refused(report, what, &path, &both, says),
            None => report.skip(&format!("long-term refuses {what}"), name),
        }
    }

    // Revocation data that cannot be had: exit 3, nothing written.
    let pki = Pki::start(Plan {
        signer_ocsp: Some(Serve::Revoked),
        authority_ocsp: Some(Serve::Good),
        ..Plan::default()
    });
    let url = authority_by(Answer::Token(test_tsa::Faults::default()), pki.tsa.clone());
    let signed = dir.join("signed-revoked.pdf");
    let (code, stderr) = signed_by(&pki, &plain, &signed, &url);
    report.check("the third fixture is signed", code == 0, &stderr);
    let both = [pki.root.certificate.clone(), pki.tsa.root.clone()];
    let out = dir.join("revoked.pdf");
    let (code, _, stderr) = long_term(&signed, &out, &url, &[], &both);
    report.check(
        "long-term for a certificate its authority says is revoked: exit 3, nothing written",
        code == 3 && !out.exists() && stderr.contains("has been revoked"),
        &format!("exit {code}: {stderr}"),
    );

    // The built tool: a malformed line is 2, and the command is listed.
    let (code, _, stderr) = tool(&["long-term", &s(&signed), "-o", &s(&out)], &[]);
    report.check(
        "control: long-term without --timestamp is exit 2 and nothing is written",
        code == 2 && !out.exists() && stderr.contains("--timestamp"),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
