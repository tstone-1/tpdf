//! Versioned edit requests reuse the GUI's journal and sandboxed copy writer.
//! Page numbers are one-based positions immediately before each operation.
//! The request is validated in full before a staging file is allocated.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use serde::Deserialize;
#[cfg(test)]
use serde_json::json as value;

use super::args::{unknown, value as argument};
use super::fill::SignedState;
use super::pages::{check_target, read_input, same_sizes, Temporary};
use super::report::{self, SCHEMA};
use super::text::{password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::docmodel::{MarkKind, PageSource, StampName, INK_WIDTH, NIB_MAX, NIB_MIN};
use crate::edits::{EditState, Edits, NewMark, Plan};
use crate::render::PageSize;
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "edit",
    usage: "edit <in.pdf> --plan <edits.json | -> -o <out.pdf>\n        [--password-env VAR] [--invalidate-signatures] [--force] [--dry-run] [--json]",
    summary: "Applies a versioned JSON edit plan through the same model as the GUI.\n            Page numbers refer to the current order at each step. Any invalid\n            operation refuses the whole plan without publishing an output.",
    parse: |args| parse(args, Mode::Edit).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

pub const COMMENTS: Registered = Registered {
    name: "comments",
    usage: "comments <in.pdf> [--password-env VAR] [--json]",
    summary: "Lists annotations and comments, including PDF object identities and scan limits.",
    parse: |args| parse(args, Mode::Comments).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

pub const TEXT_RUNS: Registered = Registered {
    name: "text-runs",
    usage: "text-runs <in.pdf> [--page N] [--password-env VAR] [--json]",
    summary: "Inspects editable text runs and their revision for replace_text operations.",
    parse: |args| parse(args, Mode::TextRuns).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Edit,
    Comments,
    TextRuns,
}

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_OPERATIONS: usize = 1_000;

#[derive(Debug)]
struct Edit {
    input: PathBuf,
    output: PathBuf,
    request: String,
    password_env: Option<String>,
    invalidate: bool,
    force: bool,
    dry_run: bool,
    json: bool,
    mode: Mode,
    page: u32,
}

fn parse(args: &[String], mode: Mode) -> Result<Edit, String> {
    let mut command = Edit {
        input: PathBuf::new(),
        output: PathBuf::new(),
        request: String::new(),
        password_env: None,
        invalidate: false,
        force: false,
        dry_run: false,
        json: false,
        mode,
        page: 1,
    };
    let mut paths = Vec::new();
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" if mode == Mode::Edit => {
                command.output = argument(arg, &mut rest)?.into()
            }
            "--plan" if mode == Mode::Edit => command.request.clone_from(argument(arg, &mut rest)?),
            "--password-env" => command.password_env = Some(variable(argument(arg, &mut rest)?)?),
            "--invalidate-signatures" if mode == Mode::Edit => command.invalidate = true,
            "--force" if mode == Mode::Edit => command.force = true,
            "--dry-run" if mode == Mode::Edit => command.dry_run = true,
            "--page" if mode == Mode::TextRuns => {
                command.page = argument(arg, &mut rest)?
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("--page needs a positive integer")?;
            }
            "--json" => command.json = true,
            flag if flag.starts_with('-') => {
                return Err(unknown(
                    match mode {
                        Mode::Comments => "comments",
                        Mode::TextRuns => "text-runs",
                        Mode::Edit => "edit",
                    },
                    flag,
                ))
            }
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("exactly one input document is required".into());
    }
    command.input = paths.remove(0);
    if mode == Mode::Edit && (command.output.as_os_str().is_empty() || command.request.is_empty()) {
        return Err("edit requires --plan <edits.json | -> and -o <out.pdf>".into());
    }
    Ok(command)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    schema: u32,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Rotate {
        page: u32,
        degrees: i16,
    },
    DeletePage {
        page: u32,
    },
    MovePage {
        page: u32,
        to: u32,
    },
    InsertBlank {
        after: u32,
        width: f64,
        height: f64,
    },
    Annotate {
        page: u32,
        kind: MarkKind,
        #[serde(default)]
        rect: Option<[f32; 4]>,
        #[serde(default)]
        strokes: Vec<Vec<f32>>,
        #[serde(default)]
        stamp: Option<StampName>,
        #[serde(default = "default_color")]
        color: [f32; 3],
        #[serde(default = "default_width")]
        width: f64,
        #[serde(default)]
        author: String,
        #[serde(default)]
        text: String,
    },
    ReplaceText {
        page: u32,
        operator: u32,
        revision: Vec<u8>,
        original: String,
        replacement: String,
    },
    RewriteComment {
        page: u32,
        object: (u32, u16),
        text: String,
    },
    DeleteComment {
        page: u32,
        object: (u32, u16),
    },
    Undo,
    Redo,
}

