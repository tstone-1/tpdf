//! The command-line tool's pure parts: the command line, the identity, the
//! exit codes, and the committed JSON and wording samples.
//!
//! What needs a worker --- signing and verifying real documents --- is in
//! `tests/cli.rs`, whose test binary can be its own worker, which this one
//! cannot: `cargo test`'s harness owns `main` here, so a spawned worker would
//! re-exec a test runner that knows nothing of `worker::WORKER_ARGV`.

use std::path::{Path, PathBuf};

use super::args::{parse, Line};
use super::identities::{identities_report, listing, resolve, Listed};
use super::report::{self, ErrorKind};
use super::sign::{Lines, Placement};
use super::verify::{verified, verify_exit};
use super::*;
use crate::integrity::{Integrity, Verdict, Why};
use crate::sign_cms;
use crate::sign_cms::testkeys::{certificate, Soft, Spec, NOW};
use crate::trust::{Doubt, Standing, Store as TrustStore, Trust};

fn argv(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn refused(line: &str) -> String {
    match parse(&argv(line)) {
        Err(why) => why,
        Ok(command) => panic!("`{line}` parsed as {command:?}"),
    }
}

/// A `sign` line, parsed by the command's own parser --- after the top-level
/// parse has also accepted it, so the registration is exercised too.
fn signed(line: &str) -> sign::Sign {
    let args = argv(line);
    assert_eq!(args.first().map(String::as_str), Some("sign"), "{line}");
    assert!(
        matches!(parse(&args), Ok(Line::Run(_))),
        "`{line}` was not accepted"
    );
    match sign::parse(&args[1..]) {
        Ok(sign) => sign,
        Err(why) => panic!("`{line}` did not parse as a signing: {why}"),
    }
}

// --- the command line -------------------------------------------------------

#[test]
fn a_whole_visible_signing_line_parses_into_what_the_worker_is_sent() {
    let sign = signed(
        "sign in.pdf -o out.pdf --identity 2a14 --visible --page 3 --rect 10,20,150,60 \
         --lines name,date --reason Approved --location Hamburg --no-image --json --force",
    );
    assert_eq!(sign.input, PathBuf::from("in.pdf"));
    assert_eq!(sign.output, PathBuf::from("out.pdf"));
    assert_eq!(sign.identity, "2a14");
    assert_eq!(
        sign.visible,
        Some(Placement {
            page: 3,
            rect: [10.0, 20.0, 160.0, 80.0],
        })
    );
    assert!(!sign.image);
    assert_eq!(
        sign.lines,
        Lines {
            label: false,
            name: true,
            date: true
        }
    );
    assert_eq!(sign.reason, "Approved");
    assert_eq!(sign.location, "Hamburg");
    assert!(sign.json && sign.force);

    // And the defaults: invisible, the image when visible, all three lines,
    // page 1, no replacing.
    let plain = signed("sign in.pdf --output out.pdf --identity A");
    assert_eq!(plain.visible, None);
    assert!(plain.image && !plain.force && !plain.json);
    assert_eq!(plain.lines, Lines::default());
    let visible = signed("sign in.pdf -o out.pdf --identity A --visible --rect 0,0,100,40");
    assert_eq!(visible.visible.map(|p| p.page), Some(1));
}

#[test]
fn signing_without_an_output_is_refused() {
    assert!(refused("sign in.pdf --identity A").contains("-o <out.pdf>"));
}

#[test]
fn an_output_that_names_the_input_is_refused() {
    for line in [
        "sign a.pdf -o a.pdf --identity A",
        "sign a.pdf -o ./a.pdf --identity A",
        "sign dir/a.pdf -o dir/../dir/a.pdf --identity A",
    ] {
        assert!(refused(line).contains("output names the input"), "{line}");
    }
    // The control: a different name in the same directory is a signing.
    signed("sign a.pdf -o a-signed.pdf --identity A");
}

#[test]
fn a_rectangle_that_is_not_four_numbers_with_a_size_is_refused() {
    for rect in [
        "10,20,30",
        "10,20,30,40,50",
        "a,b,c,d",
        "-1,0,50,50",
        "0,-1,50,50",
        "0,0,0,40",
        "0,0,40,-2",
        "0,0,NaN,40",
        "0,0,inf,40",
        "",
    ] {
        let line = format!("sign a.pdf -o b.pdf --identity A --visible --rect {rect}");
        let args: Vec<String> = line.split(' ').map(str::to_string).collect();
        let why = parse(&args).expect_err(&line);
        assert!(why.contains("`--rect` is four numbers"), "{line}: {why}");
    }
}

#[test]
fn a_page_that_is_not_counted_from_one_is_refused() {
    for page in ["0", "-1", "two", "1.5"] {
        let why = refused(&format!(
            "sign a.pdf -o b.pdf --identity A --visible --rect 0,0,50,50 --page {page}"
        ));
        assert!(why.contains("counted from 1"), "{page}: {why}");
    }
}

#[test]
fn an_appearance_option_without_visible_is_refused_rather_than_dropped() {
    for flag in [
        "--page 2",
        "--rect 0,0,50,50",
        "--no-image",
        "--lines name",
        "--reason Why",
        "--location Where",
    ] {
        let why = refused(&format!("sign a.pdf -o b.pdf --identity A {flag}"));
        assert!(why.contains("add `--visible`"), "{flag}: {why}");
    }
    assert!(refused("sign a.pdf -o b.pdf --identity A --visible").contains("needs `--rect"));
}

#[test]
fn every_other_malformed_line_is_refused_with_its_reason() {
    let cases = [
        ("sign -o b.pdf --identity A", "needs the document"),
        ("sign a.pdf -o b.pdf", "needs `--identity`"),
        ("sign a.pdf c.pdf -o b.pdf --identity A", "one document"),
        ("sign a.pdf -o", "needs a value"),
        (
            "sign a.pdf -o b.pdf --identity A --colour red",
            "no option `--colour`",
        ),
        (
            "sign a.pdf -o b.pdf --identity A --visible --rect 0,0,50,50 --lines name,logo",
            "`logo`",
        ),
        ("verify", "at least one document"),
        ("verify a.pdf --fast", "no option `--fast`"),
        ("identities extra", "takes no argument"),
        ("encrypt a.pdf", "not a command"),
    ];
    for (line, expected) in cases {
        let why = refused(line);
        assert!(why.contains(expected), "`{line}` gave `{why}`");
    }
    assert!(matches!(parse(&[]), Ok(Line::Help)));
    assert!(matches!(parse(&argv("--version")), Ok(Line::Version)));
    assert_eq!(
        verify::parse(&argv("a.pdf b.pdf --strict --json")).expect("verify"),
        verify::Verify {
            files: vec![PathBuf::from("a.pdf"), PathBuf::from("b.pdf")],
            json: true,
            strict: true,
        }
    );
}

#[test]
fn every_registered_command_is_reached_by_its_name_and_listed_in_help() {
    // The registration is the whole of adding a command, so it is what is
    // held: each name dispatches to its own parser (a line it accepts), each
    // appears in the usage text with its synopsis, and no two share a name.
    let lines = [
        ("sign", "sign a.pdf -o b.pdf --identity A"),
        ("verify", "verify a.pdf"),
        ("identities", "identities --json"),
    ];
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, lines.map(|(name, _)| name).to_vec());
    let text = usage("tpdf");
    for (command, (_, line)) in COMMANDS.iter().zip(lines) {
        assert!(matches!(parse(&argv(line)), Ok(Line::Run(_))), "{line}");
        assert!(
            text.contains(&format!("tpdf {}", command.usage)),
            "{}",
            command.name
        );
        assert!(
            text.contains(command.summary.lines().next().unwrap_or("?")),
            "{}",
            command.name
        );
    }
    let why = refused("encrypt a.pdf");
    assert!(why.contains("sign, verify, identities"), "{why}");
}

