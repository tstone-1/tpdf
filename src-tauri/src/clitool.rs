//! *Install command-line tool…* and its undo: the `tpdf` link on macOS.
//!
//! The tool ships inside the bundle as `Contents/MacOS/tpdf-cli`, beside the
//! application, signed and notarized with it (`src/bin/tpdf-cli.rs`). Putting
//! it on a reader's `PATH` is one symbolic link, `/usr/local/bin/tpdf`, which is
//! what VS Code's *Install 'code' command in PATH* does and for the same
//! reason: a link follows the application when it updates in place, where a
//! copy would go stale at the first update.
//!
//! **The link is written without privilege when the directory allows it, and
//! through the OS's own administrator prompt when it does not.** A fresh Apple
//! silicon Mac has no `/usr/local/bin` at all, and creating it needs root; that
//! is asked for with AppleScript's `do shell script ... with administrator
//! privileges`, which is the system's authorization dialog --- tpdf never sees
//! the password. The path of the tool travels as an argument to `osascript`
//! and reaches the shell through `quoted form of`, so no part of it is ever
//! read as script or as shell syntax.
//!
//! **What is there already decides everything, and is read before and after.**
//! [`plan`] is the decision, and it is pure over what the link path holds: a
//! file that is not tpdf's is never replaced or removed, and the answer the
//! reader is shown is [`plan`] asked again after the change, not the exit code
//! of the command that made it.
//!
//! **Two links since 2026-10-02, `tpdf` and `tpdf-cli`, for one reason:** on
//! Windows the tool can only be `tpdf-cli`, because `tpdf.exe` is the
//! application, so a script written on one platform did not run on the other.
//! `tpdf-cli` is now a name on both. [`LINK`] stays the one that decides
//! everything --- whether the tool counts as installed, and whether a refusal
//! is an error --- and [`ALIAS`] follows it: made, repointed and removed with
//! it under the same rule, and left alone, with a sentence, when the path
//! holds something that is not tpdf's.
//!
//! Windows has no counterpart to install: `tpdf-cli.exe` is installed beside
//! `tpdf.exe` by the installer, and the answer says where.

use std::path::{Path, PathBuf};

/// Where the link goes.
pub const LINK: &str = "/usr/local/bin/tpdf";

/// The same tool under the name it has on Windows, so one script runs on both.
pub const ALIAS: &str = "/usr/local/bin/tpdf-cli";

/// The tool's file name inside the bundle.
pub const TOOL: &str = if cfg!(windows) {
    "tpdf-cli.exe"
} else {
    "tpdf-cli"
};

/// What the link path holds, and so what installing or removing would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing is there: installing creates the link.
    Create,
    /// A link to a tpdf tool somewhere else --- an older copy of the
    /// application, moved or replaced: installing points it here.
    Repoint(PathBuf),
    /// The link is there and names this tool.
    Present,
    /// A tpdf link to be removed.
    Remove,
    /// Nothing to remove.
    Absent,
    /// Something that is not tpdf's link: left alone either way.
    Foreign(String),
}

/// Whether `target` is a tpdf command-line tool inside an application bundle.
///
/// The bundle is required by name, and not only its inner layout: until the
/// 26.9.21 release audit this accepted any `.../Contents/MacOS/tpdf-cli`, so a
/// link into a plain folder laid out that way counted as tpdf's own.
fn ours(target: &Path) -> bool {
    target.file_name().is_some_and(|name| name == TOOL)
        && target.parent().is_some_and(|dir| {
            dir.ends_with("Contents/MacOS")
                && dir
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::extension)
                    .is_some_and(|ext| ext == "app")
        })
}

/// What installing (`install`) or uninstalling would do, given what `link`
/// holds now and that `tool` is this application's tool.
#[must_use]
pub fn plan(install: bool, link: &Path, tool: &Path) -> Step {
    let held = match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if install { Step::Create } else { Step::Absent };
        }
        Err(e) => return Step::Foreign(format!("{} could not be read: {e}", link.display())),
        Ok(meta) => meta,
    };
    if !held.file_type().is_symlink() {
        return Step::Foreign(format!(
            "{} is a file that tpdf did not put there",
            link.display()
        ));
    }
    let Ok(target) = std::fs::read_link(link) else {
        return Step::Foreign(format!("{} could not be read", link.display()));
    };
    if !ours(&target) {
        return Step::Foreign(format!(
            "{} links to {}, which is not tpdf's",
            link.display(),
            target.display()
        ));
    }
    match (install, target == tool) {
        (true, true) => Step::Present,
        (true, false) => Step::Repoint(target),
        (false, _) => Step::Remove,
    }
}

