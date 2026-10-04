//! `tpdf fields <file.pdf> [--password-env VAR] [--json]`: a document's form,
//! field by field, in the vocabulary `tpdf fill` takes.
//!
//! **The viewer's form reader, and nothing of its own.** One worker, one
//! question: `Request::Form`, which is `forms::scan` in the worker --- the scan
//! the application's form filling is built on, so a field this lists as
//! editable is one the window would let a reader type into, and the reason one
//! is not is the sentence the window shows. What is added here is the grouping:
//! the scan answers widget by widget, and a script addresses fields.
//!
//! **An answer is printed in the form `fill` takes one**, so the `value` of
//! every field in `fields --json` is itself a valid answer for it. A radio
//! group's and a choice's answer is an **export value**, never an index: the
//! index is the application's journal's vocabulary, stable across a session,
//! and a script's answers file outlives any session.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

use super::args::unknown;
use super::report::{self, FieldKind, FieldOption, NotEditable, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::forms::{self, Control, Form, Value, Widget};
use crate::save_outside::{Declined, Session};
use crate::worker_proto::{Reply, Request};

/// `fields`, registered.
pub const COMMAND: Registered = Registered {
    name: "fields",
    usage: "fields <file.pdf> [--password-env VAR] [--json]",
    summary: "Lists the document's form fields: each one's full name, kind,\n            current value and options, and whether fill can answer it.",
    parse: boxed,
};

/// `tpdf fields`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields {
    /// The document.
    pub input: PathBuf,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `fields`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Fields, String> {
    let mut input: Option<PathBuf> = None;
    let mut password_env = None;
    let mut json = false;
    let mut positional = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "--json") => json = true,
            (false, "--password-env") => {
                password_env = Some(variable(super::args::value(arg, &mut rest)?)?);
            }
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("fields", flag))
            }
            (_, path) => {
                if input.is_some() {
                    return Err(format!(
                        "`fields` takes one document, and `{path}` is a second"
                    ));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }
    Ok(Fields {
        input: input.ok_or("`fields` needs the document to read")?,
        password_env,
        json,
    })
}

impl Subcommand for Fields {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let password = password(self.password_env.as_deref())?;
        let shown = self.input.display().to_string();
        let (file, len) =
            super::opened_or_stdin(&self.input).map_err(|why| Failure::new(Exit::Refused, why))?;
        let mut session = env
            .worker()
            .session(&file, len, password.as_deref())
            .map_err(|why| declined(&shown, why, password.is_some()))?;
        let form = ask_form(&mut session, &shown, password.is_some())?;
        let report = report(&shown, &form);
        if self.json {
            json(out, &report);
        } else {
            say(out, &fields_text(&report));
        }
        Ok(Exit::Ok)
    }
}

/// The document's form, from the worker holding it, or the refusal to report.
///
/// An XFA form is refused by the scan itself (`forms::XFA_REFUSAL`) and is
/// named as such; tpdf neither reads nor fills one.
///
/// # Errors
///
/// Exit 3 for a locked document, an XFA form or one the scan refuses; 4 for a
/// worker that did not answer.
pub(crate) fn ask_form(
    session: &mut Session,
    shown: &str,
    password_given: bool,
) -> Result<Form, Failure> {
    match session.ask(Request::Form) {
        Ok(Reply::Form(form)) => Ok(form),
        Ok(other) => Err(Failure::new(
            Exit::Internal,
            format!("the worker answered the form request with {other:?}"),
        )),
        Err(Declined::Refused(why)) if why == forms::XFA_REFUSAL => Err(Failure::new(
            Exit::Refused,
            format!("{shown} has an XFA form, which tpdf does not read or fill"),
        )),
        Err(Declined::Refused(why)) => Err(Failure::new(
            Exit::Refused,
            format!("{shown}: its form could not be read: {why}"),
        )),
        Err(why) => Err(declined(shown, why, password_given)),
    }
}

