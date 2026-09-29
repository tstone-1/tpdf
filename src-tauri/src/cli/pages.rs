//! Page operations use the GUI's sandboxed writers. Outputs are staged beside
//! their destinations and checked before publication. Without --force, hard
//! linking the completed file refuses a destination that appeared mid-command.
//! Splits stage all parts first; a publication failure reports exactly the
//! parts already published. No rollback deletes a file another process may use.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::args::{lexically_same, unknown, value};
use super::fill::{plan, signed_state, SignedState};
use super::report::{self, SCHEMA};
use super::text::{declined, page_list, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::edits::Plan;
use crate::fingerprint::Fingerprint;
use crate::render::PageSize;
use crate::save;
use crate::save_outside::Session;
use crate::worker_proto::{Reply, Request};

const MAX_OUTPUTS: usize = 10_000;

macro_rules! command {
    ($constant:ident, $kind:ident, $name:literal, $usage:literal, $summary:literal) => {
        pub const $constant: Registered = Registered {
            name: $name,
            usage: concat!(
                $usage,
                "\n        [--password-env VAR] [--invalidate-signatures] [--force] [--json]"
            ),
            summary: $summary,
            parse: |args| parse(Kind::$kind, args).map(|p| Box::new(p) as Box<dyn Subcommand>),
        };
    };
}
command!(
    MERGE,
    Merge,
    "merge",
    "merge <in.pdf> <other.pdf>... -o <out.pdf>",
    "Combines documents in argument order. Only the first input may be encrypted."
);
command!(
    EXTRACT,
    Extract,
    "extract",
    "extract <in.pdf> --pages 1-3,7 -o <out.pdf>",
    "Copies selected pages in document order, each once."
);
command!(
    SPLIT,
    Split,
    "split",
    "split <in.pdf> -o <part.pdf> [--every N]",
    "Writes consecutive groups of N pages (default 1) as part-1.pdf, part-2.pdf, ..."
);
command!(
    ROTATE,
    Rotate,
    "rotate",
    "rotate <in.pdf> --degrees 90|180|270 -o <out.pdf> [--pages 1-3,7]",
    "Turns selected pages clockwise, relative to their existing rotation. -o is required."
);
command!(CROP, Crop, "crop", "crop <in.pdf> --rect x,y,w,h -o <out.pdf> [--pages 1-3,7]",
    "Sets the visible area in display points from the top-left corner. Hidden content stays in the PDF; this is not redaction.");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Merge,
    Extract,
    Split,
    Rotate,
    Crop,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Extract => "extract",
            Self::Split => "split",
            Self::Rotate => "rotate",
            Self::Crop => "crop",
        }
    }
}

#[derive(Debug)]
struct Pages {
    kind: Kind,
    inputs: Vec<PathBuf>,
    output: PathBuf,
    pages: Option<Vec<u32>>,
    every: usize,
    turns: u8,
    rect: Option<[f32; 4]>,
    password_env: Option<String>,
    invalidate: bool,
    force: bool,
    json: bool,
}

fn parse(kind: Kind, args: &[String]) -> Result<Pages, String> {
    let mut parsed = Pages {
        kind,
        inputs: Vec::new(),
        output: PathBuf::new(),
        pages: None,
        every: 1,
        turns: 0,
        rect: None,
        password_env: None,
        invalidate: false,
        force: false,
        json: false,
    };
    let mut output = None;
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            parsed.inputs.push(arg.into());
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--pages" if matches!(kind, Kind::Extract | Kind::Rotate | Kind::Crop) => {
                parsed.pages = Some(page_list(value(arg, &mut rest)?)?)
            }
            "--every" if kind == Kind::Split => {
                parsed.every = value(arg, &mut rest)?
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("--every needs a positive integer")?;
            }
            "--degrees" if kind == Kind::Rotate => {
                parsed.turns = match value(arg, &mut rest)?.as_str() {
                    "90" => 1,
                    "180" => 2,
                    "270" => 3,
                    _ => return Err("--degrees must be 90, 180 or 270 (clockwise)".into()),
                };
            }
            "--rect" if kind == Kind::Crop => {
                parsed.rect = Some(rectangle(value(arg, &mut rest)?)?)
            }
            "--password-env" => parsed.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => parsed.invalidate = true,
            "--force" => parsed.force = true,
            "--json" => parsed.json = true,
            flag if flag.starts_with('-') => return Err(unknown(kind.name(), flag)),
            path => parsed.inputs.push(path.into()),
        }
    }
    if kind != Kind::Merge && parsed.inputs.len() != 1 {
        return Err(format!("{} needs exactly one input document", kind.name()));
    }
    if kind == Kind::Merge && parsed.inputs.len() < 2 {
        return Err("merge needs at least two input documents".into());
    }
    parsed.output = output.ok_or("-o <out.pdf> is required; inputs are never overwritten")?;
    if parsed
        .inputs
        .iter()
        .any(|p| lexically_same(p, &parsed.output))
    {
        return Err("the output names an input; choose a different name".into());
    }
    if kind == Kind::Extract && parsed.pages.is_none() {
        return Err("extract needs --pages".into());
    }
    if kind == Kind::Rotate && parsed.turns == 0 {
        return Err("rotate needs --degrees".into());
    }
    if kind == Kind::Crop && parsed.rect.is_none() {
        return Err("crop needs --rect".into());
    }
    Ok(parsed)
}