/// Where this application's tool is: beside the running executable.
///
/// # Errors
///
/// The executable's path cannot be read, the tool is not there, or the
/// application is running from where macOS put it to check it --- App
/// Translocation, a random read-only path that is gone at the next launch, so
/// a link to it would dangle.
pub fn tool() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("tpdf could not find where it is running from: {e}"))?;
    let tool = exe
        .parent()
        .map(|dir| dir.join(TOOL))
        .ok_or("tpdf could not find the folder it is running from")?;
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        return Err(
            "tpdf is running from a temporary location macOS chose, so a link to it would stop \
             working. Move tpdf to the Applications folder, open it from there, and install \
             the command-line tool again."
                .into(),
        );
    }
    if !tool.is_file() {
        return Err(format!(
            "The command-line tool is not beside this copy of tpdf ({}).",
            tool.display()
        ));
    }
    Ok(tool)
}

/// The sentence for the reader after asking `plan` again, following a change.
#[must_use]
pub fn outcome(install: bool, after: &Step, tool: &Path) -> String {
    match (install, after) {
        (true, Step::Present) => format!(
            "Installed. `tpdf` in a terminal now runs the command-line tool ({LINK} links to \
             {}). Try `tpdf help`.",
            tool.display()
        ),
        (false, Step::Absent) => {
            format!("Removed {LINK} and {ALIAS}. The application is unchanged.")
        }
        (_, Step::Foreign(why)) => format!("{why}, so it was left alone."),
        (true, _) => format!("{LINK} was not installed."),
        (false, _) => format!("{LINK} is still there."),
    }
}

/// Installs or removes the link, asking the OS for administrator rights only
/// when the directory needs them. Returns the sentence for the reader.
///
/// # Errors
///
/// The tool cannot be found, or the change could not be made --- including
/// the reader cancelling the administrator prompt.
#[cfg(target_os = "macos")]
pub fn apply(install: bool) -> Result<String, String> {
    let tool = tool()?;
    let link = Path::new(LINK);
    match plan(install, link, &tool) {
        // Settled only when the second name is too: an installation from
        // before 2026-10-02 has the link and not the alias, and installing
        // again is how it gains it.
        Step::Present if changes(install, link, Path::new(ALIAS), &tool).is_empty() => {
            return Ok(format!(
                "Already installed: {LINK} links to {}.",
                tool.display()
            ))
        }
        Step::Absent if changes(install, link, Path::new(ALIAS), &tool).is_empty() => {
            return Ok(format!(
                "The command-line tool is not installed ({LINK} does not exist)."
            ))
        }
        Step::Foreign(why) => {
            return Err(format!(
                "{why}, so it was left alone. Remove it yourself to install tpdf's."
            ))
        }
        Step::Create | Step::Repoint(_) | Step::Remove | Step::Present | Step::Absent => {}
    }
    let alias = Path::new(ALIAS);
    let both = changes(install, link, alias, &tool);
    if both
        .iter()
        .any(|path| unprivileged(install, path, &tool).is_err())
    {
        // Asked again: what the unprivileged attempt managed is not redone.
        privileged(install, &tool, &changes(install, link, alias, &tool))?;
    }
    let after = plan(install, link, &tool);
    let mut text = outcome(install, &after, &tool);
    if let Some(also) = alias_outcome(install, &plan(install, alias, &tool)) {
        text.push(' ');
        text.push_str(&also);
    }
    if matches!(
        (install, &after),
        (true, Step::Present) | (false, Step::Absent)
    ) {
        Ok(text)
    } else {
        Err(text)
    }
}

