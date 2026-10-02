//! `tpdf completions bash|zsh|fish|powershell`: a completion script for a shell.
//!
//! **Generated from the registry, so it cannot fall behind it.** The commands
//! and their summaries are [`COMMANDS`], and each command's options are read
//! out of its own usage line --- the text `help` prints --- so a command or an
//! option added tomorrow is completed tomorrow with no second list to edit.
//!
//! **Both names.** The tool is `tpdf` on macOS and `tpdf-cli` on Windows and,
//! since 2026-10-02, on macOS too; every script registers both.
//!
//! The script is printed and nothing is installed: where a completion file
//! goes differs by shell and by how the shell was set up, and the README says
//! the one line for each.

use std::io::Write;

use super::args::unknown;
use super::{Env, Exit, Failure, Registered, Subcommand, COMMANDS};

/// `completions`, registered.
pub const COMMAND: Registered = Registered {
    name: "completions",
    usage: "completions bash|zsh|fish|powershell",
    summary: "Prints a completion script for that shell: the commands, each\n            command's options, and file names.",
    parse: boxed,
};

/// The names the tool is run by.
const PROGRAMS: [&str; 2] = ["tpdf", "tpdf-cli"];

/// A shell a script can be printed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// GNU Bash.
    Bash,
    /// Z shell.
    Zsh,
    /// fish.
    Fish,
    /// PowerShell, Windows' and the portable one.
    PowerShell,
}

/// `tpdf completions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// The shell asked for.
    pub shell: Shell,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `completions`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Completions, String> {
    let mut shell = None;
    for arg in args {
        let named = match arg.as_str() {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "powershell" | "pwsh" => Shell::PowerShell,
            flag if flag.starts_with('-') => return Err(unknown("completions", flag)),
            other => {
                return Err(format!(
                    "`completions {other}`: the shells are bash, zsh, fish and powershell"
                ))
            }
        };
        if shell.replace(named).is_some() {
            return Err("`completions` takes one shell".into());
        }
    }
    shell
        .map(|shell| Completions { shell })
        .ok_or_else(|| "`completions` needs a shell: bash, zsh, fish or powershell".into())
}

impl Subcommand for Completions {
    fn run(
        &self,
        _env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let _ = out.write_all(script(self.shell).as_bytes());
        let _ = out.flush();
        Ok(Exit::Ok)
    }
}

/// The options a command takes, read from its usage line: every `--word`, and
/// `-o` where it has one. Sorted, each once.
#[must_use]
pub fn options(command: &Registered) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    let usage = command.usage;
    for (at, _) in usage.match_indices('-') {
        // The start of an option: a dash that follows a space, a bracket, a
        // parenthesis or a bar, never one inside a word such as `text-runs`.
        let before = usage[..at].chars().next_back();
        if !matches!(before, Some(' ' | '[' | '(' | '|')) {
            continue;
        }
        let word: String = usage[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if word.starts_with("--") && word.len() > 2 || word == "-o" {
            found.insert(word);
        }
    }
    found.insert("--help".to_string());
    found.into_iter().collect()
}

/// A summary's first sentence on one line, for a shell that shows one.
fn brief(command: &Registered) -> String {
    let joined = command
        .summary
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    let first = joined.split(". ").next().unwrap_or(&joined);
    first.trim_end_matches('.').to_string()
}

/// The completion script for `shell`.
#[must_use]
pub fn script(shell: Shell) -> String {
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).chain(["help"]).collect();
    match shell {
        Shell::Bash => bash(&names),
        Shell::Zsh => zsh(&names),
        Shell::Fish => fish(),
        Shell::PowerShell => powershell(&names),
    }
}

fn bash(names: &[&str]) -> String {
    let mut cases = String::new();
    for command in COMMANDS {
        cases.push_str(&format!(
            "        {}) options=\"{}\" ;;\n",
            command.name,
            options(command).join(" ")
        ));
    }
    format!(
        "# tpdf completion for bash. Load it with:  source <(tpdf completions bash)\n\
         _tpdf() {{\n\
         \x20   local current=\"${{COMP_WORDS[COMP_CWORD]}}\" options=\"\"\n\
         \x20   if [ \"$COMP_CWORD\" -eq 1 ]; then\n\
         \x20       COMPREPLY=( $(compgen -W \"{commands} --version\" -- \"$current\") )\n\
         \x20       return\n\
         \x20   fi\n\
         \x20   case \"${{COMP_WORDS[1]}}\" in\n\
         {cases}\
         \x20       help) options=\"{commands}\" ;;\n\
         \x20   esac\n\
         \x20   case \"$current\" in\n\
         \x20       -*) COMPREPLY=( $(compgen -W \"$options\" -- \"$current\") ) ;;\n\
         \x20       *)\n\
         \x20           if [ \"${{COMP_WORDS[1]}}\" = help ]; then\n\
         \x20               COMPREPLY=( $(compgen -W \"$options\" -- \"$current\") )\n\
         \x20           else\n\
         \x20               COMPREPLY=( $(compgen -f -- \"$current\") )\n\
         \x20           fi ;;\n\
         \x20   esac\n\
         }}\n\
         complete -o filenames -F _tpdf {programs}\n",
        commands = names.join(" "),
        programs = PROGRAMS.join(" "),
    )
}