fn rectangle(raw: &str) -> Result<[f32; 4], String> {
    let values = raw
        .split(',')
        .map(|s| s.trim().parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "--rect needs four finite numbers: x,y,w,h")?;
    let [x, y, w, h]: [f32; 4] = values.try_into().map_err(|_| "--rect needs x,y,w,h")?;
    if ![x, y, w, h, x + w, y + h].iter().all(|n| n.is_finite())
        || x < 0.0
        || y < 0.0
        || w <= 0.0
        || h <= 0.0
    {
        return Err("--rect needs nonnegative x,y and positive width,height, all finite".into());
    }
    Ok([x, y, w, h])
}

pub(super) struct Input {
    pub plan: Plan,
    pub sizes: Vec<PageSize>,
    pub encrypted: bool,
    pub signed: Option<SignedState>,
    pub signatures_unknown: bool,
}

fn ask(
    session: &mut Session,
    request: Request,
    path: &Path,
    has_password: bool,
) -> Result<Reply, Failure> {
    session
        .ask(request)
        .map_err(|why| declined(&path.display().to_string(), why, has_password))
}

fn unexpected(reply: &Reply) -> Failure {
    Failure::new(
        Exit::Internal,
        format!("unexpected worker reply: {reply:?}"),
    )
}

pub(super) fn read_input(
    env: &Env<'_>,
    path: &Path,
    key: Option<&str>,
) -> Result<(Input, Session), Failure> {
    let (file, len) = opened(path).map_err(|why| Failure::new(Exit::Refused, why))?;
    let fingerprint =
        Fingerprint::of_open(&file, path).map_err(|why| Failure::new(Exit::Refused, why))?;
    let mut session = env
        .worker()
        .session(&file, len, key)
        .map_err(|why| declined(&path.display().to_string(), why, key.is_some()))?;
    let Reply::Properties(properties) =
        ask(&mut session, Request::Properties, path, key.is_some())?
    else {
        return Err(Failure::new(
            Exit::Internal,
            "the worker did not return document properties",
        ));
    };
    let reply = ask(
        &mut session,
        Request::Open {
            lazy_geometry: false,
        },
        path,
        key.is_some(),
    )?;
    let Reply::Open {
        page_count, pages, ..
    } = reply
    else {
        return Err(unexpected(&reply));
    };
    let count = u32::try_from(page_count)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Failure::new(Exit::Refused, "the document has no usable pages"))?;
    if pages.len() != page_count {
        return Err(Failure::new(
            Exit::Internal,
            "the worker returned incomplete page sizes",
        ));
    }
    Ok((
        Input {
            plan: plan(count, &[], fingerprint),
            sizes: pages,
            encrypted: properties.encryption.is_some(),
            signed: signed_state(&properties),
            signatures_unknown: properties.limits.locked
                || properties.limits.unreadable > 0
                || properties.limits.signatures_dropped > 0,
        },
        session,
    ))
}

pub(super) fn check_target(inputs: &[PathBuf], target: &Path, force: bool) -> Result<(), Failure> {
    if inputs.iter().any(|p| save::same_file(p, target)) {
        return Err(Failure::new(
            Exit::Usage,
            "an output names an input under another name",
        ));
    }
    // symlink_metadata also sees dangling links; --force does not follow
    // them to an unrelated file when the save helper resolves its target.
    if let Ok(metadata) = target.symlink_metadata() {
        if metadata.file_type().is_symlink() || !metadata.is_file() || !force {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} exists; choose a new name or use --force for a regular file",
                    target.display()
                ),
            ));
        }
    }
    Ok(())
}