// --- the identity -----------------------------------------------------------

/// A certificate for a P-256 key, issued to itself, as `spec` describes.
fn cert(seed: u8, spec: &Spec<'_>) -> Vec<u8> {
    certificate(&Soft::p256(seed), spec)
}

fn listed(certs: &[Vec<u8>]) -> Vec<(Listed, String)> {
    let found: Vec<(String, Vec<u8>)> = certs
        .iter()
        .map(|der| (crate::keystore::id_of(der), der.clone()))
        .collect();
    listing(&found, NOW)
}

fn expired(subject: &str) -> Spec<'_> {
    Spec {
        not_before: NOW - 86_400 * 400,
        not_after: NOW - 86_400 * 30,
        serial: 2,
        ..Spec::new(subject)
    }
}

#[test]
fn an_identity_nobody_holds_is_refused_and_says_where_to_look() {
    let list = listed(&[cert(3, &Spec::new("Alice"))]);
    let why = resolve("Bob", &list).expect_err("no Bob");
    assert!(
        why.contains("\"Bob\"") && why.contains("`identities`"),
        "{why}"
    );
    let why = resolve(&"ab".repeat(32), &list).expect_err("no such hash");
    assert!(why.contains("SHA-256"), "{why}");
}

#[test]
fn a_subject_two_usable_certificates_share_is_refused_and_both_are_listed() {
    let first = cert(3, &Spec::new("Alice"));
    let second = cert(
        4,
        &Spec {
            serial: 9,
            ..Spec::new("Alice")
        },
    );
    let list = listed(&[first.clone(), second.clone()]);
    let why = resolve("Alice", &list).expect_err("ambiguous");
    for der in [&first, &second] {
        assert!(why.contains(&crate::keystore::id_of(der)), "{why}");
    }
    assert!(why.contains("2 certificates"), "{why}");
    // Either one is still reachable by its hash, in either case of hex.
    assert_eq!(resolve(&crate::keystore::id_of(&second), &list), Ok(1));
    assert_eq!(
        resolve(&crate::keystore::id_of(&first).to_ascii_uppercase(), &list),
        Ok(0)
    );
}

