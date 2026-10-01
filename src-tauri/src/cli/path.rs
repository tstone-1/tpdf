//! `tpdf path [--add | --remove]`: whether the folder this tool is in is on
//! the user's `PATH`, and putting it there or taking it out. Windows only:
//! the per-user installer runs `--add` after installing and `--remove` before
//! uninstalling, and anybody can run either by hand.

use std::io::Write;

use super::args::unknown;
use super::{say, Env, Exit, Failure, Registered, Subcommand};

/// `path`, registered.
pub const COMMAND: Registered = Registered {
    name: "path",
    usage: "path [--add | --remove]",
    summary: "Windows: says whether the folder this tool is in is on your PATH;\n            --add puts it there and --remove takes it out. Only your own\n            PATH is changed, which needs no administrator. A terminal that\n            is already open keeps its old PATH.",
    parse: boxed,
};

/// `tpdf path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// Say whether it is there.
    Show,
    /// `--add`.
    Add,
    /// `--remove`.
    Remove,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `path`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Path, String> {
    let mut what = Path::Show;
    for arg in args {
        let asked = match arg.as_str() {
            "--add" => Path::Add,
            "--remove" => Path::Remove,
            other => return Err(unknown("path", other)),
        };
        if what != Path::Show && what != asked {
            return Err("`--add` and `--remove` are opposites --- give one of them".into());
        }
        what = asked;
    }
    Ok(what)
}

impl Subcommand for Path {
    fn run(
        &self,
        _env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let said = run(*self).map_err(|why| Failure::new(Exit::Refused, why))?;
        say(out, &said);
        Ok(Exit::Ok)
    }
}

#[cfg(windows)]
fn run(what: Path) -> Result<String, String> {
    use crate::userpath::{self, Outcome};
    let dir = crate::clitool::folder()?;
    Ok(match what {
        Path::Show => {
            // Two answers, because they differ right after an install: the
            // stored value changed and a terminal opened before it has not.
            let here = std::env::var("PATH").unwrap_or_default();
            let here = userpath::with(&here, &dir).is_none();
            match (userpath::stored(&dir)?, here) {
                (true, true) => format!("{dir} is on your PATH."),
                (true, false) => format!(
                    "{dir} is on your PATH, and this terminal was opened before it was added \
                     --- open a new one."
                ),
                (false, true) => {
                    format!("{dir} is on the PATH of this terminal, and not on your own PATH.")
                }
                (false, false) => format!("{dir} is not on your PATH --- `path --add` adds it."),
            }
        }
        Path::Add => match userpath::apply(&dir, true)? {
            Outcome::Changed => {
                format!("Added {dir} to your PATH. Open a new terminal to use it.")
            }
            Outcome::Unchanged => format!("{dir} is already on your PATH."),
        },
        Path::Remove => match userpath::apply(&dir, false)? {
            Outcome::Changed => format!("Removed {dir} from your PATH."),
            Outcome::Unchanged => format!("{dir} is not on your PATH."),
        },
    })
}

#[cfg(not(windows))]
fn run(_: Path) -> Result<String, String> {
    Err(
        "`path` changes the Windows PATH --- on macOS, choose Install command-line tool… in \
         tpdf, which links the tool into a folder that is already on yours"
            .into(),
    )
}
