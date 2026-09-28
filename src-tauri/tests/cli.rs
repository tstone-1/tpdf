//! The command-line tool against real workers and real documents.
//!
//! **Its own `main`** (`harness = false` in `Cargo.toml`): signing spawns
//! workers by re-executing the current binary with `worker::WORKER_ARGV`, and in
//! here the current binary is this test. So the first thing `main` does is be
//! a worker when asked --- exactly what `cli::main` does in the shipped tool.
//!
//! Four checks, each with the control that keeps it from passing vacuously:
//!
//! 1. **`verify` agrees with the in-process reader** on every signed fixture,
//!    signature by signature --- run as the **built binary**
//!    (`CARGO_BIN_EXE_tpdf-cli`), so the parse, the worker spawn and the JSON
//!    are the shipped ones. Control: the fixtures' verdicts are required to
//!    include intact, altered, broken and weak, so a tool that answered one
//!    word for everything cannot agree.
//! 2. **A signature made through `cli::run` with a software key is intact** when
//!    the built binary reads it back, invisible and visible, on a document with
//!    an earlier signature too. The key is this file's; nothing touches a
//!    keychain or a certificate store. Controls: a key that signs the wrong
//!    digest is refused and writes nothing; so is an encrypted document.
//!    **And a timestamp on it** (`timestamp_reads_back`): a token minted by
//!    the software authority `src/integrity/test_tsa.rs` --- included here by
//!    path --- spliced in as a timestamp authority's answer would be, reads
//!    intact and attested from the built tool; damaged, of another signature,
//!    SHA-1 and unbound, it reads broken, altered, weak and unchecked, the
//!    time attested only for the two that earn it. The in-process reader
//!    agrees on every one.
//!    **And a timestamp asked for while signing** (`timestamp_when_signing`):
//!    `sign --timestamp` against a fake authority on 127.0.0.1 that mints a
//!    token over the imprint and nonce the request carried. Sound, it reads
//!    back intact and attested from the built tool, and the in-process reader
//!    agrees; every fault --- another imprint, another nonce, an SHA-1
//!    signature, a refusal, an answer too long, nobody listening --- exits 3
//!    and writes nothing. Control: an address that is not `http`/`https` is
//!    exit 2, before anything is asked.
//! 3. **The tool's own process never maps PDFium, and its workers do** (macOS:
//!    `DYLD_PRINT_LIBRARIES` on the built binary, separated by pid; both
//!    platforms: this process's own module list after `cli::run` has verified a
//!    document, with PDFium bound in-process last as the control that the list
//!    can show it).
//! 4. **The workers the tool spawns are sandboxed** (macOS: `sandbox_check` on a
//!    worker started by `Worker::spawn_shared`, the call `save::InWorker` makes,
//!    against this process as the unsandboxed control).
//! 5. **`info --json` is what the in-process reader says**, document for
//!    document and key for key: the same `cli::info::document` built from
//!    `DocumentGraph::properties`, PDFium's page sizes and `DocumentGraph::form`
//!    in this process. Controls: the fixtures must between them be tagged,
//!    encrypted, signed and carry a form, so a report that dropped any of those
//!    cannot agree; a password-protected document is `locked` without its
//!    password and described with it; and `verify` calls it `locked` too.
//! 6. **`text --json` is what the in-process extraction says**, page for page,
//!    ordered by `reading::read` --- and, independently of both, the lines each
//!    fixture's manifest records. Control: `tagged.pdf`'s first page with its
//!    tags taken away reads the margin note before the second heading, so a
//!    tool that ignored the tags cannot agree with the manifest. `--pages`
//!    selects, and refuses a page past the end (3) and a backwards range (2).
//! 7. **Beside `pdftotext`**, when Poppler is installed: word overlap and word
//!    order per fixture, printed, never counted. A second reader with its own
//!    reading-order rules is expected to differ, most on a tagged page.
//! 8. **`redact`** (`cli/redact.rs`): what it matches is gone for three readers
//!    and what it does not match is not; a dry run predicts the write; the
//!    verdict and every reason equal the application's path driven in this
//!    process; refusals write nothing. Its containment is 3's, below.
//!
//! Fixtures under `testdata/` are generated, not committed (`BUILD.md`); a
//! check whose fixtures are absent says `[SKIP]` and names them, and is never
//! counted as a pass.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr as _;

use der::asn1::{BitString, GeneralizedTime, ObjectIdentifier};
use der::Encode as _;
use ecdsa::signature::hazmat::PrehashSigner as _;
use sha2_10::Digest as _;
use tpdf_lib::cli::{self, Env, Held, Store};
use tpdf_lib::sign_cms::{Key, KeyKind};
use tpdf_lib::{worker, worker_child};
use x509_cert::certificate::{TbsCertificate, Version};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use x509_cert::time::{Time, Validity};

// `fields` and `fill`. Under `tests/cli/` and named by path, because a file
// directly in `tests/` would be a test target of its own to cargo.
#[path = "cli/forms.rs"]
mod forms;
// `redact`, for the same reason.
#[path = "cli/redact.rs"]
mod redact;
// The software timestamp authority the library's unit tests mint tokens with,
// included by path because this binary links the library as a release
// consumer does and cannot see a `#[cfg(test)]` module (`test_tsa.rs`'s note).
#[path = "../src/integrity/test_tsa.rs"]
mod test_tsa;

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    // `redact` runs the OCR gate, whose engine is this binary re-executed with
    // its own marker --- and so does the application's path the parity check
    // drives in this process.
    tpdf_lib::ocr_worker::child_main_if_asked(&argv);
    if argv.get(1).map(String::as_str) == Some(worker::WORKER_ARGV) {
        worker_child::main(&argv);
    }
    // The order is load-bearing: 3 asserts this process has not mapped PDFium,
    // and 5 and 6 map it here to extract in-process, so they come after.
    let checks: [Check; 15] = [
        ("verify agrees with the in-process reader", verify_agrees),
        (
            "a signature made through the tool reads back intact",
            sign_reads_back,
        ),
        (
            "a timestamp minted by the test authority reads back through the tool",
            timestamp_reads_back,
        ),
        (
            "revocation data a document carries reads back through the tool",
            revocation_reads_back,
        ),
        (
            "sign --timestamp asks a local authority, and writes nothing when it fails",
            timestamp_when_signing,
        ),
        ("the tool's process never maps PDFium", never_maps_pdfium),
        ("the tool's workers are sandboxed", workers_are_sandboxed),
        ("info agrees with the in-process reader", info_agrees),
        ("text agrees with the in-process extraction", text_agrees),
        ("text beside pdftotext, for information", beside_pdftotext),
        (
            "fields agrees with the in-process form reader",
            forms::fields_agree,
        ),
        (
            "fill writes every answer and reads it back",
            forms::fill_round_trips,
        ),
        (
            "redact removes what it finds and nothing else",
            redact::removes_what_it_finds,
        ),
        (
            "redact's verdict is the application's",
            redact::verdict_is_the_applications,
        ),
        (
            "redact's refusals write nothing",
            redact::refusals_write_nothing,
        ),
    ];
    let mut report = Report::default();
    for (name, check) in checks {
        println!("--- {name}");
        check(&mut report);
    }
    println!(
        "\ncli: {} passed, {} failed, {} skipped",
        report.passed, report.failed, report.skipped
    );
    std::process::exit(i32::from(report.failed > 0 || report.passed == 0));
}

