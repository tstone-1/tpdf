//! `tpdf fill <in.pdf> -o <out.pdf> --values <answers.json | -> ...`: the
//! application's form filling --- a worker's scan, the application's writer, a
//! worker's read-back --- from a terminal.
//!
//! **All or nothing.** Every answer is checked before anything is written, by
//! the rules the window checks a typed answer by (`forms::check`, over every
//! widget of the field, as `forms::write` does), plus the ones only a file of
//! answers can break: a name no field has, a name two fields share, a JSON type
//! the field does not take, an export value no option has. Every problem is
//! reported at once, and a single one means no file is written.
//!
//! **The writer is the save's.** The answers become `Plan::forms` over a plan
//! that keeps every page as it is, and `save::write_copy` writes it --- the
//! rewrite the application's *Save As* performs when a document carries form
//! answers, in a worker, staged beside the output and renamed onto it. Then a
//! fresh worker reads the written file's form, and every answered field must
//! say what was asked and every other field what it said before; if not, the
//! output is removed and the run fails with exit 4.
//!
//! **A signed document is refused.** That writer is a full rewrite, which
//! invalidates every existing signature --- the application warns before it
//! does so (`signedsave.ts`), and a command line has nobody to warn. Fill the
//! unsigned document and sign the filled copy; filling as an appended revision,
//! which keeps signatures and is what a DocMDP form-filling permission
//! expects, is the later option `docs/PLAN.md` records.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::args::{lexically_same, unknown, value};
use super::fields::{ask_form, grouped, kind, multiple, options, state_name, value_json, Grouped};
use super::report::{self, FieldKind, Problem, ProblemKind, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::docinfo::Properties;
use crate::docmodel::PageSource;
use crate::edits::{PageView, Plan};
use crate::forms::{self, Change, Control, Form, Invalid, Value};
use crate::save_outside::Session;
use crate::worker_proto::{Reply, Request};

/// `fill`, registered.
pub const COMMAND: Registered = Registered {
    name: "fill",
    usage: "fill <in.pdf> -o <out.pdf> --values <answers.json | -> [--force]\n        [--password-env VAR] [--json]",
    summary: "Fills the form from a JSON object of full field names and answers\n            (read from stdin for -): text, true or false for a checkbox, an\n            export value for a radio group or a choice, an array of them for a\n            list that takes several. Every answer is checked first, and one\n            problem means nothing is written; the filled copy is read back\n            before success is reported. A signed document is refused.",
    parse: boxed,
};

/// The most an answers file may hold, in bytes. Every answer is held to 16 KB
/// and a whole form's text to 1 MiB by `forms.rs`; this only stops a file that
/// is not an answers file from being read whole.
pub const MAX_ANSWERS_BYTES: u64 = 16 * 1024 * 1024;

/// Where the answers come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Values {
    /// A file.
    File(PathBuf),
    /// `-`: standard input.
    Stdin,
}

/// `tpdf fill`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    /// The document to fill. It is never written.
    pub input: PathBuf,
    /// Where the filled copy goes.
    pub output: PathBuf,
    /// The answers.
    pub values: Values,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
    /// `--force`: replace an existing output file.
    pub force: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `fill`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Fill, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut values: Option<Values> = None;
    let mut password_env = None;
    let mut json = false;
    let mut force = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--values" => {
                values = Some(match value(arg, &mut rest)?.as_str() {
                    "-" => Values::Stdin,
                    path => Values::File(PathBuf::from(path)),
                });
            }
            "--password-env" => password_env = Some(variable(value(arg, &mut rest)?)?),
            "--json" => json = true,
            "--force" => force = true,
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown("fill", flag)),
            path => {
                if input.is_some() {
                    return Err(format!(
                        "`fill` takes one document, and `{path}` is a second --- fill them one \
                         at a time"
                    ));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }
    let input = input.ok_or("`fill` needs the document to fill")?;
    let output = output.ok_or(
        "`fill` needs `-o <out.pdf>`: the filled document is written as a new file, and the \
         original is never changed",
    )?;
    if lexically_same(&input, &output) {
        return Err(
            "the output names the input --- the filled document is written as a new file, so \
             choose another name for it"
                .into(),
        );
    }
    let values = values.ok_or(
        "`fill` needs `--values <answers.json>`, or `--values -` to read the answers from \
         standard input",
    )?;
    if let Values::File(path) = &values {
        if lexically_same(path, &output) {
            return Err("`-o` names the answers file".into());
        }
    }
    Ok(Fill {
        input,
        output,
        values,
        password_env,
        json,
        force,
    })
}

