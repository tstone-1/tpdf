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
fn a_timestamp_authority_is_named_or_given_and_judged_before_anything_runs() {
    let named = signed("sign in.pdf -o out.pdf --identity Me --timestamp digicert");
    assert_eq!(
        named.timestamp.as_ref().map(url::Url::as_str),
        Some("http://timestamp.digicert.com/")
    );
    let given = signed("sign in.pdf -o out.pdf --identity Me --timestamp https://tsa.example/x");
    assert_eq!(
        given.timestamp.as_ref().map(url::Url::as_str),
        Some("https://tsa.example/x")
    );
    // Nothing asked unless asked for.
    assert_eq!(
        signed("sign in.pdf -o out.pdf --identity Me").timestamp,
        None
    );
    // An address tpdf will not ask is a malformed line, exit 2, before a
    // worker, a key or a socket: `refused` runs the parser alone.
    for bad in [
        "ftp://tsa.example/",
        "file:///tmp/x",
        "http://user:pw@tsa.example/",
        "verisign",
    ] {
        let why = refused(&format!(
            "sign in.pdf -o out.pdf --identity Me --timestamp {bad}"
        ));
        assert!(why.starts_with("tpdf does not ask"), "{bad}: {why}");
    }
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
        ("info", "info a.pdf --json"),
        ("text", "text a.pdf --pages 2"),
        ("fields", "fields a.pdf --json"),
        ("fill", "fill a.pdf -o b.pdf --values answers.json"),
        ("redact", "redact a.pdf -o b.pdf --text Secret"),
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
    assert!(
        why.contains("sign, verify, identities, info, text, fields, fill, redact"),
        "{why}"
    );
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
            attested_at: String::new(),
            sentence: String::new(),
        }),
        timestamp: None,
        revocation: None,
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
        Why::Binding,
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
        Some(Doubt::Timestamping),
        Some(Doubt::NotInForce),
    ];
    // Judged now, or at the time a trusted timestamp attests.
    let moments = ["", "2026-08-21 12:00:00 UTC"];
    let mut trusts = Vec::new();
    for standing in [
        Standing::Trusted,
        Standing::TrustedAtTimestamp,
        Standing::Expired,
        Standing::NotYetValid,
        Standing::Untrusted,
        Standing::Unchecked,
    ] {
        for store in [Some(TrustStore::Mac), Some(TrustStore::Windows), None] {
            for why in doubts {
                for ((from, until), attested_at) in [
                    ("", ""),
                    ("2026-01-02 03:04:05 UTC", "2027-01-02 03:04:05 UTC"),
                ]
                .into_iter()
                .flat_map(|dates| moments.map(|m| (dates, m)))
                {
                    let trust = Trust {
                        standing,
                        why,
                        store,
                        attested_at: attested_at.into(),
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
    // The same signings, each again with a timestamp on the new signature:
    // sound and named, sound and unnamed, and one that does not check out ---
    // whose words are the properties dialog's, whatever the verdict.
    let trusted = Trust {
        standing: Standing::Trusted,
        why: None,
        store: Some(TrustStore::Mac),
        attested_at: String::new(),
    };
    let stranger = Trust {
        standing: Standing::Untrusted,
        why: Some(Doubt::Root),
        store: Some(TrustStore::Windows),
        attested_at: String::new(),
    };
    let stamps: [Option<(&str, Integrity, Option<Trust>)>; 5] = [
        None,
        Some((
            "Acme Time Authority",
            integrity(Verdict::Intact, None, "SHA-256", "ECDSA P-256"),
            Some(trusted),
        )),
        // Checks out, from an authority nobody vouches for: what a token
        // substituted over plain HTTP looks like, and it must say so.
        Some((
            "Somebody Else",
            integrity(Verdict::Intact, None, "SHA-256", "RSA"),
            Some(stranger),
        )),
        Some(("", integrity(Verdict::Intact, None, "SHA-256", "RSA"), None)),
        Some((
            "Acme Time Authority",
            integrity(Verdict::Broken, None, "", ""),
            None,
        )),
    ];
    let mut after = Vec::new();
    for (stamp_at, stamp) in stamps.iter().enumerate() {
        for (n, signatures) in cases.iter().enumerate() {
            let field = signatures
                .iter()
                .find(|(_, ours, _)| *ours)
                .map_or("Signature2".to_string(), |(f, _, _)| f.clone());
            let path = format!("/tmp/signed-{stamp_at}-{n}.pdf");
            let name = format!("signed-{stamp_at}-{n}.pdf");
            let when = "2026-09-28 10:11:12 UTC";
            // Past dates, for `scripts/check_dates.py`: only the expired and
            // not-yet-valid sentences show them, and neither is among these.
            let (from, until) = ("2025-01-02 03:04:05 UTC", "2026-01-02 03:04:05 UTC");
            let stamp_json = stamp.as_ref().map(|(by, i, trust)| {
                serde_json::json!({
                    "when": when,
                    "authority": if by.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!({
                            "subject": format!("CN={by}"),
                            "subject_cn": by,
                            "from": from,
                            "until": until,
                        })
                    },
                    "integrity": i,
                    "trust": trust,
                    "attested": matches!(i.verdict, Verdict::Intact | Verdict::Weak),
                })
            });
            let sentences = stamp.as_ref().map(|(by, i, trust)| {
                let (from, until) = if by.is_empty() {
                    ("", "")
                } else {
                    (from, until)
                };
                (
                    words::timestamp_sentence(when, by, Some(i), false),
                    trust
                        .as_ref()
                        .map(|trust| words::authority_sentence(trust, from, until)),
                )
            });
            after.push(serde_json::json!({
                "signed": {
                    "path": path,
                    "field": field,
                    "signatures": signatures.iter().map(|(f, ours, i)| serde_json::json!({
                        "field": f, "ours": ours, "integrity": i,
                        "timestamp": if *ours { stamp_json.clone() } else { None },
                    })).collect::<Vec<_>>(),
                },
                "sentence": words::after_signing(
                    &name,
                    &field,
                    signatures,
                    sentences.as_ref().map(|(t, a)| (t.as_str(), a.as_deref())),
                ),
            }));
        }
    }

    // Every shape `afterRedaction` has: verified or not, one or several of
    // each count, one reason or several, changed or not.
    let mut after_redaction = Vec::new();
    let reasons: [Vec<String>; 3] = [
        Vec::new(),
        vec![
            "page 1: object 0 is of kind path and overlaps the region; only text is removed here"
                .into(),
        ],
        vec![
            "4711-0815 is still in the file, on page 2".into(),
            "page 2: the removed area could not be shown unreadable. the engine rejected the image"
                .into(),
        ],
    ];
    for (regions, shows) in [(1, 1), (3, 2), (0, 0)] {
        for why in &reasons {
            for changed in [false, true] {
                let verified = why.is_empty();
                after_redaction.push(serde_json::json!({
                    "applied": {
                        "regions": regions,
                        "shows": shows,
                        "verified": verified,
                        "why": why,
                        "changed": changed,
                    },
                    "sentence": words::after_redaction(regions, shows, verified, why, changed),
                }));
            }
        }
    }

    // A timestamp's row: every verdict in the three shapes, the unchecked one
    // with every reason; with no verdict at all; named and unnamed; attached
    // to a signature and a document timestamp.
    let mut timestamps = Vec::new();
    let mut stamp_shapes: Vec<Option<Integrity>> = vec![None];
    for verdict in [
        Verdict::Intact,
        Verdict::Weak,
        Verdict::Altered,
        Verdict::Broken,
        Verdict::Unchecked,
    ] {
        stamp_shapes.push(Some(integrity(verdict, None, "SHA-256", "ECDSA P-256")));
        stamp_shapes.push(Some(integrity(verdict, None, "", "")));
        if verdict == Verdict::Unchecked {
            stamp_shapes.extend(
                whys.iter()
                    .map(|w| Some(integrity(verdict, Some(*w), "", ""))),
            );
        }
    }
    for shape in &stamp_shapes {
        for by in ["Acme Time Authority", ""] {
            for document in [false, true] {
                let when = "2026-08-21 12:00:00 UTC";
                timestamps.push(serde_json::json!({
                    "when": when,
                    "by": by,
                    "integrity": shape,
                    "document": document,
                    "sentence": words::timestamp_sentence(when, by, shape.as_ref(), document),
                }));
            }
        }
    }
    // The authority's row: every standing, store and reason, both date pairs.
    let mut authorities = Vec::new();
    for standing in [
        Standing::Trusted,
        Standing::TrustedAtTimestamp,
        Standing::Expired,
        Standing::NotYetValid,
        Standing::Untrusted,
        Standing::Unchecked,
    ] {
        for store in [Some(TrustStore::Mac), Some(TrustStore::Windows), None] {
            for why in doubts {
                for ((from, until), attested_at) in [
                    ("", ""),
                    ("2026-01-02 03:04:05 UTC", "2027-01-02 03:04:05 UTC"),
                ]
                .into_iter()
                .flat_map(|dates| moments.map(|m| (dates, m)))
                {
                    let trust = Trust {
                        standing,
                        why,
                        store,
                        attested_at: attested_at.into(),
                    };
                    authorities.push(serde_json::json!({
                        "trust": trust,
                        "from": from,
                        "until": until,
                        "sentence": words::authority_sentence(&trust, from, until),
                    }));
                }
            }
        }
    }

    // A revocation row: every standing, in the shapes each can take --- every
    // reason a revocation gives, every gap, every basis, both sources --- for
    // the signer's certificate and an authority's.
    let mut revocations = Vec::new();
    {
        use crate::revocation::{Basis, Gap, Reason, Revocation, Source, Status};
        let bases = [Basis::Attested, Basis::Stated, Basis::Claimed, Basis::Now];
        let reasons = [
            None,
            Some(Reason::Unspecified),
            Some(Reason::KeyCompromise),
            Some(Reason::CaCompromise),
            Some(Reason::AffiliationChanged),
            Some(Reason::Superseded),
            Some(Reason::CessationOfOperation),
            Some(Reason::CertificateHold),
            Some(Reason::RemoveFromCrl),
            Some(Reason::PrivilegeWithdrawn),
            Some(Reason::AaCompromise),
        ];
        let gaps = [
            None,
            Some(Gap::Unreadable),
            Some(Gap::Bound),
            Some(Gap::Issuer),
            Some(Gap::Signature),
            Some(Gap::Unauthorised),
            Some(Gap::Algorithm),
            Some(Gap::Unsupported),
            Some(Gap::Stale),
            Some(Gap::Expired),
            Some(Gap::Dates),
            Some(Gap::Budget),
        ];
        let at = |basis: Basis| Revocation {
            basis,
            moment: "2026-08-21 12:00:00 UTC".into(),
            ..Revocation::default()
        };
        let answered = |basis: Basis, standing: Status, source: Source, next: &str| Revocation {
            standing,
            source: Some(source),
            issued: "2026-08-20 09:00:00 UTC".into(),
            next: next.into(),
            ..at(basis)
        };
        let mut shapes = Vec::new();
        for basis in bases {
            shapes.push(at(basis));
            for why in gaps {
                shapes.push(Revocation {
                    standing: Status::Unchecked,
                    why,
                    ..at(basis)
                });
            }
            for source in [Source::Ocsp, Source::Crl] {
                for next in ["", "2026-08-27 09:00:00 UTC"] {
                    shapes.push(answered(basis, Status::Good, source, next));
                    shapes.push(answered(basis, Status::Unknown, source, next));
                }
                for reason in reasons {
                    for after_moment in [false, true] {
                        shapes.push(Revocation {
                            revoked: "2026-08-01 00:00:00 UTC".into(),
                            reason,
                            after_moment,
                            ..answered(basis, Status::Revoked, source, "2026-08-27 09:00:00 UTC")
                        });
                    }
                }
            }
        }
        for shape in shapes {
            for authority in [false, true] {
                revocations.push(serde_json::json!({
                    "revocation": shape,
                    "authority": authority,
                    "sentence": words::revocation_sentence(&shape, authority),
                }));
            }
        }
    }

    serde_json::json!({
        "integrity": integrities,
        "trust": trusts,
        "timestamp": timestamps,
        "authority": authorities,
        "revocation": revocations,
        "after_signing": after,
        "after_redaction": after_redaction,
        // The window offers these by name and the tool takes their names:
        // `signtimestamp.test.ts` holds its list to this one.
        "timestamp_servers": crate::tsa::SERVERS,
    })
}

fn full_signature() -> report::Signature {
    let integrity = integrity(Verdict::Intact, None, "SHA-256", "RSA");
    let trust = Trust {
        standing: Standing::Untrusted,
        why: Some(Doubt::Root),
        store: Some(TrustStore::Mac),
        attested_at: String::new(),
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
            attested_at: trust.attested_at.clone(),
            sentence: words::trust_sentence(&trust, "", ""),
        }),
        revocation: Some(super::verify::revocation_report(
            &crate::revocation::Revocation {
                standing: crate::revocation::Status::Good,
                why: None,
                source: Some(crate::revocation::Source::Ocsp),
                issued: "2026-09-26 12:00:00 UTC".into(),
                next: "2026-09-27 12:00:00 UTC".into(),
                revoked: String::new(),
                reason: None,
                basis: crate::revocation::Basis::Claimed,
                moment: "2026-09-26 18:20:13 UTC".into(),
                after_moment: false,
            },
            false,
        )),
        timestamp: Some(super::verify::timestamp_report(
            &crate::docinfo::Timestamp {
                when: "2026-09-26 18:20:14 UTC".into(),
                authority: Some(crate::docinfo::Certificate {
                    subject: "CN=Acme Time Authority".into(),
                    subject_cn: "Acme Time Authority".into(),
                    ..crate::docinfo::Certificate::default()
                }),
                integrity: Some(Integrity {
                    verdict: Verdict::Intact,
                    why: None,
                    digest: "SHA-256".into(),
                    method: "ECDSA P-256".into(),
                }),
                trust: Some(Trust {
                    standing: Standing::Untrusted,
                    why: Some(Doubt::Root),
                    store: Some(TrustStore::Mac),
                    attested_at: String::new(),
                }),
                attested: true,
                revocation: Some(crate::revocation::Revocation {
                    basis: crate::revocation::Basis::Stated,
                    moment: "2026-09-26 18:20:14 UTC".into(),
                    ..crate::revocation::Revocation::default()
                }),
            },
            false,
        )),
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
        timestamp: None,
        revocation: None,
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
        None,
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
        ("info", pretty(&info_sample())),
        ("text", pretty(&text_sample())),
        ("reading", pretty(&reading_sample())),
        ("fields", pretty(&super::form_tests::fields_sample())),
        ("fill", pretty(&super::form_tests::fill_samples().0)),
        ("fill-refused", pretty(&super::form_tests::fill_samples().1)),
        ("redact", pretty(&super::redact_tests::redact_samples().0)),
        (
            "redact-dry-run",
            pretty(&super::redact_tests::redact_samples().1),
        ),
        ("regions", pretty(&super::regions::sample())),
    ]
}

/// `serde_json::to_string_pretty` over the shapes, behind one call.
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
         sample must agree with `src/lib/integrity.ts`, `signing.ts` and `recovery.ts`, \
         which `cliwording.test.ts` checks, and the regions sample with the viewer's \
         search, text and selection code, which `cliregions.test.ts` checks. \
         Regenerate with TPDF_CLI_SAMPLES=write.\n\n{}",
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
    assert_eq!(want.len(), 13, "the sample table itself");
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
        if name == "wording" || name == "reading" || name == "regions" {
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
        "page_sizes",
        "opened_without_password",
        "encoding",
        "custom_text",
        "not_editable",
        "problem",
        "written",
        "signatures_invalidated",
        "form_text_removals",
        "attested",
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

// --- info and text ------------------------------------------------------------

/// `info` for one described document, one whose form is XFA, and one locked.
fn info_sample() -> report::Info {
    let form = super::info::form_report(Ok(crate::forms::Form::default()));
    let document = report::Document {
        version: "1.7".into(),
        bytes: 19_252,
        pages: 3,
        page_sizes: super::info::page_sizes(&[
            crate::render::PageSize {
                width_pt: 595.2756,
                height_pt: 841.8898,
            },
            crate::render::PageSize {
                width_pt: 595.2756,
                height_pt: 841.8898,
            },
            crate::render::PageSize {
                width_pt: 841.8898,
                height_pt: 595.2756,
            },
        ]),
        revisions: 3,
        metadata: vec![
            crate::docinfo::Field {
                name: "Title".into(),
                value: "Quarterly review".into(),
                standard: true,
            },
            crate::docinfo::Field {
                name: "Producer".into(),
                value: "pyHanko 0.37.0".into(),
                standard: true,
            },
            crate::docinfo::Field {
                name: "Department".into(),
                value: "Compliance".into(),
                standard: false,
            },
        ],
        language: "en-GB".into(),
        encryption: Some(crate::docinfo::Encryption {
            method: "AES-256".into(),
            revision: 6,
            opened_without_password: true,
            permissions: vec![
                crate::docinfo::Permission {
                    what: "print".into(),
                    allowed: true,
                },
                crate::docinfo::Permission {
                    what: "modify".into(),
                    allowed: false,
                },
            ],
        }),
        tagged: Some(true),
        conformance: Some(report::Conformance {
            claimed: vec!["PDF/A-3B".into(), "PDF/UA-1".into()],
            unread: false,
        }),
        attachments: Some(1),
        form,
        signatures: vec![full_signature()],
        unsigned_signature_fields: 1,
        limits: crate::docinfo::Limits::default(),
    };
    let xfa = report::Document {
        tagged: None,
        encryption: None,
        conformance: None,
        attachments: None,
        signatures: Vec::new(),
        unsigned_signature_fields: 0,
        form: super::info::form_report(Err(crate::forms::XFA_REFUSAL.into())),
        ..document.clone()
    };
    report::Info {
        schema: report::SCHEMA,
        command: "info".into(),
        files: vec![
            report::Described {
                path: "contract.pdf".into(),
                error: None,
                document: Some(document),
            },
            report::Described {
                path: "xfa.pdf".into(),
                error: None,
                document: Some(xfa),
            },
            report::Described {
                path: "locked.pdf".into(),
                error: Some(report::FileError {
                    kind: ErrorKind::Locked,
                    message: "locked.pdf is encrypted with a password --- give it with \
                              --password-env to describe it"
                        .into(),
                }),
                document: None,
            },
        ],
    }
}

/// `text` over every `order` and every `encoding`.
fn text_sample() -> report::Text {
    let page = |page, order, encoding, text: &str| report::PageText {
        page,
        order,
        encoding,
        text: text.into(),
    };
    report::Text {
        schema: report::SCHEMA,
        command: "text".into(),
        path: "report.pdf".into(),
        pages: vec![
            page(
                1,
                report::Order::Tagged,
                report::Encoding::Stated,
                "Quarterly review\nThe first paragraph.",
            ),
            page(
                2,
                report::Order::Geometric,
                report::Encoding::Guessed,
                "alpha one\nbeta one",
            ),
            page(3, report::Order::None, report::Encoding::Unknown, ""),
        ],
    }
}

/// Every reading-order case, with the order `reading.rs` gives it --- what
/// `clireading.test.ts` asks `reading.ts` to agree with.
fn reading_sample() -> serde_json::Value {
    let cases: Vec<serde_json::Value> = crate::reading::tests::cases()
        .into_iter()
        .map(|case| {
            let reading = crate::reading::read(&case.text);
            let mut text = case.text;
            text.extract_ms = 0.0;
            // Through the serializer, as `commands::read::page_text` sends it to
            // the webview, and not through `json!`: a `Value` widens each `f32`
            // to the `f64` a cast gives, which is not the number the viewer
            // holds --- `reading.rs`'s module note, and the `f32-tie` case.
            let text: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(&text).expect("serialises"))
                    .expect("parses");
            serde_json::json!({
                "name": case.name,
                "text": text,
                "route": reading.route,
                "lines": reading.lines,
                "order": reading.order(),
            })
        })
        .collect();
    serde_json::json!({ "cases": cases })
}

fn text_line(line: &str) -> super::text::Text {
    let args = argv(line);
    assert!(
        matches!(parse(&args), Ok(Line::Run(_))),
        "`{line}` was not accepted"
    );
    super::text::parse(&args[1..]).unwrap_or_else(|why| panic!("`{line}`: {why}"))
}

#[test]
fn a_page_list_is_read_as_the_palette_reads_one() {
    assert_eq!(
        super::text::page_list("1-3,7").expect("ok"),
        vec![1, 2, 3, 7]
    );
    // Merged and in document order, whatever order it was typed in.
    assert_eq!(
        super::text::page_list("7, 2-3,1-2").expect("ok"),
        vec![1, 2, 3, 7]
    );
    assert_eq!(text_line("text a.pdf --pages 4").pages, Some(vec![4]));
    assert_eq!(text_line("text a.pdf").pages, None);
    for (raw, says) in [
        ("0", "count from 1"),
        ("3-1", "runs backwards"),
        ("1,,2", "empty part"),
        ("1,", "empty part"),
        ("two", "not a page number"),
        ("1-", "not a page number"),
        ("-2", "not a page number"),
        ("1-2-3", "not a page number"),
    ] {
        let why = super::text::page_list(raw).expect_err(raw);
        assert!(why.contains(says), "{raw}: {why}");
    }
}

#[test]
fn a_page_past_the_end_is_refused_by_number() {
    assert_eq!(super::text::selected(None, 3).expect("all"), vec![1, 2, 3]);
    assert_eq!(
        super::text::selected(Some(&[1, 3]), 3).expect("ok"),
        vec![1, 3]
    );
    let why = super::text::selected(Some(&[2, 4]), 3).expect_err("4 of 3");
    assert!(
        why.contains("no page 4") && why.contains("3 pages"),
        "{why}"
    );
}

#[test]
fn every_malformed_text_or_info_line_is_refused_with_its_reason() {
    for (line, says) in [
        ("text", "needs the document"),
        ("text a.pdf b.pdf", "second"),
        ("text a.pdf -o a.pdf", "names the document being read"),
        ("text a.pdf -o ./a.pdf", "names the document being read"),
        ("text a.pdf --force", "there is no `-o`"),
        ("text a.pdf --pages", "needs a value"),
        ("text a.pdf --pages 0", "count from 1"),
        ("text a.pdf --password swordfish", "no option `--password`"),
        ("text a.pdf --password-env A=b", "cannot be one"),
        ("info", "at least one document"),
        ("info a.pdf --pages 1", "no option `--pages`"),
        ("info a.pdf --password-env", "needs a value"),
    ] {
        let why = refused(line);
        assert!(why.contains(says), "`{line}`: {why}");
    }
    let info = super::info::parse(&argv("a.pdf b.pdf --password-env PW --json")).expect("info");
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.password_env.as_deref(), Some("PW"));
    assert!(info.json);
}

