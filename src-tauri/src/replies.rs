//! One serialised sample of every named reply payload, committed as JSON.
//!
//! `src/lib/ipc.test.ts` diffs the command *names* between `generate_handler!`
//! and `ipc.ts`'s `Commands` map, and stops there on purpose: a name is a
//! string a machine can compare, and a shape is a type on one side of a boundary
//! TypeScript cannot see across. What it could not answer is whether the mirror
//! still describes the reply --- which is the drift `ipc.ts`'s own header
//! records, four hand-written copies of `DocumentInfo` of which two had already
//! stopped listing the same fields.
//!
//! These files close that. Rust writes the bytes it would actually send, this
//! module pins them, and `src/lib/replyshapes.test.ts` reads the same files and
//! checks them against the TypeScript mirror. Neither side is generated from the
//! other, so the two can disagree, and a disagreement is what the JSON in the
//! middle makes visible: a renamed Rust field changes the sample, and the sample
//! no longer satisfies the mirror.
//!
//! **What a sample is chosen to be.** Not `T::default()`, which is the cheap
//! route and weaker than it looks: it pins field names and JSON types and
//! nothing else. Every `Option` here is `Some` at least once, because
//! `skip_serializing_if` means a `None` writes no key at all and a mirror can
//! then be wrong about a field the sample never mentions; every `Vec` is
//! non-empty for the same reason; and where a nested enum has several arms, the
//! containing list carries more than one so an arm the mirror omits is in the
//! bytes rather than in nobody's sample.
//!
//! **Regenerate with `TPDF_REPLIES=write cargo test --lib
//! replies::tests::every_named_reply_payload_serialises_to_its_committed_sample`**, which rewrites the files from the
//! samples below and is the only supported way to change them --- editing one by
//! hand states what Rust sends without asking Rust.
//!
//! The directory is compared as a **set**, both ways. A type with no file fails,
//! which is what makes a new payload visible instead of silently uncovered; a
//! file with no type fails too, which is what a removed payload leaves behind.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{annots, docinfo, docmodel, edits, encoding, links, outline, redact, render, save};
use crate::{search, session, structure, text, xmp};

/// Where the committed samples live.
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("replies")
}

/// A comment, with every optional field carrying a value.
fn comment_full() -> annots::Comment {
    annots::Comment {
        id: 12,
        page: 0,
        kind: annots::Kind::Text,
        author: "A. Reader".into(),
        body: "the first note".into(),
        subject: "a subject".into(),
        date: Some("2026-01-02T03:04:05Z".into()),
        rect: [10.0, 20.0, 110.0, 60.0],
        quads: vec![10.0, 20.0, 110.0, 20.0, 10.0, 60.0, 110.0, 60.0],
        reply_to: Some(11),
        hidden: false,
        color: Some([1.0, 0.9, 0.2]),
        object: Some((7, 0)),
    }
}

/// The same shape with every optional field empty, so both spellings are pinned.
fn comment_bare() -> annots::Comment {
    annots::Comment {
        id: 13,
        page: 1,
        kind: annots::Kind::Highlight,
        author: String::new(),
        body: "the second note".into(),
        subject: String::new(),
        date: None,
        rect: [0.0, 0.0, 1.0, 1.0],
        quads: Vec::new(),
        reply_to: None,
        hidden: true,
        color: None,
        object: None,
    }
}

/// A certificate with both of its optional lists present.
fn certificate() -> docinfo::Certificate {
    docinfo::Certificate {
        subject: "CN=A. Signer,O=Example".into(),
        subject_cn: "A. Signer".into(),
        issuer: "CN=Example CA,O=Example".into(),
        issuer_cn: "Example CA".into(),
        serial: "0a0b0c".into(),
        // Both in the past, and deliberately: `scripts/check_dates.py` reads
        // any `20dd-dd-dd` in a tracked file as a claim about when something
        // was measured, and a sample is not the place to argue with it. The
        // `T` after the day happens to keep these out of its regex today, which
        // is not a property to depend on.
        from: "2025-01-01T00:00:00Z".into(),
        until: "2026-01-01T00:00:00Z".into(),
        self_issued: false,
        chain: 2,
        matched_signer: true,
        key_usage: Some(vec!["digitalSignature".into()]),
        extended_usage: Some(vec!["1.2.840.113583.1.1.5".into()]),
        authority: Some(false),
        extensions_unread: 1,
    }
}

