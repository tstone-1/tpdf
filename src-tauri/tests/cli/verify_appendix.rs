//! `verify --json` says what was appended after each signature, and the line
//! `verify` prints under a document can be worked out from it.
//!
//! The fixture is `incr-two-signers.pdf`: the second signature is the first
//! one's appendix, and nothing follows the second. A revision written here
//! after both replaces the first page's content stream and changes no signed
//! byte, so both signatures stay intact with a page rewritten after them.
//! The control is the fixture as generated, where the same reading of the
//! same keys finds nothing to say.
use super::{fixture, scratch, tool, Report};
use std::path::Path;

/// `bytes` with one more revision, which writes the first page's content
/// stream again and nothing else.
fn with_a_page_rewritten(bytes: &[u8]) -> Vec<u8> {
    use lopdf::{dictionary, Document, IncrementalDocument, Object, Stream};
    let prev = Document::load_mem(bytes).expect("the fixture parses");
    let page = *prev.get_pages().get(&1).expect("a first page");
    let content = prev
        .get_object(page)
        .and_then(Object::as_dict)
        .and_then(|page| page.get(b"Contents"))
        .and_then(Object::as_reference)
        .expect("a content stream of its own");
    let mut incremental = IncrementalDocument::create_from(bytes.to_vec(), prev);
    incremental.new_document.set_object(
        content,
        Stream::new(dictionary! {}, b"0 0 200 200 re f".to_vec()),
    );
    let mut out = Vec::new();
    incremental.save_to(&mut out).expect("the revision saves");
    assert_eq!(&out[..bytes.len()], bytes, "a revision appended");
    out
}