impl Subcommand for Fill {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        run_fill(env, self, out, err)
    }
}

/// The answers, in the order the file gives them, a repeated name kept so that
/// it can be reported rather than silently resolved to its last value.
#[derive(Debug, Clone, PartialEq)]
pub struct Answers(pub Vec<(String, serde_json::Value)>);

impl<'de> serde::Deserialize<'de> for Answers {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Answers;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object of field names and answers")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Answers, M::Error> {
                let mut out = Vec::new();
                while let Some((name, answer)) = map.next_entry::<String, serde_json::Value>()? {
                    out.push((name, answer));
                }
                Ok(Answers(out))
            }
        }
        d.deserialize_map(Visitor)
    }
}

/// Reads an answers document.
///
/// # Errors
///
/// Not JSON, not an object, or an object with no answer in it.
pub fn answers(text: &str) -> Result<Answers, String> {
    let answers: Answers = serde_json::from_str(text).map_err(|e| {
        format!(
            "the answers are not a JSON object of field names and answers ({e}) --- `tpdf \
             fields --json` lists the names"
        )
    })?;
    if answers.0.is_empty() {
        return Err("the answers name no field, so there is nothing to fill".into());
    }
    Ok(answers)
}

/// One answer, resolved to the field it names and the value the writer takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The name, as given.
    pub name: String,
    /// The terminal field.
    pub object: lopdf::ObjectId,
    /// Its kind, for the report.
    pub kind: FieldKind,
    /// What `forms::write` is handed.
    pub value: Value,
    /// The answer as `fields` would print it after filling.
    pub expected: serde_json::Value,
}

fn problem(field: &str, problem: ProblemKind, why: impl Into<String>) -> Problem {
    Problem {
        field: field.to_string(),
        problem,
        why: why.into(),
    }
}

/// The problem kind for a rule `forms::check` applies.
#[must_use]
pub fn problem_kind(invalid: Invalid) -> ProblemKind {
    match invalid {
        Invalid::NotEditable => ProblemKind::NotEditable,
        Invalid::Type => ProblemKind::Type,
        Invalid::Option => ProblemKind::Option,
        Invalid::Characters => ProblemKind::Characters,
        Invalid::Length => ProblemKind::Length,
        Invalid::Line => ProblemKind::Line,
        Invalid::Layout => ProblemKind::Layout,
    }
}

fn listed(options: &[String]) -> String {
    let quoted: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
    if quoted.is_empty() {
        "it has none".into()
    } else {
        format!("its options are {}", quoted.join(", "))
    }
}

/// The index of the one option whose export value is `wanted`.
fn one_of(exports: &[String], wanted: &str, name: &str) -> Result<Option<usize>, Problem> {
    let at: Vec<usize> = exports
        .iter()
        .enumerate()
        .filter(|(_, e)| e.as_str() == wanted)
        .map(|(i, _)| i)
        .collect();
    match at.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        several => Err(problem(
            name,
            ProblemKind::Ambiguous,
            format!(
                "{} of its options have the export value `{wanted}`, so an export value \
                 cannot say which is meant",
                several.len()
            ),
        )),
    }
}