#[test]
fn a_password_variable_that_is_not_set_is_a_malformed_line() {
    let name = "TPDF_CLI_TEST_A_VARIABLE_NOBODY_SETS";
    assert!(
        std::env::var_os(name).is_none(),
        "the control: {name} is unset"
    );
    let failure = super::text::password(Some(name)).expect_err("unset");
    assert_eq!(failure.exit, Exit::Usage);
    assert!(failure.message.contains(name), "{}", failure.message);
    assert_eq!(super::text::password(None).expect("none"), None);
    // Run end to end, the refusal comes before any worker is asked for.
    let store = Soft256(Vec::new());
    for command in ["text", "info"] {
        let (code, out, err) = ran(
            &line(&[command, "missing.pdf", "--password-env", name]),
            &store,
        );
        assert_eq!(code, 2, "{command}: {err}");
        assert!(out.is_empty(), "{command}: {out}");
    }
}

#[test]
fn text_refusals_that_need_no_worker_exit_before_one_is_asked_for() {
    // `library_dir` names nothing: a refusal that reached a worker would be 4.
    let dir = scratch("text-refusals");
    let input = dir.join("in.pdf");
    std::fs::write(&input, crate::sign_cms::testkeys::plain_pdf()).expect("input");
    let store = Soft256(Vec::new());
    let s = |p: &Path| p.display().to_string();

    let out = dir.join("out.txt");
    std::fs::write(&out, b"keep me").expect("existing");
    let (code, _, err) = ran(&line(&["text", &s(&input), "-o", &s(&out)]), &store);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("--force"), "{err}");
    assert_eq!(std::fs::read(&out).expect("kept"), b"keep me");

    let alias = dir.join("alias.pdf");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&input, &alias).expect("link");
    #[cfg(windows)]
    std::fs::hard_link(&input, &alias).expect("link");
    let (code, _, err) = ran(
        &line(&["text", &s(&input), "-o", &s(&alias), "--force"]),
        &store,
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("under another name"), "{err}");
    assert_eq!(
        std::fs::read(&input).expect("input"),
        crate::sign_cms::testkeys::plain_pdf()
    );

    let (code, _, err) = ran(&line(&["text", &s(&dir.join("missing.pdf"))]), &store);
    assert_eq!(code, 3, "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_locked_document_is_reported_by_info_and_exits_0() {
    let file = |kind: Option<ErrorKind>| report::Described {
        path: "a.pdf".into(),
        error: kind.map(|kind| report::FileError {
            kind,
            message: String::new(),
        }),
        document: None,
    };
    let info = |files| report::Info {
        schema: report::SCHEMA,
        command: "info".into(),
        files,
    };
    use super::info::info_exit;
    assert_eq!(info_exit(&info(vec![file(None)])), Exit::Ok);
    assert_eq!(
        info_exit(&info(vec![file(Some(ErrorKind::Locked))])),
        Exit::Ok
    );
    assert_eq!(
        info_exit(&info(vec![
            file(Some(ErrorKind::Locked)),
            file(Some(ErrorKind::Refused))
        ])),
        Exit::Refused
    );
    assert_eq!(
        info_exit(&info(vec![
            file(Some(ErrorKind::Failed)),
            file(Some(ErrorKind::Unreadable))
        ])),
        Exit::Internal
    );
}