/// What the two commands would find, read and not changed: what the window
/// greys them by.
///
/// **It decides nothing.** [`apply`] reads the filesystem again each time a
/// command runs and answers from what it finds then; this is only whether a
/// command is worth offering, and it is built from [`plan`] so that the two
/// cannot read a link differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ToolState {
    /// This copy's tool is what a terminal gets under both names, so
    /// installing has nothing to do.
    pub installed: bool,
    /// One of the two paths holds something: a link of tpdf's, to this copy or
    /// to another, which removing takes away, or a file that is not tpdf's,
    /// which removing leaves alone and says so. False only when both paths are
    /// empty, which is when removing has nothing to do and nothing to say.
    pub occupied: bool,
}

/// [`ToolState`] over what `link` and `alias` hold, where `tool` is this
/// application's tool. Reads the two paths and writes nothing.
///
/// A path that cannot be read counts as held and not as installed, so both
/// commands stay offered and the one that is run says what is wrong.
#[must_use]
pub fn state_of(link: &Path, alias: &Path, tool: &Path) -> ToolState {
    ToolState {
        installed: [link, alias]
            .into_iter()
            .all(|path| plan(true, path, tool) == Step::Present),
        occupied: [link, alias]
            .into_iter()
            .any(|path| plan(false, path, tool) != Step::Absent),
    }
}

/// [`ToolState`] of this machine: the two links on macOS.
///
/// # Errors
///
/// The tool cannot be found, for [`tool`]'s reasons. The caller then knows
/// nothing and greys nothing.
#[cfg(target_os = "macos")]
pub fn state() -> Result<Option<ToolState>, String> {
    let tool = tool()?;
    Ok(Some(state_of(Path::new(LINK), Path::new(ALIAS), &tool)))
}

/// [`ToolState`] of this machine: whether the tool's folder is on the user's own
/// `PATH`, which is what both commands change on Windows. Read through a
/// handle that can only be asked (`userpath::stored`).
///
/// # Errors
///
/// The tool is not beside the application, or the `PATH` could not be read.
#[cfg(windows)]
pub fn state() -> Result<Option<ToolState>, String> {
    let on = crate::userpath::stored(&folder()?)?;
    Ok(Some(ToolState {
        installed: on,
        occupied: on,
    }))
}

/// Neither macOS nor Windows: both commands only say a sentence, so there is
/// no state and nothing to grey.
///
/// # Errors
///
/// None; the signature is the other platforms'.
#[cfg(not(any(target_os = "macos", windows)))]
#[allow(clippy::unnecessary_wraps)]
pub fn state() -> Result<Option<ToolState>, String> {
    Ok(None)
}

/// Which of the two paths installing or removing has to change, the link first.
///
/// A path holding something that is not tpdf's is never among them.
#[must_use]
pub fn changes<'a>(install: bool, link: &'a Path, alias: &'a Path, tool: &Path) -> Vec<&'a Path> {
    [link, alias]
        .into_iter()
        .filter(|path| {
            matches!(
                plan(install, path, tool),
                Step::Create | Step::Repoint(_) | Step::Remove
            )
        })
        .collect()
}

/// What to add about [`ALIAS`], asked after the change; `None` when it is as
/// the link is and there is nothing to add.
#[must_use]
pub fn alias_outcome(install: bool, after: &Step) -> Option<String> {
    match (install, after) {
        (true, Step::Present) => Some(
            "`tpdf-cli` runs it too, which is its name on Windows, so one script runs on both."
                .into(),
        ),
        (false, Step::Absent) => None,
        (_, Step::Foreign(why)) => Some(format!("{why}, so it was left alone.")),
        (true, _) => Some(format!("{ALIAS} was not installed.")),
        (false, _) => Some(format!("{ALIAS} is still there.")),
    }
}

/// The change, as the reader's own user. Fails with the OS's error when the
/// directory is not theirs to write, which is the ordinary case.
#[cfg(target_os = "macos")]
fn unprivileged(install: bool, link: &Path, tool: &Path) -> std::io::Result<()> {
    if install {
        // Beside the link and renamed over it, so a link that is being
        // repointed is never absent in between.
        let staged = link.with_file_name(format!(".tpdf-link-{}", std::process::id()));
        let _ = std::fs::remove_file(&staged);
        std::os::unix::fs::symlink(tool, &staged)?;
        std::fs::rename(&staged, link).inspect_err(|_| {
            let _ = std::fs::remove_file(&staged);
        })
    } else {
        std::fs::remove_file(link)
    }
}