/// An answer as JSON, turned into what the field's control takes.
///
/// # Errors
///
/// The answer is the wrong JSON type, names no option, or names one that two
/// options share.
pub fn to_value(group: &Grouped<'_>, answer: &serde_json::Value) -> Result<Value, Problem> {
    use serde_json::Value as J;
    let name = group.name;
    let control = &group.first().control;
    let wrong = |takes: &str| {
        problem(
            name,
            ProblemKind::Type,
            format!("takes {takes}, and the answer is {answer}"),
        )
    };
    match control {
        Control::Text => match answer {
            J::String(text) => Ok(Value::Text(text.clone())),
            _ => Err(wrong("a string")),
        },
        Control::Checkbox => match answer {
            J::Bool(on) => Ok(Value::Checked(*on)),
            _ => Err(wrong("true or false")),
        },
        Control::Radio { states, unison, .. } => {
            let names: Vec<String> = states.iter().map(|s| state_name(s)).collect();
            match answer {
                J::Null => Ok(Value::Selection(Vec::new())),
                J::String(wanted) => {
                    let at: Vec<usize> = names
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| *n == wanted)
                        .map(|(i, _)| i)
                        .collect();
                    match at.as_slice() {
                        [] => {
                            let unique: Vec<String> =
                                options(control, &Value::Selection(Vec::new()))
                                    .into_iter()
                                    .map(|o| o.export)
                                    .collect();
                            Err(problem(
                                name,
                                ProblemKind::Option,
                                format!("has no option `{wanted}` --- {}", listed(&unique)),
                            ))
                        }
                        // Buttons in unison share the state and turn on
                        // together, so the first one says everything.
                        [first, ..] if at.len() == 1 || *unison => {
                            Ok(Value::Selection(vec![*first]))
                        }
                        several => Err(problem(
                            name,
                            ProblemKind::Ambiguous,
                            format!(
                                "{} of its buttons have the state `{wanted}` and are not in \
                                 unison, so the state cannot say which is meant",
                                several.len()
                            ),
                        )),
                    }
                }
                _ => Err(wrong(
                    "the export value of one of its options as a string, or null",
                )),
            }
        }
        Control::Choice {
            options, editable, ..
        } => {
            let exports: Vec<String> = options.iter().map(|o| o.export.clone()).collect();
            if multiple(control) {
                let J::Array(items) = answer else {
                    return Err(wrong("an array of its options' export values"));
                };
                let mut at = Vec::with_capacity(items.len());
                for item in items {
                    let J::String(wanted) = item else {
                        return Err(wrong("an array of its options' export values"));
                    };
                    match one_of(&exports, wanted, name)? {
                        Some(i) => at.push(i),
                        None => {
                            return Err(problem(
                                name,
                                ProblemKind::Option,
                                format!("has no option `{wanted}` --- {}", listed(&exports)),
                            ))
                        }
                    }
                }
                at.sort_unstable();
                at.dedup();
                return Ok(Value::Selection(at));
            }
            match answer {
                J::Null => Ok(Value::Selection(Vec::new())),
                J::String(wanted) => match one_of(&exports, wanted, name)? {
                    Some(i) => Ok(Value::Selection(vec![i])),
                    None if *editable => Ok(Value::Text(wanted.clone())),
                    None => Err(problem(
                        name,
                        ProblemKind::Option,
                        format!("has no option `{wanted}` --- {}", listed(&exports)),
                    )),
                },
                _ => Err(wrong(
                    "the export value of one of its options as a string, or null",
                )),
            }
        }
        Control::Unsupported => Err(problem(name, ProblemKind::NotEditable, forms::UNSUPPORTED)),
    }
}