fn default_color() -> [f32; 3] {
    [1., 0., 0.]
}
fn default_width() -> f64 {
    INK_WIDTH
}

fn decode(text: &str) -> Result<EditRequest, String> {
    if text.len() as u64 > MAX_BYTES {
        return Err("edit plan exceeds 1 MiB".into());
    }
    let request: EditRequest =
        serde_json::from_str(text).map_err(|e| format!("invalid edit plan: {e}"))?;
    if request.schema != 1 {
        return Err(format!(
            "unsupported edit plan schema {}; expected 1",
            request.schema
        ));
    }
    if request.operations.is_empty() || request.operations.len() > MAX_OPERATIONS {
        return Err(format!(
            "an edit plan needs 1 to {MAX_OPERATIONS} operations"
        ));
    }
    Ok(request)
}

fn read_request(path: &str) -> Result<EditRequest, Failure> {
    let mut text = String::new();
    let read = if path == "-" {
        std::io::stdin()
            .lock()
            .take(MAX_BYTES + 1)
            .read_to_string(&mut text)
    } else {
        std::fs::File::open(path).and_then(|f| f.take(MAX_BYTES + 1).read_to_string(&mut text))
    };
    read.map_err(|e| Failure::new(Exit::Refused, format!("could not read edit plan: {e}")))?;
    decode(&text).map_err(|e| Failure::new(Exit::Refused, e))
}

fn page(state: &EditState, number: u32) -> Result<u64, String> {
    number
        .checked_sub(1)
        .and_then(|i| state.pages.get(i as usize))
        .map(|p| p.id)
        .ok_or_else(|| {
            format!(
                "page {number} is outside the current 1..={} pages",
                state.pages.len()
            )
        })
}

fn dimensions(plan: &Plan, baseline: &[PageSize]) -> Vec<PageSize> {
    plan.pages
        .iter()
        .map(|p| {
            let size = match p.source {
                PageSource::Baseline(n) => baseline[n as usize],
                PageSource::Blank(size) => PageSize {
                    width_pt: size.width as f32,
                    height_pt: size.height as f32,
                },
                PageSource::Imported { .. } => unreachable!("edit requests cannot import pages"),
            };
            if p.turns % 2 == 0 {
                size
            } else {
                PageSize {
                    width_pt: size.height_pt,
                    height_pt: size.width_pt,
                }
            }
        })
        .collect()
}

fn unturn(x: f32, y: f32, turns: u8, shown: PageSize) -> [f32; 2] {
    match turns % 4 {
        1 => [y, shown.width_pt - x],
        2 => [shown.width_pt - x, shown.height_pt - y],
        3 => [shown.height_pt - y, x],
        _ => [x, y],
    }
}

// Model validation is a refusal; transport failures keep their internal-error
// status even when adding the operation number to the diagnostic.
struct OperationError(Failure);
impl From<String> for OperationError {
    fn from(message: String) -> Self {
        Self(Failure::new(Exit::Refused, message))
    }
}
impl From<&str> for OperationError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
impl From<crate::save_outside::Declined> for OperationError {
    fn from(error: crate::save_outside::Declined) -> Self {
        Self(error.into())
    }
}
impl OperationError {
    fn failed(message: &str) -> Self {
        Self(Failure::new(Exit::Internal, message))
    }
    fn at(self, index: usize) -> Failure {
        Failure::new(
            self.0.exit,
            format!("operation {}: {}", index + 1, self.0.message),
        )
    }
}