/// One named check over the shared report.
type Check = (&'static str, fn(&mut Report));

#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, what: &str, ok: bool, detail: &str) {
        if ok {
            self.passed += 1;
            println!("[PASS] {what}");
        } else {
            self.failed += 1;
            println!("[FAIL] {what}: {detail}");
        }
    }
    fn skip(&mut self, what: &str, why: &str) {
        self.skipped += 1;
        println!("[SKIP] {what}: {why}");
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> Option<PathBuf> {
    let path = root().join("testdata").join(name);
    path.exists().then_some(path)
}

fn library_dir() -> PathBuf {
    root().join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpdf-cli-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// The built tool, run with `args`: exit code, stdout, stderr.
fn tool(args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tpdf-cli"))
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("the built tool runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// --- 1 ----------------------------------------------------------------------

const SIGNED: [&str; 15] = [
    "incr-signed.pdf",
    "incr-doc-timestamped.pdf",
    "incr-two-signers.pdf",
    "incr-certified-1.pdf",
    "incr-certified-2.pdf",
    "incr-certified-3.pdf",
    "incr-certified-3-indirect.pdf",
    "incr-timestamped.pdf",
    "incr-ber.pdf",
    "signed-altered.pdf",
    "signed-broken.pdf",
    "signed-p256.pdf",
    "signed-p384.pdf",
    "signed-pss.pdf",
    "signed-sha1.pdf",
];

/// One signature's verdicts, as `(field, verdict, why, standing, doubt,
/// timestamp)` --- the last the token's verdict, reason, whether its time is
/// attested and its authority's standing, joined, or empty with no token.
/// A signature as both readers report it: field, verdict, reason, standing,
/// doubt, the timestamp's line, and the revocation line (standing/basis).
type Line = (String, String, String, String, String, String, String);

fn in_process(path: &Path) -> Vec<Line> {
    use tpdf_lib::save::Verifier as _;
    let mut file = std::fs::File::open(path).expect("fixture");
    let len = usize::try_from(file.metadata().expect("len").len()).expect("len");
    tpdf_lib::save::Here
        .signatures(&mut file, len)
        .expect("the in-process reader reads the fixture")
        .into_iter()
        .filter(|s| s.signed)
        .map(|s| {
            let text = |v: serde_json::Value| match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            let integrity = s.integrity.unwrap_or_default();
            let (standing, doubt) = s.trust.map_or((String::new(), String::new()), |t| {
                (
                    text(serde_json::to_value(t.standing).expect("json")),
                    text(serde_json::to_value(t.why).expect("json")),
                )
            });
            let revoked = |r: Option<tpdf_lib::revocation::Revocation>| {
                r.map_or_else(String::new, |r| {
                    revocation_line(
                        &text(serde_json::to_value(r.standing).expect("json")),
                        &text(serde_json::to_value(r.basis).expect("json")),
                    )
                })
            };
            let stamp = s.timestamp.map_or_else(String::new, |t| {
                let verdict = t.integrity.unwrap_or_default();
                format!(
                    "{}/{}",
                    stamp_line(
                        &text(serde_json::to_value(verdict.verdict).expect("json")),
                        &text(serde_json::to_value(verdict.why).expect("json")),
                        t.attested,
                        &t.trust.map_or_else(String::new, |t| {
                            text(serde_json::to_value(t.standing).expect("json"))
                        }),
                    ),
                    revoked(t.revocation)
                )
            });
            let revocation = revoked(s.revocation);
            (
                s.field,
                text(serde_json::to_value(integrity.verdict).expect("json")),
                text(serde_json::to_value(integrity.why).expect("json")),
                standing,
                doubt,
                stamp,
                revocation,
            )
        })
        .collect()
}

fn from_json(file: &serde_json::Value) -> Vec<Line> {
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    file["signatures"]
        .as_array()
        .expect("signatures")
        .iter()
        .map(|s| {
            let t = &s["timestamp"];
            let revoked = |r: &serde_json::Value| {
                if r.is_null() {
                    String::new()
                } else {
                    revocation_line(&text(&r["standing"]), &text(&r["basis"]))
                }
            };
            let stamp = if t.is_null() {
                String::new()
            } else {
                format!(
                    "{}/{}",
                    stamp_line(
                        &text(&t["integrity"]["verdict"]),
                        &text(&t["integrity"]["why"]),
                        t["attested"].as_bool().unwrap_or_default(),
                        &text(&t["trust"]["standing"]),
                    ),
                    revoked(&t["revocation"])
                )
            };
            (
                text(&s["field"]),
                text(&s["integrity"]["verdict"]),
                text(&s["integrity"]["why"]),
                text(&s["trust"]["standing"]),
                text(&s["trust"]["why"]),
                stamp,
                revoked(&s["revocation"]),
            )
        })
        .collect()
}

/// A revocation answer's line, as both readers spell it.
fn revocation_line(standing: &str, basis: &str) -> String {
    format!("revocation={standing}@{basis}")
}

/// A token's line, as both readers spell it.
fn stamp_line(verdict: &str, why: &str, attested: bool, standing: &str) -> String {
    format!("{verdict}/{why}/attested={attested}/{standing}")
}

fn verify_agrees(report: &mut Report) {
    let present: Vec<PathBuf> = SIGNED.iter().filter_map(|n| fixture(n)).collect();
    let missing: Vec<&str> = SIGNED
        .iter()
        .copied()
        .filter(|n| fixture(n).is_none())
        .collect();
    if present.is_empty() {
        report.skip(
            "verify against the signed fixtures",
            "none generated --- `python3 scripts/ci_fixtures.py --signed`",
        );
        return;
    }
    if !missing.is_empty() {
        println!("(not generated, left out: {missing:?})");
    }
    let mut args = vec!["verify", "--json"];
    let shown: Vec<String> = present.iter().map(|p| p.display().to_string()).collect();
    args.extend(shown.iter().map(String::as_str));
    let (code, stdout, stderr) = tool(&args, &[]);
    report.check(
        "verify exits 0 when every document was read",
        code == 0,
        &stderr,
    );
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) else {
        report.check("verify --json prints one JSON document", false, &stdout);
        return;
    };
    report.check(
        "the document says schema 1 and command verify",
        json["schema"] == 1 && json["command"] == "verify",
        &json.to_string(),
    );
    let files = json["files"].as_array().cloned().unwrap_or_default();
    report.check(
        "one entry per document, in order",
        files.len() == present.len()
            && files
                .iter()
                .zip(&shown)
                .all(|(f, p)| f["path"] == p.as_str()),
        &format!("{} entries for {} documents", files.len(), present.len()),
    );
    let mut verdicts = std::collections::BTreeSet::new();
    for (file, path) in files.iter().zip(&present) {
        let theirs = from_json(file);
        let ours = in_process(path);
        verdicts.extend(theirs.iter().map(|l| l.1.clone()));
        report.check(
            &format!(
                "{}: the tool reads what the in-process reader reads",
                path.display()
            ),
            !ours.is_empty() && theirs == ours,
            &format!("tool {theirs:?}\n       here {ours:?}"),
        );
    }
    // The control: a tool that said one word for everything would agree with
    // nothing here, but only if the fixtures ask for several words.
    let wanted = ["intact", "altered", "broken", "weak"];
    let all_present = SIGNED.iter().all(|n| fixture(n).is_some());
    if all_present {
        report.check(
            "the fixtures' verdicts include intact, altered, broken and weak",
            wanted.iter().all(|w| verdicts.contains(*w)),
            &format!("{verdicts:?}"),
        );
    } else {
        report.skip(
            "the verdict-variety control",
            "not every signed fixture is generated",
        );
    }
    // Every fixture is signed and none chains to a root this machine trusts,
    // so --strict must refuse, with the same document on stdout.
    let mut strict = args.clone();
    strict.push("--strict");
    let (code, stdout, _) = tool(&strict, &[]);
    report.check(
        "--strict exits 1 on signatures that are not trusted",
        code == 1 && stdout.contains("\"strict_passed\": false"),
        &format!("exit {code}"),
    );
}

// --- 2b: a timestamp through the tool ----------------------------------------

/// The one signature's `/ByteRange` numbers in `bytes`.
fn byte_range(bytes: &[u8]) -> [usize; 4] {
    // tpdf writes `/ByteRange[` with no space, pyHanko with one.
    let at = bytes
        .windows(10)
        .position(|w| w == b"/ByteRange")
        .expect("a /ByteRange");
    let open = at + bytes[at..].iter().position(|b| *b == b'[').expect("[");
    let close = open + bytes[open..].iter().position(|b| *b == b']').expect("]");
    let numbers: Vec<usize> = std::str::from_utf8(&bytes[open + 1..close])
        .expect("ascii")
        .split_whitespace()
        .map(|n| n.parse().expect("an integer"))
        .collect();
    numbers.try_into().expect("four numbers")
}

/// `bytes`, one signature, with `token` added as that signature's
/// `id-aa-timeStampToken` unsigned attribute --- where a timestamp authority's
/// answer goes, and outside everything the signature covers, so the signature
/// is untouched. `token_for` is handed the signature value and mints the token.
fn with_timestamp(bytes: &[u8], token_for: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::{SignedData, SignerInfos};
    use der::{Decode as _, Encode as _};

    let [_, first, second, _] = byte_range(bytes);
    let hex = &bytes[first + 1..second - 1];
    let raw: Vec<u8> = hex
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("hex"))
        .collect();
    let mut reader = der::SliceReader::new(&raw).expect("a reader");
    let info = ContentInfo::decode(&mut reader).expect("a CMS, then padding");
    let mut signed: SignedData = info.content.decode_as().expect("signed data");
    let mut signer = signed.signer_infos.0.as_slice()[0].clone();
    let token = token_for(signer.signature.as_bytes());
    let attribute = x509_cert::attr::Attribute {
        oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.14"),
        values: der::asn1::SetOfVec::try_from(vec![der::Any::from_der(&token).expect("a token")])
            .expect("a set"),
    };
    signer.unsigned_attrs = Some(der::asn1::SetOfVec::try_from(vec![attribute]).expect("a set"));
    signed.signer_infos = SignerInfos::try_from(vec![signer]).expect("signer infos");
    let mut encoded = ContentInfo {
        content_type: info.content_type,
        content: der::Any::encode_from(&signed).expect("signed data"),
    }
    .to_der()
    .expect("a CMS");
    assert!(
        encoded.len() <= raw.len(),
        "the token fits the reserved span"
    );
    encoded.resize(raw.len(), 0);
    let digits: String = encoded.iter().map(|b| format!("{b:02X}")).collect();
    let mut out = bytes.to_vec();
    out[first + 1..second - 1].copy_from_slice(digits.as_bytes());
    out
}

/// A token minted by `test_tsa` on a signature `cli::run` made, spliced in as
/// the authority's answer would be, and read back by the built tool.
///
/// What a later increment will do over the network, done here by hand, so the
/// reading half is proved before the requesting half exists: the tool reports
/// the token intact and its time attested, the in-process reader agrees, and
/// a token whose own signature was damaged reads broken with no attested time
/// and no authority standing. The authority's root is in no store, so its
/// standing through the system store is a missing link --- the token carries
/// the authority's certificate and not the root.
fn timestamp_reads_back(report: &mut Report) {
    use test_tsa::{mint_with, Faults, Imprint, TestTsa};

    let now = now();
    let dir = scratch("timestamp");
    let plain = dir.join("plain.pdf");
    let signed = dir.join("signed.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let s = |p: &Path| p.display().to_string();
    let args = strings(&["sign", &s(&plain), "-o", &s(&signed), "--identity", SUBJECT]);
    let (code, _, stderr) = signs(&args, &store, now);
    if code != 0 {
        report.check("the document to timestamp is signed", false, &stderr);
        return;
    }
    let bytes = std::fs::read(&signed).expect("signed");
    let tsa = TestTsa::new();
    let sha256 = Imprint::Sha256;
    let cases = [
        ("a sound token", sha256, Faults::default(), "intact", true),
        (
            "a token whose own signature is damaged",
            sha256,
            Faults {
                corrupt_signature: true,
                ..Faults::default()
            },
            "broken",
            false,
        ),
        (
            "a token of another signature",
            sha256,
            Faults {
                wrong_imprint: true,
                ..Faults::default()
            },
            "altered",
            false,
        ),
        (
            "a token whose imprint is SHA-1",
            Imprint::Sha1,
            Faults::default(),
            "weak",
            true,
        ),
        (
            "a token that does not bind its certificate",
            sha256,
            Faults {
                binding: test_tsa::Binding::Neither,
                ..Faults::default()
            },
            "unchecked",
            false,
        ),
    ];
    for (n, (what, hash, faults, verdict, attested)) in cases.iter().enumerate() {
        let stamped = dir.join(format!("stamped-{n}.pdf"));
        std::fs::write(
            &stamped,
            with_timestamp(&bytes, |value| {
                mint_with(
                    *hash,
                    &hash.digest(value),
                    Some(&[0x5a; 8]),
                    now,
                    &tsa,
                    faults,
                )
            }),
        )
        .expect("stamped");
        let (code, stdout, stderr) = tool(&["verify", "--json", &s(&stamped)], &[]);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let signature = &json["files"][0]["signatures"][0];
        let stamp = &signature["timestamp"];
        report.check(
            &format!("{what}: the signature itself is still intact"),
            code == 0 && signature["integrity"]["verdict"] == "intact",
            &format!("exit {code}: {stderr}{stdout}"),
        );
        report.check(
            &format!("{what}: the token reads {verdict}, attested={attested}"),
            stamp["integrity"]["verdict"] == *verdict && stamp["attested"] == *attested,
            &stamp.to_string(),
        );
        report.check(
            &format!("{what}: its authority is asked about exactly when the time is attested"),
            if *attested {
                stamp["trust"]["standing"] == "untrusted" && stamp["trust"]["why"] == "incomplete"
            } else {
                stamp["trust"].is_null()
            },
            &stamp["trust"].to_string(),
        );
        report.check(
            &format!("{what}: the tool reads what the in-process reader reads"),
            from_json(&json["files"][0]) == in_process(&stamped),
            &format!(
                "tool {:?}\n       here {:?}",
                from_json(&json["files"][0]),
                in_process(&stamped)
            ),
        );
    }
}

/// A timestamped signature with a `/DSS` appended the way a PAdES B-LT writer
/// appends one --- the authority's root, a response about the authority's
/// certificate, and a list from the root --- read back by the built tool.
///
/// The authority's root is in no store, so the authority reads untrusted and
/// the signer is judged now, on its own claimed date. The authority's
/// revocation is judged at the time its token states, from the document's
/// data alone: `good`. The signer's certificate is the tool's test identity,
/// about which the document carries nothing: `none`, which the sentence must
/// call not checked. A response whose signature was damaged reads unchecked.
fn revocation_reads_back(report: &mut Report) {
    use test_tsa::{
        mint, mint_crl, mint_ocsp, with_dss, CrlFaults, Imprint, OcspFaults, Responder, Status,
        TestCa, TestTsa,
    };

    let now = now();
    let dir = scratch("revocation");
    let plain = dir.join("plain.pdf");
    let signed = dir.join("signed.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let s = |p: &Path| p.display().to_string();
    let args = strings(&["sign", &s(&plain), "-o", &s(&signed), "--identity", SUBJECT]);
    let (code, _, stderr) = signs(&args, &store, now);
    if code != 0 {
        report.check(
            "the document to add validation data to is signed",
            false,
            &stderr,
        );
        return;
    }
    let tsa = TestTsa::new();
    let root = TestCa::of_tsa();
    let stamped = with_timestamp(&std::fs::read(&signed).expect("signed"), |value| {
        mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(value),
            None,
            now,
            &tsa,
        )
    });
    let response = |faults: &OcspFaults| {
        mint_ocsp(
            &tsa.certificate,
            &root,
            Status::Good,
            now - 3_600,
            Some(now + 7 * 86_400),
            Responder::Issuer,
            faults,
        )
    };
    let list = mint_crl(
        &root,
        &[],
        now - 3_600,
        Some(now + 7 * 86_400),
        &CrlFaults::default(),
    );
    for (what, faults, expected) in [
        ("a sound response and list", OcspFaults::default(), "good"),
        (
            "a response whose signature is damaged, beside a sound list",
            OcspFaults {
                corrupt_signature: true,
                ..OcspFaults::default()
            },
            // The list still answers; a damaged response beside it is not a
            // revocation, so the list's good stands.
            "good",
        ),
    ] {
        let path = dir.join(format!("b-lt-{}.pdf", faults.corrupt_signature));
        std::fs::write(
            &path,
            with_dss(
                &stamped,
                std::slice::from_ref(&root.certificate),
                &[response(&faults)],
                std::slice::from_ref(&list),
            ),
        )
        .expect("written");
        let (code, stdout, stderr) = tool(&["verify", "--json", &s(&path)], &[]);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let signature = &json["files"][0]["signatures"][0];
        let authority = &signature["timestamp"]["revocation"];
        report.check(
            &format!(
                "{what}: the authority's revocation reads {expected}, at the time its token states"
            ),
            code == 0 && authority["standing"] == expected && authority["basis"] == "stated",
            &format!("exit {code}: {stderr}{authority}"),
        );
        report.check(
            &format!(
                "{what}: the signer's certificate, about which nothing is carried, reads none"
            ),
            signature["revocation"]["standing"] == "none"
                && signature["revocation"]["sentence"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("not checked")),
            &signature["revocation"].to_string(),
        );
        report.check(
            &format!("{what}: the tool reads what the in-process reader reads"),
            from_json(&json["files"][0]) == in_process(&path),
            &format!(
                "tool {:?}
       here {:?}",
                from_json(&json["files"][0]),
                in_process(&path)
            ),
        );
    }
    // A response alone, damaged: nothing that checks out answers.
    let path = dir.join("b-lt-damaged-alone.pdf");
    std::fs::write(
        &path,
        with_dss(
            &stamped,
            std::slice::from_ref(&root.certificate),
            &[response(&OcspFaults {
                corrupt_signature: true,
                ..OcspFaults::default()
            })],
            &[],
        ),
    )
    .expect("written");
    let (code, stdout, _) = tool(&["verify", "--json", &s(&path)], &[]);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let authority = &json["files"][0]["signatures"][0]["timestamp"]["revocation"];
    report.check(
        "a damaged response alone reads unchecked, signature",
        code == 0 && authority["standing"] == "unchecked" && authority["why"] == "signature",
        &authority.to_string(),
    );
}

// --- 2c: a timestamp asked for while signing --------------------------------

/// What the fake authority does with one request.
enum Answer {
    /// A token minted over what was asked, with these faults, granted.
    Token(test_tsa::Faults),
    /// A sound token over what was asked carrying another nonce.
    OtherNonce,
    /// A sound token under a rejection status.
    Rejected,
    /// A body longer than any timestamp.
    Long,
}

/// The imprint and nonce a `TimeStampReq` carries, read by walking its DER
/// by hand: this binary cannot see the library's own request type, and a
/// reader that shares no code with the writer is the better check anyway.
fn asked(request: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use der::{Decode as _, Reader as _, Tagged as _};
    let outer = der::asn1::AnyRef::from_der(request).ok()?;
    let mut fields = der::SliceReader::new(outer.value()).ok()?;
    let _version = der::asn1::AnyRef::decode(&mut fields).ok()?;
    let imprint = der::asn1::AnyRef::decode(&mut fields).ok()?;
    let mut inner = der::SliceReader::new(imprint.value()).ok()?;
    let _algorithm = der::asn1::AnyRef::decode(&mut inner).ok()?;
    let digest = der::asn1::OctetStringRef::decode(&mut inner).ok()?;
    let mut nonce = None;
    while !fields.is_finished() {
        let field = der::asn1::AnyRef::decode(&mut fields).ok()?;
        if field.tag() == der::Tag::Integer {
            nonce = Some(field.value().to_vec());
        }
    }
    Some((digest.as_bytes().to_vec(), nonce?))
}

/// A fake authority on 127.0.0.1 answering one request: its URL.
fn authority(answer: Answer) -> String {
    use std::io::{BufRead as _, Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = std::io::BufReader::new(stream.try_clone().expect("a clone"));
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end().to_ascii_lowercase();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("content-length:") {
                length = n.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let Some((digest, nonce)) = asked(&body) else {
            return;
        };
        let tsa = test_tsa::TestTsa::new();
        let mint = |faults: &test_tsa::Faults, nonce: &[u8]| {
            test_tsa::mint_with(
                test_tsa::Imprint::Sha256,
                &digest,
                Some(nonce),
                now(),
                &tsa,
                faults,
            )
        };
        let reply = match answer {
            Answer::Token(faults) => test_tsa::granted(&mint(&faults, &nonce)),
            Answer::OtherNonce => {
                let mut other = nonce.clone();
                other[0] ^= 0x01;
                test_tsa::granted(&mint(&test_tsa::Faults::default(), &other))
            }
            Answer::Rejected => {
                // PKIStatusInfo { status 2 }, then the token.
                let token = mint(&test_tsa::Faults::default(), &nonce);
                let mut out = vec![0x30, 0x82, 0, 0, 0x30, 0x03, 0x02, 0x01, 0x02];
                out.extend(&token);
                let len = out.len() - 4;
                out[2] = (len >> 8) as u8;
                out[3] = len as u8;
                out
            }
            Answer::Long => vec![0x30; 70 * 1024],
        };
        let mut stream = stream;
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/timestamp-reply\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(&reply);
    });
    format!("http://127.0.0.1:{port}/tsa")
}

fn timestamp_when_signing(report: &mut Report) {
    let now = now();
    let dir = scratch("stamping");
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let s = |p: &Path| p.display().to_string();

    // Sound: written, and read back with an attested time.
    let out = dir.join("stamped.pdf");
    let url = authority(Answer::Token(test_tsa::Faults::default()));
    let (code, stdout, stderr) = signs(
        &strings(&[
            "sign",
            &s(&plain),
            "-o",
            &s(&out),
            "--identity",
            SUBJECT,
            "--timestamp",
            &url,
            "--json",
        ]),
        &store,
        now,
    );
    report.check(
        "a sound timestamp: sign exits 0",
        code == 0,
        &format!("exit {code}: {stderr}"),
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "a sound timestamp: the summary says the time is attested",
        json["summary"]
            .as_str()
            .is_some_and(|t| t.contains(" Timestamp: ") && t.contains("attested by")),
        &stdout,
    );
    match read_back(&out) {
        Err(why) => report.check("a sound timestamp: the tool reads it back", false, &why),
        Ok((_, json)) => {
            let signature = &json["files"][0]["signatures"][0];
            let stamp = &signature["timestamp"];
            report.check(
                "a sound timestamp: the signature is intact and its timestamp intact and attested",
                signature["integrity"]["verdict"] == "intact"
                    && stamp["integrity"]["verdict"] == "intact"
                    && stamp["attested"] == true
                    && stamp["authority"] == test_tsa::AUTHORITY,
                &signature.to_string(),
            );
            report.check(
                "a sound timestamp: the tool reads what the in-process reader reads",
                from_json(&json["files"][0]) == in_process(&out),
                &format!(
                    "{:?} / {:?}",
                    from_json(&json["files"][0]),
                    in_process(&out)
                ),
            );
        }
    }

    // Every failure: 3, and nothing written.
    let refused = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let nobody = format!(
        "http://127.0.0.1:{}/",
        refused.local_addr().expect("an address").port()
    );
    drop(refused);
    let failures: Vec<(&str, String)> = vec![
        (
            "a token of another signature",
            authority(Answer::Token(test_tsa::Faults {
                wrong_imprint: true,
                ..test_tsa::Faults::default()
            })),
        ),
        (
            "a token answering another request",
            authority(Answer::OtherNonce),
        ),
        (
            "a token signed under SHA-1",
            authority(Answer::Token(test_tsa::Faults {
                sha1_signature: true,
                ..test_tsa::Faults::default()
            })),
        ),
        ("a refusal carrying a token", authority(Answer::Rejected)),
        (
            "an answer longer than any timestamp",
            authority(Answer::Long),
        ),
        ("nobody listening", nobody),
    ];
    for (n, (what, url)) in failures.iter().enumerate() {
        let out = dir.join(format!("refused-{n}.pdf"));
        let (code, stdout, stderr) = signs(
            &strings(&[
                "sign",
                &s(&plain),
                "-o",
                &s(&out),
                "--identity",
                SUBJECT,
                "--timestamp",
                url,
            ]),
            &store,
            now,
        );
        report.check(
            &format!("{what}: exit 3, nothing written, and says so"),
            code == 3
                && !out.exists()
                && stdout.is_empty()
                && stderr.contains("nothing was written")
                && stderr.contains("127.0.0.1"),
            &format!("exit {code}, exists {}: {stderr}", out.exists()),
        );
    }

    // Control: an address tpdf does not ask is a malformed line.
    let out = dir.join("ftp.pdf");
    let (code, _, stderr) = signs(
        &strings(&[
            "sign",
            &s(&plain),
            "-o",
            &s(&out),
            "--identity",
            SUBJECT,
            "--timestamp",
            "ftp://127.0.0.1/",
        ]),
        &store,
        now,
    );
    report.check(
        "control: an ftp: authority is exit 2 and nothing is written",
        code == 2 && !out.exists(),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 2 ----------------------------------------------------------------------

/// A P-256 key this test holds, which only a test may.
struct Soft(p256::ecdsa::SigningKey);

impl Key for Soft {
    fn sign_digest(&self, _kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let value: p256::ecdsa::Signature =
            self.0.sign_prehash(digest).map_err(|e| e.to_string())?;
        Ok(value.to_der().as_bytes().to_vec())
    }
}

/// The same key, signing a digest one bit off: the wrong-digest control.
struct Misdirected(p256::ecdsa::SigningKey);

impl Key for Misdirected {
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let mut other = *digest;
        other[0] ^= 1;
        Soft(self.0.clone()).sign_digest(kind, &other)
    }
}

const SUBJECT: &str = "tpdf CLI test signer - not a real identity";

fn key() -> p256::ecdsa::SigningKey {
    p256::ecdsa::SigningKey::from_bytes(&[7u8; 32].into()).expect("a scalar")
}

/// A self-issued certificate for [`key`], valid around `now`, stating no usage.
fn certificate(now: u64) -> Vec<u8> {
    let point = key().verifying_key().to_encoded_point(false);
    let spki = SubjectPublicKeyInfoOwned {
        algorithm: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.10045.2.1"),
            parameters: Some(
                der::Any::encode_from(&ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7"))
                    .expect("curve"),
            ),
        },
        subject_public_key: BitString::from_bytes(point.as_bytes()).expect("bits"),
    };
    let ecdsa_sha256 = AlgorithmIdentifierOwned {
        oid: ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2"),
        parameters: None,
    };
    let time = |s: u64| {
        Time::GeneralTime(
            GeneralizedTime::from_unix_duration(std::time::Duration::from_secs(s)).expect("time"),
        )
    };
    let name = Name::from_str(&format!("CN={SUBJECT}")).expect("name");
    let tbs = TbsCertificate {
        version: Version::V3,
        serial_number: SerialNumber::new(&[1]).expect("serial"),
        signature: ecdsa_sha256.clone(),
        issuer: name.clone(),
        validity: Validity {
            not_before: time(now - 86_400),
            not_after: time(now + 86_400 * 30),
        },
        subject: name,
        subject_public_key_info: spki,
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions: None,
    };
    let digest: [u8; 32] = sha2_10::Sha256::digest(tbs.to_der().expect("tbs")).into();
    let value = Soft(key())
        .sign_digest(KeyKind::P256, &digest)
        .expect("signed");
    x509_cert::Certificate {
        tbs_certificate: tbs,
        signature_algorithm: ecdsa_sha256,
        signature: BitString::from_bytes(&value).expect("bits"),
    }
    .to_der()
    .expect("certificate")
}

/// The store the tool is handed: this file's key, and a saved image.
struct TestStore {
    certificate: Vec<u8>,
    misdirected: bool,
}

impl Store for TestStore {
    fn identities(&self) -> Result<Vec<Held>, String> {
        let key: Box<dyn Key> = if self.misdirected {
            Box::new(Misdirected(key()))
        } else {
            Box::new(Soft(key()))
        };
        Ok(vec![Held {
            certificate: self.certificate.clone(),
            chain: Vec::new(),
            key,
        }])
    }

    fn saved_image(&self) -> Result<Option<tpdf_lib::signature::Image>, String> {
        let (width, height) = (64u32, 32u32);
        let mut rgba = Vec::new();
        for y in 0..height {
            for _ in 0..width {
                let ink = (12..20).contains(&y);
                rgba.extend_from_slice(if ink {
                    &[20, 30, 120, 255]
                } else {
                    &[0, 0, 0, 0]
                });
            }
        }
        Ok(Some(tpdf_lib::signature::Image {
            width,
            height,
            rgba,
        }))
    }
}

/// A one-page document with nothing in it, saved by `lopdf`.
fn plain_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 10 10 l S".to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 400.into(), 300.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// Runs `cli::run` in this process against `store`: exit code, stdout, stderr.
fn signs(args: &[String], store: &dyn Store, now: u64) -> (i32, String, String) {
    let env = Env {
        store,
        library_dir: library_dir(),
        now,
        program: "tpdf".into(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::run(args, &env, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

/// The built tool's verdict lines for `path`, as `(field, verdict)`.
fn read_back(path: &Path) -> Result<(Vec<(String, String)>, serde_json::Value), String> {
    let (code, stdout, stderr) = tool(&["verify", "--json", &path.display().to_string()], &[]);
    if code != 0 {
        return Err(format!("verify exited {code}: {stderr}"));
    }
    let json: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| e.to_string())?;
    let lines = from_json(&json["files"][0])
        .into_iter()
        .map(|l| (l.0, l.1))
        .collect();
    Ok((lines, json))
}

fn sign_reads_back(report: &mut Report) {
    let now = now();
    let dir = scratch("sign");
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let s = |p: &Path| p.display().to_string();

    let mut cases: Vec<(String, PathBuf, Vec<&str>, usize)> = vec![
        (
            "invisible, on a plain document".into(),
            plain.clone(),
            vec![],
            1,
        ),
        (
            "visible, with the saved image, a reason and a location".into(),
            plain.clone(),
            vec![
                "--visible",
                "--rect",
                "40,40,240,80",
                "--reason",
                "Approved",
                "--location",
                "Hamburg",
            ],
            1,
        ),
        (
            "visible, words only, two lines, on page 1 named".into(),
            plain.clone(),
            vec![
                "--visible",
                "--page",
                "1",
                "--rect",
                "20,200,120,60",
                "--no-image",
                "--lines",
                "name,date",
            ],
            1,
        ),
    ];
    match fixture("incr-signed.pdf") {
        Some(signed) => cases.push((
            "invisible, after an earlier signature".into(),
            signed,
            vec![],
            2,
        )),
        None => report.skip(
            "signing after an earlier signature",
            "incr-signed.pdf is not generated",
        ),
    }

    for (n, (what, input, extra, count)) in cases.iter().enumerate() {
        let out = dir.join(format!("signed-{n}.pdf"));
        let mut args = strings(&[
            "sign",
            &s(input),
            "-o",
            &s(&out),
            "--identity",
            SUBJECT,
            "--json",
        ]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        let (code, stdout, stderr) = signs(&args, &store, now);
        report.check(&format!("{what}: sign exits 0"), code == 0, &stderr);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let field = json["field"].as_str().unwrap_or_default().to_string();
        report.check(
            &format!("{what}: the report names the new field and says it is intact"),
            !field.is_empty()
                && json["summary"]
                    .as_str()
                    .is_some_and(|t| t.starts_with(&format!("Signed as {field}"))),
            &stdout,
        );
        match read_back(&out) {
            Err(why) => report.check(&format!("{what}: the tool reads it back"), false, &why),
            Ok((lines, json)) => {
                report.check(
                    &format!("{what}: {count} signature(s), every one intact, ours among them"),
                    lines.len() == *count
                        && lines.iter().all(|(_, v)| v == "intact")
                        && lines.iter().any(|(f, _)| *f == field),
                    &format!("{lines:?}"),
                );
                if extra.contains(&"--reason") {
                    let file = std::fs::read(&out).expect("output");
                    let found = tpdf_lib::docinfo::scan(&file, 1, None).expect("scan");
                    let ours = found.signatures.iter().find(|s| s.field == field);
                    report.check(
                        &format!("{what}: /Reason and /Location are what was given"),
                        ours.is_some_and(|s| s.reason == "Approved" && s.location == "Hamburg"),
                        &format!("{:?}", ours.map(|s| (&s.reason, &s.location))),
                    );
                }
                let _ = json;
            }
        }
        // The original is never written.
        report.check(
            &format!("{what}: the input is unchanged"),
            *count > 1 || std::fs::read(input).expect("input") == plain_pdf(),
            "the input changed",
        );
    }

    // Controls. A key that signs the wrong digest: `finish` refuses, 3,
    // and nothing is written.
    let wrong = TestStore {
        certificate: certificate(now),
        misdirected: true,
    };
    let out = dir.join("misdirected.pdf");
    let (code, _, stderr) = signs(
        &strings(&["sign", &s(&plain), "-o", &s(&out), "--identity", SUBJECT]),
        &wrong,
        now,
    );
    report.check(
        "control: a key that signs the wrong digest is refused and nothing is written",
        code == 3 && !out.exists() && stderr.contains("did not find it intact"),
        &format!("exit {code}, exists {}, {stderr}", out.exists()),
    );

    // A document the worker refuses to sign.
    match fixture("incr-encrypted-open.pdf") {
        Some(locked) => {
            let out = dir.join("encrypted.pdf");
            let (code, _, stderr) = signs(
                &strings(&["sign", &s(&locked), "-o", &s(&out), "--identity", SUBJECT]),
                &store,
                now,
            );
            report.check(
                "control: an encrypted document is refused by the worker, 3, nothing written",
                code == 3 && !out.exists() && stderr.contains("encrypted"),
                &format!("exit {code}: {stderr}"),
            );
        }
        None => report.skip(
            "the encrypted-document control",
            "incr-encrypted-open.pdf is not generated",
        ),
    }

    // Not a PDF at all.
    let junk = dir.join("junk.pdf");
    std::fs::write(&junk, b"this is not a PDF").expect("junk");
    let out = dir.join("junk-signed.pdf");
    let (code, _, stderr) = signs(
        &strings(&["sign", &s(&junk), "-o", &s(&out), "--identity", SUBJECT]),
        &store,
        now,
    );
    report.check(
        "control: a file that is not a PDF is refused by the worker, 3, nothing written",
        code == 3 && !out.exists() && stderr.contains("not a PDF"),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 3 ----------------------------------------------------------------------

fn pdfium_in(images: &[String]) -> bool {
    images
        .iter()
        .any(|image| image.to_ascii_lowercase().contains("pdfium"))
}

fn never_maps_pdfium(report: &mut Report) {
    let dir = scratch("maps");
    let document = dir.join("plain.pdf");
    std::fs::write(&document, plain_pdf()).expect("document");
    let shown = document.display().to_string();
    // A form, and answers for it, for `fields` and `fill`.
    let form = dir.join("form.pdf");
    std::fs::write(&form, forms::acme_pdf()).expect("form");
    let answers = dir.join("answers.json");
    std::fs::write(&answers, r#"{"ACME.answer": "Filled", "consent": true}"#).expect("answers");
    let form_shown = form.display().to_string();
    let answers_shown = answers.display().to_string();
    // A document with words to redact, for `redact`.
    let contacts = dir.join("contacts.pdf");
    std::fs::write(&contacts, redact::contacts_pdf()).expect("contacts");
    let contacts_shown = contacts.display().to_string();

    #[cfg(target_os = "macos")]
    {
        // The built binary, from outside: dyld names every image each process
        // loads, prefixed by its pid, and the workers inherit the variable.
        let (code, _, stderr) = tool(&["verify", &shown], &[("DYLD_PRINT_LIBRARIES", "1")]);
        // `info`, `text`, `fields` and `fill` read the document through the
        // same workers; each is held to the same rule, by the same parse of
        // dyld's output. `fill` spawns three --- the reading, the writing and
        // the read-back --- and every one of them is a worker here.
        let filled = dir.join("filled.pdf").display().to_string();
        let redacted = dir.join("redacted.pdf").display().to_string();
        let lines: [Vec<&str>; 6] = [
            vec!["info", &shown],
            vec!["text", &shown],
            vec!["fields", &form_shown],
            vec![
                "fill",
                &form_shown,
                "-o",
                &filled,
                "--values",
                &answers_shown,
            ],
            vec![
                "redact",
                &contacts_shown,
                "--dry-run",
                "--text",
                "Rumpelstilzchen",
            ],
            vec![
                "redact",
                &contacts_shown,
                "-o",
                &redacted,
                "--text",
                "Rumpelstilzchen",
            ],
        ];
        for line in &lines {
            let command = if line.contains(&"--dry-run") {
                "redact --dry-run"
            } else {
                line[0]
            };
            let (code, _, stderr) = tool(line, &[("DYLD_PRINT_LIBRARIES", "1")]);
            let parents: std::collections::BTreeSet<String> = stderr
                .lines()
                .filter_map(|line| line.strip_prefix("dyld["))
                .filter_map(|rest| rest.split_once("]: "))
                .filter(|(_, image)| image.ends_with("/tpdf-cli"))
                .map(|(pid, _)| pid.to_string())
                .collect();
            let mapped_by = |pid: &str| {
                stderr
                    .lines()
                    .filter_map(|line| line.strip_prefix("dyld["))
                    .filter_map(|rest| rest.split_once("]: "))
                    .filter(|(p, _)| *p == pid)
                    .any(|(_, image)| image.to_ascii_lowercase().contains("pdfium"))
            };
            // The tool is the one process running tpdf-cli that maps no PDFium;
            // every worker is tpdf-cli too, and maps it --- except two of
            // `redact`'s, whose render service is the application's: the OCR
            // worker, which reads pixels and never a document, and a spare the
            // pool pre-spawned and nobody handed a document before the tool
            // exited, which ends before it maps anything. So the tool is named
            // by being first --- dyld's first line is the process that was
            // started, before it could spawn anything --- and `redact` is
            // allowed those two beside it, and one more. Every process here
            // writes its dyld lines to the one stderr, and they interleave
            // mid-line (`libCheckFix.dylidyld[55923]: <uuid>b`, captured
            // 2026-09-27), so a worker whose `libpdfium` line was cut in two
            // counts as clean. `redact` runs the most processes for the longest,
            // and a full gate run saw 4 clean where four runs alone saw 2 or 3.
            // The slack is in the upper bound only; the two assertions that
            // carry the claim --- the first process maps no PDFium, a worker
            // does --- are not loosened.
            let clean = parents.iter().filter(|pid| !mapped_by(pid)).count();
            let workers = parents.iter().filter(|pid| mapped_by(pid)).count();
            let first = stderr
                .lines()
                .filter_map(|line| line.strip_prefix("dyld["))
                .find_map(|rest| rest.split_once("]: ").map(|(pid, _)| pid.to_string()));
            let tool_clean = first
                .as_deref()
                .is_some_and(|pid| parents.contains(pid) && !mapped_by(pid));
            let (ok, ocr) = match command {
                "redact" => (code == 0 || code == 1, 3),
                "redact --dry-run" => (code == 0, 1),
                _ => (code == 0, 0),
            };
            report.check(
                &format!("{command}: the tool's own process never loaded PDFium, and a worker did"),
                ok && tool_clean
                    && clean <= 1 + ocr
                    && workers >= if command == "fill" { 3 } else { 1 },
                &format!("exit {code}, {clean} clean, {workers} with PDFium"),
            );
        }
        let mut by_pid: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for line in stderr.lines() {
            if let Some(rest) = line.strip_prefix("dyld[") {
                if let Some((pid, image)) = rest.split_once("]: ") {
                    by_pid
                        .entry(pid.to_string())
                        .or_default()
                        .push(image.to_string());
                }
            }
        }
        // The tool is the process that loaded the tool's own image.
        let tool_pid = by_pid
            .iter()
            .find(|(_, images)| {
                images.iter().any(|i| i.ends_with("/tpdf-cli")) && !pdfium_in(images)
            })
            .or_else(|| {
                by_pid
                    .iter()
                    .find(|(_, images)| images.first().is_some_and(|i| i.ends_with("/tpdf-cli")))
            })
            .map(|(pid, _)| pid.clone());
        let workers: Vec<&String> = by_pid
            .iter()
            .filter(|(pid, _)| Some(*pid) != tool_pid.as_ref())
            .map(|(pid, _)| pid)
            .collect();
        let tool_images = tool_pid
            .as_ref()
            .and_then(|p| by_pid.get(p))
            .cloned()
            .unwrap_or_default();
        report.check(
            "the built tool verified the document",
            code == 0,
            &format!("exit {code}"),
        );
        report.check(
            "control: dyld's list for the tool's process is readable (libSystem is in it)",
            tool_images.iter().any(|i| i.contains("libSystem")),
            &format!(
                "{} images from {} processes",
                tool_images.len(),
                by_pid.len()
            ),
        );
        report.check(
            "the tool's own process never loaded PDFium",
            !tool_images.is_empty() && !pdfium_in(&tool_images),
            &format!("{tool_images:?}"),
        );
        report.check(
            "control: a worker it spawned did load PDFium",
            workers.iter().any(|pid| pdfium_in(&by_pid[*pid])),
            &format!("{} other processes", workers.len()),
        );
    }

    // This process, as the tool's coordinator: `cli::run` has read a document
    // through a worker, and nothing here mapped the parser. Then the control.
    let before = tpdf_lib::images::mapped();
    let store = TestStore {
        certificate: Vec::new(),
        misdirected: false,
    };
    let (code, stdout, stderr) = signs(&strings(&["verify", &shown]), &store, now());
    let (info_code, info_out, _) = signs(&strings(&["info", &shown]), &store, now());
    let (text_code, _, _) = signs(&strings(&["text", &shown]), &store, now());
    let (fields_code, fields_out, _) = signs(&strings(&["fields", &form_shown]), &store, now());
    let (fill_code, fill_out, fill_err) = signs(
        &strings(&[
            "fill",
            &form_shown,
            "-o",
            &dir.join("filled-here.pdf").display().to_string(),
            "--values",
            &answers_shown,
        ]),
        &store,
        now(),
    );
    let (redact_code, redact_out, redact_err) = signs(
        &strings(&[
            "redact",
            &contacts_shown,
            "-o",
            &dir.join("redacted-here.pdf").display().to_string(),
            "--text",
            "Rumpelstilzchen",
        ]),
        &store,
        now(),
    );
    let after = tpdf_lib::images::mapped();
    report.check(
        "cli::run redacted a document through workers too",
        (redact_code == 0 || redact_code == 1) && redact_out.contains("Redact"),
        &format!("redact exit {redact_code}: {redact_err}"),
    );
    report.check(
        "cli::run read the document through a worker",
        code == 0 && stdout.contains("no signatures"),
        &format!("exit {code}: {stderr}"),
    );
    report.check(
        "cli::run described it and read its text through workers too",
        info_code == 0 && info_out.contains("1 page") && text_code == 0,
        &format!("info exit {info_code}, text exit {text_code}"),
    );
    report.check(
        "cli::run listed a form's fields and filled it through workers too",
        fields_code == 0
            && fields_out.contains("ACME.answer")
            && fill_code == 0
            && fill_out.contains("Filled 2 fields"),
        &format!("fields exit {fields_code}, fill exit {fill_code}: {fill_err}"),
    );
    report.check(
        "no PDFium in this process after cli::run verified a document",
        !after.is_empty() && !pdfium_in(&before) && !pdfium_in(&after),
        &format!("{} modules", after.len()),
    );
    let bound = tpdf_lib::progressive::bind(&library_dir());
    report.check(
        "control: bound here, PDFium shows in the same list",
        bound.is_ok() && pdfium_in(&tpdf_lib::images::mapped()),
        &format!("{:?}", bound.err()),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 4 ----------------------------------------------------------------------

fn workers_are_sandboxed(report: &mut Report) {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            // libsystem_sandbox: with no operation, nonzero when `pid` is
            // sandboxed at all.
            fn sandbox_check(
                pid: libc::pid_t,
                operation: *const libc::c_char,
                kind: libc::c_int,
                ...
            ) -> libc::c_int;
        }
        let dir = scratch("sandbox");
        let document = dir.join("plain.pdf");
        std::fs::write(&document, plain_pdf()).expect("document");
        let file = std::fs::File::open(&document).expect("open");
        let len = usize::try_from(file.metadata().expect("len").len()).expect("len");
        let mapped = tpdf_lib::worker::Shm::map_open_file(&file, len).expect("map");
        let mut worker =
            tpdf_lib::worker::Worker::spawn_shared(std::sync::Arc::new(mapped), &library_dir())
                .expect("a worker, as save::InWorker starts one");
        // An answer means the child is past its boundary: it binds PDFium,
        // applies the profile, then serves.
        let answered = worker.call(&tpdf_lib::worker::Request::Properties);
        let pid = worker.pid();
        // SAFETY: a pid and a null operation, which the call documents.
        let theirs = unsafe { sandbox_check(pid as libc::pid_t, std::ptr::null(), 0) };
        // SAFETY: as above, for this process.
        let ours = unsafe { sandbox_check(std::process::id() as libc::pid_t, std::ptr::null(), 0) };
        report.check(
            "the worker answered",
            answered.as_ref().is_ok_and(|a| a.ok),
            &format!("{:?}", answered.err()),
        );
        report.check(
            "control: this process is not sandboxed",
            ours == 0,
            &ours.to_string(),
        );
        report.check("the worker is sandboxed", theirs != 0, &theirs.to_string());
        drop(worker);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(not(target_os = "macos"))]
    report.skip(
        "sandbox_check",
        "a Windows worker is contained by its parent; `scripts/win_modules.py` is that platform's instrument",
    );
}

// --- 5 ----------------------------------------------------------------------

/// PDFium, bound in this process: after check 3, which needs it not to be.
fn bindings() -> Option<tpdf_lib::progressive::Bindings> {
    tpdf_lib::progressive::bind(&library_dir())
        .ok()
        .map(tpdf_lib::progressive::bindings_of)
}

/// `cli::info::document` for `path`, built in this process.
fn info_here(
    bindings: tpdf_lib::progressive::Bindings,
    path: &Path,
    password: Option<&str>,
) -> Result<serde_json::Value, String> {
    let document = tpdf_lib::document::OpenDocument::open(bindings, path, password)
        .map_err(|refusal| refusal.reason)?;
    let count = document.page_count();
    let properties = document.graph().properties(count)?;
    let sizes = (0..count)
        .map(|i| {
            document.page(i).map(|page| tpdf_lib::render::PageSize {
                width_pt: page.width_pt(),
                height_pt: page.height_pt(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let form = document.graph().form();
    serde_json::to_value(cli::info::document(&properties, &sizes, form)).map_err(|e| e.to_string())
}

/// The keys at which two JSON values differ, for a failure message.
fn differing(a: &serde_json::Value, b: &serde_json::Value) -> Vec<String> {
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            keys.into_iter()
                .filter(|k| x.get(*k) != y.get(*k))
                .map(|k| format!("{k}: tool {:?} here {:?}", x.get(k), y.get(k)))
                .collect()
        }
        _ if a == b => Vec::new(),
        _ => vec![format!("tool {a} here {b}")],
    }
}

const DESCRIBED: [&str; 9] = [
    "incr-two-signers.pdf",
    "incr-certified-1.pdf",
    "incr-encrypted-open.pdf",
    "signed-altered.pdf",
    "form.pdf",
    "tagged.pdf",
    "columns.pdf",
    "rotated.pdf",
    "multilingual.pdf",
];

fn info_agrees(report: &mut Report) {
    let Some(bindings) = bindings() else {
        report.check("PDFium binds in this process", false, "no library");
        return;
    };
    let present: Vec<PathBuf> = DESCRIBED.iter().filter_map(|n| fixture(n)).collect();
    if present.is_empty() {
        report.skip("info against the fixtures", "none generated");
        return;
    }
    let shown: Vec<String> = present.iter().map(|p| p.display().to_string()).collect();
    let mut args = vec!["info", "--json"];
    args.extend(shown.iter().map(String::as_str));
    let (code, stdout, stderr) = tool(&args, &[]);
    report.check(
        "info exits 0 when every document was read",
        code == 0,
        &stderr,
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "the document says schema 1 and command info, one entry per document",
        json["schema"] == 1
            && json["command"] == "info"
            && json["files"].as_array().map(Vec::len) == Some(present.len()),
        &stdout,
    );
    let mut seen = std::collections::BTreeSet::new();
    for (at, path) in present.iter().enumerate() {
        let theirs = &json["files"][at]["document"];
        let ours = match info_here(bindings, path, None) {
            Ok(ours) => ours,
            Err(why) => {
                report.check(&format!("{}: read here", path.display()), false, &why);
                continue;
            }
        };
        if theirs["tagged"] == true {
            seen.insert("tagged");
        }
        if !theirs["encryption"].is_null() {
            seen.insert("encrypted");
        }
        if theirs["signatures"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
        {
            seen.insert("signed");
        }
        if theirs["form"]["fields"].as_u64().unwrap_or(0) > 0 {
            seen.insert("form");
        }
        report.check(
            &format!(
                "{}: info says what the in-process reader says",
                path.display()
            ),
            !theirs.is_null() && *theirs == ours,
            &differing(theirs, &ours).join("\n       "),
        );
    }
    if present.len() == DESCRIBED.len() {
        report.check(
            "control: the fixtures are between them tagged, encrypted, signed and carry a form",
            seen.len() == 4,
            &format!("{seen:?}"),
        );
    } else {
        report.skip("the variety control", "not every fixture is generated");
    }

    // A document behind a real password.
    let Some(locked) = fixture("incr-encrypted-pw.pdf") else {
        report.skip(
            "the password checks",
            "incr-encrypted-pw.pdf is not generated",
        );
        return;
    };
    let at = locked.display().to_string();
    let (code, stdout, stderr) = tool(&["info", "--json", &at], &[]);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "a locked document is reported as locked, and info still exits 0",
        code == 0
            && json["files"][0]["error"]["kind"] == "locked"
            && json["files"][0]["document"].is_null()
            && stderr.contains("--password-env"),
        &format!("exit {code}: {stdout}"),
    );
    let (code, stdout, _) = tool(
        &["info", "--json", "--password-env", "TPDF_IT_PASSWORD", &at],
        &[("TPDF_IT_PASSWORD", "swordfish")],
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let theirs = &json["files"][0]["document"];
    let ours = info_here(bindings, &locked, Some("swordfish")).unwrap_or_default();
    report.check(
        "with its password from the environment, it is described as the in-process reader describes it",
        code == 0 && !theirs.is_null() && *theirs == ours,
        &differing(theirs, &ours).join("\n       "),
    );
    report.check(
        "the password never appears in what the tool prints",
        !stdout.contains("swordfish"),
        &stdout,
    );
    let (code, stdout, _) = tool(
        &["info", "--json", "--password-env", "TPDF_IT_PASSWORD", &at],
        &[("TPDF_IT_PASSWORD", "not the password")],
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "a wrong password leaves it locked",
        code == 0 && json["files"][0]["error"]["kind"] == "locked",
        &stdout,
    );
    let (code, stdout, _) = tool(&["verify", "--json", &at], &[]);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "verify reports the same document as locked, not refused",
        code == 3 && json["files"][0]["error"]["kind"] == "locked",
        &stdout,
    );
}

// --- 6 ----------------------------------------------------------------------

/// Every page's `text` report for `path`, built in this process.
fn text_here(
    bindings: tpdf_lib::progressive::Bindings,
    path: &Path,
    password: Option<&str>,
    strip_tags: bool,
) -> Result<Vec<serde_json::Value>, String> {
    let document = tpdf_lib::document::OpenDocument::open(bindings, path, password)
        .map_err(|refusal| refusal.reason)?;
    let count = document.page_count();
    let mapping = document.graph().mapping(count as usize).to_vec();
    (0..count)
        .map(|i| {
            let mut text = tpdf_lib::text::extract(&document.page(i)?)?;
            if strip_tags {
                text.runs.clear();
            }
            serde_json::to_value(cli::text::ordered(i + 1, &text, mapping.get(i as usize)))
                .map_err(|e| e.to_string())
        })
        .collect()
}

const READ: [&str; 6] = [
    "tagged.pdf",
    "columns.pdf",
    "multilingual.pdf",
    "encodings.pdf",
    "rotated.pdf",
    "incr-two-signers.pdf",
];

/// The manifest's lines for each page, where it records them.
fn manifest_lines(name: &str) -> Vec<(usize, Vec<String>)> {
    let stem = name.trim_end_matches(".pdf");
    let Ok(text) = std::fs::read_to_string(
        root()
            .join("testdata")
            .join(format!("{stem}-manifest.json")),
    ) else {
        return Vec::new();
    };
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    json["pages"]
        .as_array()
        .map(|pages| {
            pages
                .iter()
                .filter_map(|p| {
                    let lines = p["lines"].as_array()?;
                    Some((
                        usize::try_from(p["page"].as_u64()?).ok()?,
                        lines
                            .iter()
                            .map(|l| l.as_str().unwrap_or_default().to_string())
                            .collect(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[allow(clippy::too_many_lines)]
fn text_agrees(report: &mut Report) {
    let Some(bindings) = bindings() else {
        report.check("PDFium binds in this process", false, "no library");
        return;
    };
    let mut orders = std::collections::BTreeSet::new();
    let mut encodings = std::collections::BTreeSet::new();
    let mut manifest_pages = 0;
    for name in READ {
        let Some(path) = fixture(name) else {
            report.skip(&format!("text of {name}"), "not generated");
            continue;
        };
        let at = path.display().to_string();
        let (code, stdout, stderr) = tool(&["text", "--json", &at], &[]);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let theirs = json["pages"].as_array().cloned().unwrap_or_default();
        let ours = text_here(bindings, &path, None, false).unwrap_or_default();
        for page in &theirs {
            orders.insert(page["order"].as_str().unwrap_or_default().to_string());
            encodings.insert(page["encoding"].as_str().unwrap_or_default().to_string());
        }
        report.check(
            &format!(
                "{name}: every page's text, order and encoding is the in-process extraction's"
            ),
            code == 0 && !theirs.is_empty() && theirs == ours,
            &format!("exit {code} {stderr}\n       tool {theirs:?}\n       here {ours:?}"),
        );
        for (page, lines) in manifest_lines(name) {
            manifest_pages += 1;
            let got: Vec<String> = theirs
                .get(page)
                .and_then(|p| p["text"].as_str())
                .map(|t| t.split('\n').map(str::to_string).collect())
                .unwrap_or_default();
            report.check(
                &format!("{name} page {}: the lines its manifest records", page + 1),
                got == lines,
                &format!("\n       want {lines:?}\n       got  {got:?}"),
            );
        }
        // Plain output: every page ends with a form feed.
        let (code, plain, _) = tool(&["text", &at], &[]);
        report.check(
            &format!(
                "{name}: plain text ends each of its {} pages with a form feed",
                theirs.len()
            ),
            code == 0 && plain.matches('\u{c}').count() == theirs.len() && plain.ends_with('\u{c}'),
            &format!("exit {code}"),
        );
    }
    let all = READ.iter().all(|n| fixture(n).is_some());
    if all {
        report.check(
            "control: the pages read include tagged and geometric orders, stated and guessed encodings",
            ["tagged", "geometric"].iter().all(|o| orders.contains(*o))
                && ["stated", "guessed"].iter().all(|e| encodings.contains(*e))
                && manifest_pages >= 10,
            &format!("{orders:?} {encodings:?}, {manifest_pages} manifest pages"),
        );
    } else {
        report.skip("the variety control", "not every fixture is generated");
    }

    // The tagged page, and the control that it is the tags that order it.
    if let Some(tagged) = fixture("tagged.pdf") {
        let with = text_here(bindings, &tagged, None, false).unwrap_or_default();
        let without = text_here(bindings, &tagged, None, true).unwrap_or_default();
        let text = |pages: &[serde_json::Value]| {
            pages
                .first()
                .and_then(|p| p["text"].as_str())
                .unwrap_or_default()
                .to_string()
        };
        let (a, b) = (text(&with), text(&without));
        let before = |t: &str, x: &str, y: &str| match (t.find(x), t.find(y)) {
            (Some(i), Some(j)) => i < j,
            _ => false,
        };
        report.check(
            "tagged.pdf: the tags put the margin note last, and the geometry puts it before the second heading",
            with.first().is_some_and(|p| p["order"] == "tagged")
                && without.first().is_some_and(|p| p["order"] == "geometric")
                && before(&a, "Second half", "Marginal")
                && before(&b, "Marginal", "Second half"),
            &format!("\n       tagged    {a:?}\n       geometric {b:?}"),
        );
    }

    // --pages, and its refusals.
    if let Some(columns) = fixture("columns.pdf") {
        let at = columns.display().to_string();
        let all = text_here(bindings, &columns, None, false).unwrap_or_default();
        let (code, stdout, _) = tool(&["text", "--json", "--pages", "3,1", &at], &[]);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let got = json["pages"].as_array().cloned().unwrap_or_default();
        report.check(
            "--pages 3,1 reads pages 1 and 3, in document order",
            code == 0 && all.len() >= 3 && got == vec![all[0].clone(), all[2].clone()],
            &format!("exit {code}: {got:?}"),
        );
        let beyond = (all.len() + 1).to_string();
        let (code, stdout, stderr) = tool(&["text", "--pages", &beyond, &at], &[]);
        report.check(
            "a page past the end is refused, 3, and nothing is printed",
            code == 3 && stdout.is_empty() && stderr.contains(&format!("no page {beyond}")),
            &format!("exit {code}: {stderr}"),
        );
        let (code, _, stderr) = tool(&["text", "--pages", "2-1", &at], &[]);
        report.check(
            "a backwards range is refused, 2",
            code == 2 && stderr.contains("backwards"),
            &format!("exit {code}: {stderr}"),
        );
        let dir = scratch("text-out");
        let out = dir.join("out.txt");
        let (code, stdout, _) = tool(&["text", &at, "-o", &out.display().to_string()], &[]);
        let (_, printed, _) = tool(&["text", &at], &[]);
        report.check(
            "-o writes exactly what stdout would carry, and stdout carries nothing",
            code == 0 && stdout.is_empty() && std::fs::read_to_string(&out).ok() == Some(printed),
            &format!("exit {code}"),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A document behind a password.
    if let Some(locked) = fixture("incr-encrypted-pw.pdf") {
        let at = locked.display().to_string();
        let (code, stdout, stderr) = tool(&["text", &at], &[]);
        report.check(
            "text refuses a locked document, 3, and says how to give the password",
            code == 3 && stdout.is_empty() && stderr.contains("--password-env"),
            &format!("exit {code}: {stderr}"),
        );
        let (code, stdout, _) = tool(
            &["text", "--json", "--password-env", "TPDF_IT_PASSWORD", &at],
            &[("TPDF_IT_PASSWORD", "swordfish")],
        );
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let ours = text_here(bindings, &locked, Some("swordfish"), false).unwrap_or_default();
        report.check(
            "with its password, text reads it as the in-process extraction does",
            code == 0
                && !ours.is_empty()
                && json["pages"].as_array() == Some(&ours)
                && ours[0]["text"].as_str().is_some_and(|t| !t.is_empty()),
            &format!("exit {code}: {stdout}"),
        );
        let (code, _, stderr) = tool(
            &["text", "--password-env", "TPDF_IT_PASSWORD", &at],
            &[("TPDF_IT_PASSWORD", "not the password")],
        );
        report.check(
            "a wrong password is refused, 3",
            code == 3 && stderr.contains("did not open it"),
            &format!("exit {code}: {stderr}"),
        );
    }
}

// --- 7 ----------------------------------------------------------------------

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// Shared words over the larger count: 1.0 when the two hold the same words.
fn overlap(a: &[String], b: &[String]) -> f64 {
    let mut count: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for w in a {
        *count.entry(w).or_default() += 1;
    }
    let mut shared = 0;
    for w in b {
        if let Some(n) = count.get_mut(w.as_str()) {
            if *n > 0 {
                *n -= 1;
                shared += 1;
            }
        }
    }
    shared as f64 / a.len().max(b.len()).max(1) as f64
}

/// Longest common subsequence of words over the larger count: word order.
fn in_order(a: &[String], b: &[String]) -> f64 {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()] as f64 / a.len().max(b.len()).max(1) as f64
}

fn beside_pdftotext(report: &mut Report) {
    let available = Command::new("pdftotext").arg("-v").output().is_ok();
    if !available {
        report.skip(
            "the pdftotext comparison",
            "Poppler's pdftotext is not installed",
        );
        return;
    }
    for name in READ {
        let Some(path) = fixture(name) else { continue };
        let at = path.display().to_string();
        let (_, ours, _) = tool(&["text", &at], &[]);
        let theirs = Command::new("pdftotext")
            .args([at.as_str(), "-"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let pages = |t: &str| t.split('\u{c}').map(words).collect::<Vec<_>>();
        let (a, b) = (pages(&ours), pages(&theirs));
        let per_page: Vec<String> = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| !x.is_empty() || !y.is_empty())
            .map(|(x, y)| format!("{:.2}/{:.2}", overlap(x, y), in_order(x, y)))
            .collect();
        let (x, y) = (words(&ours), words(&theirs));
        println!(
            "[INFO] {name}: words {:.3} shared, {:.3} in the same order ({} against {}); per page {}",
            overlap(&x, &y),
            in_order(&x, &y),
            x.len(),
            y.len(),
            per_page.join(" ")
        );
    }
}
