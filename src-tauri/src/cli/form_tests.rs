//! `tpdf fields` and `tpdf fill`: their pure parts.
//!
//! Every form here is one `forms::tests` builds and `forms::scan` reads in this
//! process --- the scan a worker runs --- so the grouping, the answers and the
//! problems are tested against the reader the commands receive, and nothing
//! here spawns a worker. `tests/cli.rs` runs both commands end to end through
//! the built tool.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId};

use super::args::{parse, Line};
use super::fields::{self, grouped};
use super::fill::{self, answers, mismatches, resolve, signed_refusal, Answers, Values};
use super::report::{self, FieldKind, NotEditable, ProblemKind};
use super::*;
use crate::forms::{self, tests::fixture, tests::mixed_fixture, Form, Value};

fn argv(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn scanned(doc: &Document) -> Form {
    forms::scan(doc).expect("the fixture's form scans")
}

/// The mixed fixture's form: every kind of control there is.
fn mixed() -> (Document, Form) {
    let (doc, ..) = mixed_fixture();
    let form = scanned(&doc);
    (doc, form)
}

fn given(json: &str) -> Answers {
    answers(json).expect("the answers parse")
}

fn problems(form: &Form, json: &str) -> Vec<(String, ProblemKind)> {
    match resolve(form, &given(json)) {
        Ok(resolved) => panic!("`{json}` was accepted: {resolved:?}"),
        Err(problems) => problems.into_iter().map(|p| (p.field, p.problem)).collect(),
    }
}

// --- the command lines --------------------------------------------------------

#[test]
fn fields_and_fill_lines_parse_and_every_malformed_one_is_refused() {
    let fill = fill::parse(&argv(
        "in.pdf -o out.pdf --values answers.json --force --password-env PW --json",
    ))
    .expect("a whole fill line");
    assert_eq!(fill.input, PathBuf::from("in.pdf"));
    assert_eq!(fill.output, PathBuf::from("out.pdf"));
    assert_eq!(fill.values, Values::File(PathBuf::from("answers.json")));
    assert_eq!(fill.password_env.as_deref(), Some("PW"));
    assert!(fill.force && fill.json);
    let stdin = fill::parse(&argv("in.pdf --output out.pdf --values -")).expect("stdin");
    assert_eq!(stdin.values, Values::Stdin);
    assert!(!stdin.force && !stdin.json);

    let fields = fields::parse(&argv("in.pdf --json --password-env PW")).expect("fields");
    assert_eq!(fields.input, PathBuf::from("in.pdf"));
    assert!(fields.json);

    for (line, says) in [
        ("fill", "needs the document"),
        ("fill a.pdf --values v.json", "-o <out.pdf>"),
        ("fill a.pdf -o b.pdf", "--values"),
        (
            "fill a.pdf -o a.pdf --values v.json",
            "output names the input",
        ),
        (
            "fill a.pdf -o ./a.pdf --values v.json",
            "output names the input",
        ),
        (
            "fill a.pdf -o v.json --values v.json",
            "names the answers file",
        ),
        ("fill a.pdf b.pdf -o c.pdf --values v.json", "second"),
        ("fill a.pdf -o b.pdf --values", "needs a value"),
        (
            "fill a.pdf -o b.pdf --values v.json --pages 1",
            "no option `--pages`",
        ),
        (
            "fill a.pdf -o b.pdf --values v.json --password-env A=b",
            "cannot be one",
        ),
        ("fields", "needs the document"),
        ("fields a.pdf b.pdf", "second"),
        ("fields a.pdf --force", "no option `--force`"),
    ] {
        match parse(&argv(line)) {
            Err(why) => assert!(why.contains(says), "`{line}`: {why}"),
            Ok(parsed) => panic!("`{line}` parsed as {parsed:?}"),
        }
    }
    assert!(matches!(
        parse(&argv("fill a.pdf -o b.pdf --values -")),
        Ok(Line::Run(_))
    ));
}

// --- fields -----------------------------------------------------------------

#[test]
fn fields_are_grouped_by_field_with_their_answers_in_the_form_fill_takes() {
    let (_, form) = mixed();
    let report = fields::report("mixed.pdf", &form);
    let by: std::collections::BTreeMap<&str, &report::Field> =
        report.fields.iter().map(|f| (f.name.as_str(), f)).collect();
    // The emptiness control: nine widgets, seven fields.
    assert_eq!(form.widgets.len(), 9);
    assert_eq!(report.fields.len(), 7, "{:?}", by.keys());

    let text = by["ACME.answer"];
    assert_eq!(text.kind, FieldKind::Text);
    assert_eq!(text.value, serde_json::json!("OLD"));
    assert_eq!((text.widgets, text.pages.clone()), (2, vec![1, 2]));
    assert_eq!(text.max_length, Some(20));
    assert!(text.editable && text.not_editable.is_none() && text.why.is_none());

    assert_eq!(by["consent"].kind, FieldKind::Checkbox);
    assert_eq!(by["consent"].value, serde_json::json!(false));

    let radio = by["delivery"];
    assert_eq!(radio.kind, FieldKind::Radio);
    assert_eq!(radio.value, serde_json::json!("First"));
    let states: Vec<(&str, bool)> = radio
        .options
        .iter()
        .map(|o| (o.export.as_str(), o.selected))
        .collect();
    assert_eq!(states, [("First", true), ("Second", false)]);
    assert_eq!(radio.pages, vec![1, 2]);

    let combo = by["delivery_choice"];
    assert_eq!(combo.kind, FieldKind::ChoiceCombo);
    assert_eq!(combo.value, serde_json::json!("SAME"));
    // Two options share the export value; `selected` is what tells them apart.
    let chosen: Vec<(&str, &str, bool)> = combo
        .options
        .iter()
        .map(|o| (o.export.as_str(), o.label.as_str(), o.selected))
        .collect();
    assert_eq!(
        chosen,
        [
            ("SAME", "First label", true),
            ("SAME", "Second label", false),
            ("OTHER", "Third label", false)
        ]
    );

    let list = by["items"];
    assert_eq!(list.kind, FieldKind::ChoiceList);
    assert!(list.multiple);
    assert_eq!(list.value, serde_json::json!(["SAME"]));
    let single = by["single_item"];
    assert_eq!(single.kind, FieldKind::ChoiceList);
    assert!(!single.multiple);
    assert_eq!(single.value, serde_json::json!("Alpha"));
    let custom = by["custom_choice"];
    assert!(custom.custom_text && !combo.custom_text);

    // The property the value's shape exists for: every field's value is an
    // answer `fill` accepts --- save the two whose export value two options
    // share, which are refused as ambiguous rather than guessed.
    for f in &report.fields {
        let json = serde_json::json!({ f.name.clone(): f.value.clone() }).to_string();
        let resolved = resolve(&form, &given(&json));
        if f.name == "delivery_choice" || f.name == "items" {
            assert_eq!(
                problems(&form, &json),
                [(f.name.clone(), ProblemKind::Ambiguous)]
            );
        } else {
            assert!(resolved.is_ok(), "{}: {resolved:?}", f.name);
        }
    }
}

/// The inherited fixture, with `edit` applied to its terminal text field.
fn acme_with(edit: impl FnOnce(&mut Document, ObjectId, ObjectId)) -> Form {
    let (mut doc, field, check) = fixture();
    edit(&mut doc, field, check);
    scanned(&doc)
}

#[test]
fn every_reason_a_field_is_not_editable_is_named_and_refused() {
    let flagged = |flag: i64| {
        acme_with(|doc, field, _| {
            doc.get_dictionary_mut(field)
                .expect("field")
                .set("Ff", flag);
        })
    };
    let cases = [
        (flagged(1), NotEditable::ReadOnly),
        (flagged(1 << 13), NotEditable::Password),
        (flagged(1 << 20), NotEditable::FileSelect),
        (flagged(1 << 24), NotEditable::Comb),
        (flagged(1 << 25), NotEditable::RichText),
        (
            acme_with(|doc, field, _| {
                // One of its two widgets hidden: the field is not filled at all.
                let kids = doc
                    .get_dictionary(field)
                    .and_then(|d| d.get(b"Kids"))
                    .and_then(Object::as_array)
                    .expect("kids")
                    .clone();
                let second = kids[1].as_reference().expect("a widget");
                doc.get_dictionary_mut(second).expect("widget").set("F", 2);
            }),
            NotEditable::Hidden,
        ),
    ];
    for (form, reason) in cases {
        let report = fields::report("acme.pdf", &form);
        let text = &report.fields[0];
        assert_eq!(text.name, "ACME.answer");
        assert_eq!(text.not_editable, Some(reason), "{reason:?}");
        assert!(!text.editable && text.why.is_some(), "{reason:?}");
        // Whatever the answer: a field that cannot be filled is reported as
        // that, not as a type it would not have taken anyway.
        for answer in [r#"{"ACME.answer": "new"}"#, r#"{"ACME.answer": 7}"#] {
            assert_eq!(
                problems(&form, answer),
                [("ACME.answer".to_string(), ProblemKind::NotEditable)],
                "{reason:?} {answer}"
            );
        }
    }
    // A signature field is listed, as `other`, and never answered.
    let signature = acme_with(|doc, _, check| {
        doc.get_dictionary_mut(check)
            .expect("check")
            .set("FT", "Sig");
    });
    let report = fields::report("acme.pdf", &signature);
    let sig = report
        .fields
        .iter()
        .find(|f| f.name == "consent")
        .expect("listed");
    assert_eq!(
        (sig.kind, sig.not_editable, sig.value.clone()),
        (
            FieldKind::Other,
            Some(NotEditable::Unsupported),
            serde_json::Value::Null
        )
    );
    assert_eq!(
        problems(&signature, r#"{"consent": true}"#),
        [("consent".to_string(), ProblemKind::NotEditable)]
    );
    // The control: the same fixture, unflagged, is editable and answered.
    let plain = acme_with(|_, _, _| {});
    assert!(fields::report("acme.pdf", &plain).fields[0].editable);
    assert!(resolve(&plain, &given(r#"{"ACME.answer": "new"}"#)).is_ok());
    // And a sentence nobody named is `other`, not a guess.
    assert_eq!(fields::not_editable("Something new"), NotEditable::Other);
}

#[test]
fn an_answer_is_checked_against_every_widget_of_its_field() {
    // The second of `ACME.answer`'s two widgets is narrow: the answer fits the
    // first and cannot be drawn legibly in the second, and `forms::write`
    // draws it in both.
    let narrow = acme_with(|doc, field, _| {
        let kids = doc
            .get_dictionary(field)
            .and_then(|d| d.get(b"Kids"))
            .and_then(Object::as_array)
            .expect("kids")
            .clone();
        let second = kids[1].as_reference().expect("a widget");
        doc.get_dictionary_mut(second)
            .expect("widget")
            .set("Rect", vec![20.into(), 70.into(), 40.into(), 100.into()]);
    });
    assert_eq!(
        problems(&narrow, r#"{"ACME.answer": "Hello world"}"#),
        [("ACME.answer".to_string(), ProblemKind::Layout)]
    );
    // The control: both widgets wide, the same answer is fine.
    assert!(resolve(
        &acme_with(|_, _, _| {}),
        &given(r#"{"ACME.answer": "Hello world"}"#)
    )
    .is_ok());
}

// --- fill: resolving ------------------------------------------------------------

#[test]
fn every_kind_of_answer_resolves_to_what_the_writer_takes() {
    let (_, form) = mixed();
    let resolved = resolve(
        &form,
        &given(
            r#"{"ACME.answer": "Grüße", "consent": true, "delivery": "Second",
                "items": ["OTHER", "SAME", "OTHER"], "single_item": "Gamma",
                "custom_choice": "My own"}"#,
        ),
    );
    // "SAME" in a list is ambiguous too; the list here is asked for "OTHER".
    let problems = resolved.expect_err("SAME is shared by two options of `items`");
    assert_eq!(
        problems
            .iter()
            .map(|p| (p.field.as_str(), p.problem))
            .collect::<Vec<_>>(),
        [("items", ProblemKind::Ambiguous)]
    );
    let resolved = resolve(
        &form,
        &given(
            r#"{"ACME.answer": "Grüße", "consent": true, "delivery": "Second",
                "items": ["OTHER", "OTHER"], "single_item": "Gamma",
                "custom_choice": "My own"}"#,
        ),
    )
    .expect("every answer resolves");
    let values: Vec<(&str, &Value)> = resolved
        .iter()
        .map(|r| (r.name.as_str(), &r.value))
        .collect();
    assert_eq!(
        values,
        [
            ("ACME.answer", &Value::Text("Grüße".into())),
            ("consent", &Value::Checked(true)),
            ("delivery", &Value::Selection(vec![1])),
            ("items", &Value::Selection(vec![2])),
            ("single_item", &Value::Selection(vec![2])),
            ("custom_choice", &Value::Text("My own".into())),
        ]
    );
    // An editable dropdown's text that IS an option is the option.
    let option = resolve(&form, &given(r#"{"custom_choice": "Beta"}"#)).expect("an option");
    assert_eq!(option[0].value, Value::Selection(vec![1]));
    // `null` clears a radio group or a single choice; `[]` a list.
    let cleared = resolve(
        &form,
        &given(r#"{"delivery": null, "single_item": null, "items": []}"#),
    )
    .expect("cleared");
    assert!(cleared
        .iter()
        .all(|r| r.value == Value::Selection(Vec::new())));
}

#[test]
fn each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together() {
    let (mut doc, ..) = mixed_fixture();
    // A second field named `items`, so that one name is two fields.
    let single = doc
        .objects
        .iter()
        .find(|(_, o)| {
            o.as_dict()
                .ok()
                .and_then(|d| d.get(b"T").ok())
                .and_then(|t| t.as_str().ok())
                == Some(b"single_item".as_slice())
        })
        .map(|(id, _)| *id)
        .expect("single_item");
    let (plain_doc, ..) = mixed_fixture();
    let plain = scanned(&plain_doc);
    doc.get_dictionary_mut(single)
        .expect("single")
        .set("T", Object::string_literal("ACME.answer"));
    let twice = scanned(&doc);

    let long = "W".repeat(300);
    let cases: Vec<(&Form, String, ProblemKind)> = vec![
        (&plain, r#"{"nobody": "x"}"#.into(), ProblemKind::Unknown),
        (
            &twice,
            r#"{"ACME.answer": "x"}"#.into(),
            ProblemKind::Ambiguous,
        ),
        (
            &plain,
            r#"{"consent": true, "consent": false}"#.into(),
            ProblemKind::Ambiguous,
        ),
        (
            &plain,
            r#"{"delivery_choice": "SAME"}"#.into(),
            ProblemKind::Ambiguous,
        ),
        (&plain, r#"{"consent": "yes"}"#.into(), ProblemKind::Type),
        (&plain, r#"{"ACME.answer": 7}"#.into(), ProblemKind::Type),
        (&plain, r#"{"items": "OTHER"}"#.into(), ProblemKind::Type),
        (
            &plain,
            r#"{"single_item": ["Alpha"]}"#.into(),
            ProblemKind::Type,
        ),
        (
            &plain,
            r#"{"delivery": "Third"}"#.into(),
            ProblemKind::Option,
        ),
        (
            &plain,
            r#"{"single_item": "Delta"}"#.into(),
            ProblemKind::Option,
        ),
        (
            &plain,
            r#"{"items": ["OTHER", "Delta"]}"#.into(),
            ProblemKind::Option,
        ),
        (
            &plain,
            r#"{"ACME.answer": "日本"}"#.into(),
            ProblemKind::Characters,
        ),
        (
            &plain,
            r#"{"ACME.answer": "twenty-one characters"}"#.into(),
            ProblemKind::Length,
        ),
        (
            &plain,
            r#"{"custom_choice": "one\ntwo"}"#.into(),
            ProblemKind::Line,
        ),
        (
            &plain,
            format!(r#"{{"custom_choice": "{long}"}}"#),
            ProblemKind::Layout,
        ),
    ];
    for (form, json, kind) in &cases {
        let found = problems(form, json);
        assert_eq!(found.len(), 1, "{json}: {found:?}");
        assert_eq!(found[0].1, *kind, "{json}: {found:?}");
    }
    // Several problems beside an answer that is fine: every problem is listed,
    // in the order the answers give them, and nothing is resolved.
    let all = r#"{"nobody": "x", "consent": "yes", "delivery": "Third", "delivery_choice": "SAME",
                 "items": "OTHER", "ACME.answer": "日本", "custom_choice": "one\ntwo",
                 "single_item": "Gamma"}"#;
    let listed = problems(&plain, all);
    assert_eq!(
        listed,
        [
            ("nobody".to_string(), ProblemKind::Unknown),
            ("consent".to_string(), ProblemKind::Type),
            ("delivery".to_string(), ProblemKind::Option),
            ("delivery_choice".to_string(), ProblemKind::Ambiguous),
            ("items".to_string(), ProblemKind::Type),
            ("ACME.answer".to_string(), ProblemKind::Characters),
            ("custom_choice".to_string(), ProblemKind::Line),
        ]
    );
    // The control: the fine answer alone resolves.
    assert!(resolve(&plain, &given(r#"{"single_item": "Gamma"}"#)).is_ok());
}

#[test]
fn an_answers_file_is_an_object_with_at_least_one_answer_and_repeats_are_kept() {
    for bad in ["", "[]", "\"x\"", "{", "{\"a\": 1,}", "{}"] {
        assert!(answers(bad).is_err(), "{bad:?}");
    }
    let kept = given(r#"{"b": 1, "a": 2, "b": 3}"#);
    assert_eq!(
        kept.0,
        [
            ("b".to_string(), serde_json::json!(1)),
            ("a".to_string(), serde_json::json!(2)),
            ("b".to_string(), serde_json::json!(3)),
        ]
    );
}

// --- fill: signatures, the plan, the read-back ------------------------------------

#[test]
fn a_signed_or_certified_document_or_one_whose_signatures_were_not_all_read_is_refused() {
    use crate::docinfo::{Limits, Properties, Signature};
    let with = |signatures: Vec<Signature>, limits: Limits| Properties {
        signatures,
        limits,
        ..Properties::default()
    };
    let signed = Signature {
        field: "Signature1".into(),
        signed: true,
        ..Signature::default()
    };
    let certifying = Signature {
        certification: 2,
        ..Signature::default()
    };
    let empty_field = Signature {
        field: "Waiting".into(),
        ..Signature::default()
    };
    for (properties, refused) in [
        (with(vec![signed], Limits::default()), true),
        (with(vec![certifying], Limits::default()), true),
        (
            with(
                Vec::new(),
                Limits {
                    locked: true,
                    ..Limits::default()
                },
            ),
            true,
        ),
        (
            with(
                Vec::new(),
                Limits {
                    unreadable: 1,
                    ..Limits::default()
                },
            ),
            true,
        ),
        (
            with(
                Vec::new(),
                Limits {
                    signatures_dropped: 1,
                    ..Limits::default()
                },
            ),
            true,
        ),
        // The controls: an empty signature field waiting to be signed, and an
        // `/Info` dictionary too long to read whole, say nothing about a
        // signature this would break.
        (with(vec![empty_field], Limits::default()), false),
        (
            with(
                Vec::new(),
                Limits {
                    fields_dropped: 3,
                    values_clipped: 1,
                    ..Limits::default()
                },
            ),
            false,
        ),
        (Properties::default(), false),
    ] {
        let why = signed_refusal("form.pdf", &properties);
        assert_eq!(why.is_some(), refused, "{properties:?}");
    }
    let why = signed_refusal(
        "form.pdf",
        &with(
            vec![Signature {
                signed: true,
                ..Signature::default()
            }],
            Limits::default(),
        ),
    )
    .expect("refused");
    assert!(why.contains("tpdf sign"), "{why}");
}

#[test]
fn the_plan_keeps_every_page_in_its_place_and_carries_every_answer() {
    let (_, form) = mixed();
    let resolved =
        resolve(&form, &given(r#"{"consent": true, "delivery": "Second"}"#)).expect("resolved");
    let file = std::env::current_exe().expect("this test");
    let opened = std::fs::File::open(&file).expect("open");
    let fingerprint =
        crate::fingerprint::Fingerprint::of_open(&opened, &file).expect("fingerprint");
    let plan = fill::plan(3, &resolved, fingerprint);
    assert_eq!(plan.baseline, 3);
    let pages: Vec<(crate::docmodel::PageSource, u8, bool)> = plan
        .pages
        .iter()
        .map(|p| (p.source, p.turns, p.crop.is_none()))
        .collect();
    assert_eq!(
        pages,
        (0..3)
            .map(|i| (crate::docmodel::PageSource::Baseline(i), 0, true))
            .collect::<Vec<_>>()
    );
    assert_eq!(plan.forms.len(), 2);
    assert!(plan.opened_as.is_some());
    assert!(plan.marks.is_empty() && plan.redactions.is_empty() && plan.text_edits.is_empty());
}

/// `doc` with `json` written by the application's writer, saved and read again.
fn written(mut doc: Document, form: &Form, json: &str) -> (Vec<fill::Resolved>, Form) {
    let resolved = resolve(form, &given(json)).expect("resolved");
    let changes: Vec<forms::Change> = resolved
        .iter()
        .map(|r| forms::Change {
            object: r.object,
            value: r.value.clone(),
        })
        .collect();
    forms::write(&mut doc, &changes).expect("written");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    let reread = Document::load_mem(&bytes).expect("reloaded");
    (resolved, scanned(&reread))
}

#[test]
fn a_written_form_reads_back_as_asked_and_a_difference_is_named() {
    let (doc, form) = mixed();
    let json = r#"{"ACME.answer": "Grüße", "consent": true, "delivery": "Second",
                   "items": ["OTHER"], "single_item": "Gamma", "custom_choice": "My own"}"#;
    let (asked, after) = written(doc, &form, json);
    assert_eq!(mismatches(&asked, &form, &after), []);
    let back = fill::read_back(&asked, &after);
    assert_eq!(back.len(), 6);
    assert_eq!(back[2].value, serde_json::json!("Second"));

    // Asked for one thing, written another: the answered field is named.
    let (doc, form) = mixed();
    let (_, other) = written(doc, &form, r#"{"consent": false, "delivery": "First"}"#);
    let wrong = mismatches(&asked, &form, &other);
    let named: std::collections::BTreeSet<&str> = wrong.iter().map(|p| p.field.as_str()).collect();
    assert!(
        named.contains("consent") && named.contains("delivery"),
        "{wrong:?}"
    );
    assert!(wrong.iter().all(|p| p.problem == ProblemKind::ReadBack));

    // A field nobody answered that changed anyway is named too.
    let (doc, form) = mixed();
    let only_consent = resolve(&form, &given(r#"{"consent": true}"#)).expect("resolved");
    let (_, changed) = written(doc, &form, r#"{"consent": true, "single_item": "Beta"}"#);
    let wrong = mismatches(&only_consent, &form, &changed);
    assert_eq!(
        wrong.iter().map(|p| p.field.as_str()).collect::<Vec<_>>(),
        ["single_item"]
    );

    // And a field that is no longer there.
    let (doc, form) = mixed();
    let (_, _) = written(doc, &form, r#"{"consent": true}"#);
    let gone = mismatches(&only_consent, &form, &Form::default());
    assert!(gone.iter().any(|p| p.field == "consent"), "{gone:?}");
}

// --- fill: the refusals that need no worker -------------------------------------

/// Runs a command line in-process with no workers to be had.
fn ran(line: &[String]) -> (i32, String, String) {
    struct Nobody;
    impl Store for Nobody {
        fn identities(&self) -> Result<Vec<Held>, String> {
            Ok(Vec::new())
        }
        fn saved_image(&self) -> Result<Option<crate::signature::Image>, String> {
            Ok(None)
        }
    }
    let env = Env {
        store: &Nobody,
        library_dir: PathBuf::from("/nonexistent/no-workers-here"),
        now: 0,
        program: "tpdf".into(),
        anchors: crate::trust::Anchors::Only(&[]),
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
    let dir = std::env::temp_dir().join(format!("tpdf-cli-form-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

#[test]
fn fill_refusals_that_need_no_worker_exit_before_one_is_asked_for() {
    // `library_dir` names nothing: a refusal that reached a worker would be 4.
    let dir = scratch("fill-refusals");
    let input = dir.join("in.pdf");
    std::fs::write(&input, crate::sign_cms::testkeys::plain_pdf()).expect("input");
    let values = dir.join("answers.json");
    std::fs::write(&values, br#"{"a": "b"}"#).expect("answers");
    let s = |p: &Path| p.display().to_string();
    let fill = |out: &Path, force: bool, values: &Path| {
        let mut args = vec![
            "fill".to_string(),
            s(&input),
            "-o".into(),
            s(out),
            "--values".into(),
            s(values),
        ];
        if force {
            args.push("--force".into());
        }
        ran(&args)
    };

    let out = dir.join("out.pdf");
    std::fs::write(&out, b"keep me").expect("existing");
    let (code, stdout, err) = fill(&out, false, &values);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("--force") && stdout.is_empty(), "{err}");
    assert_eq!(std::fs::read(&out).expect("kept"), b"keep me");

    let alias = dir.join("alias.pdf");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&input, &alias).expect("link");
    #[cfg(windows)]
    std::fs::hard_link(&input, &alias).expect("link");
    let (code, _, err) = fill(&alias, true, &values);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("under another name"), "{err}");
    assert_eq!(
        std::fs::read(&input).expect("input"),
        crate::sign_cms::testkeys::plain_pdf()
    );

    let values_alias = dir.join("answers-alias.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&values, &values_alias).expect("link");
    #[cfg(windows)]
    std::fs::hard_link(&values, &values_alias).expect("link");
    let (code, _, err) = fill(&values_alias, true, &values);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("answers file"), "{err}");
    assert_eq!(std::fs::read(&values).expect("kept"), br#"{"a": "b"}"#);

    // Answers that are not answers are refused before the document is read.
    for (text, says) in [("[1]", "not a JSON object"), ("{}", "nothing to fill")] {
        std::fs::write(&values, text).expect("answers");
        let (code, _, err) = fill(&dir.join("new.pdf"), false, &values);
        assert_eq!(code, 3, "{text}: {err}");
        assert!(err.contains(says), "{text}: {err}");
    }
    let (code, _, err) = fill(&dir.join("new.pdf"), false, &dir.join("missing.json"));
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("could not read the answers"), "{err}");
    assert!(!dir.join("new.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the samples ----------------------------------------------------------------

/// `fields --json` for the mixed fixture, with one field not editable.
pub(super) fn fields_sample() -> report::Fields {
    let (mut doc, ..) = mixed_fixture();
    let custom = doc
        .objects
        .iter()
        .find(|(_, o)| {
            o.as_dict()
                .ok()
                .and_then(|d| d.get(b"T").ok())
                .and_then(|t| t.as_str().ok())
                == Some(b"custom_choice".as_slice())
        })
        .map(|(id, _)| *id)
        .expect("custom_choice");
    doc.get_dictionary_mut(custom)
        .expect("custom")
        .set("Ff", (1 << 17) | (1 << 18) | 1);
    fields::report("form.pdf", &scanned(&doc))
}

/// `fill --json`, written and refused.
pub(super) fn fill_samples() -> (report::Filled, report::Filled) {
    let (doc, form) = mixed();
    let json = r#"{"ACME.answer": "Grüße", "consent": true, "delivery": "Second",
                   "items": ["OTHER"], "custom_choice": "My own"}"#;
    let (asked, after) = written(doc, &form, json);
    let done = report::Filled {
        schema: report::SCHEMA,
        command: "fill".into(),
        input: "form.pdf".into(),
        output: "form-filled.pdf".into(),
        written: true,
        problems: Vec::new(),
        fields: fill::read_back(&asked, &after),
    };
    let refused = report::Filled {
        schema: report::SCHEMA,
        command: "fill".into(),
        input: "form.pdf".into(),
        output: "form-filled.pdf".into(),
        written: false,
        problems: resolve(
            &form,
            &given(r#"{"nobody": "x", "consent": "yes", "ACME.answer": "twenty-one characters"}"#),
        )
        .expect_err("refused"),
        fields: Vec::new(),
    };
    (done, refused)
}

#[test]
fn the_fill_samples_are_what_the_commands_build() {
    let (done, refused) = fill_samples();
    assert!(done.written && done.problems.is_empty() && done.fields.len() == 5);
    assert!(!refused.written && refused.problems.len() == 3 && refused.fields.is_empty());
    let sample = fields_sample();
    assert!(sample.fields.iter().any(|f| !f.editable));
    assert!(grouped(&Form::default()).is_empty());
}