impl Pages {
    fn targets(&self, count: usize) -> Result<Vec<PathBuf>, Failure> {
        if self.kind != Kind::Split {
            return Ok(vec![self.output.clone()]);
        }
        let parts = count.div_ceil(self.every);
        if parts > MAX_OUTPUTS {
            return Err(Failure::new(Exit::Refused, format!("split would write {parts} files; at most {MAX_OUTPUTS} are allowed, use a larger --every")));
        }
        Ok(save::split_paths(&self.output, parts))
    }

    fn check_target(&self, target: &Path) -> Result<(), Failure> {
        check_target(&self.inputs, target, self.force)
    }

    fn run_pages(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let key = password(self.password_env.as_deref())?;
        let mut inputs = Vec::new();
        for (i, path) in self.inputs.iter().enumerate() {
            let (mut input, mut session) =
                read_input(env, path, if i == 0 { key.as_deref() } else { None })?;
            if input.signed.is_some() && !self.invalidate {
                return Err(Failure::new(Exit::Refused, format!("{} is signed or its signatures could not be fully read; this rewrite requires --invalidate-signatures", path.display())));
            }
            if i > 0 && input.encrypted {
                return Err(Failure::new(Exit::Refused, "merge accepts encryption only on its first input, whose encryption is preserved"));
            }
            if i == 0 {
                self.prepare(&mut input, &mut session, path, key.is_some())?;
            }
            inputs.push(input);
        }
        let targets = self.targets(inputs[0].plan.pages.len())?;
        for target in &targets {
            self.check_target(target)?;
        }
        let mut report = report::Pages {
            schema: SCHEMA,
            command: self.kind.name().into(),
            inputs: self
                .inputs
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            complete: false,
            outputs: Vec::new(),
            signatures_invalidated: inputs
                .iter()
                .map(|i| match i.signed {
                    Some(SignedState::Signed(n)) => n,
                    _ => 0,
                })
                .sum(),
            signatures_unknown: inputs.iter().any(|i| i.signatures_unknown),
            error: None,
        };
        let base = &inputs[0];
        let staging = Temporary::beside(&self.output)?;
        let staged_base = staging.0.join("output.pdf");
        unchanged(&inputs, &self.inputs)?;
        let (staged_paths, sizes, changed) = if self.kind == Kind::Split && targets.len() > 1 {
            let mut template = base.plan.clone();
            template.pages.clear();
            let plans: Vec<Plan> = base
                .plan
                .pages
                .chunks(self.every)
                .map(|chunk| {
                    let mut selected = template.clone();
                    selected.pages = chunk.to_vec();
                    selected
                })
                .collect();
            let result = save::write_split(
                &self.inputs[0],
                &plans,
                &staged_base,
                key.as_deref(),
                &env.worker(),
            )
            .map_err(|why| Failure::new(Exit::Refused, why.message))?;
            (
                save::split_paths(&staged_base, targets.len()),
                base.sizes
                    .chunks(self.every)
                    .map(<[PageSize]>::to_vec)
                    .collect::<Vec<_>>(),
                result.changed,
            )
        } else {
            let (expected, changed) = if self.kind == Kind::Merge {
                let result = save::write_merged(
                    &self.inputs[0],
                    &base.plan,
                    &self.inputs[1..],
                    &staged_base,
                    key.as_deref(),
                    &env.worker(),
                )
                .map_err(|why| Failure::new(Exit::Refused, why.message))?;
                (
                    inputs
                        .iter()
                        .flat_map(|i| i.sizes.iter().copied())
                        .collect(),
                    result.changed,
                )
            } else {
                let result = save::write_copy(
                    &self.inputs[0],
                    &base.plan,
                    &staged_base,
                    key.as_deref(),
                    &env.worker(),
                )
                .map_err(|why| Failure::new(Exit::Refused, why.message))?;
                (base.sizes.clone(), result.changed)
            };
            (vec![staged_base], vec![expected], changed)
        };
        if changed {
            return Err(Failure::new(
                Exit::Refused,
                "the source changed while writing; no output was published",
            ));
        }
        for (temp, sizes) in staged_paths.iter().zip(&sizes) {
            let (after, session) = read_input(env, temp, key.as_deref()).map_err(|why| {
                Failure::new(
                    Exit::Internal,
                    format!("the staged file could not be checked: {}", why.message),
                )
            })?;
            drop(session);
            if after.encrypted != base.encrypted || !same_sizes(&after.sizes, sizes) {
                return Err(Failure::new(Exit::Internal, "the staged file's pages or encryption did not match the request; no output was published"));
            }
        }
        unchanged(&inputs, &self.inputs)?;
        let result = self.publish(&staged_paths, &sizes, &targets, &mut report);
        let exit = match result {
            Ok(()) => Exit::Ok,
            Err(failure) => {
                report.error = Some(report::CommandError::from_failure(&failure));
                say(
                    err,
                    &format!(
                        "{}: {} ({} of {} files published)",
                        env.program,
                        failure.message,
                        report.outputs.len(),
                        targets.len()
                    ),
                );
                failure.exit
            }
        };
        self.print(out, &report);
        Ok(exit)
    }

