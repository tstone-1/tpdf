//! `tpdf form`: a copy with form fields added.
//!
//! `fields` lists a form and `fill` answers it; this is what makes a field
//! exist. The work is `formfields::add`, which the one rewrite every copy takes
//! runs with the plan this command builds. What is here is the reading of the
//! list, and the reading back: the staged copy is opened again and each field
//! has to be there once, of the kind and on the page and at the place asked
//! for, empty and answerable, with every field the source had still holding
//! what it held. A copy that fails that is not published.

use std::io::Write;
use std::path::PathBuf;

use serde::Deserialize;

use super::args::{lexically_same, unknown, value};
use super::fields::{ask_form, grouped, kind, value_json};
use super::fill::{read_values, SignedState, Values};
use super::pages::{check_target, copy_of, publish_copy, read_input, same_sizes};
use super::report::{self, AddedField, FieldKind, SCHEMA};
use super::text::{password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::formfields::{Kind, NewField};
use crate::forms::Form;

pub const COMMAND: Registered = Registered {
    name: "form",
    usage: "form <in.pdf> -o <out.pdf> --fields <fields.json | ->\n        [--password-env VAR] [--invalidate-signatures] [--force] [--json]",
    summary: "Adds form fields from a JSON array (read from stdin for -). Each is\n            an object with a name, a kind (text, multiline, checkbox, dropdown or radio),\n            a page counted from 1 and a rect [left, top, width, height] in points\n            from the page's top-left corner; a dropdown also has options, the\n            list of its choices, and a radio button one option, its value, and\n            the name of its group; tooltip, required, max_length, text_size (in points) and default_value (what\n            a text field starts with) are optional. Every field is checked first, and one problem means\n            nothing is written; the copy is read back before success is reported.",
    parse: |args| parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFields {
    pub input: PathBuf,
    pub output: PathBuf,
    pub fields: Values,
    pub password_env: Option<String>,
    pub invalidate: bool,
    pub force: bool,
    pub json: bool,
}

pub fn parse(args: &[String]) -> Result<AddFields, String> {
    let mut paths = Vec::new();
    let mut output = None;
    let mut fields = None;
    let mut password_env = None;
    let (mut invalidate, mut force, mut json, mut positional) = (false, false, false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--fields" => {
                fields = Some(match value(arg, &mut rest)?.as_str() {
                    "-" => Values::Stdin,
                    path => Values::File(PathBuf::from(path)),
                });
            }
            "--password-env" => password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => invalidate = true,
            "--force" => force = true,
            "--json" => json = true,
            flag if flag.starts_with('-') => return Err(unknown("form", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("form needs exactly one input document".into());
    }
    let input = paths.remove(0);
    let output = output.ok_or("-o <out.pdf> is required; the input is never overwritten")?;
    if lexically_same(&input, &output) {
        return Err("the output names the input; choose a different name".into());
    }
    let fields = fields.ok_or(
        "form needs `--fields <fields.json>`, or `--fields -` to read the list from standard \
         input",
    )?;
    if let Values::File(path) = &fields {
        if lexically_same(path, &output) {
            return Err("`-o` names the file of fields".into());
        }
    }
    Ok(AddFields {
        input,
        output,
        fields,
        password_env,
        invalidate,
        force,
        json,
    })
}

/// One field as the list spells it. The page is counted from 1, as every page
/// this tool takes or prints is; [`NewField`] counts from 0.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    name: String,
    kind: Kind,
    page: u32,
    rect: [f64; 4],
    #[serde(default)]
    tooltip: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    max_length: Option<u32>,
    #[serde(default)]
    border: bool,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    text_size: Option<f32>,
    #[serde(default)]
    default_value: Option<String>,
}

/// The fields a list asks for, or why it is not a list of fields.
pub fn fields(text: &str) -> Result<Vec<NewField>, String> {
    let specs: Vec<Spec> = serde_json::from_str(text).map_err(|e| {
        format!(
            "the fields are not a JSON array of fields ({e}) --- each is an object like \
             {{\"name\": \"Name\", \"kind\": \"text\", \"page\": 1, \"rect\": [72, 100, 200, 20]}}"
        )
    })?;
    if specs.is_empty() {
        return Err("the list names no field, so there is nothing to add".into());
    }
    specs
        .into_iter()
        .map(|spec| {
            let page = spec.page.checked_sub(1).ok_or_else(|| {
                format!(
                    "`{}`: pages are counted from 1, and its page is 0",
                    spec.name
                )
            })?;
            Ok(NewField {
                name: spec.name,
                kind: spec.kind,
                page,
                rect: spec.rect,
                tooltip: spec.tooltip,
                required: spec.required,
                max_length: spec.max_length,
                border: spec.border,
                options: spec.options,
                text_size: spec.text_size,
                default_value: spec.default_value,
            })
        })
        .collect()
}

/// What the copy has to hold and does not, or the fields as they read back.
///
/// A second reading of the written file through `forms::scan`, which shares
/// the page tree and the field walk with the writer and none of its writing.
pub fn read_back(
    asked: &[NewField],
    before: &Form,
    after: &Form,
) -> Result<Vec<AddedField>, Vec<String>> {
    let now = grouped(after);
    let mut problems = Vec::new();
    let mut added = Vec::new();
    for field in asked {
        let name = &field.name;
        let found: Vec<_> = now.iter().filter(|g| g.name == name).collect();
        let [group] = found.as_slice() else {
            problems.push(format!(
                "`{name}`: the written file has {} fields of this name, where it should have one",
                found.len()
            ));
            continue;
        };
        // A group of radio buttons is one field with a box for each button;
        // the one asked for is the box that gives the group its value.
        let radio = field.kind == Kind::Radio;
        let value = field.options.first().map_or("", String::as_str);
        let widget = if radio {
            group.widgets.iter().find(|w| {
                matches!(&w.control, crate::forms::Control::Radio { index, states, .. }
                    if states.get(*index).is_some_and(|state| state == value.as_bytes()))
            })
        } else {
            match group.widgets.as_slice() {
                [widget] => Some(widget),
                _ => None,
            }
        };
        let Some(widget) = widget else {
            problems.push(if radio {
                format!("`{name}` reads back with no button of the value `{value}`")
            } else {
                format!("`{name}` reads back with {} boxes", group.widgets.len())
            });
            continue;
        };
        let wanted = match field.kind {
            Kind::Checkbox => FieldKind::Checkbox,
            Kind::Text | Kind::Multiline => FieldKind::Text,
            Kind::Dropdown => FieldKind::ChoiceCombo,
            Kind::Radio => FieldKind::Radio,
            // `fields` lists a signature field among the kinds it does not fill.
            Kind::Signature => FieldKind::Other,
        };
        let signature = field.kind == Kind::Signature;
        let unsigned = matches!(
            widget.control,
            crate::forms::Control::Signature { signed: false }
        );
        let got = kind(&widget.control);
        let [left, top, right, bottom] = widget.display_rect;
        let rect = [left, top, right - left, bottom - top];
        let moved = rect
            .iter()
            .zip(field.rect)
            .any(|(got, want)| (f64::from(*got) - want).abs() > 0.01);
        let empty = match field.kind {
            Kind::Checkbox => serde_json::Value::Bool(false),
            // Nothing chosen yet, and nothing signed.
            Kind::Dropdown | Kind::Signature => serde_json::Value::Null,
            // What the group held before, when the button joined one.
            Kind::Radio => before
                .widgets
                .iter()
                .find(|was| &was.name == name)
                .map_or(serde_json::Value::Null, |was| {
                    value_json(&was.control, &was.value)
                }),
            // The default value it was given, or nothing.
            _ => serde_json::Value::String(field.default_value.clone().unwrap_or_default()),
        };
        let held = value_json(&widget.control, &widget.value);
        if got != wanted
            || widget.multiline != (field.kind == Kind::Multiline)
            || signature != unsigned
        {
            problems.push(format!("`{name}` reads back as another kind of field"));
        } else if widget.page != field.page {
            problems.push(format!("`{name}` reads back on page {}", widget.page + 1));
        } else if moved {
            problems.push(format!("`{name}` reads back at {rect:?}"));
        } else if held != empty {
            problems.push(format!("`{name}` reads back holding {held}"));
        } else if let Some(reason) = widget.reason.as_ref().filter(|_| !signature) {
            problems.push(format!(
                "`{name}` reads back as one that cannot be filled: {reason}"
            ));
        } else if widget.max_length != field.max_length.map(|most| most as usize) {
            problems.push(format!("`{name}` reads back with another most characters"));
        } else if widget.text_size != field.text_size {
            problems.push(format!("`{name}` reads back with another text size"));
        } else if widget.default_value != field.default_value.clone().unwrap_or_default() {
            problems.push(format!("`{name}` reads back with another default value"));
        } else {
            added.push(AddedField {
                name: name.clone(),
                kind: got,
                multiline: widget.multiline,
                page: widget.page + 1,
                rect,
            });
        }
    }
    // Every box the source had is still there, holding what it held.
    if after.widgets.len() != before.widgets.len() + asked.len() {
        problems.push(format!(
            "the written file has {} form boxes, where the source's {} and the {} added make {}",
            after.widgets.len(),
            before.widgets.len(),
            asked.len(),
            before.widgets.len() + asked.len()
        ));
    }
    for was in &before.widgets {
        let kept = after
            .widgets
            .iter()
            .any(|w| w.name == was.name && w.page == was.page && w.value == was.value);
        if !kept {
            problems.push(format!(
                "`{}`, which the source had, does not read back as it was",
                was.name
            ));
        }
    }
    if problems.is_empty() {
        Ok(added)
    } else {
        Err(problems)
    }
}

fn plain(report: &report::FormAdded) -> String {
    let mut lines = vec![format!(
        "{}: {} added, {} in the form now",
        report.output,
        match report.added.len() {
            1 => "1 field".to_string(),
            n => format!("{n} fields"),
        },
        report.fields_after
    )];
    for field in &report.added {
        lines.push(format!(
            "  {} ({}), page {}",
            field.name,
            match (field.kind, field.multiline) {
                (FieldKind::Checkbox, _) => "checkbox",
                (FieldKind::ChoiceCombo, _) => "dropdown",
                (FieldKind::Radio, _) => "radio button",
                // The only kind `form` adds that `fields` does not fill.
                (FieldKind::Other, _) => "signature, empty",
                (_, true) => "text, several lines",
                _ => "text",
            },
            field.page
        ));
    }
    if report.signatures_invalidated > 0 || report.signatures_unknown {
        lines.push("The rewrite invalidates existing signatures.".into());
    }
    lines.join("\n")
}

impl Subcommand for AddFields {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        check_target(inputs, &self.output, self.force)?;
        let shown = self.input.display().to_string();
        let asked = fields(&read_values(&self.fields, "fields")?)
            .map_err(|why| Failure::new(Exit::Refused, why))?;
        let key = password(self.password_env.as_deref())?;
        let (mut input, mut session) = read_input(env, &self.input, key.as_deref())?;
        let before = ask_form(&mut session, &shown, key.is_some())?;
        drop(session);
        if input.signed.is_some() && !self.invalidate {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{shown} is signed or its signatures could not be fully read; adding a \
                     field rewrites the document and requires --invalidate-signatures"
                ),
            ));
        }
        input.plan.new_fields = asked.clone();

        let unpublished = |what: String| {
            Failure::new(
                Exit::Internal,
                format!("the staged file {what}; no output was published"),
            )
        };
        let (form, added) = publish_copy(
            inputs,
            &[(self.input.as_path(), input.opened_as())],
            &self.output,
            self.force,
            copy_of(env, &self.input, &input.plan, key.as_deref()),
            |staged, ()| {
                let (after, mut session) =
                    read_input(env, staged, key.as_deref()).map_err(|why| {
                        unpublished(format!("could not be opened again: {}", why.message))
                    })?;
                let form =
                    ask_form(&mut session, "the staged file", key.is_some()).map_err(|why| {
                        unpublished(format!("has a form that cannot be read: {}", why.message))
                    })?;
                drop(session);
                if after.encrypted != input.encrypted || !same_sizes(&after.sizes, &input.sizes) {
                    return Err(unpublished(
                        "does not have the source's pages or encryption".into(),
                    ));
                }
                let added = read_back(&asked, &before, &form).map_err(|problems| {
                    unpublished(format!(
                        "is not what was asked for: {}",
                        problems.join("; ")
                    ))
                })?;
                Ok((form, added))
            },
        )?;

        let report = report::FormAdded {
            schema: SCHEMA,
            command: "form".into(),
            input: shown,
            output: self.output.display().to_string(),
            pages: input.plan.baseline,
            added,
            fields_before: grouped(&before).len(),
            fields_after: grouped(&form).len(),
            signatures_invalidated: match input.signed {
                Some(SignedState::Signed(n)) => n,
                _ => 0,
            },
            signatures_unknown: input.signatures_unknown,
        };
        if self.json {
            json(out, &report);
        } else {
            say(out, &plain(&report));
        }
        Ok(Exit::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::{Control, Value, Widget};

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn form_needs_one_document_an_output_and_a_list() {
        for line in [
            "",
            "a.pdf",
            "a.pdf -o b.pdf",
            "a.pdf --fields f.json",
            "a.pdf b.pdf -o c.pdf --fields f.json",
            "a.pdf -o a.pdf --fields f.json",
            "a.pdf -o b.pdf --fields b.pdf",
            "a.pdf -o b.pdf --fields",
            "a.pdf -o b.pdf --fields f.json --unknown",
            "a.pdf -o b.pdf --fields f.json --password-env A=B",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
        let parsed = parse(&args(
            "a.pdf -o b.pdf --fields f.json --password-env KEY --invalidate-signatures --json",
        ))
        .unwrap();
        assert_eq!(parsed.fields, Values::File("f.json".into()));
        assert_eq!(parsed.password_env.as_deref(), Some("KEY"));
        assert!(parsed.invalidate && parsed.json && !parsed.force);
        let piped = parse(&args("a.pdf --output b.pdf --fields - --force")).unwrap();
        assert_eq!(piped.fields, Values::Stdin);
        assert!(piped.force && !piped.invalidate && !piped.json);
    }

    #[test]
    fn a_list_counts_pages_from_one() {
        let asked = fields(
            r#"[{"name":"Name","kind":"text","page":1,"rect":[72,100,200,20],"max_length":40,
                 "text_size":9.5,"default_value":"n/a"},
                {"name":"Agree","kind":"checkbox","page":3,"rect":[72,140,12,12],
                 "tooltip":"Tick to agree","required":true,"border":true}]"#,
        )
        .unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].text_size, Some(9.5));
        assert_eq!(asked[0].default_value.as_deref(), Some("n/a"));
        assert_eq!(
            (asked[1].text_size, asked[1].default_value.clone()),
            (None, None)
        );
        assert_eq!((asked[0].page, asked[1].page), (0, 2));
        assert_eq!(asked[0].max_length, Some(40));
        assert_eq!(asked[1].tooltip.as_deref(), Some("Tick to agree"));
        assert!(asked[1].required && !asked[0].required);
        assert!(asked[1].border && !asked[0].border);

        for (text, why) in [
            ("{}", "not a JSON array of fields"),
            ("[]", "names no field"),
            (
                r#"[{"name":"N","kind":"text","page":0,"rect":[1,2,30,40]}]"#,
                "counted from 1",
            ),
            (
                r#"[{"name":"N","kind":"listbox","page":1,"rect":[1,2,30,40]}]"#,
                "not a JSON array",
            ),
            (
                r#"[{"name":"N","kind":"text","page":1,"rect":[1,2,30]}]"#,
                "not a JSON array",
            ),
            (
                r#"[{"name":"N","kind":"text","page":1,"rect":[1,2,30,40],"colour":1}]"#,
                "not a JSON array",
            ),
        ] {
            assert!(fields(text).unwrap_err().contains(why), "{text}");
        }
    }

    fn widget(name: &str, control: Control, value: Value, page: u32, rect: [f32; 4]) -> Widget {
        Widget {
            object: (u32::from(name.as_bytes()[0]), 0),
            widget: (u32::from(name.as_bytes()[0]), 0),
            page,
            rect: [0.0; 4],
            display_rect: [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            name: name.into(),
            value,
            control,
            multiline: false,
            max_length: None,
            reason: None,
            tooltip: String::new(),
            required: false,
            read_only: false,
            align: crate::forms::Align::Left,
            text_size: None,
            default_value: String::new(),
            turns: 0,
        }
    }

    fn new(name: &str, kind: Kind, page: u32, rect: [f64; 4]) -> NewField {
        NewField {
            text_size: None,
            default_value: None,
            options: Vec::new(),
            name: name.into(),
            kind,
            page,
            rect,
            tooltip: None,
            required: false,
            max_length: None,
            border: false,
        }
    }

    #[test]
    fn radio_buttons_read_back_as_boxes_of_one_group_each_by_its_value() {
        use lopdf::{dictionary, Document, Object};
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
        });
        doc.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)] }
                .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let button = |value: &str, top: f64| NewField {
            options: vec![value.into()],
            ..new("Pay", Kind::Radio, 0, [20.0, top, 12.0, 12.0])
        };
        let empty = crate::forms::scan(&doc).unwrap();
        let first = [button("Card", 20.0), button("Cash", 40.0)];
        crate::formfields::add(&mut doc, &first).unwrap();
        let two = crate::forms::scan(&doc).unwrap();
        let added = read_back(&first, &empty, &two).expect("reads back");
        assert_eq!(added.len(), 2);
        assert!(added
            .iter()
            .all(|f| f.name == "Pay" && f.kind == FieldKind::Radio));
        assert_eq!(added[1].rect, [20.0, 40.0, 12.0, 12.0]);
        // A third joins the group, which holds an answer by then.
        crate::forms::write(
            &mut doc,
            &[crate::forms::Change {
                object: two.widgets[0].object,
                value: Value::Selection(vec![0]),
            }],
        )
        .unwrap();
        let answered = crate::forms::scan(&doc).unwrap();
        let third = [button("Cheque", 60.0)];
        crate::formfields::add(&mut doc, &third).unwrap();
        let three = crate::forms::scan(&doc).unwrap();
        assert!(read_back(&third, &answered, &three).is_ok());
        // Broken: asked for a value the group does not have, and for a
        // button somewhere it is not.
        let why = read_back(&[button("Other", 60.0)], &answered, &three).unwrap_err();
        assert!(why[0].contains("no button of the value `Other`"), "{why:?}");
        let why = read_back(&[button("Cheque", 80.0)], &answered, &three).unwrap_err();
        assert!(why[0].contains("reads back at"), "{why:?}");
    }

    #[test]
    fn the_copy_is_read_back_field_by_field() {
        let old = widget(
            "old",
            Control::Text,
            Value::Text("kept".into()),
            0,
            [1.0, 1.0, 50.0, 20.0],
        );
        let before = Form {
            widgets: vec![old.clone()],
        };
        let asked = vec![
            new("Name", Kind::Text, 0, [72.0, 100.0, 200.0, 20.0]),
            new("Agree", Kind::Checkbox, 1, [72.0, 140.0, 12.0, 12.0]),
        ];
        let name = widget(
            "Name",
            Control::Text,
            Value::Text(String::new()),
            0,
            [72.0, 100.0, 200.0, 20.0],
        );
        let agree = widget(
            "Agree",
            Control::Checkbox,
            Value::Checked(false),
            1,
            [72.0, 140.0, 12.0, 12.0],
        );
        let good = Form {
            widgets: vec![old.clone(), name.clone(), agree.clone()],
        };
        let added = read_back(&asked, &before, &good).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!((added[0].page, added[1].page), (1, 2));
        assert_eq!(added[0].rect, [72.0, 100.0, 200.0, 20.0]);
        assert_eq!(
            (added[0].kind, added[1].kind),
            (FieldKind::Text, FieldKind::Checkbox)
        );

        let broken = |change: &dyn Fn(&mut Form)| {
            let mut form = good.clone();
            change(&mut form);
            read_back(&asked, &before, &form).unwrap_err().join("; ")
        };
        assert!(broken(&|f| {
            f.widgets.remove(1);
        })
        .contains("has 0 fields of this name"));
        assert!(broken(&|f| f.widgets.push(Widget {
            object: (999, 0),
            ..name.clone()
        }))
        .contains("has 2 fields of this name"));
        assert!(broken(&|f| f.widgets.push(name.clone())).contains("reads back with 2 boxes"));
        assert!(broken(&|f| f.widgets[1].control = Control::Checkbox).contains("another kind"));
        assert!(broken(&|f| f.widgets[1].multiline = true).contains("another kind"));
        assert!(broken(&|f| f.widgets[1].page = 1).contains("reads back on page 2"));
        assert!(broken(&|f| f.widgets[1].display_rect[0] += 0.02).contains("reads back at"));
        assert!(broken(&|f| f.widgets[1].display_rect[3] += 0.02).contains("reads back at"));
        assert!(broken(&|f| f.widgets[1].value = Value::Text("x".into())).contains("holding \"x\""));
        assert!(broken(&|f| f.widgets[2].value = Value::Checked(true)).contains("holding true"));
        assert!(broken(&|f| f.widgets[1].reason = Some("read-only".into()))
            .contains("cannot be filled"));
        assert!(broken(&|f| f.widgets[1].max_length = Some(3)).contains("another most characters"));
        assert!(broken(&|f| f.widgets[0].value = Value::Text("lost".into()))
            .contains("`old`, which the source had"));
        assert!(broken(&|f| {
            f.widgets.remove(0);
        })
        .contains("has 2 form boxes"));
        // Within a hundredth of a point is the same place: the copy holds `f32`s.
        let mut near = good.clone();
        near.widgets[1].display_rect[0] += 0.005;
        assert!(read_back(&asked, &before, &near).is_ok());
    }

    #[test]
    fn the_sentence_names_each_field_and_its_page() {
        let mut report = report::FormAdded {
            schema: SCHEMA,
            command: "form".into(),
            input: "a.pdf".into(),
            output: "b.pdf".into(),
            pages: 2,
            added: vec![AddedField {
                name: "Name".into(),
                kind: FieldKind::Text,
                multiline: false,
                page: 1,
                rect: [72.0, 100.0, 200.0, 20.0],
            }],
            fields_before: 3,
            fields_after: 4,
            signatures_invalidated: 0,
            signatures_unknown: false,
        };
        assert_eq!(
            plain(&report),
            "b.pdf: 1 field added, 4 in the form now\n  Name (text), page 1"
        );
        report.added.push(AddedField {
            name: "Notes".into(),
            kind: FieldKind::Text,
            multiline: true,
            page: 2,
            rect: [0.0; 4],
        });
        report.added.push(AddedField {
            name: "Agree".into(),
            kind: FieldKind::Checkbox,
            multiline: false,
            page: 2,
            rect: [0.0; 4],
        });
        report.added.push(AddedField {
            name: "Approved".into(),
            kind: FieldKind::Other,
            multiline: false,
            page: 2,
            rect: [0.0; 4],
        });
        report.signatures_unknown = true;
        assert_eq!(
            plain(&report),
            "b.pdf: 4 fields added, 4 in the form now\n  Name (text), page 1\n  \
             Notes (text, several lines), page 2\n  Agree (checkbox), page 2\n  \
             Approved (signature, empty), page 2\n\
             The rewrite invalidates existing signatures."
        );
    }
}