fn apply(
    request: &EditRequest,
    baseline: &super::pages::Input,
    now: u64,
    mut ask: impl FnMut(Request) -> Result<Reply, crate::save_outside::Declined>,
) -> Result<Plan, Failure> {
    let model = Edits::default();
    model.open(1, baseline.plan.baseline, None);
    let made = crate::save::pdf_date(UNIX_EPOCH + Duration::from_secs(now));
    let mut comments = None;
    for (index, op) in request.operations.iter().enumerate() {
        let result = (|| -> Result<(), OperationError> {
            let state = model.state(1)?;
            match op {
                Operation::Rotate { page: n, degrees } => {
                    let turns = match degrees {
                        90 => 1,
                        180 => 2,
                        270 => 3,
                        -90 => -1,
                        -180 => -2,
                        -270 => -3,
                        _ => return Err("degrees must be 90, 180, 270, -90, -180 or -270".into()),
                    };
                    model.rotate(1, page(&state, *n)?, turns)?;
                }
                Operation::DeletePage { page: n } => {
                    model.delete(1, page(&state, *n)?)?;
                }
                Operation::MovePage { page: n, to } => {
                    let id = page(&state, *n)?;
                    page(&state, *to)?;
                    if n != to {
                        let others: Vec<_> = state.pages.iter().filter(|p| p.id != id).collect();
                        let after = to.checked_sub(2).map(|i| others[i as usize].id);
                        model.move_page(1, id, after)?;
                    }
                }
                Operation::InsertBlank {
                    after,
                    width,
                    height,
                } => {
                    if state.pages.len() >= 100_000 {
                        return Err("at most 100000 pages are supported".into());
                    }
                    if !width.is_finite()
                        || !height.is_finite()
                        || *width <= 0.
                        || *height <= 0.
                        || *width > 14400.
                        || *height > 14400.
                    {
                        return Err(
                            "blank page dimensions must be positive and at most 14400 points"
                                .into(),
                        );
                    }
                    let anchor = if *after == 0 {
                        None
                    } else {
                        Some(page(&state, *after)?)
                    };
                    model.insert(1, anchor, [*width, *height])?;
                }
                Operation::Annotate {
                    page: n,
                    kind,
                    rect,
                    strokes,
                    stamp,
                    color,
                    width,
                    author,
                    text,
                } => {
                    let id = page(&state, *n)?;
                    if *kind == MarkKind::Signature {
                        return Err("signature images are not supported by edit schema 1".into());
                    }
                    if color
                        .iter()
                        .any(|c| !c.is_finite() || !(0. ..=1.).contains(c))
                        || !width.is_finite()
                        || !(NIB_MIN..=NIB_MAX).contains(width)
                    {
                        return Err(format!(
                            "color channels must be 0..1 and width {NIB_MIN}..{NIB_MAX} points"
                        )
                        .into());
                    }
                    if author.chars().count() > 120 || text.chars().count() > 4_000 {
                        return Err(
                            "annotation author exceeds 120 characters or text exceeds 4000".into(),
                        );
                    }
                    let size = dimensions(&model.plan(1)?, &baseline.sizes)[(*n - 1) as usize];
                    let within = |x: f32, y: f32| {
                        x.is_finite()
                            && y.is_finite()
                            && x >= 0.
                            && y >= 0.
                            && x <= size.width_pt
                            && y <= size.height_pt
                    };
                    let mut quads = if *kind == MarkKind::Ink {
                        if rect.is_some() {
                            return Err("ink takes strokes, not rect".into());
                        }
                        if strokes.iter().any(|s| {
                            s.len() % 2 != 0 || s.chunks_exact(2).any(|p| !within(p[0], p[1]))
                        }) {
                            return Err("ink strokes must contain x,y pairs within the page".into());
                        }
                        Vec::new()
                    } else {
                        let [x, y, w, h] =
                            rect.ok_or("annotation needs rect: [x,y,width,height]")?;
                        if !within(x, y) || !within(x + w, y + h) || w <= 0. || h <= 0. {
                            return Err("annotation rectangle must fit the displayed page".into());
                        }
                        vec![x, y, x + w, y + h]
                    };
                    // The GUI journals geometry before its extra page turns.
                    // Undo those turns here as viewer.unturnQuad does; the writer
                    // applies the journal rotation after writing the marks.
                    let turns = state.pages[(*n - 1) as usize].turns;
                    if !quads.is_empty() {
                        let a = unturn(quads[0], quads[1], turns, size);
                        let b = unturn(quads[2], quads[3], turns, size);
                        quads = vec![
                            a[0].min(b[0]),
                            a[1].min(b[1]),
                            a[0].max(b[0]),
                            a[1].max(b[1]),
                        ];
                    }
                    let strokes = strokes
                        .iter()
                        .map(|stroke| {
                            stroke
                                .chunks_exact(2)
                                .flat_map(|p| unturn(p[0], p[1], turns, size))
                                .collect()
                        })
                        .collect();
                    model.annotate(
                        1,
                        NewMark {
                            kind: *kind,
                            page: id,
                            quads,
                            strokes,
                            stamp: *stamp,
                            image: None,
                            reply_to: None,
                            color: *color,
                            width: *width,
                            author: author.clone(),
                            note: text.clone(),
                        },
                        made.clone(),
                    )?;
                }
                Operation::ReplaceText {
                    page: n,
                    operator,
                    revision,
                    original,
                    replacement,
                } => {
                    let id = page(&state, *n)?;
                    let PageSource::Baseline(baseline_page) = state.pages[(*n - 1) as usize].source
                    else {
                        return Err("this page has no original text".into());
                    };
                    if revision.len() != 32 {
                        return Err(
                            "revision must contain the 32 bytes returned by text-runs".into()
                        );
                    }
                    let change = crate::textedit::Change {
                        page: baseline_page,
                        operator: *operator,
                        revision: revision.clone(),
                        original: original.clone(),
                        replacement: replacement.clone(),
                        layout: None,
                    };
                    let mut pending: Vec<_> = model
                        .text_changes(1)
                        .into_iter()
                        .map(|e| e.change)
                        .filter(|c| (c.page, c.operator) != (change.page, change.operator))
                        .collect();
                    // The worker validates the original revision, the text, glyph
                    // coverage and available space; unchanged replacements are refused.
                    pending.push(change.clone());
                    let reply = ask(Request::TextRuns {
                        page: baseline_page,
                        changes: pending,
                    })?;
                    if !matches!(reply, Reply::TextRuns(_)) {
                        return Err(OperationError::failed(
                            "worker did not validate the text change",
                        ));
                    }
                    model.replace_text(1, id, change)?;
                }
                Operation::RewriteComment {
                    page: n, object, ..
                }
                | Operation::DeleteComment { page: n, object } => {
                    let id = page(&state, *n)?;
                    let PageSource::Baseline(baseline_page) = state.pages[(*n - 1) as usize].source
                    else {
                        return Err("this page has no original comments".into());
                    };
                    if comments.is_none() {
                        let reply = ask(Request::Comments)?;
                        let Reply::Comments(found) = reply else {
                            return Err(OperationError::failed("worker did not return comments"));
                        };
                        comments = Some(found);
                    }
                    if !comments
                        .as_ref()
                        .unwrap()
                        .items
                        .iter()
                        .any(|c| c.page == baseline_page && c.object == Some(*object))
                    {
                        return Err("comment object is not on the selected page".into());
                    }
                    if let Operation::RewriteComment { text, .. } = op {
                        model.rewrite(1, *object, id, text.clone(), made.clone())?;
                    } else {
                        model.discard(1, *object, id)?;
                    }
                }
                Operation::Undo => {
                    if !state.can_undo {
                        return Err("nothing to undo".into());
                    }
                    model.undo(1)?;
                }
                Operation::Redo => {
                    if !state.can_redo {
                        return Err("nothing to redo".into());
                    }
                    model.redo(1)?;
                }
            }
            Ok(())
        })();
        result.map_err(|error| error.at(index))?;
    }
    let mut plan = model.plan(1).map_err(|e| Failure::new(Exit::Internal, e))?;
    plan.opened_as.clone_from(&baseline.plan.opened_as);
    Ok(plan)
}