/// Every named payload a `#[tauri::command]` answers with, as it goes on the wire.
///
/// Keyed by the Rust type's own name, which is also the file's stem and the name
/// `src/lib/replyshapes.test.ts` reads them back under.
fn samples() -> BTreeMap<&'static str, String> {
    let mut out: BTreeMap<&'static str, String> = BTreeMap::new();
    let mut put = |name: &'static str, value: &dyn erased::Sample| {
        out.insert(name, value.pretty());
    };

    put(
        "DocumentInfo",
        &render::DocumentInfo {
            id: 1,
            pages: vec![
                render::PageSize {
                    width_pt: 595.0,
                    height_pt: 842.0,
                },
                render::PageSize {
                    width_pt: 612.0,
                    height_pt: 792.0,
                },
            ],
            page_count: 2,
            lazy_geometry: false,
            open_ms: 12.5,
            at_ms: 340.25,
        },
    );

    put(
        "CropGeometry",
        &render::CropGeometry {
            width_pt: 300.0,
            height_pt: 400.0,
            left: 20.0,
            top: 30.0,
        },
    );

    put(
        "Comments",
        &annots::Comments {
            items: vec![comment_full(), comment_bare()],
            limits: annots::Limits {
                crowded_pages: 1,
                over_budget: true,
                bodies_clipped: 2,
                unknown_kinds: 3,
                unreadable: 4,
                cycles: 5,
                pages_missed: 6,
            },
            scan_ms: 1.5,
        },
    );

    put(
        "Properties",
        &docinfo::Properties {
            version: "1.7".into(),
            bytes: 123_456,
            pages: 2,
            revisions: 3,
            fields: vec![docinfo::Field {
                name: "Title".into(),
                value: "A document".into(),
                standard: true,
            }],
            encryption: Some(docinfo::Encryption {
                method: "AESV3".into(),
                revision: 6,
                opened_without_password: false,
                permissions: vec![
                    docinfo::Permission {
                        what: "print".into(),
                        allowed: true,
                    },
                    docinfo::Permission {
                        what: "copy".into(),
                        allowed: false,
                    },
                ],
            }),
            signatures: vec![docinfo::Signature {
                field: "Signature1".into(),
                signed: true,
                handler: "Adobe.PPKLite".into(),
                kind: "ETSI.CAdES.detached".into(),
                name: "A. Signer".into(),
                reason: "I approve".into(),
                location: "Hamburg".into(),
                when: "2026-01-02T03:04:05Z".into(),
                covers_whole_file: false,
                covered_bytes: 100_000,
                appended_bytes: 23_456,
                appendix: Some(docinfo::Appendix {
                    added: 4,
                    replaced: 1,
                    kinds: vec!["/Annot".into()],
                    catalog_gained: vec!["/AcroForm".into()],
                    pages_touched: 1,
                    unread: false,
                }),
                certification: 1,
                certificate: Some(certificate()),
                timestamp: Some(docinfo::Timestamp {
                    when: "2026-01-02T03:04:06Z".into(),
                    authority: Some(certificate()),
                }),
            }],
            tagged: Some(true),
            language: "en-GB".into(),
            attachments: Some(2),
            xmp: Some(xmp::Xmp {
                bytes: 2048,
                conformance: vec!["PDF/A-2b".into()],
                unread: false,
            }),
            limits: docinfo::Limits {
                locked: false,
                fields_dropped: 1,
                values_clipped: 2,
                timestamps_unread: 3,
                signatures_dropped: 4,
                unreadable: 5,
                certificates_unread: 6,
            },
            scan_ms: 11.75,
        },
    );

    put("Form", &{
        let text = crate::forms::Widget {
            object: (12, 0),
            widget: (13, 0),
            page: 0,
            rect: [10.0, 20.0, 110.0, 40.0],
            display_rect: [10.0, 20.0, 110.0, 40.0],
            name: "ACME.answer".into(),
            value: crate::forms::Value::Text("answer".into()),
            control: crate::forms::Control::Text,
            multiline: false,
            max_length: Some(20),
            reason: Some("Read-only".into()),
        };
        let mut radio = text.clone();
        radio.value = crate::forms::Value::Selection(vec![1]);
        radio.control = crate::forms::Control::Radio {
            index: 0,
            states: vec![b"One".to_vec(), b"Two".to_vec()],
            unison: false,
            no_toggle_off: true,
        };
        let mut choice = text.clone();
        choice.value = crate::forms::Value::Selection(vec![0]);
        choice.control = crate::forms::Control::Choice {
            options: vec![crate::forms::Choice {
                export: "VALUE".into(),
                label: "Visible label".into(),
            }],
            combo: false,
            editable: false,
            multiple: true,
        };
        crate::forms::Form {
            widgets: vec![text, radio, choice],
        }
    });

    put(
        "EditState",
        &edits::EditState {
            forms: vec![crate::forms::Change {
                object: (12, 0),
                value: crate::forms::Value::Text("ACME answer".into()),
            }],
            pages: vec![
                edits::PageView {
                    id: 1,
                    source: docmodel::PageSource::Baseline(0),
                    turns: 1,
                    crop: Some([10.0, 20.0, 300.0, 400.0]),
                },
                edits::PageView {
                    id: 2,
                    source: docmodel::PageSource::Blank(docmodel::Size {
                        width: 595.0,
                        height: 842.0,
                    }),
                    turns: 0,
                    crop: None,
                },
            ],
            can_undo: true,
            can_redo: false,
            marks: vec![
                edits::MarkView {
                    id: 3,
                    kind: docmodel::MarkKind::Highlight,
                    page: 1,
                    quads: vec![10.0, 20.0, 110.0, 20.0, 10.0, 60.0, 110.0, 60.0],
                    strokes: Vec::new(),
                    stamp: None,
                    image: None,
                    color: [1.0, 0.9, 0.2],
                    width: 1.0,
                    note: "a note on the mark".into(),
                    lines: vec!["the line under it".into()],
                },
                edits::MarkView {
                    id: 4,
                    kind: docmodel::MarkKind::Stamp,
                    page: 2,
                    quads: vec![0.0, 0.0, 100.0, 40.0],
                    strokes: vec![vec![1.0, 2.0, 3.0, 4.0]],
                    stamp: Some(docmodel::StampName::Draft),
                    image: None,
                    color: [0.8, 0.1, 0.1],
                    width: 2.5,
                    note: String::new(),
                    lines: Vec::new(),
                },
                edits::MarkView {
                    id: 6,
                    kind: docmodel::MarkKind::Signature,
                    page: 2,
                    quads: vec![10.0, 20.0, 110.0, 70.0],
                    strokes: Vec::new(),
                    stamp: None,
                    image: Some(std::sync::Arc::new(crate::signature::Image {
                        width: 2,
                        height: 1,
                        rgba: vec![10, 20, 30, 255, 40, 50, 60, 128],
                    })),
                    color: [0.0; 3],
                    width: 1.0,
                    note: String::new(),
                    lines: Vec::new(),
                },
            ],
            redactions: vec![edits::RedactionView {
                id: 5,
                page: 1,
                area: [1.0, 2.0, 3.0, 4.0],
            }],
            notes: vec![edits::NoteEditView {
                object: (9, 0),
                page: 1,
                body: "the rewritten body".into(),
                made: "2026-01-02T03:04:05Z".into(),
                shown: Some("2026-01-02T03:04:05Z".into()),
            }],
            discards: vec![edits::DiscardView {
                object: (10, 0),
                page: 2,
            }],
            dirty: true,
        },
    );

    put(
        "PageMapping",
        &encoding::PageMapping {
            composite: 3,
            guessing: 1,
            truncated: true,
        },
    );

    put(
        "Links",
        &links::Links {
            items: vec![
                links::Link {
                    id: 1,
                    page: 0,
                    rect: [10.0, 20.0, 110.0, 60.0],
                    target: outline::Target::Page {
                        page: 2,
                        top_pt: Some(700.0),
                    },
                },
                links::Link {
                    id: 2,
                    page: 0,
                    rect: [0.0, 0.0, 10.0, 10.0],
                    target: outline::Target::Broken,
                },
                links::Link {
                    id: 3,
                    page: 1,
                    rect: [0.0, 0.0, 10.0, 10.0],
                    target: outline::Target::Refused {
                        action: "Launch".into(),
                    },
                },
                links::Link {
                    id: 4,
                    page: 1,
                    rect: [0.0, 0.0, 10.0, 10.0],
                    target: outline::Target::None,
                },
                // The fifth arm, and the sample carries it for the reason the
                // header gives: an arm no sample mentions is one the mirror can
                // be wrong about indefinitely.
                links::Link {
                    id: 5,
                    page: 1,
                    rect: [0.0, 0.0, 10.0, 10.0],
                    target: outline::Target::Web {
                        token: 0,
                        host: "example.com".into(),
                        rest: "/spec#section-4".into(),
                    },
                },
            ],
            limits: links::Limits {
                crowded_pages: 1,
                over_budget: true,
                unreadable: 2,
                unresolved_names: 3,
                pages_missed: 4,
            },
            scan_ms: 2.25,
            // Non-empty, so the key is in the bytes and `UNMIRRORED`'s entry
            // for it names a field that is really sent. What the *frontend*
            // receives is empty, because `document_links` drains it --- see
            // `webopen::Registry::adopt`, which is where that guarantee lives
            // rather than here.
            urls: vec!["https://example.com/spec#section-4".into()],
        },
    );

    put(
        "Outline",
        &outline::Outline {
            items: vec![outline::OutlineItem {
                title: "Chapter one".into(),
                open: true,
                target: outline::Target::Page {
                    page: 0,
                    top_pt: None,
                },
                children: vec![
                    outline::OutlineItem {
                        title: "A section".into(),
                        open: false,
                        target: outline::Target::Broken,
                        children: Vec::new(),
                    },
                    // A web entry here too: the outline's `Target` is the same
                    // type as a link's, and a sample that exercised the arm on
                    // one side only would leave the other's mirror untested.
                    outline::OutlineItem {
                        title: "Further reading".into(),
                        open: false,
                        target: outline::Target::Web {
                            token: 0,
                            host: "example.org".into(),
                            rest: "/further".into(),
                        },
                        children: Vec::new(),
                    },
                ],
            }],
            total: 3,
            limits: outline::Limits {
                cycles: 1,
                too_deep: 2,
                over_budget: true,
                titles_clipped: 3,
            },
            walk_ms: 0.75,
            // Non-empty for `Links::urls`'s reason, and its own list: the two
            // scans number their tokens independently.
            urls: vec!["https://example.org/further".into()],
        },
    );

    put(
        "Applied",
        &redact::Applied {
            regions: 2,
            shows: 5,
            changed: true,
            verified: false,
            why: vec!["one object could not be read".into()],
        },
    );

    put(
        "RegionPlan",
        &redact::RegionPlan {
            shows: vec![1, 2],
            text_objects: 2,
            images: vec![3],
            image_objects: 1,
            form_shows: vec![(4, 5)],
            form_text_objects: vec![(4, 6)],
            area: [10.0, 20.0, 110.0, 60.0],
            taking: "the quick brown fox".into(),
            unhandled: vec![redact::Unhandled {
                at: 7,
                kind: "Type3".into(),
                drawn: Some(3),
            }],
        },
    );

    put("Copied", &save::Copied { changed: true });

    put(
        "Merged",
        &save::Merged {
            changed: false,
            pages: 6,
            files: 3,
        },
    );

    put(
        "Split",
        &save::Split {
            changed: true,
            paths: vec!["/tmp/one.pdf".into(), "/tmp/two.pdf".into()],
        },
    );

    put(
        "PageMatches",
        &search::PageMatches {
            page: 1,
            matches: vec![search::Match {
                page: 1,
                start: 10,
                end: 14,
                end_page: Some(2),
                before: "the ".into(),
                hit: "quick".into(),
                after: " brown".into(),
            }],
            chars: 1200,
            problem: Some("the page has no character map".into()),
            tail: Some(search::Carry {
                page: 1,
                from: 1190,
                codes: vec![113, 117, 105],
            }),
            more: vec![search::PageMatches {
                page: 2,
                matches: Vec::new(),
                chars: 900,
                problem: None,
                tail: None,
                more: Vec::new(),
            }],
        },
    );

    put(
        "Session",
        &session::Session {
            places: vec![session::Place {
                path: "/tmp/one.pdf".into(),
                page: 3,
                top_pt: 120.5,
                zoom: 1.25,
                fit: session::Fit::Page,
                turns: 2,
                sidebar: true,
                page_count: 40,
            }],
            invert_pages: true,
        },
    );

    put(
        "PageText",
        &text::PageText {
            codes: vec![84, 104, 101],
            boxes: vec![10.0, 20.0, 18.0, 32.0],
            height_pt: 842.0,
            width_pt: 595.0,
            quarter_turns: 1,
            extract_ms: 3.5,
            runs: vec![structure::TaggedRun {
                tag: "H1".into(),
                path: vec!["Document".into(), "Sect".into()],
                start: 0,
                end: 3,
            }],
        },
    );

    put(
        "ScrollBenchConfig",
        &crate::commands::spike::ScrollBenchConfig {
            path: "/tmp/one.pdf".into(),
            rounds: 3,
            frames: 120,
            warmup_frames: 10,
            px_per_frame: 40.0,
            tile_px: 512,
            zooms: vec![1.0, 2.0],
            layouts: vec!["single".into()],
            cache_tiles: 64,
            max_in_flight: 4,
            prefetch_screens: 1.5,
            cancels: vec![0, 1],
        },
    );

    out
}