/// The document's signatures as `verify --json` reports them.
pub(super) fn signatures(path: &Path) -> Vec<serde_json::Value> {
    let (_, stdout, _) = tool(&["verify", "--json", &path.display().to_string()], &[]);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    json["files"][0]["signatures"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// What `--strict` judges, read off the report alone as the README says to:
/// of the intact signature with the fewest `appended_bytes`, whether its
/// appendix is unread and how many pages it touches beyond those listing a
/// field. Nothing appended is `(false, 0)`.
pub(super) fn judged(signatures: &[serde_json::Value]) -> Option<(bool, u64)> {
    let last = signatures
        .iter()
        .filter(|s| s["integrity"]["verdict"] == "intact")
        .min_by_key(|s| s["appended_bytes"].as_u64().unwrap_or(u64::MAX))?;
    let appendix = &last["appendix"];
    if appendix.is_null() {
        return Some((false, 0));
    }
    let listed = appendix["pages_listing"].as_array()?.len() as u64;
    Some((
        appendix["unread"].as_bool()?,
        appendix["pages_touched"].as_u64()?.saturating_sub(listed),
    ))
}

/// What the in-process reader finds after each signed field: nothing, or
/// `(unread, added, replaced, pages touched, pages listing a field)`.
type Read = Option<(bool, u64, u64, u64, Vec<(u64, bool)>)>;

fn in_process(path: &Path) -> Vec<Read> {
    use tpdf_lib::save::Verifier as _;
    let mut file = std::fs::File::open(path).expect("the document");
    let len = usize::try_from(file.metadata().expect("len").len()).expect("len");
    tpdf_lib::save::Here
        .signatures(&mut file, len)
        .expect("the in-process reader reads it")
        .into_iter()
        .filter(|s| s.signed)
        .map(|s| {
            s.appendix.map(|a| {
                (
                    a.unread,
                    a.added as u64,
                    a.replaced as u64,
                    a.pages_touched as u64,
                    a.pages_listing
                        .iter()
                        .map(|l| (u64::from(l.page), l.timestamp))
                        .collect(),
                )
            })
        })
        .collect()
}

fn from_json(signatures: &[serde_json::Value]) -> Vec<Read> {
    signatures
        .iter()
        .map(|s| {
            let a = &s["appendix"];
            (!a.is_null()).then(|| {
                (
                    a["unread"].as_bool().unwrap_or(true),
                    a["added"].as_u64().unwrap_or(u64::MAX),
                    a["replaced"].as_u64().unwrap_or(u64::MAX),
                    a["pages_touched"].as_u64().unwrap_or(u64::MAX),
                    a["pages_listing"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|l| {
                            (
                                l["page"].as_u64().unwrap_or(u64::MAX),
                                l["timestamp"].as_bool().unwrap_or(true),
                            )
                        })
                        .collect(),
                )
            })
        })
        .collect()
}

pub fn says_what_was_appended(report: &mut Report) {
    let Some(two) = fixture("incr-two-signers.pdf") else {
        report.skip(
            "verify reports what was appended after a signature",
            "incr-two-signers.pdf is not generated",
        );
        return;
    };

    // As generated: the second signature is all that follows the first.
    let found = signatures(&two);
    let first = found.first().cloned().unwrap_or_default();
    let appendix = &first["appendix"];
    report.check(
        "the first of two signatures reports the second as its appendix",
        found.len() == 2
            && first["appended_bytes"].as_u64().is_some_and(|n| n > 0)
            && appendix["unread"] == false
            && appendix["added"].as_u64().is_some_and(|n| n > 0)
            && appendix["kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "Sig"))
            && appendix["catalog_gained"]
                .as_array()
                .is_some_and(Vec::is_empty),
        &appendix.to_string(),
    );
    report.check(
        "the page it touched is the one that lists the new field",
        appendix["pages_touched"] == 1
            && appendix["pages_listing"] == serde_json::json!([{ "page": 1, "timestamp": false }])
            && appendix["sentence"]
                == "another signature, and a signature field was added to page 1's annotations \
                    (the page's content is unchanged)",
        &appendix.to_string(),
    );
    report.check(
        "a signature nothing was appended after reports null",
        found
            .get(1)
            .is_some_and(|s| s["appended_bytes"] == 0 && s["appendix"].is_null()),
        &format!("{:?}", found.get(1).map(|s| &s["appendix"])),
    );
    let (ours, theirs) = (in_process(&two), from_json(&found));
    report.check(
        "the tool reports the counts the in-process reader reads",
        ours.len() == 2 && theirs == ours,
        &format!("tool {theirs:?}\n       here {ours:?}"),
    );
    let (_, text, _) = tool(&["verify", &two.display().to_string()], &[]);
    report.check(
        "nothing follows the last signature, by the report and by the text",
        judged(&found) == Some((false, 0))
            && text.matches("\n    Appended: ").count() == 1
            && !text.contains("After the last signature"),
        &text,
    );

    // With a page rewritten after both.
    let dir = scratch("verify-appendix");
    let rewritten = dir.join("rewritten.pdf");
    std::fs::write(
        &rewritten,
        with_a_page_rewritten(&std::fs::read(&two).expect("the fixture")),
    )
    .expect("the rewritten copy");
    let found = signatures(&rewritten);
    let last = found.get(1).cloned().unwrap_or_default();
    report.check(
        "both signatures are still intact, and each has an appendix",
        found.len() == 2
            && found
                .iter()
                .all(|s| s["integrity"]["verdict"] == "intact" && s["appendix"].is_object()),
        &format!("{found:?}"),
    );
    report.check(
        "the last signature's appendix is one stream replaced and one page touched",
        last["appendix"]["added"] == 0
            && last["appendix"]["replaced"] == 1
            && last["appendix"]["kinds"] == serde_json::json!(["stream"])
            && last["appendix"]["pages_touched"] == 1
            && last["appendix"]["pages_listing"] == serde_json::json!([])
            && last["appendix"]["sentence"] == "1 object: stream, and 1 page was rewritten",
        &last["appendix"].to_string(),
    );
    let (ours, theirs) = (in_process(&rewritten), from_json(&found));
    report.check(
        "the tool reports the counts the in-process reader reads, rewritten",
        ours.len() == 2 && theirs == ours,
        &format!("tool {theirs:?}\n       here {ours:?}"),
    );
    let (code, text, _) = tool(
        &["verify", "--strict", &rewritten.display().to_string()],
        &[],
    );
    report.check(
        "the report gives the page the text says no signature covers",
        code == 1
            && judged(&found) == Some((false, 1))
            && text.contains("\n    Appended: 1 object: stream, and 1 page was rewritten\n")
            && text.ends_with(
                "\n  After the last signature: 1 page was rewritten, which no signature covers\n",
            ),
        &format!("exit {code}, judged {:?}\n{text}", judged(&found)),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