impl Subcommand for Edit {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let request = if self.mode != Mode::Edit {
            None
        } else {
            Some(read_request(&self.request)?)
        };
        let key = password(self.password_env.as_deref())?;
        let (before, mut session) = read_input(env, &self.input, key.as_deref())?;
        if self.mode == Mode::TextRuns {
            if self.page as usize > before.sizes.len() {
                return Err(Failure::new(Exit::Refused, "page is outside the document"));
            }
            let reply = session
                .ask(Request::TextRuns {
                    page: self.page - 1,
                    changes: Vec::new(),
                })
                .map_err(Failure::from)?;
            let Reply::TextRuns(runs) = reply else {
                return Err(Failure::new(
                    Exit::Internal,
                    "worker did not return text runs",
                ));
            };
            if self.json {
                json(
                    out,
                    &report::TextRuns {
                        schema: SCHEMA,
                        command: "text-runs".into(),
                        input: self.input.display().to_string(),
                        page: self.page,
                        revision: runs.revision,
                        runs: runs.runs,
                    },
                );
            } else {
                for run in runs.runs {
                    say(out, &format!("{}: {}", run.operator, run.text));
                }
            }
            return Ok(Exit::Ok);
        }
        if self.mode == Mode::Comments {
            let reply = session.ask(Request::Comments).map_err(Failure::from)?;
            let Reply::Comments(mut comments) = reply else {
                return Err(Failure::new(
                    Exit::Internal,
                    "worker did not return comments",
                ));
            };
            let complete = !comments.limits.any();
            if self.json {
                for item in &mut comments.items {
                    item.page += 1;
                }
                json(
                    out,
                    &report::Comments {
                        schema: SCHEMA,
                        command: "comments".into(),
                        input: self.input.display().to_string(),
                        complete,
                        comments: comments.items,
                        limits: comments.limits,
                    },
                );
            } else {
                for item in &comments.items {
                    say(
                        out,
                        &format!("Page {}: {}: {}", item.page + 1, item.author, item.body),
                    );
                }
                if !complete {
                    say(
                        out,
                        "Some annotations could not be fully read; use --json for scan limits.",
                    );
                }
            }
            return Ok(if complete { Exit::Ok } else { Exit::Strict });
        }
        if before.signed.is_some() && !self.invalidate {
            return Err(Failure::new(Exit::Refused, "the document is signed or its signatures could not be fully read; edit requires --invalidate-signatures"));
        }
        let request = request.expect("edit has a request");
        let plan = apply(&request, &before, env.now, |request| session.ask(request))?;
        drop(session);
        let sizes = dimensions(&plan, &before.sizes);
        let inputs = [self.input.clone()];
        check_target(&inputs, &self.output, self.force)?;
        let fingerprint = before
            .plan
            .opened_as
            .as_ref()
            .expect("read_input fingerprints the file");
        fingerprint
            .agrees_with(&self.input)
            .map_err(|e| Failure::new(Exit::Refused, e))?;
        if !self.dry_run {
            let staging = Temporary::beside(&self.output)?;
            let staged = staging.0.join("edited.pdf");
            let written =
                crate::save::write_copy(&self.input, &plan, &staged, key.as_deref(), &env.worker())
                    .map_err(|e| Failure::new(Exit::Refused, e.message))?;
            if written.changed {
                return Err(Failure::new(
                    Exit::Refused,
                    "source changed while writing; no output was published",
                ));
            }
            let (after, session) = read_input(env, &staged, key.as_deref()).map_err(|e| {
                Failure::new(
                    Exit::Internal,
                    format!("staged output could not be read: {}", e.message),
                )
            })?;
            drop(session);
            if before.encrypted != after.encrypted || !same_sizes(&after.sizes, &sizes) {
                return Err(Failure::new(
                    Exit::Internal,
                    "staged page sizes or encryption do not match; no output was published",
                ));
            }
            fingerprint
                .agrees_with(&self.input)
                .map_err(|e| Failure::new(Exit::Refused, e))?;
            check_target(&inputs, &self.output, self.force)?;
            Temporary::publish(&staged, &self.output, self.force)?;
        }
        if self.json {
            json(
                out,
                &report::Edited {
                    schema: SCHEMA,
                    command: "edit".into(),
                    input: self.input.display().to_string(),
                    output: self.output.display().to_string(),
                    written: !self.dry_run,
                    operations: request.operations.len(),
                    pages: sizes,
                    annotations: plan.marks.len(),
                    signatures_invalidated: if self.dry_run {
                        0
                    } else {
                        match before.signed {
                            Some(SignedState::Signed(n)) => n,
                            _ => 0,
                        }
                    },
                    signatures_unknown: before.signatures_unknown,
                },
            );
        } else {
            say(
                out,
                &format!(
                    "{}: {} operations, {} pages{}",
                    self.output.display(),
                    request.operations.len(),
                    plan.pages.len(),
                    if self.dry_run {
                        " (dry run; nothing written)"
                    } else {
                        ""
                    }
                ),
            );
        }
        Ok(Exit::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_plan_rejects_unknown_fields_versions_and_unbounded_requests() {
        for text in [
            r#"{"schema":2,"operations":[{"op":"undo"}]}"#,
            r#"{"schema":1,"operations":[]}"#,
            r#"{"schema":1,"operations":[{"op":"rotate","page":1,"degrees":90,"degres":180}]}"#,
            r#"{"schema":1,"operations":[{"op":"undo"}],"force":true}"#,
            r#"{"schema":1,"operations":[{"op":"redact","page":1}]}"#,
            r#"{"schema":1,"operations":[{"op":"undo","op":"redo"}]}"#,
        ] {
            assert!(decode(text).is_err(), "{text}");
        }
        assert!(decode(&" ".repeat(MAX_BYTES as usize + 1))
            .unwrap_err()
            .contains("1 MiB"));
        let oversized =
            value!({"schema": 1, "operations": vec![value!({"op":"undo"}); MAX_OPERATIONS + 1]});
        assert!(decode(&oversized.to_string()).unwrap_err().contains("1000"));
        assert!(decode(r#"{"schema":1,"operations":[{"op":"undo"}]}"#).is_ok());
    }

    #[test]
    fn worker_failures_keep_their_exit_code_and_operation_context() {
        use crate::save_outside::Declined;
        let model = Edits::default();
        model.open(1, 1, None);
        let baseline = super::super::pages::Input {
            plan: model.plan(1).unwrap(),
            sizes: vec![PageSize {
                width_pt: 200.,
                height_pt: 300.,
            }],
            encrypted: false,
            signed: None,
            signatures_unknown: false,
        };
        let request = decode(&value!({"schema":1,"operations":[
            {"op":"rotate","page":1,"degrees":90},
            {"op":"replace_text","page":1,"operator":3,"revision":vec![0;32],"original":"Original","replacement":"New"}
        ]}).to_string()).unwrap();
        for (response, expected) in [
            (
                Err(Declined::Failed("worker stopped".into())),
                Exit::Internal,
            ),
            (
                Err(Declined::Refused("stale revision".into())),
                Exit::Refused,
            ),
            (
                Ok(Reply::Comments(crate::annots::Comments::default())),
                Exit::Internal,
            ),
        ] {
            let mut response = Some(response);
            let failure = apply(&request, &baseline, 0, |_| response.take().unwrap()).unwrap_err();
            assert_eq!(failure.exit, expected);
            assert!(
                failure.message.starts_with("operation 2:"),
                "{}",
                failure.message
            );
            assert!(response.is_none(), "the worker was actually asked");
        }
    }

    #[test]
    fn edit_readers_refuse_write_flags_and_bad_page_arguments() {
        for (mode, args) in [
            (Mode::Comments, "a.pdf --force"),
            (Mode::TextRuns, "a.pdf --page 0"),
            (Mode::TextRuns, "a.pdf -o b.pdf"),
            (Mode::Edit, "a.pdf --plan -"),
            (Mode::Edit, "a.pdf -o b.pdf"),
        ] {
            assert!(parse(
                &args
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                mode
            )
            .is_err());
        }
    }
}