#[test]
fn a_form_is_counted_by_field_and_an_xfa_refusal_is_named() {
    use super::info::form_report;
    let widget = |object: u32, widget: u32| crate::forms::Widget {
        object: (object, 0),
        widget: (widget, 0),
        page: 0,
        rect: [0.0; 4],
        display_rect: [0.0; 4],
        name: format!("f{object}"),
        value: crate::forms::Value::Text(String::new()),
        control: crate::forms::Control::Text,
        multiline: false,
        max_length: None,
        reason: None,
    };
    let two = form_report(Ok(crate::forms::Form {
        // One field with two widgets --- a radio group, or a name shown twice.
        widgets: vec![widget(1, 101), widget(1, 102), widget(2, 103)],
    }));
    assert_eq!(
        (two.readable, two.fields, two.widgets, two.xfa),
        (true, 2, 3, false)
    );
    let xfa = form_report(Err(crate::forms::XFA_REFUSAL.into()));
    assert!(!xfa.readable && xfa.xfa, "{xfa:?}");
    let bound = form_report(Err("This form exceeds the field-tree limit".into()));
    assert!(!bound.readable && !bound.xfa, "{bound:?}");
}

#[test]
fn page_sizes_are_counted_by_displayed_size_first_seen_first() {
    let size = |w, h| crate::render::PageSize {
        width_pt: w,
        height_pt: h,
    };
    let sizes = super::info::page_sizes(&[
        size(612.0, 792.0),
        size(595.2756, 841.8898),
        size(612.0, 792.0),
        size(792.0, 612.0),
    ]);
    let seen: Vec<(f64, f64, usize)> = sizes
        .iter()
        .map(|s| (s.width_pt, s.height_pt, s.count))
        .collect();
    assert_eq!(
        seen,
        vec![(612.0, 792.0, 2), (595.28, 841.89, 1), (792.0, 612.0, 1)]
    );
}