/// One field and its widgets, as the scan listed them.
#[derive(Debug, Clone)]
pub struct Grouped<'a> {
    /// The fully qualified name.
    pub name: &'a str,
    /// The terminal field's object, which answers are keyed by.
    pub object: lopdf::ObjectId,
    /// Every widget of it, in the scan's order. Never empty.
    pub widgets: Vec<&'a Widget>,
}

impl Grouped<'_> {
    /// The first widget, whose control every sibling shares.
    #[must_use]
    pub fn first(&self) -> &Widget {
        self.widgets[0]
    }

    /// Why no answer can be written, from the first widget that says so.
    ///
    /// Every widget, because `forms::write` checks every widget of a field
    /// before it writes any: a field one of whose widgets is hidden is not
    /// filled at all.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.widgets.iter().find_map(|w| w.reason.as_deref())
    }
}

/// The scan's widgets, grouped by field, in the order each field is first met.
#[must_use]
pub fn grouped(form: &Form) -> Vec<Grouped<'_>> {
    let mut out: Vec<Grouped<'_>> = Vec::new();
    for widget in &form.widgets {
        match out.iter_mut().find(|g| g.object == widget.object) {
            Some(group) => group.widgets.push(widget),
            None => out.push(Grouped {
                name: &widget.name,
                object: widget.object,
                widgets: vec![widget],
            }),
        }
    }
    out
}

/// The kind of answer a control takes.
#[must_use]
pub fn kind(control: &Control) -> FieldKind {
    match control {
        Control::Text => FieldKind::Text,
        Control::Checkbox => FieldKind::Checkbox,
        Control::Radio { .. } => FieldKind::Radio,
        Control::Choice { combo: true, .. } => FieldKind::ChoiceCombo,
        Control::Choice { .. } => FieldKind::ChoiceList,
        Control::Unsupported | Control::Signature { .. } => FieldKind::Other,
    }
}

/// Whether a choice takes several selections: a list with `/Ff` MultiSelect.
/// A dropdown with the flag takes one, as `forms::check` holds it.
#[must_use]
pub fn multiple(control: &Control) -> bool {
    matches!(
        control,
        Control::Choice {
            multiple: true,
            combo: false,
            ..
        }
    )
}

/// A radio state's name as text. PDF names are bytes; the ones a form uses are
/// ASCII in practice, and anything that is not UTF-8 is read as Latin-1 so that
/// no byte is lost or turned into U+FFFD.
#[must_use]
pub fn state_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
    }
}