/// The change, through the system's administrator prompt.
#[cfg(target_os = "macos")]
fn privileged(install: bool, tool: &Path, paths: &[&Path]) -> Result<(), String> {
    let script = admin_script(install, paths);
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .arg(tool)
        .output()
        .map_err(|e| format!("the administrator prompt could not be shown: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    if said.contains("-128") {
        return Err("Nothing was changed: the administrator prompt was cancelled.".into());
    }
    Err(format!("Nothing was changed: {}", said.trim()))
}

/// The AppleScript that makes or removes `paths` behind the administrator
/// prompt, in one prompt for both.
///
/// `quoted form of` is AppleScript's own shell quoting, applied to an argument
/// rather than to text spliced into the script: the tool's path is never part
/// of either program. The link paths *are* spliced, and may be, because they
/// are [`LINK`] and [`ALIAS`] --- constants with no character a shell reads ---
/// and anything else is refused here rather than quoted.
#[cfg(any(target_os = "macos", test))]
fn admin_script(install: bool, paths: &[&Path]) -> String {
    let named: Vec<&str> = paths
        .iter()
        .filter_map(|path| path.to_str())
        .filter(|path| *path == LINK || *path == ALIAS)
        .collect();
    let list = named.join(" and ");
    let shell = if install {
        let links: Vec<String> = named
            .iter()
            .map(|path| {
                format!(" && /bin/ln -sfn \" & quoted form of (item 1 of argv) & \" {path}")
            })
            .collect();
        format!("\"/bin/mkdir -p /usr/local/bin{}\"", links.concat())
    } else {
        format!("\"/bin/rm -f {}\"", named.join(" "))
    };
    let prompt = if install {
        format!("tpdf wants to install its command-line tool as {list}.")
    } else {
        format!("tpdf wants to remove its command-line tool, {list}.")
    };
    format!(
        "on run argv\ndo shell script {shell} with prompt \"{prompt}\" with administrator \
         privileges\nend run"
    )
}

/// A path as a reader types it. `canonicalize` on Windows answers in the
/// verbatim form, `\\?\C:\...` or `\\?\UNC\server\share\...`, which a
/// command prompt and the PATH editor do not take.
#[cfg(any(not(target_os = "macos"), test))]
fn shown(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    }
}

/// The folder the tool is in, as a reader types it: what goes on `PATH`.
///
/// # Errors
///
/// The tool is not beside the application.
#[cfg(not(target_os = "macos"))]
pub fn folder() -> Result<String, String> {
    let tool = tool()?;
    let dir = tool
        .parent()
        .ok_or("the command-line tool is in no folder")?;
    Ok(shown(&dir.display().to_string()))
}

/// The Windows answer: nothing to link. The tool is installed beside the
/// application, and its folder is put on the user's `PATH` or taken off it.
///
/// # Errors
///
/// The tool is not beside the application, or the `PATH` could not be read
/// or written.
#[cfg(windows)]
pub fn apply(install: bool) -> Result<String, String> {
    use crate::userpath::Outcome;
    let dir = folder()?;
    Ok(match (install, crate::userpath::apply(&dir, install)?) {
        (true, Outcome::Changed) => {
            format!("Added {dir} to your PATH. Open a new terminal and run tpdf-cli.")
        }
        (true, Outcome::Unchanged) => {
            format!("{dir} is already on your PATH: run tpdf-cli in a terminal.")
        }
        (false, Outcome::Changed) => {
            format!("Removed {dir} from your PATH. The tool itself is removed together with tpdf.")
        }
        (false, Outcome::Unchanged) => {
            format!("{dir} is not on your PATH. The tool itself is removed together with tpdf.")
        }
    })
}