    fn publish(
        &self,
        staged: &[PathBuf],
        sizes: &[Vec<PageSize>],
        targets: &[PathBuf],
        report: &mut report::Pages,
    ) -> Result<(), Failure> {
        for ((temp, sizes), target) in staged.iter().zip(sizes).zip(targets) {
            self.check_target(target)?;
            Temporary::publish(temp, target, self.force)?;
            report.outputs.push(report::PageOutput {
                path: target.display().to_string(),
                pages: sizes.len(),
            });
        }
        report.complete = true;
        Ok(())
    }

    fn print(&self, out: &mut dyn Write, report: &report::Pages) {
        if self.json {
            json(out, report);
        } else {
            for output in &report.outputs {
                say(
                    out,
                    &format!("{}: {} pages written", output.path, output.pages),
                );
            }
            if report.signatures_invalidated > 0 || report.signatures_unknown {
                say(out, "The rewrite invalidates existing signatures.");
            }
        }
    }

    fn prepare(
        &self,
        input: &mut Input,
        session: &mut Session,
        path: &Path,
        has_password: bool,
    ) -> Result<(), Failure> {
        let selection = self
            .pages
            .clone()
            .unwrap_or_else(|| (1..=input.plan.baseline).collect());
        if selection.iter().any(|n| *n > input.plan.baseline) {
            return Err(Failure::new(
                Exit::Refused,
                "--pages names a page past the end of the document",
            ));
        }
        match self.kind {
            Kind::Extract => {
                input.plan.pages = selection
                    .iter()
                    .map(|n| input.plan.pages[*n as usize - 1])
                    .collect();
                input.sizes = selection
                    .iter()
                    .map(|n| input.sizes[*n as usize - 1])
                    .collect();
            }
            Kind::Rotate => {
                for n in selection {
                    let at = n as usize - 1;
                    input.plan.pages[at].turns = self.turns;
                    if self.turns % 2 == 1 {
                        let size = &mut input.sizes[at];
                        std::mem::swap(&mut size.width_pt, &mut size.height_pt);
                    }
                }
            }
            Kind::Crop => {
                for n in selection {
                    let at = n as usize - 1;
                    let rect = self.rect.expect("parser requires rect");
                    if rect[0] + rect[2] > input.sizes[at].width_pt
                        || rect[1] + rect[3] > input.sizes[at].height_pt
                    {
                        return Err(Failure::new(
                            Exit::Refused,
                            format!("--rect lies outside page {n}"),
                        ));
                    }
                    let reply = ask(
                        session,
                        Request::CropBox {
                            page: n - 1,
                            rect: [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                        },
                        path,
                        has_password,
                    )?;
                    let Reply::CropBox(crop) = reply else {
                        return Err(unexpected(&reply));
                    };
                    input.plan.pages[at].crop = Some(crop.map(f64::from));
                    input.sizes[at] = PageSize {
                        width_pt: rect[2],
                        height_pt: rect[3],
                    };
                }
            }
            Kind::Merge | Kind::Split => {}
        }
        Ok(())
    }
}

impl Subcommand for Pages {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_pages(env, out, err)
    }
}

pub(super) fn same_sizes(actual: &[PageSize], expected: &[PageSize]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, b)| {
            (a.width_pt - b.width_pt).abs() < 0.02 && (a.height_pt - b.height_pt).abs() < 0.02
        })
}

fn unchanged(inputs: &[Input], paths: &[PathBuf]) -> Result<(), Failure> {
    for (input, path) in inputs.iter().zip(paths) {
        input
            .plan
            .opened_as
            .as_ref()
            .expect("read_input fingerprints every source")
            .agrees_with(path)
            .map_err(|why| Failure::new(Exit::Refused, why))?;
    }
    Ok(())
}

