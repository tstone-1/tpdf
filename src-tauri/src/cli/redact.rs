//! `tpdf redact <in.pdf> -o <out.pdf> (--text STR | --pattern RE | --regions
//! FILE)...`: the application's redaction --- found by the viewer's search,
//! marked the way the viewer marks a match, removed, verified and filled by the
//! window's own code --- from a terminal.
//!
//! **Nothing here removes, verifies or words a verdict.** The regions go onto
//! an [`crate::edits::Edits`] through [`crate::edits::Edits::redact`], the call
//! the window's *Mark all matches for redaction* makes, and from there
//! `commands::redact::ask_redactions` and `commands::redact::redact_copy_asked`
//! do what they do for the window's *Redact and save as*: plans from a worker,
//! the rewrite, the byte scan of the written file for what was taken, the OCR
//! gate, the black fill, and `verified` only when every reason list is empty.
//! The sentence printed is `recovery.ts`'s `afterRedaction`, restated in
//! `words.rs` and held to it by `cliwording.test.ts`, so a script is never told
//! more than a reader of the window would be.
//!
//! **What this adds is the finding and one more reader.** The finding is
//! `search.rs` --- the viewer's matcher, with its folding --- over each page's
//! `PageText`, and `regions.rs`, the viewer's route from a hit to rectangles.
//! The reader is a second search of the *written* file for the same queries,
//! before the fill: a hit there is one more reason, never a lesser one. It is
//! the check a reader would make by hand in the window, and it catches a match
//! that could not become a region --- characters with no box --- which the
//! window's own verification would not look for, because it looks for what was
//! taken rather than what was asked for.
//!
//! **Containment is the application's.** The document is opened by a
//! [`crate::render::RenderService`] on the worker backend, whose workers are
//! this executable re-executed and sandboxed, as `Env::worker`'s are; the OCR
//! gate's engine is its own worker (`ocr_worker`), which `cli::main` answers
//! too. This process parses no document.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use super::args::{lexically_same, unknown, value};
use super::fill::{signed_state, SignedState};
use super::regions::{self, MAX_MATCHES_TO_MARK};
use super::report::{self, RedactedPage, SearchKind, SCHEMA};
use super::text::{page_list, password, selected, variable};
use super::{json, opened, say, words, Env, Exit, Failure, Registered, Subcommand};
use crate::commands::redact::{ask_redactions, redact_copy_asked, Stopped};
use crate::render::{Backend, RenderService};
use crate::search::{Carry, Match, Options, Prepared};
use crate::text::PageText;

/// `redact`, registered.
pub const COMMAND: Registered = Registered {
    name: "redact",
    usage: "redact <in.pdf> -o <out.pdf> (--text STR | --pattern REGEX | --regions FILE)...\n        [--case-sensitive] [--pages 1-3,7] [--dry-run] [--invalidate-signatures]\n        [--password-env VAR] [--force] [--json]",
    summary: "Removes every match of each --text and --pattern (found as the\n            viewer's search finds it) and every rectangle a --regions file\n            names, writes the result to -o, and reads it back. Exit 0 means\n            the copy was proved clean; 1 means it was written and could not be,\n            with every reason. --dry-run writes nothing and says what would go.\n            A signed document is refused unless --invalidate-signatures.",
    parse: boxed,
};

/// The most a regions file may hold, in bytes: not a bound on the regions,
/// which is the matches' bound, but on reading something that is not one.
pub const MAX_REGIONS_BYTES: u64 = 16 * 1024 * 1024;

/// How long any one answer from the render service may take. Its own call
/// deadline kills a worker that hangs well inside this; this is the backstop
/// for a service that has stopped answering altogether.
const ANSWER_BOUND: Duration = Duration::from_secs(120);