/// Neither macOS nor Windows: where the tool is, and nothing changed.
///
/// # Errors
///
/// The tool is not beside the application.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn apply(install: bool) -> Result<String, String> {
    let dir = folder()?;
    Ok(if install {
        format!("The command-line tool is installed with tpdf, in {dir}.")
    } else {
        "The command-line tool is removed together with tpdf; there is nothing to remove \
         separately."
            .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shown_path_drops_the_verbatim_prefix_and_nothing_else() {
        for (path, expected) in [
            (
                r"\\?\C:\Users\a\AppData\Local\tpdf\tpdf-cli.exe",
                r"C:\Users\a\AppData\Local\tpdf\tpdf-cli.exe",
            ),
            (r"\\?\UNC\server\share\tpdf", r"\\server\share\tpdf"),
            (r"C:\Program Files\tpdf", r"C:\Program Files\tpdf"),
            (r"\\server\share\tpdf", r"\\server\share\tpdf"),
            ("/usr/local/bin/tpdf", "/usr/local/bin/tpdf"),
        ] {
            assert_eq!(shown(path), expected);
        }
    }

    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpdf-clitool-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("tpdf.app/Contents/MacOS")).expect("scratch");
        dir
    }

    #[cfg(unix)]
    #[test]
    fn only_tpdfs_own_link_is_created_repointed_or_removed() {
        let dir = scratch("plan");
        let tool = dir.join("tpdf.app/Contents/MacOS").join(TOOL);
        std::fs::write(&tool, b"#!").expect("tool");
        let link = dir.join("tpdf");

        // Nothing there.
        assert_eq!(plan(true, &link, &tool), Step::Create);
        assert_eq!(plan(false, &link, &tool), Step::Absent);

        // This tool.
        std::os::unix::fs::symlink(&tool, &link).expect("link");
        assert_eq!(plan(true, &link, &tool), Step::Present);
        assert_eq!(plan(false, &link, &tool), Step::Remove);

        // An older copy of the application somewhere else: repointed on
        // install, removed on uninstall.
        std::fs::remove_file(&link).expect("unlink");
        let old = PathBuf::from("/Volumes/Old/tpdf.app/Contents/MacOS").join(TOOL);
        std::os::unix::fs::symlink(&old, &link).expect("link");
        assert_eq!(plan(true, &link, &tool), Step::Repoint(old));
        assert_eq!(plan(false, &link, &tool), Step::Remove);

        // Somebody else's: never touched, whichever way.
        std::fs::remove_file(&link).expect("unlink");
        std::os::unix::fs::symlink("/opt/other/bin/tpdf", &link).expect("link");
        for install in [true, false] {
            assert!(
                matches!(plan(install, &link, &tool), Step::Foreign(_)),
                "{install}"
            );
        }
        std::fs::remove_file(&link).expect("unlink");
        // A tool of that name outside a bundle is not ours either.
        std::os::unix::fs::symlink(PathBuf::from("/usr/local/lib").join(TOOL), &link)
            .expect("link");
        assert!(matches!(plan(false, &link, &tool), Step::Foreign(_)));
        std::fs::remove_file(&link).expect("unlink");
        // Nor is one inside a bundle but outside `Contents/MacOS`: the layout
        // is checked as well as the bundle's name.
        std::os::unix::fs::symlink(
            PathBuf::from("/Applications/tpdf.app/Contents/Resources").join(TOOL),
            &link,
        )
        .expect("link");
        for install in [true, false] {
            assert!(
                matches!(plan(install, &link, &tool), Step::Foreign(_)),
                "{install}"
            );
        }
        std::fs::remove_file(&link).expect("unlink");
        // Nor is one laid out like a bundle inside a folder that is not one.
        std::os::unix::fs::symlink(PathBuf::from("/tmp/x/Contents/MacOS").join(TOOL), &link)
            .expect("link");
        for install in [true, false] {
            assert!(
                matches!(plan(install, &link, &tool), Step::Foreign(_)),
                "{install}"
            );
        }
        std::fs::remove_file(&link).expect("unlink");
        std::fs::write(&link, b"a script of somebody's").expect("file");
        for install in [true, false] {
            assert!(
                matches!(plan(install, &link, &tool), Step::Foreign(_)),
                "{install}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_second_name_follows_the_first_and_a_foreign_one_is_left_alone() {
        let dir = scratch("alias");
        let tool = dir.join("tpdf.app/Contents/MacOS").join(TOOL);
        std::fs::write(&tool, b"#!").expect("tool");
        let (link, alias) = (dir.join("tpdf"), dir.join("tpdf-cli"));

        // A fresh machine: both are made.
        assert_eq!(
            changes(true, &link, &alias, &tool),
            vec![link.as_path(), alias.as_path()]
        );
        assert!(changes(false, &link, &alias, &tool).is_empty());

        // An installation from before the alias existed: only the alias is owed.
        std::os::unix::fs::symlink(&tool, &link).expect("link");
        assert_eq!(changes(true, &link, &alias, &tool), vec![alias.as_path()]);
        std::os::unix::fs::symlink(&tool, &alias).expect("alias");
        assert!(changes(true, &link, &alias, &tool).is_empty());
        assert_eq!(
            changes(false, &link, &alias, &tool),
            vec![link.as_path(), alias.as_path()]
        );

        // Somebody else's `tpdf-cli`: never among the changes, either way, and said.
        std::fs::remove_file(&alias).expect("unlink");
        std::fs::write(&alias, b"somebody's script").expect("file");
        assert!(changes(true, &link, &alias, &tool).is_empty());
        assert_eq!(changes(false, &link, &alias, &tool), vec![link.as_path()]);
        let said = alias_outcome(true, &plan(true, &alias, &tool)).expect("said");
        assert!(said.contains("left alone"), "{said}");
        assert_eq!(std::fs::read(&alias).expect("kept"), b"somebody's script");

        assert!(alias_outcome(true, &Step::Present)
            .expect("said")
            .contains("Windows"));
        assert_eq!(alias_outcome(false, &Step::Absent), None);
        assert!(alias_outcome(true, &Step::Create)
            .expect("said")
            .contains("was not installed"));
        assert!(alias_outcome(false, &Step::Remove)
            .expect("said")
            .contains("still there"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every name in `dir` and what it holds, so a read can be shown to have
    /// changed nothing.
    #[cfg(unix)]
    fn held(dir: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
        let mut all: Vec<_> = std::fs::read_dir(dir)
            .expect("listing")
            .map(|entry| entry.expect("entry").path())
            .map(|path| (path.clone(), std::fs::read_link(&path).ok()))
            .collect();
        all.sort();
        all
    }

    #[cfg(unix)]
    #[test]
    fn the_state_is_read_from_both_links_and_reading_changes_neither() {
        let dir = scratch("state");
        let tool = dir.join("tpdf.app/Contents/MacOS").join(TOOL);
        std::fs::write(&tool, b"#!").expect("tool");
        let (link, alias) = (dir.join("tpdf"), dir.join("tpdf-cli"));
        let old = PathBuf::from("/Volumes/Old/tpdf.app/Contents/MacOS").join(TOOL);
        let state = |installed, occupied| ToolState {
            installed,
            occupied,
        };
        let read = || {
            let before = held(&dir);
            let read = state_of(&link, &alias, &tool);
            assert_eq!(held(&dir), before, "reading changed what is there");
            read
        };

        // Nothing there: install is offered, uninstall has nothing to do.
        assert_eq!(read(), state(false, false));

        // An installation from before the second name: installing still owes
        // the alias, so it is not installed yet.
        std::os::unix::fs::symlink(&tool, &link).expect("link");
        assert_eq!(read(), state(false, true));

        // Both names, this copy: installed.
        std::os::unix::fs::symlink(&tool, &alias).expect("alias");
        assert_eq!(read(), state(true, true));

        // The alias alone is as little installed as the link alone.
        std::fs::remove_file(&link).expect("unlink");
        assert_eq!(read(), state(false, true));

        // Both names, another copy of tpdf: installing repoints, removing removes.
        std::fs::remove_file(&alias).expect("unlink");
        std::os::unix::fs::symlink(&old, &link).expect("link");
        std::os::unix::fs::symlink(&old, &alias).expect("alias");
        assert_eq!(read(), state(false, true));

        // One name this copy's and one another copy's is not installed either.
        std::fs::remove_file(&link).expect("unlink");
        std::os::unix::fs::symlink(&tool, &link).expect("link");
        assert_eq!(read(), state(false, true));

        // Somebody else's file at either path, with nothing of tpdf's: both
        // stay offered, because running either is what explains it.
        std::fs::remove_file(&link).expect("unlink");
        std::fs::remove_file(&alias).expect("unlink");
        std::fs::write(&link, b"somebody's script").expect("file");
        assert_eq!(read(), state(false, true));
        std::fs::remove_file(&link).expect("unlink");
        std::os::unix::fs::symlink("/opt/other/bin/tpdf-cli", &alias).expect("alias");
        assert_eq!(read(), state(false, true));

        // This copy's link beside somebody else's `tpdf-cli`: not installed
        // under both names, so installing stays offered.
        std::os::unix::fs::symlink(&tool, &link).expect("link");
        assert_eq!(read(), state(false, true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_read_greys_neither_command() {
        let dir = scratch("unreadable");
        let tool = dir.join("tpdf.app/Contents/MacOS").join(TOOL);
        std::fs::write(&tool, b"#!").expect("tool");
        // A file where the folder should be: asking about a name inside it
        // fails, and not with "nothing is there".
        let blocked = dir.join("bin");
        std::fs::write(&blocked, b"not a folder").expect("file");
        let (link, alias) = (blocked.join("tpdf"), blocked.join("tpdf-cli"));
        let error = std::fs::symlink_metadata(&link).expect_err("unreadable");
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
        assert_eq!(
            state_of(&link, &alias, &tool),
            ToolState {
                installed: false,
                occupied: true,
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_state_crosses_to_the_window_under_the_names_it_reads() {
        let sent = serde_json::to_value(ToolState {
            installed: true,
            occupied: false,
        })
        .expect("json");
        assert_eq!(
            sent,
            serde_json::json!({ "installed": true, "occupied": false })
        );
    }

    #[test]
    fn the_administrator_script_names_only_tpdfs_two_paths() {
        let (link, alias) = (Path::new(LINK), Path::new(ALIAS));
        let both = admin_script(true, &[link, alias]);
        assert_eq!(
            both,
            "on run argv\ndo shell script \"/bin/mkdir -p /usr/local/bin && /bin/ln -sfn \" & \
             quoted form of (item 1 of argv) & \" /usr/local/bin/tpdf && /bin/ln -sfn \" & quoted \
             form of (item 1 of argv) & \" /usr/local/bin/tpdf-cli\" with prompt \"tpdf wants to \
             install its command-line tool as /usr/local/bin/tpdf and /usr/local/bin/tpdf-cli.\" \
             with administrator privileges\nend run"
        );
        let one = admin_script(true, &[alias]);
        assert!(
            one.contains("/usr/local/bin/tpdf-cli\" with prompt"),
            "{one}"
        );
        assert!(!one.contains("/usr/local/bin/tpdf &&"), "{one}");
        assert_eq!(
            admin_script(false, &[link, alias]),
            "on run argv\ndo shell script \"/bin/rm -f /usr/local/bin/tpdf \
             /usr/local/bin/tpdf-cli\" with prompt \"tpdf wants to remove its command-line tool, \
             /usr/local/bin/tpdf and /usr/local/bin/tpdf-cli.\" with administrator \
             privileges\nend run"
        );
        // A path that is not one of the two never reaches the script.
        let odd = admin_script(false, &[Path::new("/etc/passwd; rm -rf /"), link]);
        assert!(!odd.contains("passwd"), "{odd}");
        assert!(odd.contains("/bin/rm -f /usr/local/bin/tpdf\""), "{odd}");
    }

    #[test]
    fn the_reader_is_told_what_is_there_afterwards_not_what_was_attempted() {
        let tool = Path::new("/Applications/tpdf.app/Contents/MacOS/tpdf-cli");
        assert!(outcome(true, &Step::Present, tool).starts_with("Installed."));
        assert!(outcome(false, &Step::Absent, tool).starts_with("Removed"));
        // Both names are removed, so both are said.
        assert!(outcome(false, &Step::Absent, tool).contains(ALIAS));
        // A change that did not take is not reported as one.
        assert!(outcome(true, &Step::Create, tool).contains("was not installed"));
        assert!(outcome(false, &Step::Remove, tool).contains("still there"));
        assert!(outcome(true, &Step::Foreign("x is odd".into()), tool).contains("left alone"));
    }
}
