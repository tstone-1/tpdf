//! `tpdf explorer [--add [--replace] | --remove]`: whether Explorer shows PDF
//! thumbnails and the preview pane with tpdf, and turning that on or off for
//! the current user. Windows only. The window's *Show PDFs in Explorer with
//! tpdf* does the same (`explorer.rs`); the uninstaller runs `--remove`.

use std::io::Write;

use super::args::unknown;
use super::{say, Env, Exit, Failure, Registered, Subcommand};

/// `explorer`, registered.
pub const COMMAND: Registered = Registered {
    name: "explorer",
    usage: "explorer [--add [--replace] | --remove]",
    summary: "Windows: says whether Explorer shows PDF thumbnails and the preview\n            pane with tpdf; --add turns that on for you, and --remove turns it\n            off and puts back what it replaced. A handler another program has\n            is replaced only with --replace. No administrator is needed.",
    parse: boxed,
};

/// `tpdf explorer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Explorer {
    Show,
    Add { replace: bool },
    Remove,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `explorer`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Explorer, String> {
    let (mut add, mut remove, mut replace) = (false, false, false);
    for arg in args {
        match arg.as_str() {
            "--add" => add = true,
            "--remove" => remove = true,
            "--replace" => replace = true,
            other => return Err(unknown("explorer", other)),
        }
    }
    match (add, remove, replace) {
        (true, true, _) => Err("`--add` and `--remove` are opposites --- give one of them".into()),
        (false, _, true) => Err("`--replace` goes with `--add`".into()),
        (true, false, _) => Ok(Explorer::Add { replace }),
        (false, true, false) => Ok(Explorer::Remove),
        (false, false, false) => Ok(Explorer::Show),
    }
}

impl Subcommand for Explorer {
    fn run(
        &self,
        _env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let change = match *self {
            Explorer::Show => None,
            Explorer::Add { replace } => Some((true, replace)),
            Explorer::Remove => Some((false, false)),
        };
        let (said, _) =
            crate::explorer::run(change).map_err(|why| Failure::new(Exit::Refused, why))?;
        say(out, &said);
        Ok(Exit::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Result<Explorer, String> {
        parse(
            &text
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn the_options_are_read_and_contradictions_refused() {
        assert_eq!(line(""), Ok(Explorer::Show));
        assert_eq!(line("--add"), Ok(Explorer::Add { replace: false }));
        assert_eq!(line("--replace --add"), Ok(Explorer::Add { replace: true }));
        assert_eq!(line("--remove"), Ok(Explorer::Remove));
        for wrong in ["--add --remove", "--replace", "--remove --replace", "--yes"] {
            assert!(line(wrong).is_err(), "{wrong}");
        }
    }

    /// On macOS and Linux the command says what shows PDFs there, and writes
    /// nothing.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn explorer_is_refused_with_this_platforms_own_file_manager() {
        let why = crate::explorer::run(Some((true, false))).expect_err("no Explorer here");
        let here = if cfg!(target_os = "macos") {
            "macOS"
        } else {
            "Linux"
        };
        assert!(
            why.contains("Windows file manager") && why.contains(here),
            "{why}"
        );
    }
}