#[test]
fn an_expired_certificate_beside_its_renewal_is_not_a_rival() {
    let old = cert(3, &expired("Alice"));
    let renewed = cert(4, &Spec::new("Alice"));
    let list = listed(&[old, renewed]);
    assert_eq!(resolve("Alice", &list), Ok(1));
}

#[test]
fn a_subject_that_names_only_certificates_that_cannot_sign_says_why() {
    let old = cert(3, &expired("Alice"));
    let code = cert(
        4,
        &Spec {
            purposes: Some(vec!["1.3.6.1.5.5.7.3.3"]),
            ..Spec::new("Alice")
        },
    );
    let list = listed(&[old.clone(), code.clone()]);
    let why = resolve("Alice", &list).expect_err("nothing can sign");
    assert!(why.contains("it has expired"), "{why}");
    assert!(why.contains("code signing"), "{why}");
    // Named by its hash, an unusable certificate is refused with its reason too.
    let why = resolve(&crate::keystore::id_of(&code), &list).expect_err("code signing");
    assert!(
        why.contains("cannot sign") && why.contains("code signing"),
        "{why}"
    );
}

#[test]
fn the_identities_report_sorts_by_the_choosers_rule_and_keeps_every_id() {
    let good = cert(3, &Spec::new("Alice"));
    let old = cert(4, &expired("Bob"));
    let list = listed(&[good.clone(), old.clone()]);
    let report = identities_report(&list);
    assert_eq!(report.usable.len(), 1);
    assert_eq!(report.usable[0].id, crate::keystore::id_of(&good));
    assert_eq!(report.usable[0].subject, "Alice");
    assert_eq!(report.not_usable.len(), 1);
    assert_eq!(report.not_usable[0].id, crate::keystore::id_of(&old));
    assert_eq!(report.not_usable[0].subject, "Bob");
    assert_eq!(report.not_usable[0].why, "it has expired");
    // The same sentences the application's chooser gives.
    let choices = sign_cms::choices(
        &[
            (crate::keystore::id_of(&good), good),
            (crate::keystore::id_of(&old), old),
        ],
        NOW,
    );
    assert_eq!(choices.skipped[0].why, report.not_usable[0].why);
    assert_eq!(choices.usable[0].subject, report.usable[0].subject);
}