/// `tpdf redact`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Redact {
    /// The document. It is never written.
    pub input: PathBuf,
    /// Where the redacted copy goes; optional only with `--dry-run`.
    pub output: Option<PathBuf>,
    /// `--text`, in the order given.
    pub texts: Vec<String>,
    /// `--pattern`, in the order given.
    pub patterns: Vec<String>,
    /// `--regions` files, in the order given.
    pub regions: Vec<PathBuf>,
    /// `--case-sensitive`, for every `--text` and `--pattern`.
    pub case_sensitive: bool,
    /// `--pages`, counted from 1; `None` for all. Limits the search, and the
    /// search of the written file; a `--regions` rectangle names its own page.
    pub pages: Option<Vec<u32>>,
    /// `--dry-run`.
    pub dry_run: bool,
    /// `--invalidate-signatures`.
    pub invalidate_signatures: bool,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--force`: replace an existing output file.
    pub force: bool,
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// How a query is matched: the viewer's plain search, or its regular
/// expression, with case folded unless `--case-sensitive`.
fn options(kind: SearchKind, case_sensitive: bool) -> Options {
    Options {
        match_case: case_sensitive,
        whole_word: false,
        regex: kind == SearchKind::Pattern,
    }
}

/// One query compiled, or the sentence for exit code 2.
fn prepared(kind: SearchKind, query: &str, case_sensitive: bool) -> Result<Prepared, String> {
    let flag = match kind {
        SearchKind::Text => "--text",
        SearchKind::Pattern => "--pattern",
    };
    let compiled = Prepared::new(query, options(kind, case_sensitive))
        .map_err(|problem| format!("`{flag} {query}`: {problem}"))?;
    if compiled.matches_nothing() {
        return Err(format!(
            "`{flag}` needs something to find, and `{query}` can match nothing --- a search \
             that cannot match reads exactly like a document with nothing to remove"
        ));
    }
    Ok(compiled)
}

/// Reads the arguments after `redact`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Redact, String> {
    let mut command = Redact {
        input: PathBuf::new(),
        output: None,
        texts: Vec::new(),
        patterns: Vec::new(),
        regions: Vec::new(),
        case_sensitive: false,
        pages: None,
        dry_run: false,
        invalidate_signatures: false,
        password_env: None,
        force: false,
        json: false,
    };
    let mut input: Option<PathBuf> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" | "--output" => command.output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--text" => command.texts.push(value(arg, &mut rest)?.clone()),
            "--pattern" => command.patterns.push(value(arg, &mut rest)?.clone()),
            "--regions" => command.regions.push(PathBuf::from(value(arg, &mut rest)?)),
            "--case-sensitive" => command.case_sensitive = true,
            "--pages" => command.pages = Some(page_list(value(arg, &mut rest)?)?),
            "--dry-run" => command.dry_run = true,
            "--invalidate-signatures" => command.invalidate_signatures = true,
            "--password-env" => {
                command.password_env = Some(variable(value(arg, &mut rest)?)?);
            }
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown("redact", flag)),
            path => {
                if input.is_some() {
                    return Err(format!(
                        "`redact` takes one document, and `{path}` is a second --- redact them \
                         one at a time"
                    ));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }
    command.input = input.ok_or("`redact` needs the document to redact")?;
    if command.texts.is_empty() && command.patterns.is_empty() && command.regions.is_empty() {
        return Err(
            "`redact` needs something to remove: `--text`, `--pattern` or `--regions`, as \
             often as needed"
                .into(),
        );
    }
    for text in &command.texts {
        prepared(SearchKind::Text, text, command.case_sensitive)?;
    }
    for pattern in &command.patterns {
        prepared(SearchKind::Pattern, pattern, command.case_sensitive)?;
    }
    match &command.output {
        None if !command.dry_run => {
            return Err(
                "`redact` needs `-o <out.pdf>`: the redacted document is written as a new file, \
                 and the original is never changed"
                    .into(),
            )
        }
        None if command.force => {
            return Err("`--force` replaces an existing `-o` file, and there is no `-o`".into())
        }
        None => {}
        Some(output) => {
            if lexically_same(&command.input, output) {
                return Err(
                    "the output names the input --- the redacted document is written as a new \
                     file, so choose another name for it"
                        .into(),
                );
            }
            if command.regions.iter().any(|r| lexically_same(r, output)) {
                return Err("`-o` names a regions file".into());
            }
        }
    }
    Ok(command)
}

impl Subcommand for Redact {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        run_redact(env, self, out, err)
    }
}

/// One rectangle of a regions file, as written.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    /// The page, counted from 1.
    pub page: u32,
    /// `[x, y, w, h]`, points from the top-left corner of the page as it is
    /// displayed --- `sign --rect`'s convention.
    pub rect: [f32; 4],
}

/// A regions file's rectangles, as `(page counted from 1, [left, top, right,
/// bottom])`.
///
/// # Errors
///
/// Not a JSON array of `{"page", "rect"}` objects, a page not counted from 1,
/// or a rectangle `sign --rect` would refuse.
pub fn region_list(text: &str, shown: &str) -> Result<Vec<(u32, [f32; 4])>, String> {
    let specs: Vec<RegionSpec> = serde_json::from_str(text).map_err(|e| {
        format!(
            "{shown} is not a JSON array of regions, each `{{\"page\": 1, \"rect\": [x, y, w, \
             h]}}` ({e})"
        )
    })?;
    let mut out = Vec::with_capacity(specs.len());
    for (at, spec) in specs.iter().enumerate() {
        if spec.page == 0 {
            return Err(format!(
                "{shown}, region {}: page 0 --- pages count from 1",
                at + 1
            ));
        }
        let [x, y, w, h] = spec.rect;
        let area = super::sign::rectangle_of(x, y, w, h).ok_or_else(|| {
            format!(
                "{shown}, region {}: {:?} is not `[x, y, w, h]` in points from the page's \
                 top-left corner with a width and height above zero",
                at + 1,
                spec.rect
            )
        })?;
        out.push((spec.page, area));
    }
    Ok(out)
}

/// Every `--regions` file, read and checked, in the order given.
fn read_regions(files: &[PathBuf]) -> Result<Vec<(u32, [f32; 4])>, Failure> {
    let mut all = Vec::new();
    for path in files {
        let shown = path.display().to_string();
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_REGIONS_BYTES + 1).read_to_string(&mut text))
            .map_err(|e| {
                Failure::new(
                    Exit::Refused,
                    format!("could not read the regions from {shown}: {e}"),
                )
            })?;
        if text.len() as u64 > MAX_REGIONS_BYTES {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{shown} passes {} MiB, which is not a regions file",
                    MAX_REGIONS_BYTES / (1024 * 1024)
                ),
            ));
        }
        all.extend(region_list(&text, &shown).map_err(|why| Failure::new(Exit::Refused, why))?);
    }
    Ok(all)
}

