//! The command line's first word, and the helpers every command's parser shares.
//!
//! **Pure.** Nothing here or in a command's `parse` opens a file, asks the key
//! store or starts a worker, so every refusal is a unit test and none of them
//! can depend on what is on the machine running it. What *can* only be
//! answered by looking --- does the input exist, is the output the input under
//! another name, which certificate does `--identity` name --- is the command's
//! `run`, after its `parse` has said the line is well formed. A refusal here is
//! exit code 2, [`super::Exit::Usage`].

use std::path::{Component, Path};

use super::{Subcommand, COMMANDS};

/// What the command line asks for.
#[derive(Debug)]
pub enum Line {
    /// `help`, `--help`, `-h`, or no command at all.
    Help,
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
        "help" | "--help" | "-h" => return Ok(Line::Help),
        "--version" | "-V" => return Ok(Line::Version),
        _ => {}
    }
    match COMMANDS.iter().find(|c| c.name == command.as_str()) {
        Some(registered) => (registered.parse)(rest).map(Line::Run),
        None => {
            let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
            Err(format!(
                "`{command}` is not a command --- the commands are {}",
                names.join(", ")
            ))
        }
    }
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
    rest.next()
        .ok_or_else(|| format!("`{flag}` needs a value after it"))
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