fn zsh(names: &[&str]) -> String {
    let mut described = String::new();
    for command in COMMANDS {
        // A colon separates a name from its description in `_describe`.
        described.push_str(&format!(
            "        '{}:{}'\n",
            command.name,
            brief(command).replace('\'', "").replace(':', " -")
        ));
    }
    described.push_str("        'help:Lists the commands, or explains one'\n");
    let mut cases = String::new();
    for command in COMMANDS {
        cases.push_str(&format!(
            "        {}) options=({}) ;;\n",
            command.name,
            options(command).join(" ")
        ));
    }
    format!(
        "#compdef {programs}\n\
         # tpdf completion for zsh. Load it with:  source <(tpdf completions zsh)\n\
         _tpdf() {{\n\
         \x20   local -a commands options\n\
         \x20   commands=(\n\
         {described}\
         \x20   )\n\
         \x20   if (( CURRENT == 2 )); then\n\
         \x20       _describe 'command' commands\n\
         \x20       return\n\
         \x20   fi\n\
         \x20   case \"${{words[2]}}\" in\n\
         {cases}\
         \x20       help) options=({commands}) ;;\n\
         \x20   esac\n\
         \x20   if [[ \"${{words[CURRENT]}}\" == -* || \"${{words[2]}}\" == help ]]; then\n\
         \x20       compadd -a options\n\
         \x20   else\n\
         \x20       _files\n\
         \x20   fi\n\
         }}\n\
         compdef _tpdf {programs}\n",
        programs = PROGRAMS.join(" "),
        commands = names.join(" "),
    )
}

fn fish() -> String {
    let mut text =
        String::from("# tpdf completion for fish. Load it with:  tpdf completions fish | source\n");
    for program in PROGRAMS {
        for command in COMMANDS {
            text.push_str(&format!(
                "complete -c {program} -n __fish_use_subcommand -a {} -d '{}'\n",
                command.name,
                brief(command).replace('\'', "")
            ));
            for option in options(command) {
                let flag = match option.strip_prefix("--") {
                    Some(long) => format!("-l {long}"),
                    None => format!("-s {}", option.trim_start_matches('-')),
                };
                text.push_str(&format!(
                    "complete -c {program} -n '__fish_seen_subcommand_from {}' {flag}\n",
                    command.name
                ));
            }
        }
        text.push_str(&format!(
            "complete -c {program} -n __fish_use_subcommand -a help -d 'Lists the commands, or explains one'\n"
        ));
    }
    text
}

fn powershell(names: &[&str]) -> String {
    let mut table = String::new();
    for command in COMMANDS {
        let quoted: Vec<String> = options(command)
            .iter()
            .map(|option| format!("'{option}'"))
            .collect();
        table.push_str(&format!(
            "        '{}' = @({})\n",
            command.name,
            quoted.join(", ")
        ));
    }
    let quoted: Vec<String> = names.iter().map(|name| format!("'{name}'")).collect();
    format!(
        "# tpdf completion for PowerShell. Load it with:\n\
         #   tpdf-cli completions powershell | Out-String | Invoke-Expression\n\
         Register-ArgumentCompleter -Native -CommandName {programs} -ScriptBlock {{\n\
         \x20   param($wordToComplete, $commandAst, $cursorPosition)\n\
         \x20   $commands = @({commands})\n\
         \x20   $options = @{{\n\
         {table}\
         \x20   }}\n\
         \x20   $words = @($commandAst.CommandElements | ForEach-Object {{ $_.ToString() }})\n\
         \x20   $typed = $words.Count - 1\n\
         \x20   if ($wordToComplete) {{ $typed = $typed - 1 }}\n\
         \x20   if ($typed -lt 1) {{\n\
         \x20       $candidates = $commands\n\
         \x20   }} elseif ($words[1] -eq 'help') {{\n\
         \x20       $candidates = $commands\n\
         \x20   }} elseif ($wordToComplete -like '-*') {{\n\
         \x20       $candidates = $options[$words[1]]\n\
         \x20   }} else {{\n\
         \x20       return\n\
         \x20   }}\n\
         \x20   $candidates | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{\n\
         \x20       [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n\
         \x20   }}\n\
         }}\n",
        programs = PROGRAMS.join(", "),
        commands = quoted.join(", "),
    )
}