/// Drives one of the render service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(E::from(format!(
            "the render service did not answer within {} s",
            ANSWER_BOUND.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(E::from("the render service stopped".to_string()))
        }
    }
}

/// The queries, compiled once for the walk, in report order: every `--text`,
/// then every `--pattern`.
fn queries(command: &Redact) -> Result<Vec<(SearchKind, String, Prepared)>, Failure> {
    let texts = command.texts.iter().map(|q| (SearchKind::Text, q));
    let patterns = command.patterns.iter().map(|q| (SearchKind::Pattern, q));
    texts
        .chain(patterns)
        .map(|(kind, query)| {
            prepared(kind, query, command.case_sensitive)
                .map(|compiled| (kind, query.clone(), compiled))
                .map_err(|why| Failure::new(Exit::Usage, why))
        })
        .collect()
}

/// What the walk found: the viewer's search over every page asked for.
#[derive(Debug, Default)]
pub struct Found {
    /// Matches per query, in query order.
    pub counts: Vec<usize>,
    /// Every match, in walk order --- a page's hits over the break before it
    /// first, as `search.rs` returns them.
    pub matches: Vec<Match>,
    /// Which query found each of [`Found::matches`], by its index.
    pub by: Vec<usize>,
    /// The page text of every page a match touches, counted from 0.
    pub texts: std::collections::BTreeMap<u32, PageText>,
}