/// Every answer resolved against the form, or every problem with them.
///
/// # Errors
///
/// Each answer that names no field or two, answers a field twice or one that
/// cannot be answered, or that `forms::check` refuses for any of the field's
/// widgets --- all of them, in the order the answers give them.
pub fn resolve(form: &Form, answers: &Answers) -> Result<Vec<Resolved>, Vec<Problem>> {
    let fields = grouped(form);
    let mut problems = Vec::new();
    let mut resolved = Vec::new();
    for (at, (name, answer)) in answers.0.iter().enumerate() {
        if answers.0[..at].iter().any(|(earlier, _)| earlier == name) {
            problems.push(problem(
                name,
                ProblemKind::Ambiguous,
                "is answered more than once",
            ));
            continue;
        }
        let named: Vec<&Grouped<'_>> = fields.iter().filter(|g| g.name == name).collect();
        let group = match named.as_slice() {
            [] => {
                problems.push(problem(
                    name,
                    ProblemKind::Unknown,
                    "no field has this name --- `tpdf fields` lists them",
                ));
                continue;
            }
            [one] => *one,
            several => {
                problems.push(problem(
                    name,
                    ProblemKind::Ambiguous,
                    format!(
                        "{} fields have this name, so it cannot say which is meant",
                        several.len()
                    ),
                ));
                continue;
            }
        };
        if let Some(reason) = group.reason() {
            problems.push(problem(name, ProblemKind::NotEditable, reason));
            continue;
        }
        let value = match to_value(group, answer) {
            Ok(value) => value,
            Err(p) => {
                problems.push(p);
                continue;
            }
        };
        if let Some(refused) = group
            .widgets
            .iter()
            .find_map(|w| forms::check(w, &value).err())
        {
            problems.push(problem(
                name,
                problem_kind(refused.invalid),
                refused.message,
            ));
            continue;
        }
        resolved.push(Resolved {
            name: name.clone(),
            object: group.object,
            kind: kind(&group.first().control),
            expected: value_json(&group.first().control, &value),
            value,
        });
    }
    if problems.is_empty() {
        Ok(resolved)
    } else {
        Err(problems)
    }
}

/// Why a document must not be filled by this writer, when it must not.
///
/// The application's rule (`signedsave.ts`'s `signatureSaveMessage`), minus
/// the dialog: a signed or certified document, or one whose signatures could
/// not all be enumerated. Its `/Info` limits are left out, because they say
/// nothing about signatures.
#[must_use]
pub fn signed_refusal(shown: &str, properties: &Properties) -> Option<String> {
    if properties
        .signatures
        .iter()
        .any(|s| s.signed || s.certification > 0)
    {
        return Some(format!(
            "{shown} is signed, and filling rewrites the document, which would invalidate its \
             signatures --- fill the unsigned document, then sign the filled copy with `tpdf \
             sign`"
        ));
    }
    let limits = &properties.limits;
    if limits.locked || limits.unreadable > 0 || limits.signatures_dropped > 0 {
        return Some(format!(
            "{shown}: whether it is signed could not be read completely, and filling rewrites \
             the document, which would invalidate any signature --- so it is not filled"
        ));
    }
    None
}

/// A plan that keeps every page as it is and writes `answers`.
#[must_use]
pub fn plan(pages: u32, answers: &[Resolved], opened_as: crate::fingerprint::Fingerprint) -> Plan {
    Plan {
        text_edits: Vec::new(),
        forms: answers
            .iter()
            .map(|a| Change {
                object: a.object,
                value: a.value.clone(),
            })
            .collect(),
        baseline: pages,
        opened_as: Some(opened_as),
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at) + 1,
                source: PageSource::Baseline(at),
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        sources: Vec::new(),
    }
}