// --- exit codes -------------------------------------------------------------

#[test]
fn the_exit_codes_are_the_documented_numbers() {
    let codes: Vec<i32> = [
        Exit::Ok,
        Exit::Strict,
        Exit::Usage,
        Exit::Refused,
        Exit::Internal,
    ]
    .iter()
    .map(|e| e.code())
    .collect();
    assert_eq!(codes, vec![0, 1, 2, 3, 4]);
}

fn signature(verdict: Verdict, standing: Option<Standing>) -> report::Signature {
    report::Signature {
        field: "Signature1".into(),
        signer: "A".into(),
        issuer: "B".into(),
        claimed_time: String::new(),
        covers_whole_file: true,
        appended_bytes: 0,
        integrity: report::IntegrityReport {
            verdict,
            why: None,
            digest: "SHA-256".into(),
            method: "RSA".into(),
            sentence: String::new(),
        },
        trust: standing.map(|standing| report::TrustReport {
            standing,
            why: None,
            store: Some(TrustStore::Mac),
            sentence: String::new(),
        }),
    }
}

fn file(signatures: Vec<report::Signature>, error: Option<ErrorKind>) -> report::File {
    report::File {
        path: "a.pdf".into(),
        error: error.map(|kind| report::FileError {
            kind,
            message: "why".into(),
        }),
        signatures,
    }
}

#[test]
fn strict_passes_only_intact_and_trusted_signatures_in_every_document() {
    let good = || signature(Verdict::Intact, Some(Standing::Trusted));
    let pass = verified(vec![file(vec![good(), good()], None)]);
    assert!(pass.strict_passed);
    assert_eq!(verify_exit(&pass, true), Exit::Ok);

    let failing = [
        vec![file(
            vec![
                good(),
                signature(Verdict::Intact, Some(Standing::Untrusted)),
            ],
            None,
        )],
        vec![file(
            vec![signature(Verdict::Intact, Some(Standing::Expired))],
            None,
        )],
        vec![file(
            vec![signature(Verdict::Weak, Some(Standing::Trusted))],
            None,
        )],
        vec![file(vec![signature(Verdict::Altered, None)], None)],
        vec![file(vec![good()], None), file(Vec::new(), None)],
    ];
    for files in failing {
        let report = verified(files);
        assert!(!report.strict_passed, "{report:?}");
        assert_eq!(verify_exit(&report, true), Exit::Strict);
        // Without --strict, a verdict is an answer and not a failure.
        assert_eq!(verify_exit(&report, false), Exit::Ok);
    }
}

#[test]
fn a_document_that_could_not_be_read_outranks_every_verdict() {
    let good = || signature(Verdict::Intact, Some(Standing::Trusted));
    for (kind, exit) in [
        (ErrorKind::Unreadable, Exit::Refused),
        (ErrorKind::Refused, Exit::Refused),
        (ErrorKind::Locked, Exit::Refused),
        (ErrorKind::Failed, Exit::Internal),
    ] {
        let report = verified(vec![file(vec![good()], None), file(Vec::new(), Some(kind))]);
        assert!(!report.strict_passed);
        assert_eq!(verify_exit(&report, false), exit, "{kind:?}");
        assert_eq!(verify_exit(&report, true), exit, "{kind:?}");
    }
    let both = verified(vec![
        file(Vec::new(), Some(ErrorKind::Refused)),
        file(Vec::new(), Some(ErrorKind::Failed)),
    ]);
    assert_eq!(verify_exit(&both, false), Exit::Internal);
}