/// Searches `pages` (counted from 1, ascending) for every query.
///
/// **One text extraction per page, and the search runs on its characters.**
/// `search.rs` reads `codes` and nothing else, and a worker's search reads the
/// same extraction (`document::page_codes`), so this is the viewer's answer
/// without a second extraction to disagree with the boxes. A page's tail
/// carries to the next page only when that page is the next one in the file,
/// which is `search.ts`'s rule --- `--pages 1,3` does not join page 1 to 3.
///
/// # Errors
///
/// A page that could not be read. **Everything stops**, as the window's
/// `redactMatches` stops: a partial list becomes a partial removal that is then
/// reported clean.
pub fn search_pages(
    queries: &[(SearchKind, String, Prepared)],
    pages: &[u32],
    mut text_of: impl FnMut(u32) -> Result<PageText, String>,
) -> Result<Found, String> {
    let mut found = Found {
        counts: vec![0; queries.len()],
        ..Found::default()
    };
    let mut carries: Vec<Option<Carry>> = vec![None; queries.len()];
    let mut previous: Option<(u32, PageText)> = None;
    for &page in pages {
        let at = page - 1;
        let text = text_of(at).map_err(|why| format!("page {page} could not be read: {why}"))?;
        let adjacent = previous.as_ref().is_some_and(|(was, _)| *was + 1 == at);
        let mut touched = false;
        let mut across = false;
        for (index, (_, _, compiled)) in queries.iter().enumerate() {
            let carry = if adjacent {
                carries[index].as_ref()
            } else {
                None
            };
            let answer = compiled.search_page(&text.codes, at, carry);
            for hit in &answer.matches {
                across |= hit.end_page.is_some();
            }
            touched |= !answer.matches.is_empty();
            found.counts[index] += answer.matches.len();
            found
                .by
                .extend(std::iter::repeat_n(index, answer.matches.len()));
            found.matches.extend(answer.matches);
            carries[index] = answer.tail;
        }
        if across {
            if let Some((was, text)) = previous.take() {
                found.texts.entry(was).or_insert(text);
            }
        }
        if touched {
            found.texts.insert(at, text.clone());
        }
        previous = Some((at, text));
    }
    Ok(found)
}

/// The regions each page carries for `found`, as the viewer marks them.
#[must_use]
pub fn match_regions(found: &Found) -> std::collections::BTreeMap<u32, Vec<[f32; 4]>> {
    let halves = regions::halves(&found.matches);
    let mut out = std::collections::BTreeMap::new();
    let mut pages: Vec<u32> = halves.iter().map(|half| half.page).collect();
    pages.sort_unstable();
    pages.dedup();
    for page in pages {
        let Some(text) = found.texts.get(&page) else {
            continue;
        };
        let areas = regions::regions_on(text, page, &halves);
        if !areas.is_empty() {
            out.insert(page, areas);
        }
    }
    out
}

/// One sentence for each match that becomes no region.
///
/// A match whose characters have no box on the page is text that will not be
/// removed, and the window's verification would not notice: it looks for what
/// the removal *took*, and nothing was taken. Such a match withholds the
/// verdict, and with nothing else marked it refuses the run.
#[must_use]
pub fn unmarked(found: &Found) -> Vec<String> {
    found
        .matches
        .iter()
        .filter(|hit| {
            regions::halves(std::slice::from_ref(hit))
                .iter()
                .all(|half| {
                    found.texts.get(&half.page).is_none_or(|text| {
                        regions::regions_on(text, half.page, &[*half]).is_empty()
                    })
                })
        })
        .map(|hit| {
            format!(
                "page {}: {:?} matched, but its characters have no position on the page, so it \
                 could not be marked",
                hit.page + 1,
                hit.hit
            )
        })
        .collect()
}

/// A refusal from opening the document, as this command's failure.
fn open_refused(shown: &str, why: &crate::progressive::Refusal, password_given: bool) -> Failure {
    match (why.locked, password_given) {
        (true, true) => Failure::new(
            Exit::Refused,
            format!("{shown} is encrypted, and the password given did not open it"),
        ),
        (true, false) => Failure::new(
            Exit::Refused,
            format!("{shown} is encrypted with a password --- give it with --password-env"),
        ),
        (false, _) => Failure::new(Exit::Refused, format!("{shown}: {}", why.reason)),
    }
}

