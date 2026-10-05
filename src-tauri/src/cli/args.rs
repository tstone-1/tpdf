//! The command line's first word, and the helpers every command's parser shares.
//!
//! **Pure.** Nothing here or in a command's `parse` opens a file, asks the key
//! store or starts a worker, so every refusal is a unit test and none of them
//! can depend on what is on the machine running it. What *can* only be
//! answered by looking --- does the input exist, is the output the input under
//! another name, which certificate does `--identity` name --- is the command's
//! `run`, after its `parse` has said the line is well formed. A refusal here is
//! exit code 2, [`super::Exit::Usage`].

use std::path::{Component, Path, PathBuf};

use super::{Registered, Subcommand, COMMANDS};

/// What the command line asks for.
#[derive(Debug)]
pub enum Line {
    /// `help`, `--help`, `-h`, or no command at all.
    Help,
    /// `help <command>`, or a command followed by `--help` or `-h`.
    HelpFor(&'static Registered),
    /// `--version`.
    Version,
    /// A registered command, parsed.
    Run(Box<dyn Subcommand>),
}

/// Reads `args` --- the command line **without** the program's own name.
///
/// # Errors
///
/// The sentence to print, for exit code 2.
pub fn parse(args: &[String]) -> Result<Line, String> {
    let Some((command, rest)) = args.split_first() else {
        return Ok(Line::Help);
    };
    match command.as_str() {
        // `help redact`: the first word after it that is not an option names
        // the command. `help --json` has none and stays the whole list.
        "help" | "--help" | "-h" => {
            return match rest.iter().find(|arg| !arg.starts_with('-')) {
                Some(name) => registered(name).map(Line::HelpFor),
                None => Ok(Line::Help),
            };
        }
        "--version" | "-V" => return Ok(Line::Version),
        _ => {}
    }
    let command = registered(command)?;
    // Before the command's own parser, which would refuse `--help` as an option
    // it does not have --- the answer a reader got until 2026-10-02, for the
    // one thing typed by somebody who does not yet know the options. Only
    // before `--`, after which every word is a path, and only where an option
    // stands: `redact in.pdf -o out.pdf --text -h` looks for `-h` in the
    // document, and answering it with the help text and exit code 0 told a
    // script the redaction was done.
    if options(command, rest)
        .iter()
        .any(|option| *option == "--help" || *option == "-h")
    {
        return Ok(Line::HelpFor(command));
    }
    (command.parse)(rest).map(Line::Run)
}

/// Whether the line asks for `--json`, for the things [`super::run`] prints
/// before or instead of the command: a refusal of the line itself, and `help`.
///
/// Where an option stands, as [`parse`] reads `--help`: the `--json` in
/// `search in.pdf --text --json` is what to look for. A first word that is no
/// command has no parser to ask, and `help` takes no option with a value, so
/// there every word before `--` counts.
pub(crate) fn wants_json(args: &[String]) -> bool {
    let command = args
        .split_first()
        .and_then(|(name, rest)| Some((COMMANDS.iter().find(|c| c.name == name)?, rest)));
    match command {
        Some((command, rest)) => options(command, rest).contains(&"--json"),
        None => args
            .iter()
            .take_while(|arg| *arg != "--")
            .any(|arg| arg == "--json"),
    }
}

/// The words of `rest` that stand where an option does: before `--`,
/// beginning with `-`, and not the value of the option before them.
///
/// **The command's own parser says which options take a value**, so there is
/// no second table to fall behind the first. Every parser reads a value with
/// [`value`], and a parser handed one word that is such an option answers with
/// [`value`]'s refusal, which no other word produces. Nothing is opened by
/// asking: a parser is pure (this module's note).
fn options<'a>(command: &Registered, rest: &'a [String]) -> Vec<&'a str> {
    let takes_value = |option: &String| {
        (command.parse)(std::slice::from_ref(option)).is_err_and(|why| why == needs_value(option))
    };
    let mut found = Vec::new();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if !word.starts_with('-') {
            continue;
        }
        found.push(word.as_str());
        if takes_value(word) {
            words.next();
        }
    }
    found
}

/// The registered command called `name`, or the refusal that says what is.
fn registered(name: &str) -> Result<&'static Registered, String> {
    if let Some(found) = COMMANDS.iter().find(|c| c.name == name) {
        return Ok(found);
    }
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    let all = names.join(", ");
    Err(match nearest(name, &names) {
        Some(meant) => {
            format!("`{name}` is not a command --- did you mean `{meant}`? The commands are {all}")
        }
        None => format!("`{name}` is not a command --- the commands are {all}"),
    })
}

/// The one name a mistyped `word` most likely meant, or `None`.
///
/// Within two edits, and only when one name is strictly nearest: `encrypt` is
/// nobody's typo, and a word equally far from two commands is not a guess this
/// makes. A name `word` begins, or that begins `word`, counts as two edits at
/// most, so `ident` is answered --- and as two rather than one, so `text-run`
/// is `text-runs`, one letter away, before it is `text` with something after it.
fn nearest<'a>(word: &str, names: &[&'a str]) -> Option<&'a str> {
    // Two letters are within two edits of half the list.
    if word.chars().count() < 3 {
        return None;
    }
    let mut scored: Vec<(usize, &str)> = names
        .iter()
        .map(|name| {
            let prefix = name.starts_with(word) || word.starts_with(name);
            let distance = edits(word, name);
            (if prefix { distance.min(2) } else { distance }, *name)
        })
        .filter(|(distance, _)| *distance <= 2)
        .collect();
    scored.sort_unstable();
    match scored.as_slice() {
        [(_, only)] => Some(only),
        [(first, name), (second, _), ..] if first < second => Some(name),
        _ => None,
    }
}

/// Levenshtein distance between two words, by character.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != *y))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The refusal for an argument a command does not take.
pub(crate) fn unknown(command: &str, arg: &str) -> String {
    if arg.starts_with('-') {
        format!("`{command}` has no option `{arg}`")
    } else {
        format!("`{command}` takes no argument `{arg}`")
    }
}

/// The value after an option, or the refusal naming the option.
pub(crate) fn value<'a>(
    flag: &str,
    rest: &mut std::slice::Iter<'a, String>,
) -> Result<&'a String, String> {
    rest.next().ok_or_else(|| needs_value(flag))
}

/// [`value`]'s refusal, which [`options`] also reads a parser's answer by.
fn needs_value(flag: &str) -> String {
    format!("`{flag}` needs a value after it")
}

/// A word that names a document, as the path a command opens.
///
/// After `--` every word is a file's name, `-` among them, and the commands
/// that read standard input know it by exactly that path (`opened_or_stdin`).
/// So the file called `-` is handed over as `./-`, which names the same file
/// and is not the stream.
pub(crate) fn operand(after_separator: bool, word: &str) -> PathBuf {
    if after_separator && word == "-" {
        Path::new(".").join(word)
    } else {
        PathBuf::from(word)
    }
}

/// Whether two paths are the same path as written, `.` and `..` resolved.
///
/// Lexical only: `cli.rs` asks the filesystem afterwards (`save::same_file`),
/// which is what catches a link or a second name. This is the half a unit test
/// can hold, and the half a typo produces.
pub(crate) fn lexically_same(a: &Path, b: &Path) -> bool {
    fn normal(path: &Path) -> Vec<Component<'_>> {
        let mut out: Vec<Component<'_>> = Vec::new();
        for part in path.components() {
            match part {
                Component::CurDir => {}
                Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }
    normal(a) == normal(b)
}