/// A store holding P-256 keys made from seeds, and no saved image.
struct Soft256(Vec<(Vec<u8>, u8)>);

impl Store for Soft256 {
    fn identities(&self) -> Result<Vec<Held>, String> {
        Ok(self
            .0
            .iter()
            .map(|(der, seed)| Held {
                certificate: der.clone(),
                chain: Vec::new(),
                key: Box::new(Soft::p256(*seed)),
            })
            .collect())
    }
    fn saved_image(&self) -> Result<Option<crate::signature::Image>, String> {
        Ok(None)
    }
}

/// Runs a command line in-process, returning the code, stdout and stderr.
fn ran(line: &[String], store: &dyn Store) -> (i32, String, String) {
    let env = Env {
        store,
        library_dir: PathBuf::from("/nonexistent/no-workers-here"),
        now: NOW,
        program: "tpdf".into(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(line, &env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).expect("utf-8"),
        String::from_utf8(err).expect("utf-8"),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpdf-cli-tests-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn line(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

#[test]
fn a_malformed_line_exits_2_with_the_reason_on_stderr_and_nothing_on_stdout() {
    let store = Soft256(Vec::new());
    let (code, out, err) = ran(&line(&["sign", "a.pdf", "--identity", "A"]), &store);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("-o <out.pdf>"), "{err}");
}

#[test]
fn signing_refusals_that_need_no_worker_exit_before_one_is_asked_for() {
    // `library_dir` names nothing, so a refusal that reached a worker would
    // come back as a failure to start one --- exit 4 --- and not as these.
    let dir = scratch("refusals");
    let input = dir.join("in.pdf");
    std::fs::write(&input, crate::sign_cms::testkeys::plain_pdf()).expect("input");
    let store = Soft256(vec![(cert(3, &Spec::new("Alice")), 3)]);
    let s = |p: &Path| p.display().to_string();

    // An identity nobody holds: 3, and no output.
    let out = dir.join("out.pdf");
    let (code, stdout, err) = ran(
        &line(&["sign", &s(&input), "-o", &s(&out), "--identity", "Bob"]),
        &store,
    );
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("\"Bob\""), "{err}");
    assert!(stdout.is_empty() && !out.exists());

    // An output that exists: 3, and it is left alone.
    std::fs::write(&out, b"keep me").expect("existing");
    let (code, _, err) = ran(
        &line(&["sign", &s(&input), "-o", &s(&out), "--identity", "Alice"]),
        &store,
    );
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("--force"), "{err}");
    assert_eq!(std::fs::read(&out).expect("kept"), b"keep me");

    // The input under a second name: 2, before anything else.
    let alias = dir.join("alias.pdf");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&input, &alias).expect("link");
    #[cfg(windows)]
    std::fs::hard_link(&input, &alias).expect("link");
    let (code, _, err) = ran(
        &line(&[
            "sign",
            &s(&input),
            "-o",
            &s(&alias),
            "--identity",
            "Alice",
            "--force",
        ]),
        &store,
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("under another name"), "{err}");

    // A missing input: 3.
    let (code, _, err) = ran(
        &line(&[
            "sign",
            &s(&dir.join("missing.pdf")),
            "-o",
            &s(&dir.join("x.pdf")),
            "--identity",
            "Alice",
        ]),
        &store,
    );
    assert_eq!(code, 3, "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the samples ------------------------------------------------------------

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("cli")
}

fn writing() -> bool {
    std::env::var("TPDF_CLI_SAMPLES").as_deref() == Ok("write")
}

fn integrity(verdict: Verdict, why: Option<Why>, digest: &str, method: &str) -> Integrity {
    Integrity {
        verdict,
        why,
        digest: digest.into(),
        method: method.into(),
    }
}

/// Every case the wording can take, with what this module says for it.
fn wording() -> serde_json::Value {
    let whys = [
        Why::Format,
        Why::Range,
        Why::Unreadable,
        Why::Certificate,
        Why::Algorithm,
        Why::Attributes,
        Why::Budget,
    ];
    let mut integrities = Vec::new();
    for verdict in [
        Verdict::Intact,
        Verdict::Weak,
        Verdict::Altered,
        Verdict::Broken,
        Verdict::Unchecked,
    ] {
        let mut shapes = vec![
            integrity(verdict, None, "SHA-256", "RSA"),
            integrity(verdict, None, "", ""),
            integrity(verdict, None, "SHA-1", ""),
        ];
        if verdict == Verdict::Unchecked {
            shapes.extend(whys.iter().map(|w| integrity(verdict, Some(*w), "", "")));
        }
        for shape in shapes {
            for appended in [0_u64, 9_101] {
                for follows in [false, true] {
                    integrities.push(serde_json::json!({
                        "integrity": shape,
                        "appended": appended,
                        "trust_follows": follows,
                        "sentence": words::integrity_sentence(&shape, appended, follows),
                    }));
                }
            }
        }
    }

    let doubts = [
        None,
        Some(Doubt::Incomplete),
        Some(Doubt::Root),
        Some(Doubt::Dates),
        Some(Doubt::Purpose),
        Some(Doubt::Rejected),
        Some(Doubt::Certificate),
        Some(Doubt::Unavailable),
    ];
    let mut trusts = Vec::new();
    for standing in [
        Standing::Trusted,
        Standing::Expired,
        Standing::NotYetValid,
        Standing::Untrusted,
        Standing::Unchecked,
    ] {
        for store in [Some(TrustStore::Mac), Some(TrustStore::Windows), None] {
            for why in doubts {
                for (from, until) in [
                    ("", ""),
                    ("2026-01-02 03:04:05 UTC", "2027-01-02 03:04:05 UTC"),
                ] {
                    let trust = Trust {
                        standing,
                        why,
                        store,
                    };
                    trusts.push(serde_json::json!({
                        "trust": trust,
                        "from": from,
                        "until": until,
                        "sentence": words::trust_sentence(&trust, from, until),
                    }));
                }
            }
        }
    }

    let intact = integrity(Verdict::Intact, None, "SHA-256", "RSA");
    let cases: Vec<Vec<(String, bool, Option<Integrity>)>> = vec![
        vec![("Signature1".into(), true, Some(intact.clone()))],
        vec![
            ("Signature1".into(), false, Some(intact.clone())),
            ("Signature2".into(), true, Some(intact.clone())),
        ],
        vec![
            ("Signature1".into(), false, Some(intact.clone())),
            (
                "Signature2".into(),
                false,
                Some(integrity(Verdict::Altered, None, "", "")),
            ),
            ("Signature3".into(), true, Some(intact.clone())),
        ],
        vec![
            (
                "Signature1".into(),
                false,
                Some(integrity(Verdict::Unchecked, Some(Why::Format), "", "")),
            ),
            ("Signature2".into(), false, None),
            ("Signature3".into(), true, Some(intact.clone())),
        ],
        vec![(
            "Signature1".into(),
            true,
            Some(integrity(Verdict::Broken, None, "", "")),
        )],
        vec![(
            "Signature1".into(),
            true,
            Some(integrity(Verdict::Weak, None, "SHA-1", "RSA")),
        )],
        vec![("Signature1".into(), false, Some(intact.clone()))],
    ];
    let mut after = Vec::new();
    for (n, signatures) in cases.into_iter().enumerate() {
        let field = signatures
            .iter()
            .find(|(_, ours, _)| *ours)
            .map_or("Signature2".to_string(), |(f, _, _)| f.clone());
        let path = format!("/tmp/signed-{n}.pdf");
        let name = format!("signed-{n}.pdf");
        after.push(serde_json::json!({
            "signed": {
                "path": path,
                "field": field,
                "signatures": signatures.iter().map(|(f, ours, i)| serde_json::json!({
                    "field": f, "ours": ours, "integrity": i,
                })).collect::<Vec<_>>(),
            },
            "sentence": words::after_signing(&name, &field, &signatures),
        }));
    }

    serde_json::json!({
        "integrity": integrities,
        "trust": trusts,
        "after_signing": after,
    })
}

fn full_signature() -> report::Signature {
    let integrity = integrity(Verdict::Intact, None, "SHA-256", "RSA");
    let trust = Trust {
        standing: Standing::Untrusted,
        why: Some(Doubt::Root),
        store: Some(TrustStore::Mac),
    };
    report::Signature {
        field: "Signature1".into(),
        signer: "First Signer".into(),
        issuer: "tpdf test root CA".into(),
        claimed_time: "2026-09-26 20:20:13 +02:00".into(),
        covers_whole_file: false,
        appended_bytes: 9_101,
        integrity: report::IntegrityReport {
            verdict: integrity.verdict,
            why: None,
            digest: integrity.digest.clone(),
            method: integrity.method.clone(),
            sentence: words::integrity_sentence(&integrity, 9_101, true),
        },
        trust: Some(report::TrustReport {
            standing: trust.standing,
            why: trust.why,
            store: trust.store,
            sentence: words::trust_sentence(&trust, "", ""),
        }),
    }
}

fn bare_signature() -> report::Signature {
    let integrity = integrity(Verdict::Unchecked, Some(Why::Format), "", "");
    report::Signature {
        field: "Signature2".into(),
        signer: String::new(),
        issuer: String::new(),
        claimed_time: String::new(),
        covers_whole_file: true,
        appended_bytes: 0,
        integrity: report::IntegrityReport {
            verdict: integrity.verdict,
            why: integrity.why,
            digest: String::new(),
            method: String::new(),
            sentence: words::integrity_sentence(&integrity, 0, false),
        },
        trust: None,
    }
}

/// One sample of each JSON document, every optional field both ways.
fn samples() -> Vec<(&'static str, String)> {
    let pretty = |value: &dyn erased::Json| value.pretty();
    let usable = report::Usable {
        id: "2a144cdb0facc6919f9163d7776c3c2f7f35bbd23021c002d61c29c7f0819c74".into(),
        subject: "A. Signer".into(),
        issuer: "An Issuing CA".into(),
        expires: "2027-09-26 00:00:00 UTC".into(),
        method: "RSA 3072".into(),
    };
    let identities = report::Identities {
        schema: report::SCHEMA,
        command: "identities".into(),
        usable: vec![usable.clone()],
        not_usable: vec![report::NotUsable {
            id: "f526049003c94827b7f211c5f5bfecb156d4fdb300ed41ea722431d109b75531".into(),
            subject: "Developer ID Application: A. Signer (TEAMID1234)".into(),
            why: "it is issued for code signing, not for signing documents".into(),
        }],
    };
    let verify = verified(vec![
        report::File {
            path: "contract.pdf".into(),
            error: None,
            signatures: vec![full_signature(), bare_signature()],
        },
        report::File {
            path: "locked.pdf".into(),
            error: Some(report::FileError {
                kind: ErrorKind::Locked,
                message: "locked.pdf is encrypted with a password, and its signatures cannot be \
                          read without it --- open it in tpdf to check them"
                    .into(),
            }),
            signatures: Vec::new(),
        },
    ]);
    let summary = words::after_signing(
        "contract-signed.pdf",
        "Signature2",
        &[
            (
                "Signature1".into(),
                false,
                Some(integrity(Verdict::Intact, None, "SHA-256", "RSA")),
            ),
            (
                "Signature2".into(),
                true,
                Some(integrity(Verdict::Intact, None, "SHA-256", "RSA")),
            ),
        ],
    );
    let sign = report::Signed {
        schema: report::SCHEMA,
        command: "sign".into(),
        input: "contract.pdf".into(),
        output: "contract-signed.pdf".into(),
        field: "Signature2".into(),
        identity: usable,
        visible: true,
        signatures: vec![full_signature(), bare_signature()],
        summary,
    };
    vec![
        ("identities", pretty(&identities)),
        ("verify", pretty(&verify)),
        ("sign", pretty(&sign)),
        ("wording", pretty(&wording())),
    ]
}

/// `serde_json::to_string_pretty` over the four shapes, behind one call.
mod erased {
    pub trait Json {
        fn pretty(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn pretty(&self) -> String {
            serde_json::to_string_pretty(self).expect("serialises")
        }
    }
}

#[test]
fn every_json_shape_and_the_wording_match_their_committed_samples() {
    let at = samples_dir();
    if writing() {
        std::fs::create_dir_all(&at).expect("the samples directory");
    }
    let mut wrong = Vec::new();
    for (name, json) in samples() {
        let path = at.join(format!("{name}.json"));
        let text = format!("{json}\n");
        if writing() {
            std::fs::write(&path, &text).expect("write a sample");
            continue;
        }
        match std::fs::read_to_string(&path) {
            Err(why) => wrong.push(format!("{name}.json could not be read: {why}")),
            Ok(found) if found != text => wrong.push(format!(
                "{name}.json is not what the command-line tool writes now.\n\
                 --- committed ---\n{found}\n--- now ---\n{text}"
            )),
            Ok(_) => {}
        }
    }
    assert!(
        wrong.is_empty(),
        "{} sample(s) disagree. A changed JSON sample is a changed schema: add a field \
         freely, but a renamed or removed one moves `report::SCHEMA`. A changed wording \
         sample must agree with `src/lib/integrity.ts` and `signing.ts`, which \
         `cliwording.test.ts` checks. Regenerate with TPDF_CLI_SAMPLES=write.\n\n{}",
        wrong.len(),
        wrong.join("\n\n")
    );
}

#[test]
fn the_samples_directory_holds_one_file_per_sample_and_nothing_else() {
    let at = samples_dir();
    let mut found: Vec<String> = std::fs::read_dir(&at)
        .unwrap_or_else(|why| panic!("the samples directory at {at:?}: {why}"))
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    let mut want: Vec<String> = samples()
        .iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    want.sort();
    assert_eq!(want.len(), 4, "the sample table itself");
    assert_eq!(found, want);
}

/// Every object key in a JSON value, at every depth.
fn keys(value: &serde_json::Value, into: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                into.insert(key.clone());
                keys(inner, into);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| keys(item, into)),
        _ => {}
    }
}