/// Owns only a staging directory this process created exclusively; never an output name.
pub(super) struct Temporary(pub PathBuf);
impl Temporary {
    pub(super) fn beside(target: &Path) -> Result<Self, Failure> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        for _ in 0..100 {
            let path = parent.join(format!(
                ".tpdf-cli-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(Failure::new(
                        Exit::Refused,
                        format!("could not stage beside {}: {e}", target.display()),
                    ))
                }
            }
        }
        Err(Failure::new(
            Exit::Refused,
            "could not allocate a staging file",
        ))
    }
    pub(super) fn publish(staged: &Path, target: &Path, force: bool) -> Result<(), Failure> {
        let result = if force {
            save::commit_in_place(staged, target)
        } else {
            std::fs::hard_link(staged, target).map_err(|e| e.to_string())
        };
        result.map_err(|e| {
            Failure::new(
                Exit::Refused,
                format!("could not publish {}: {e}", target.display()),
            )
        })
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(raw: &str) -> Vec<String> {
        raw.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn page_parsers_refuse_ambiguous_or_incomplete_requests() {
        for (kind, raw) in [
            (Kind::Merge, "a -o b"),
            (Kind::Merge, "a b -o b"),
            (Kind::Extract, "a -o b"),
            (Kind::Extract, "a --pages 0 -o b"),
            (Kind::Split, "a --every 0 -o b"),
            (Kind::Split, "a --pages 1 -o b"),
            (Kind::Rotate, "a -o b"),
            (Kind::Rotate, "a --degrees 360 -o b"),
            (Kind::Crop, "a -o b"),
            (Kind::Crop, "a --rect 0,0,NaN,10 -o b"),
            (Kind::Crop, "a --rect 0,0,inf,10 -o b"),
            (Kind::Crop, "a --rect -1,0,1,2 -o b"),
        ] {
            assert!(parse(kind, &args(raw)).is_err(), "{raw}");
        }
        let rotate = parse(Kind::Rotate, &args("a --degrees 270 --pages 3,1,1 -o b")).unwrap();
        assert_eq!(rotate.pages, Some(vec![1, 3]));
        assert_eq!(rotate.turns, 3);
        let split = parse(Kind::Split, &args("a --every 2 -o b.pdf")).unwrap();
        assert_eq!(
            split.targets(3).unwrap(),
            vec![PathBuf::from("b-1.pdf"), PathBuf::from("b-2.pdf")]
        );
        assert!(split.targets(20_001).is_err());
        assert!(page_list("1-4294967295").is_err());
    }

    #[test]
    fn publication_refuses_a_late_collision_and_cleans_only_owned_staging() {
        let root =
            std::env::temp_dir().join(format!("tpdf-page-publication-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("out.pdf");
        let temp = Temporary::beside(&target).unwrap();
        let staged = temp.0.join("checked.pdf");
        std::fs::write(&staged, b"new").unwrap();
        std::fs::write(&target, b"late arrival").unwrap();
        assert!(Temporary::publish(&staged, &target, false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"late arrival");
        let scratch = temp.0.clone();
        drop(temp);
        assert!(!scratch.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"late arrival");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn partial_publication_names_only_the_files_that_landed() {
        let root = std::env::temp_dir().join(format!("tpdf-page-partial-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let command = parse(Kind::Split, &args("input.pdf -o part.pdf --json")).unwrap();
        let targets = vec![root.join("part-1.pdf"), root.join("part-2.pdf")];
        let staging = Temporary::beside(&targets[0]).unwrap();
        let staged = vec![staging.0.join("one.pdf"), staging.0.join("two.pdf")];
        for path in &staged {
            std::fs::write(path, b"checked").unwrap();
        }
        for path in &targets {
            command.check_target(path).unwrap();
        }
        std::fs::write(&targets[1], b"late arrival").unwrap();
        let sizes = vec![
            vec![PageSize {
                width_pt: 100.,
                height_pt: 200.
            }];
            2
        ];
        let mut report = report::Pages {
            schema: SCHEMA,
            command: "split".into(),
            inputs: vec!["input.pdf".into()],
            complete: false,
            outputs: Vec::new(),
            signatures_invalidated: 0,
            signatures_unknown: false,
            error: None,
        };
        let failure = command
            .publish(&staged, &sizes, &targets, &mut report)
            .unwrap_err();
        assert_eq!(failure.exit, Exit::Refused);
        assert!(!report.complete);
        assert_eq!(
            report.outputs,
            vec![report::PageOutput {
                path: targets[0].display().to_string(),
                pages: 1
            }]
        );
        assert_eq!(std::fs::read(&targets[0]).unwrap(), b"checked");
        assert_eq!(std::fs::read(&targets[1]).unwrap(), b"late arrival");
        drop(staging);
        std::fs::remove_dir_all(root).unwrap();
    }
}