/// A radio group's states or a choice's options, as `fields` lists them, each
/// saying whether `value` chooses it.
///
/// A radio group lists each state once, in the order of the first widget that
/// has it: buttons in unison share a state, and one answer turns all of them on.
#[must_use]
pub fn options(control: &Control, value: &Value) -> Vec<FieldOption> {
    let chosen: &[usize] = match value {
        Value::Selection(at) => at,
        _ => &[],
    };
    match control {
        Control::Radio { states, .. } => {
            let on: Option<String> = chosen
                .first()
                .and_then(|i| states.get(*i))
                .map(|s| state_name(s));
            let mut seen = BTreeSet::new();
            states
                .iter()
                .map(|s| state_name(s))
                .filter(|s| seen.insert(s.clone()))
                .map(|s| FieldOption {
                    selected: on.as_ref() == Some(&s),
                    export: s.clone(),
                    label: s,
                })
                .collect()
        }
        Control::Choice { options, .. } => options
            .iter()
            .enumerate()
            .map(|(i, o)| FieldOption {
                export: o.export.clone(),
                label: o.label.clone(),
                selected: chosen.contains(&i),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A field's answer as JSON, in the form `fill` takes one.
#[must_use]
pub fn value_json(control: &Control, value: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match (control, value) {
        (Control::Unsupported | Control::Signature { .. }, _) => J::Null,
        (Control::Checkbox, Value::Checked(on)) => J::Bool(*on),
        (Control::Radio { states, .. }, Value::Selection(at)) => at
            .first()
            .and_then(|i| states.get(*i))
            .map_or(J::Null, |s| J::String(state_name(s))),
        (Control::Choice { options, .. }, Value::Selection(at)) if multiple(control) => J::Array(
            at.iter()
                .filter_map(|i| options.get(*i))
                .map(|o| J::String(o.export.clone()))
                .collect(),
        ),
        (Control::Choice { options, .. }, Value::Selection(at)) => at
            .first()
            .and_then(|i| options.get(*i))
            .map_or(J::Null, |o| J::String(o.export.clone())),
        (_, Value::Text(text)) => J::String(text.clone()),
        // A shape the scan does not produce for this control: said as it is,
        // rather than as something it is not.
        (_, Value::Checked(on)) => J::Bool(*on),
        (_, Value::Selection(at)) => J::Array(at.iter().map(|i| J::from(*i)).collect()),
    }
}

/// Which of `forms`'s reasons a widget's sentence is.
#[must_use]
pub fn not_editable(reason: &str) -> NotEditable {
    match reason {
        forms::READ_ONLY => NotEditable::ReadOnly,
        forms::PASSWORD => NotEditable::Password,
        forms::FILE_SELECT => NotEditable::FileSelect,
        forms::COMB => NotEditable::Comb,
        forms::RICH_TEXT => NotEditable::RichText,
        forms::HIDDEN => NotEditable::Hidden,
        forms::UNSUPPORTED => NotEditable::Unsupported,
        _ => NotEditable::Other,
    }
}

/// One field's entry.
#[must_use]
pub fn field(group: &Grouped<'_>) -> report::Field {
    let first = group.first();
    let reason = group.reason();
    let pages: BTreeSet<u32> = group.widgets.iter().map(|w| w.page + 1).collect();
    report::Field {
        name: group.name.to_string(),
        kind: kind(&first.control),
        value: value_json(&first.control, &first.value),
        options: options(&first.control, &first.value),
        multiple: multiple(&first.control),
        custom_text: matches!(first.control, Control::Choice { editable: true, .. }),
        multiline: first.multiline,
        max_length: first.max_length,
        pages: pages.into_iter().collect(),
        widgets: group.widgets.len(),
        editable: reason.is_none(),
        not_editable: reason.map(not_editable),
        why: reason.map(str::to_string),
    }
}

/// `fields --json` for a document whose form is `form`.
#[must_use]
pub fn report(path: &str, form: &Form) -> report::Fields {
    report::Fields {
        schema: SCHEMA,
        command: "fields".into(),
        path: path.to_string(),
        fields: grouped(form).iter().map(field).collect(),
    }
}

/// `fields` without `--json`: a line per field, then what qualifies it.
#[must_use]
pub fn fields_text(report: &report::Fields) -> String {
    if report.fields.is_empty() {
        return format!("{}: no form fields", report.path);
    }
    let mut lines = vec![report.path.clone()];
    for f in &report.fields {
        let kind = match f.kind {
            FieldKind::Text => "text",
            FieldKind::Checkbox => "checkbox",
            FieldKind::Radio => "radio",
            FieldKind::ChoiceCombo => "dropdown",
            FieldKind::ChoiceList if f.multiple => "list, several",
            FieldKind::ChoiceList => "list",
            FieldKind::Other => "other",
        };
        lines.push(format!("  {} ({kind}): {}", f.name, f.value));
        if !f.options.is_empty() {
            let shown: Vec<String> = f
                .options
                .iter()
                .map(|o| {
                    let shown = if o.export == o.label {
                        o.export.clone()
                    } else {
                        format!("{} ({})", o.export, o.label)
                    };
                    if o.selected {
                        format!("{shown} [chosen]")
                    } else {
                        shown
                    }
                })
                .collect();
            lines.push(format!("    options: {}", shown.join(", ")));
        }
        if let Some(max) = f.max_length {
            lines.push(format!("    at most {max} characters"));
        }
        if let Some(why) = &f.why {
            lines.push(format!("    not editable: {why}"));
        }
    }
    lines.join("\n")
}