#[test]
fn a_page_says_which_order_it_was_read_in_and_whether_its_fonts_say_what_they_mean() {
    use super::text::ordered;
    use crate::encoding::PageMapping;
    let cases = crate::reading::tests::cases();
    let find = |name: &str| {
        cases
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.text.clone())
            .expect(name)
    };
    let stated = PageMapping::default();
    let guessed = PageMapping {
        composite: 1,
        guessing: 1,
        truncated: false,
    };
    let unsure = PageMapping {
        composite: 1,
        guessing: 0,
        truncated: true,
    };

    let tagged = ordered(1, &find("tagged"), Some(&stated));
    assert_eq!(tagged.order, report::Order::Tagged);
    assert_eq!(tagged.text, "body one\nbody two\nnote");
    assert_eq!(tagged.encoding, report::Encoding::Stated);
    let geometric = ordered(1, &find("tagged-stripped"), Some(&guessed));
    assert_eq!(geometric.order, report::Order::Geometric);
    assert_eq!(geometric.text, "note\nbody one\nbody two");
    assert_eq!(geometric.encoding, report::Encoding::Guessed);
    let empty = ordered(3, &find("empty"), Some(&unsure));
    assert_eq!(
        (empty.order, empty.text.as_str()),
        (report::Order::None, "")
    );
    assert_eq!(empty.encoding, report::Encoding::Unknown);
    assert_eq!(
        ordered(1, &find("empty"), None).encoding,
        report::Encoding::Unknown
    );
    // PDFium's line breaks are not the lines: none survives into the text.
    let separated = ordered(1, &find("tagged-separators"), None);
    assert!(!separated.text.contains('\r'), "{:?}", separated.text);
    assert_eq!(separated.text, "body one\nbody two\nnote");
}