#[test]
fn every_json_key_is_described_in_the_readme() {
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md"))
            .expect("README.md");
    let start = readme
        .find("## Command-line tool")
        .expect("README.md has a `## Command-line tool` section");
    let section = &readme[start..];
    let section = &section[..section[3..]
        .find("\n## ")
        .map_or(section.len(), |end| end + 3)];

    let mut all = std::collections::BTreeSet::new();
    for (name, json) in samples() {
        if name == "wording" {
            continue;
        }
        keys(
            &serde_json::from_str(&json).expect("a sample parses"),
            &mut all,
        );
    }
    // The emptiness control: the walk found the keys it must.
    for known in [
        "schema",
        "strict_passed",
        "sentence",
        "not_usable",
        "claimed_time",
    ] {
        assert!(
            all.contains(known),
            "the key walk missed `{known}`: {all:?}"
        );
    }
    let missing: Vec<&String> = all
        .iter()
        .filter(|key| !section.contains(&format!("`{key}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "README.md's command-line section does not describe {missing:?}"
    );
    // And the exit codes, which a script depends on as much as on a key.
    for code in 0..=4 {
        assert!(
            section.contains(&format!("| {code} |")),
            "README.md's exit-code table has no row for {code}"
        );
    }
}

#[test]
fn the_identifier_is_the_applications() {
    let config =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
            .expect("tauri.conf.json");
    let config: serde_json::Value = serde_json::from_str(&config).expect("json");
    assert_eq!(config["identifier"].as_str(), Some(IDENTIFIER));
}