/// Serialising without naming each type twice.
///
/// `put` above takes `&dyn Sample` rather than a generic parameter so the whole
/// table is one expression per payload; a generic closure cannot be called with
/// seventeen different types.
mod erased {
    /// Anything that can write itself as the JSON a command would send.
    pub trait Sample {
        /// The pretty-printed bytes, which is what the file holds.
        fn pretty(&self) -> String;
    }

    impl<T: serde::Serialize> Sample for T {
        fn pretty(&self) -> String {
            serde_json::to_string_pretty(self).expect("a reply payload serialises")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dir, samples};

    /// Whether this run is allowed to rewrite the committed samples.
    fn writing() -> bool {
        std::env::var("TPDF_REPLIES").as_deref() == Ok("write")
    }

    #[test]
    fn every_named_reply_payload_serialises_to_its_committed_sample() {
        let at = dir();
        if writing() {
            std::fs::create_dir_all(&at).expect("the samples directory");
        }
        let mut wrong = Vec::new();
        for (name, json) in samples() {
            let file = at.join(format!("{name}.json"));
            let text = format!("{json}\n");
            if writing() {
                std::fs::write(&file, &text).expect("write a sample");
                continue;
            }
            match std::fs::read_to_string(&file) {
                // A missing file is a failure and not a reason to write one.
                // A type added without a sample is otherwise silently uncovered,
                // which is the emptiness control this repository asks for
                // everywhere -- and a check that repairs itself on the run that
                // finds the disagreement can never report one.
                Err(why) => wrong.push(format!("{name}.json could not be read: {why}")),
                Ok(found) if found != text => wrong.push(format!(
                    "{name}.json is not what {name} serialises to.\n\
                     --- committed ---\n{found}\n--- now ---\n{text}"
                )),
                Ok(_) => {}
            }
        }
        assert!(
            wrong.is_empty(),
            "{} sample(s) disagree with the Rust types. \
             Regenerate with TPDF_REPLIES=write and read the diff -- a changed \
             sample is a changed wire shape, and `src/lib/ipc.ts` mirrors it.\n\n{}",
            wrong.len(),
            wrong.join("\n\n")
        );
    }

    #[test]
    fn the_samples_directory_holds_one_file_per_payload_and_nothing_else() {
        // The other direction, and the one that goes stale on its own: a payload
        // that stops being a reply leaves its sample behind, and `ipc.ts` then
        // keeps a mirror nothing sends. Compared as a set, so a failure names
        // which side is over.
        let at = dir();
        let mut found: Vec<String> = std::fs::read_dir(&at)
            .unwrap_or_else(|why| panic!("the samples directory at {at:?}: {why}"))
            .map(|entry| entry.expect("a directory entry").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        found.sort();
        let mut want: Vec<String> = samples()
            .keys()
            .map(|name| format!("{name}.json"))
            .collect();
        want.sort();
        assert!(
            !want.is_empty(),
            "no samples at all, so this check compares nothing"
        );
        assert_eq!(found, want, "the samples directory and the payload table");
    }
}