/// Why a signed document is refused, or `None` to go ahead.
#[must_use]
pub fn signed_refusal(shown: &str, state: Option<SignedState>, invalidate: bool) -> Option<String> {
    if invalidate {
        return None;
    }
    match state? {
        SignedState::Signed(n) => Some(format!(
            "{shown} carries {n} signature{s}, and redacting rewrites the document, which \
             invalidates every one --- give --invalidate-signatures to redact it anyway",
            s = if n == 1 { "" } else { "s" }
        )),
        SignedState::Unknown => Some(format!(
            "{shown}: whether it is signed could not be read completely, and redacting rewrites \
             the document, which would invalidate any signature --- give \
             --invalidate-signatures to redact it anyway"
        )),
    }
}

/// Searches the written file again, for the reasons a hit there is.
///
/// Every failure is a reason: a file that could not be reopened or a page that
/// could not be read is a search that did not happen, and a search that did
/// not happen reads exactly like one that found nothing.
fn search_back(
    service: &RenderService,
    path: &str,
    password: Option<&str>,
    pages: &[u32],
    queries: &[(SearchKind, String, Prepared)],
) -> Vec<String> {
    let info = match wait(|reply| {
        service.open(
            PathBuf::from(path),
            true,
            password.map(str::to_string),
            reply,
        );
    }) {
        Ok(info) => info,
        Err(refusal) => {
            return vec![format!(
                "the written file could not be reopened to search it again, so the matches \
                 could not be shown gone: {}",
                refusal.reason
            )]
        }
    };
    let reasons = match search_pages(queries, pages, |at| {
        wait(|reply| service.text(info.id, at, None, reply))
    }) {
        Err(why) => vec![format!(
            "the written file could not be searched again, so the matches could not be shown \
             gone: {why}"
        )],
        Ok(found) => still_found(&found, queries),
    };
    let _: Result<(), String> = wait(|reply| service.close(info.id, reply));
    reasons
}

/// One sentence per hit the search of the written file found.
#[must_use]
pub fn still_found(found: &Found, queries: &[(SearchKind, String, Prepared)]) -> Vec<String> {
    found
        .matches
        .iter()
        .zip(&found.by)
        .map(|(hit, by)| {
            let query = queries.get(*by).map_or("", |(_, query, _)| query.as_str());
            format!(
                "page {}: searching the written file for `{query}` still finds {:?}",
                hit.page + 1,
                hit.hit
            )
        })
        .collect()
}

fn discard(output: &std::path::Path, err: &mut dyn Write, program: &str) {
    if let Err(e) = std::fs::remove_file(output) {
        say(
            err,
            &format!(
                "{program}: {} could not be removed ({e}); it is not a redacted copy to rely on",
                output.display()
            ),
        );
    }
}

/// The report's pages: every page with a hit or a region, in page order.
fn report_pages(
    found: &Found,
    marked: &std::collections::BTreeMap<u32, usize>,
    summaries: &[crate::redact::PageSummary],
) -> Vec<RedactedPage> {
    let mut pages: std::collections::BTreeMap<u32, RedactedPage> = Default::default();
    let blank = |page: u32| RedactedPage {
        page: page + 1,
        hits: Vec::new(),
        regions: 0,
        text_removals: 0,
        form_text_removals: 0,
        image_removals: 0,
        taking: Vec::new(),
        left: Vec::new(),
    };
    for hit in &found.matches {
        pages
            .entry(hit.page)
            .or_insert_with(|| blank(hit.page))
            .hits
            .push(hit.hit.clone());
    }
    for (&page, &count) in marked {
        pages.entry(page).or_insert_with(|| blank(page)).regions = count;
    }
    for summary in summaries {
        let entry = pages
            .entry(summary.page)
            .or_insert_with(|| blank(summary.page));
        entry.regions = summary.regions;
        entry.text_removals = summary.text;
        entry.form_text_removals = summary.form_text;
        entry.image_removals = summary.images;
        entry.taking.clone_from(&summary.taking);
        entry.left.clone_from(&summary.left);
    }
    pages.into_values().collect()
}

