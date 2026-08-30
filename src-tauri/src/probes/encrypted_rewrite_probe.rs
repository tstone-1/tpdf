//! Does a rewrite keep an encrypted document's encryption?
//!
//! `docs/PLAN.md` §5 said for months that closing this needed the hardened
//! structural rewrite the stack table names QPDF for. It did not: `lopdf`
//! exposes `Document::encrypt`, and `save.rs`'s `checked` now takes the state
//! off the document after a password load so `rewrite` can put it back.
//!
//! **The evidence has to come from outside the writer.** A check that reloads
//! the output with `lopdf` is the writer agreeing with its own reader, which is
//! a shape `docs/TRAPS.md` records from several directions --- and here it is
//! worse than usual, because a `lopdf` load *without* the password parses no
//! objects at all and reports zero pages. The first two runs of the spike this
//! probe grew from round-tripped an empty document and printed `[OK]` three
//! times. So the verdicts below come from `qpdf`, which is a different
//! implementation entirely, and the page count is always read back with the
//! password.
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example encrypted-rewrite-probe
//! ```
//!
//! `qpdf` is required. Without it every encryption check `[SKIP]`s with its
//! reason, and the page-count checks still run --- a skip that says why is worth
//! more than a probe that refuses to start.

use std::path::{Path, PathBuf};

use tpdf_lib::docmodel::PageSource;
use tpdf_lib::edits::{PageView, Plan};
use tpdf_lib::save;

/// The fixture and the password that opens it.
///
/// Two of them because one cannot discriminate: `incr-encrypted-pw` is behind a
/// real user password, and `incr-encrypted-open` has an empty one --- which is
/// what a permission-restricted document is, opens unprompted in every reader,
/// and is the case that was being silently written in the clear before the
/// guard `checked` makes was corrected in August 2026.
const FIXTURES: [(&str, &str); 2] = [
    ("incr-encrypted-pw.pdf", "swordfish"),
    ("incr-encrypted-open.pdf", ""),
];

#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
            println!("[OK]   {name}   {}", detail.as_ref());
        } else {
            self.failed += 1;
            println!("[FAIL] {name}   {}", detail.as_ref());
        }
    }

    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.skipped += 1;
        println!("[SKIP] {name}   {}", why.as_ref());
    }

    fn finish(&self) -> ! {
        println!(
            "\n{}/{} checks passed, {} skipped",
            self.passed,
            self.passed + self.failed,
            self.skipped
        );
        std::process::exit(i32::from(self.failed != 0));
    }
}

/// `qpdf --show-encryption`, or `None` if qpdf is not installed.
fn encryption_of(path: &Path, password: &str) -> Option<String> {
    let out = std::process::Command::new("qpdf")
        .arg("--show-encryption")
        .arg(format!("--password={password}"))
        .arg(path)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// How many pages `lopdf` finds **with the password**.
///
/// Without one it finds none, on a perfectly good file, so a count taken that
/// way is not a measurement.
fn pages_with_password(path: &Path, password: &str) -> usize {
    lopdf::Document::load_with_options(
        path,
        lopdf::LoadOptions {
            password: Some(password.to_string()),
            ..Default::default()
        },
    )
    .map_or(0, |doc| doc.get_pages().len())
}

/// A plan that keeps every page but the last.
fn dropping_last(pages: usize) -> Plan {
    Plan {
        opened_as: None,
        baseline: pages as u32,
        pages: (0..pages - 1)
            .map(|at| PageView {
                id: at as u64 + 1,
                source: PageSource::Baseline(at as u32),
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: Vec::new(),
        notes: Vec::new(),
    }
}

pub fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("a repository root")
        .join("testdata");
    let mut r = Report::default();
    let have_qpdf = std::process::Command::new("qpdf")
        .arg("--version")
        .output()
        .is_ok();

    for (name, password) in FIXTURES {
        let source = root.join(name);
        if !source.exists() {
            r.skip(
                name,
                "fixture not generated -- run testdata/make_incremental_pdf.py",
            );
            continue;
        }

        let before = pages_with_password(&source, password);
        if before < 2 {
            r.skip(name, format!("needs two pages to drop one, found {before}"));
            continue;
        }

        let out = std::env::temp_dir().join(format!("tpdf-encrw-{}-{name}", std::process::id()));
        let key = (!password.is_empty()).then_some(password);
        match save::write_copy(&source, &dropping_last(before), &out, key) {
            Ok(_) => {}
            Err(why) => {
                r.check(false, &format!("{name}: rewrite"), why.message);
                continue;
            }
        }

        // The page the plan dropped is gone, read back with the password.
        let after = pages_with_password(&out, password);
        r.check(
            after == before - 1,
            &format!("{name}: the rewrite took effect"),
            format!("{before} page(s) in, {after} out"),
        );

        // The encryption is the source's own, field for field, according to a
        // reader that is not the one that wrote it.
        if have_qpdf {
            match (
                encryption_of(&source, password),
                encryption_of(&out, password),
            ) {
                (Some(was), Some(now)) => r.check(
                    was == now && !was.is_empty(),
                    &format!("{name}: the encryption is unchanged"),
                    if was == now {
                        format!("{} field(s) agree", was.lines().count())
                    } else {
                        "qpdf reports different encryption".into()
                    },
                ),
                _ => r.skip(
                    &format!("{name}: the encryption is unchanged"),
                    "qpdf failed",
                ),
            }
        } else {
            r.skip(
                &format!("{name}: the encryption is unchanged"),
                "qpdf is not installed",
            );
        }

        // **The control, and it is the check the whole probe rests on.** A
        // rewrite that dropped the encryption also passes both checks above:
        // the pages are right, and `qpdf --show-encryption` on a plain file
        // agrees with itself. What must be true is that the output IS encrypted.
        let raw = std::fs::read(&out).unwrap_or_default();
        r.check(
            raw.windows(8).any(|w| w == b"/Encrypt"),
            &format!("{name}: the output is still encrypted"),
            format!("{} bytes written", raw.len()),
        );

        let _ = std::fs::remove_file(&out);
    }

    // A locked document -- no password offered -- must still be refused, and
    // with a message naming the lock rather than something the reader cannot
    // act on.
    let locked = root.join("incr-encrypted-pw.pdf");
    if locked.exists() {
        match save::write_copy(
            &locked,
            &dropping_last(2),
            &std::env::temp_dir().join("tpdf-encrw-locked.pdf"),
            None,
        ) {
            Ok(_) => r.check(false, "a locked document is refused", "it was rewritten"),
            Err(why) => r.check(
                why.message.contains("could not unlock"),
                "a locked document is refused",
                why.message,
            ),
        }
    } else {
        r.skip("a locked document is refused", "fixture not generated");
    }

    r.finish();
}