/// What the written file's form says against what was asked.
///
/// Every answered field must read back as the answer, on every widget; every
/// other field as it read before. Fields are matched by name, because the
/// rewrite may number objects afresh. A name two fields share is left out of
/// the second half, since it could not have been answered either.
#[must_use]
pub fn mismatches(asked: &[Resolved], before: &Form, after: &Form) -> Vec<Problem> {
    let now = grouped(after);
    let then = grouped(before);
    let mut problems = Vec::new();
    let unique =
        |groups: &[Grouped<'_>], name: &str| groups.iter().filter(|g| g.name == name).count() == 1;
    let mut check = |name: &str, want: &serde_json::Value| {
        let found: Vec<&Grouped<'_>> = now.iter().filter(|g| g.name == name).collect();
        let [group] = found.as_slice() else {
            problems.push(problem(
                name,
                ProblemKind::ReadBack,
                format!(
                    "the written file has {} fields of this name, where it should have one",
                    found.len()
                ),
            ));
            return;
        };
        for widget in &group.widgets {
            let got = value_json(&widget.control, &widget.value);
            if &got != want {
                problems.push(problem(
                    name,
                    ProblemKind::ReadBack,
                    format!("was written as {want} and reads back as {got}"),
                ));
                return;
            }
        }
    };
    for answer in asked {
        check(&answer.name, &answer.expected);
    }
    for group in &then {
        if asked.iter().any(|a| a.name == group.name) || !unique(&then, group.name) {
            continue;
        }
        let first = group.first();
        check(group.name, &value_json(&first.control, &first.value));
    }
    problems
}

/// Each answered field as the written file's form says it, for the report.
///
/// From `after` rather than from the answers, so that what the report says was
/// read back is what was read back; [`mismatches`] has already held the two
/// equal by the time this is called.
#[must_use]
pub fn read_back(asked: &[Resolved], after: &Form) -> Vec<report::FilledField> {
    let now = grouped(after);
    asked
        .iter()
        .filter_map(|a| {
            let group = now.iter().find(|g| g.name == a.name)?;
            let first = group.first();
            Some(report::FilledField {
                name: a.name.clone(),
                kind: kind(&first.control),
                value: value_json(&first.control, &first.value),
            })
        })
        .collect()
}

/// The answers text, from the file or standard input, bounded.
fn read_answers(values: &Values) -> Result<String, Failure> {
    let mut text = String::new();
    let (what, read) = match values {
        Values::Stdin => (
            "standard input".to_string(),
            std::io::stdin()
                .lock()
                .take(MAX_ANSWERS_BYTES + 1)
                .read_to_string(&mut text),
        ),
        Values::File(path) => (
            path.display().to_string(),
            std::fs::File::open(path)
                .and_then(|f| f.take(MAX_ANSWERS_BYTES + 1).read_to_string(&mut text)),
        ),
    };
    read.map_err(|e| {
        Failure::new(
            Exit::Refused,
            format!("could not read the answers from {what}: {e}"),
        )
    })?;
    if text.len() as u64 > MAX_ANSWERS_BYTES {
        return Err(Failure::new(
            Exit::Refused,
            format!(
                "the answers in {what} pass {} MiB, which is not an answers file",
                MAX_ANSWERS_BYTES / (1024 * 1024)
            ),
        ));
    }
    Ok(text)
}

/// Removes a written output that is not to be kept, saying so if it cannot be.
fn discard(output: &Path, err: &mut dyn Write, program: &str) {
    if let Err(e) = std::fs::remove_file(output) {
        say(
            err,
            &format!(
                "{program}: {} could not be removed ({e}); it is not a correctly filled copy",
                output.display()
            ),
        );
    }
}

/// The session's three questions: the signatures, the page count, the form.
fn read_input(
    session: &mut Session,
    shown: &str,
    password_given: bool,
) -> Result<(u32, Form), Failure> {
    let properties = match session
        .ask(Request::Properties)
        .map_err(|why| declined(shown, why, password_given))?
    {
        Reply::Properties(properties) => *properties,
        other => {
            return Err(Failure::new(
                Exit::Internal,
                format!("the worker answered the properties request with {other:?}"),
            ))
        }
    };
    if let Some(why) = signed_refusal(shown, &properties) {
        return Err(Failure::new(Exit::Refused, why));
    }
    let pages = match session
        .ask(Request::Open {
            lazy_geometry: true,
        })
        .map_err(|why| declined(shown, why, password_given))?
    {
        Reply::Open { page_count, .. } => u32::try_from(page_count).map_err(|_| {
            Failure::new(
                Exit::Refused,
                format!("{shown} has more pages than tpdf can fill"),
            )
        })?,
        other => {
            return Err(Failure::new(
                Exit::Internal,
                format!("the worker answered an open with {other:?}"),
            ))
        }
    };
    let form = ask_form(session, shown, password_given)?;
    Ok((pages, form))
}

fn report_of(
    fill: &Fill,
    written: bool,
    problems: Vec<Problem>,
    fields: Vec<report::FilledField>,
) -> report::Filled {
    report::Filled {
        schema: SCHEMA,
        command: "fill".into(),
        input: fill.input.display().to_string(),
        output: fill.output.display().to_string(),
        written,
        problems,
        fields,
    }
}

/// Prints the problems, and the JSON document when it was asked for.
fn problems_out(
    fill: &Fill,
    problems: Vec<Problem>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    program: &str,
) -> usize {
    for p in &problems {
        say(err, &format!("{program}: {}: {}", p.field, p.why));
    }
    let count = problems.len();
    if fill.json {
        json(out, &report_of(fill, false, problems, Vec::new()));
    }
    count
}

fn run_fill(
    env: &Env<'_>,
    fill: &Fill,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    // What can be refused without a worker, first, in `sign`'s order.
    if crate::save::same_file(&fill.input, &fill.output) {
        return Err(Failure::new(
            Exit::Usage,
            "the output is the input under another name --- the filled document is written as \
             a new file, so choose another name for it",
        ));
    }
    if let Values::File(path) = &fill.values {
        if crate::save::same_file(path, &fill.output) {
            return Err(Failure::new(
                Exit::Usage,
                "-o is the answers file under another name",
            ));
        }
    }
    if !fill.force && fill.output.exists() {
        return Err(Failure::new(
            Exit::Refused,
            format!(
                "{} already exists --- choose another name, or give --force to replace it",
                fill.output.display()
            ),
        ));
    }
    let password = password(fill.password_env.as_deref())?;
    let answers =
        answers(&read_answers(&fill.values)?).map_err(|why| Failure::new(Exit::Refused, why))?;

    let shown = fill.input.display().to_string();
    let (file, len) = opened(&fill.input).map_err(|why| Failure::new(Exit::Refused, why))?;
    let opened_as = crate::fingerprint::Fingerprint::of_open(&file, &fill.input)
        .map_err(|why| Failure::new(Exit::Refused, why))?;
    let mut session = env
        .worker()
        .session(&file, len, password.as_deref())
        .map_err(|why| declined(&shown, why, password.is_some()))?;
    let (pages, before) = read_input(&mut session, &shown, password.is_some())?;
    drop(session);
    drop(file);

    let resolved = match resolve(&before, &answers) {
        Ok(resolved) => resolved,
        Err(problems) => {
            let count = problems_out(fill, problems, out, err, &env.program);
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{count} answer{} refused, and nothing was written",
                    if count == 1 { " was" } else { "s were" }
                ),
            ));
        }
    };

    let copied = crate::save::write_copy(
        &fill.input,
        &plan(pages, &resolved, opened_as),
        &fill.output,
        password.as_deref(),
        &env.worker(),
    )
    .map_err(|refusal| Failure::new(Exit::Refused, format!("{shown}: {}", refusal.message)))?;
    if copied.changed {
        discard(&fill.output, err, &env.program);
        return Err(Failure::new(
            Exit::Refused,
            format!("{shown} changed while it was being filled, so the copy was not kept"),
        ));
    }

    // Read back by a fresh worker, through the handle of the file just written.
    let written = fill.output.display().to_string();
    let after = opened(&fill.output)
        .map_err(|why| {
            Failure::new(
                Exit::Internal,
                format!("the filled file was written and could not be reopened: {why}"),
            )
        })
        .and_then(|(file, len)| {
            let mut session = env
                .worker()
                .session(&file, len, password.as_deref())
                .map_err(|why| declined(&written, why, password.is_some()))?;
            ask_form(&mut session, &written, password.is_some())
        });
    let after = match after {
        Ok(after) => after,
        Err(failure) => {
            discard(&fill.output, err, &env.program);
            return Err(Failure::new(
                Exit::Internal,
                format!(
                    "the filled file could not be checked, so it was removed: {}",
                    failure.message
                ),
            ));
        }
    };
    let wrong = mismatches(&resolved, &before, &after);
    if !wrong.is_empty() {
        discard(&fill.output, err, &env.program);
        let count = problems_out(fill, wrong, out, err, &env.program);
        return Err(Failure::new(
            Exit::Internal,
            format!(
                "{count} field{} did not read back as filled, so {written} was removed",
                if count == 1 { "" } else { "s" }
            ),
        ));
    }

    let fields = read_back(&resolved, &after);
    if fill.json {
        json(out, &report_of(fill, true, Vec::new(), fields));
    } else {
        say(
            out,
            &format!(
                "Filled {} field{} into {written}; each reads back as given.",
                resolved.len(),
                if resolved.len() == 1 { "" } else { "s" }
            ),
        );
    }
    Ok(Exit::Ok)
}