/// Everything the run has to say, before the outcome is known.
struct Draft {
    input: String,
    output: Option<String>,
    dry_run: bool,
    signatures: usize,
    searches: Vec<report::Search>,
    pages: Vec<RedactedPage>,
    regions: usize,
    removals: usize,
}

impl Draft {
    fn report(
        self,
        written: bool,
        verified: Option<bool>,
        reasons: Vec<String>,
        summary: Option<String>,
    ) -> report::Redacted {
        report::Redacted {
            schema: SCHEMA,
            command: "redact".into(),
            input: self.input,
            output: self.output,
            dry_run: self.dry_run,
            written,
            verified,
            reasons,
            summary,
            regions: self.regions,
            removals: self.removals,
            signatures_invalidated: if written || self.dry_run {
                self.signatures
            } else {
                0
            },
            searches: self.searches,
            pages: self.pages,
        }
    }
}

/// `redact` without `--json`: one line per page, then the outcome.
#[must_use]
pub fn plain(report: &report::Redacted) -> String {
    let mut lines = Vec::new();
    for search in &report.searches {
        let flag = match search.kind {
            SearchKind::Text => "--text",
            SearchKind::Pattern => "--pattern",
        };
        lines.push(format!(
            "{flag} {:?}: {} match{}",
            search.query,
            search.matches,
            if search.matches == 1 { "" } else { "es" }
        ));
    }
    for page in &report.pages {
        let mut line = format!(
            "page {}: {} region{}",
            page.page,
            page.regions,
            if page.regions == 1 { "" } else { "s" }
        );
        if page.regions > 0 {
            line.push_str(&format!(
                ", removing {} text operation{}, {} in forms, {} image{}",
                page.text_removals,
                if page.text_removals == 1 { "" } else { "s" },
                page.form_text_removals,
                page.image_removals,
                if page.image_removals == 1 { "" } else { "s" },
            ));
        }
        lines.push(line);
        for taking in &page.taking {
            lines.push(format!("  takes {taking:?}"));
        }
        for left in &page.left {
            lines.push(format!("  leaves {left}"));
        }
    }
    if report.signatures_invalidated > 0 {
        lines.push(format!(
            "The input's {} signature{} no longer cover{} the written copy.",
            report.signatures_invalidated,
            if report.signatures_invalidated == 1 {
                ""
            } else {
                "s"
            },
            if report.signatures_invalidated == 1 {
                "s"
            } else {
                ""
            },
        ));
    }
    match (&report.summary, report.dry_run, report.regions) {
        (Some(summary), _, _) => lines.push(summary.clone()),
        (None, _, 0) => {
            lines.push("Nothing matched, so nothing was marked and nothing was written.".into())
        }
        (None, true, _) => lines.push(format!(
            "Dry run: {} region{} would take {} removal{}. Nothing was written.",
            report.regions,
            if report.regions == 1 { "" } else { "s" },
            report.removals,
            if report.removals == 1 { "" } else { "s" },
        )),
        (None, false, _) => {}
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn emit(command: &Redact, out: &mut dyn Write, report: &report::Redacted) {
    if command.json {
        json(out, report);
    } else {
        let text = plain(report);
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
}

#[allow(clippy::too_many_lines)]
fn run_redact(
    env: &Env<'_>,
    command: &Redact,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    // What can be refused without a worker, first, in `fill`'s order.
    if let Some(output) = &command.output {
        if crate::save::same_file(&command.input, output) {
            return Err(Failure::new(
                Exit::Usage,
                "the output is the input under another name --- the redacted document is \
                 written as a new file, so choose another name for it",
            ));
        }
        if command
            .regions
            .iter()
            .any(|r| crate::save::same_file(r, output))
        {
            return Err(Failure::new(
                Exit::Usage,
                "-o is a regions file under another name",
            ));
        }
        if !command.force && output.exists() {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} already exists --- choose another name, or give --force to replace it",
                    output.display()
                ),
            ));
        }
    }
    let password = password(command.password_env.as_deref())?;
    let queries = queries(command)?;
    let rects = read_regions(&command.regions)?;

    let shown = command.input.display().to_string();
    let (file, _) = opened(&command.input).map_err(|why| Failure::new(Exit::Refused, why))?;
    let hashing = file
        .try_clone()
        .map_err(|e| Failure::new(Exit::Refused, format!("could not open {shown}: {e}")))?;

    // The worker backend always, whatever `TPDF_BACKEND` says: the one thing
    // this command may not do is parse the document in this process.
    let service = RenderService::start_with(env.library_dir.clone(), Backend::Worker);
    let info = wait(|reply| {
        service.open_handed(
            command.input.clone(),
            Some(file),
            true,
            password.clone(),
            reply,
        );
    })
    .map_err(|why| open_refused(&shown, &why, password.is_some()))?;
    let doc = info.id;
    let count = u32::try_from(info.page_count).map_err(|_| {
        Failure::new(
            Exit::Refused,
            format!("{shown} has more pages than tpdf can redact"),
        )
    })?;

    // Signatures, by the window's rule: a document whose status could not be
    // read is treated as signed, never as unsigned.
    let state = match wait(|reply| service.properties(doc, reply)) {
        Ok(properties) => signed_state(&properties),
        Err(_) => Some(SignedState::Unknown),
    };
    if let Some(why) = signed_refusal(&shown, state, command.invalidate_signatures) {
        return Err(Failure::new(Exit::Refused, why));
    }
    let signatures = match state {
        Some(SignedState::Signed(n)) => n,
        _ => 0,
    };
    // XFA before anything is marked, so a dry run refuses what the write would.
    if let Err(why) = wait(|reply| service.form(doc, reply)) {
        if why == crate::forms::XFA_REFUSAL {
            return Err(Failure::new(
                Exit::Refused,
                format!("{shown}: {}", crate::redact::XFA_REDACTION),
            ));
        }
    }

    let pages = selected(command.pages.as_deref(), count)
        .map_err(|why| Failure::new(Exit::Refused, format!("{shown}: {why}")))?;
    if let Some((page, _)) = rects.iter().find(|(page, _)| *page > count) {
        return Err(Failure::new(
            Exit::Refused,
            format!(
                "{shown}: a region names page {page}, and the document has {count} page{}",
                if count == 1 { "" } else { "s" }
            ),
        ));
    }

    let found = search_pages(&queries, &pages, |at| {
        wait(|reply| service.text(doc, at, None, reply))
    })
    .map_err(|why| {
        Failure::new(
            Exit::Refused,
            format!("{shown}: {why}, so nothing was marked and nothing was written"),
        )
    })?;
    if found.matches.len() > MAX_MATCHES_TO_MARK {
        return Err(Failure::new(
            Exit::Refused,
            format!(
                "{} matches is more than the {MAX_MATCHES_TO_MARK} that can be marked at once, \
                 and nothing was written --- narrow the search, or redact fewer pages at a time \
                 with --pages",
                found.matches.len()
            ),
        ));
    }

    // The window's model, marked the way the window marks it.
    let mut marked: std::collections::BTreeMap<u32, Vec<[f32; 4]>> = match_regions(&found);
    for (page, area) in &rects {
        marked.entry(page - 1).or_default().push(*area);
    }
    let edits = crate::edits::Edits::default();
    edits.open(
        doc,
        count,
        Some(crate::fingerprint::Opened {
            file: hashing,
            what: command.input.clone(),
        }),
    );
    for (page, areas) in &marked {
        for area in areas {
            edits
                .redact(doc, u64::from(*page) + 1, *area)
                .map_err(|why| {
                    Failure::new(
                        Exit::Internal,
                        format!(
                            "page {}: a region was refused by the model: {why}",
                            page + 1
                        ),
                    )
                })?;
        }
    }
    let counts: std::collections::BTreeMap<u32, usize> = marked
        .iter()
        .map(|(page, areas)| (*page, areas.len()))
        .collect();
    let regions_total: usize = counts.values().sum();
    let searches: Vec<report::Search> = queries
        .iter()
        .zip(&found.counts)
        .map(|((kind, query, _), matches)| report::Search {
            kind: *kind,
            query: query.clone(),
            matches: *matches,
        })
        .collect();
    let mut draft = Draft {
        input: shown.clone(),
        output: command.output.as_ref().map(|o| o.display().to_string()),
        dry_run: command.dry_run,
        signatures,
        searches,
        pages: report_pages(&found, &counts, &[]),
        regions: regions_total,
        removals: 0,
    };

    let unmarked = unmarked(&found);

    if regions_total == 0 {
        // Matched and could not be marked is not "nothing matched": the words
        // are on the page and nothing will take them.
        if !unmarked.is_empty() {
            for why in &unmarked {
                say(err, &format!("{}: {why}", env.program));
            }
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} match{} could not be marked, and nothing was written",
                    unmarked.len(),
                    if unmarked.len() == 1 { "" } else { "es" }
                ),
            ));
        }
        let report = draft.report(false, None, Vec::new(), None);
        emit(command, out, &report);
        return Ok(Exit::Ok);
    }

    let asked = tauri::async_runtime::block_on(ask_redactions(&edits, &service, doc))
        .map_err(|why| Failure::new(Exit::Refused, format!("{shown}: {why}")))?;
    draft.pages = report_pages(&found, &counts, &asked.pages);
    draft.removals = asked.shows;

    if command.dry_run {
        let mut reasons = unmarked;
        reasons.extend(asked.concerns.iter().cloned());
        let report = draft.report(false, None, reasons, None);
        emit(command, out, &report);
        return Ok(Exit::Ok);
    }

    let output = command
        .output
        .clone()
        .ok_or_else(|| Failure::new(Exit::Internal, "no output to write to"))?;
    let read_back = |path: &str, key: Option<&str>| -> Vec<String> {
        if queries.is_empty() {
            return Vec::new();
        }
        search_back(&service, path, key, &pages, &queries)
    };
    let written = tauri::async_runtime::block_on(redact_copy_asked(
        &service,
        env.library_dir.clone(),
        doc,
        asked,
        command.input.display().to_string(),
        output.display().to_string(),
        Some(&read_back),
    ));
    let applied = match written {
        Ok(applied) => applied,
        Err(Stopped::Refused(why)) => {
            return Err(Failure::new(Exit::Refused, format!("{shown}: {why}")))
        }
        Err(Stopped::Failed { message, written }) => {
            if written {
                discard(&output, err, &env.program);
            }
            return Err(Failure::new(
                Exit::Internal,
                format!("{message}; nothing was kept"),
            ));
        }
    };
    if applied.changed {
        discard(&output, err, &env.program);
        return Err(Failure::new(
            Exit::Refused,
            format!("{shown} changed while it was being redacted, so the copy was not kept"),
        ));
    }
    let (verified, why, exit) = outcome(applied.why, unmarked);
    let summary = words::after_redaction(applied.regions, applied.shows, verified, &why, false);
    let report = draft.report(true, Some(verified), why, Some(summary));
    emit(command, out, &report);
    Ok(exit)
}

/// The verdict on a written copy, its reasons and the exit code.
///
/// The window's reasons first, then one for each match that could not become a
/// region. `verified` is true only when both lists are empty --- the rule
/// `redact::Applied` states --- and the exit code is 0 exactly then.
#[must_use]
pub fn outcome(applied: Vec<String>, unmarked: Vec<String>) -> (bool, Vec<String>, Exit) {
    let mut why = applied;
    why.extend(unmarked);
    let verified = why.is_empty();
    (
        verified,
        why,
        if verified { Exit::Ok } else { Exit::Strict },
    )
}