#[test]
fn a_line_break_pdfium_put_inside_a_line_is_not_kept() {
    // Two runs close enough to be one line, which PDFium nonetheless separated
    // with `\r\n`: the break is inside the line, where trimming cannot reach it.
    let mut text = crate::text::PageText::default();
    let mut put = |code: u32, quad: [f32; 4]| {
        text.codes.push(code);
        text.boxes.extend_from_slice(&quad);
    };
    for (at, c) in "one".chars().enumerate() {
        let left = 72.0 + at as f32 * 5.5;
        put(c as u32, [left, 100.0, left + 5.5, 111.3]);
    }
    put(13, [0.0; 4]);
    put(10, [0.0; 4]);
    for (at, c) in "two".chars().enumerate() {
        let left = 94.0 + at as f32 * 5.5;
        put(c as u32, [left, 100.0, left + 5.5, 111.3]);
    }
    assert_eq!(super::text::ordered(1, &text, None).text, "onetwo");
}

#[test]
fn plain_text_ends_every_page_with_a_form_feed() {
    assert_eq!(
        super::text::plain(&text_sample()),
        "Quarterly review\nThe first paragraph.\n\u{c}alpha one\nbeta one\n\u{c}\u{c}"
    );
}

#[test]
fn json_output_is_ascii_and_means_the_same_text() {
    // A German path, the dash the verdict sentences use, and a character
    // outside the Basic Multilingual Plane, which needs a surrogate pair.
    let value = serde_json::json!({ "path": "C:\\Prüfung Größe.pdf", "sentence": "intact — 𝄞" });
    let text = super::ascii_json(&value).expect("encodes");
    assert!(text.is_ascii(), "{text}");
    assert!(text.contains("Pr\\u00fcfung"), "{text}");
    assert!(text.contains("\\ud834\\udd1e"), "{text}");
    let back: serde_json::Value = serde_json::from_str(&text).expect("decodes");
    assert_eq!(back, value);
}

#[test]
fn long_term_data_is_asked_for_only_with_a_timestamp() {
    let line = |extra: &[&str]| {
        let mut args: Vec<String> = ["a.pdf", "-o", "b.pdf", "--identity", "x"]
            .iter()
            .map(|a| (*a).to_string())
            .collect();
        args.extend(extra.iter().map(|a| (*a).to_string()));
        super::sign::parse(&args)
    };
    let why = line(&["--long-term"]).expect_err("refused");
    assert!(why.contains("--timestamp"), "{why}");
    let sign = line(&["--timestamp", "digicert", "--long-term"]).expect("parsed");
    assert!(sign.long_term && sign.timestamp.is_some());
    assert!(
        !line(&["--timestamp", "digicert"])
            .expect("parsed")
            .long_term
    );
}
