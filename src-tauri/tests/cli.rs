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
//! 3. **The tool's own process never maps PDFium, and its workers do** (macOS:
//!    `DYLD_PRINT_LIBRARIES` on the built binary, separated by pid; both
//!    platforms: this process's own module list after `cli::run` has verified a
//!    document, with PDFium bound in-process last as the control that the list
//!    can show it).
//! 4. **The workers the tool spawns are sandboxed** (macOS: `sandbox_check` on a
//!    worker started by `Worker::spawn_shared`, the call `save::InWorker` makes,
//!    against this process as the unsandboxed control).
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

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some(worker::WORKER_ARGV) {
        worker_child::main(&argv);
    }
    let checks: [Check; 4] = [
        ("verify agrees with the in-process reader", verify_agrees),
        (
            "a signature made through the tool reads back intact",
            sign_reads_back,
        ),
        ("the tool's process never maps PDFium", never_maps_pdfium),
        ("the tool's workers are sandboxed", workers_are_sandboxed),
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

const SIGNED: [&str; 14] = [
    "incr-signed.pdf",
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

/// One signature's verdicts, as `(field, verdict, why, standing, doubt)`.
type Line = (String, String, String, String, String);

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
            (
                s.field,
                text(serde_json::to_value(integrity.verdict).expect("json")),
                text(serde_json::to_value(integrity.why).expect("json")),
                standing,
                doubt,
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
            (
                text(&s["field"]),
                text(&s["integrity"]["verdict"]),
                text(&s["integrity"]["why"]),
                text(&s["trust"]["standing"]),
                text(&s["trust"]["why"]),
            )
        })
        .collect()
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

    #[cfg(target_os = "macos")]
    {
        // The built binary, from outside: dyld names every image each process
        // loads, prefixed by its pid, and the workers inherit the variable.
        let (code, _, stderr) = tool(&["verify", &shown], &[("DYLD_PRINT_LIBRARIES", "1")]);
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
    let after = tpdf_lib::images::mapped();
    report.check(
        "cli::run read the document through a worker",
        code == 0 && stdout.contains("no signatures"),
        &format!("exit {code}: {stderr}"),
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
