#!/usr/bin/env python3
"""Runs the window checks of this tree on a second Mac reached over SSH.

    scripts/run_on_mac.py --host HOST --clone '~/path/to/tpdf'
        [--wrap 'COMMAND --'] [--rev REV] [--only TEXT]... [--timeout SECONDS]
        [--pictures FOLDER [--picture-arg ARG]... | --phase PHASE FIXTURE | --voiceover]

WHY THIS EXISTS. `window_checks.py` drives a real window, takes the keyboard
and needs a screen that is unlocked and not covered, so on the machine somebody
is working at it is run when they remember to step away, which is seldom: the
record of 26.10.13 says no window harness was run. A Mac that is switched on
and logged in with nobody at it has such a screen all day. The release of
26.10.14 ran the eleven checks on one by hand, and this is that run as one
command.

WHAT IT SENDS. The tree as it is here, as `run_on_windows.py` sends it:
`git stash create` makes a commit of the tracked changes without touching the
working tree, and the commits the other Mac does not have travel as a `git
bundle`. **Untracked files are not sent**; stage a new file first. `--rev`
runs a commit instead.

WHERE IT RUNS. In a worktree of its own beside the clone, `<clone>-check`,
made on the first run and kept, with its own `target` and `node_modules`. The
clone's checkout is never moved. Fixtures are generated from the revision under test; the externally supplied
text-heavy.pdf comes from the clone. Each run has its own logs and takes an
exclusive lock on the worktree before it receives the bundle. The PDF engine is the one the sent tree
pins, installed by `scripts/fetch_pdfium.py` there. The first run is a cold
build.

THREE THINGS THE OTHER MAC HAS TO BE, each of them paid for on 2026-10-10:

  - **Logged in, with a session that is not locked.** A display is not what
    is missing on a Mac with none attached: it reports one all the same. What
    locks the session is the screen saver, which starts after twenty minutes
    without input when it has no setting, whatever *Require password* says.
    `defaults -currentHost write com.apple.screensaver idleTime -int 0` on
    that Mac turns it off. This script reads the lock before it builds and
    refuses a locked session, because a window check on one does not fail, it
    waits.
  - **Started in that session and not in the SSH one.** A window opened from
    an SSH login has no desktop. The run is handed to `launchd` in the
    `gui/<uid>` domain as a job that exists for this run only.
  - **Signed all the way down, when it is signed at all.** A build made with a
    Developer ID has the hardened runtime, and a library signed ad hoc is then
    refused by library validation: every check ends in *could not load its PDF
    engine*. The vendored PDFium is ad hoc. So when the built app carries an
    identity, the bundled `libpdfium.dylib` is signed with the same one and the
    app after it.

A FOURTH, WHICH THE SCRIPT NOW ARRANGES ITSELF. The screen has to be free as
well as unlocked. A window started from a `launchd` job opens behind whatever is
in front, and WebKit stops the timers of a page it cannot show: with a browser
window left across the display, every direct launch opened its document and
then printed no summary, on every tree, an old known-good release included
(`docs/TRAPS.md`, *A browser window left covering the second Mac's screen made
the open check hang on every tree*). So every visible app is hidden through
System Events before the job starts and shown again when it ends, on the
timeout and the refusal paths too. Hidden, not quit: the apps keep their
windows and whatever they were doing. If System Events refuses, the run says so
and goes on as it did before.

`--wrap` is a command put in front of the build and the signing, ending in
`--`, for a host where an SSH login has neither the pinned Node nor an unlocked
keychain and a script of its own supplies both.

WHAT THE ANSWER MEANS. Exit 0 only when the run on the other Mac printed
`window_checks.py`'s own exit code and that code is 0. A connection that
fails, a build that does not finish, a locked session and a run that never
prints its exit all end in 2, never in 0.

PICTURES INSTEAD. `--pictures FOLDER` runs `screenshots.py --more` there in
place of the window checks and copies what it wrote into FOLDER here, for
looking at a menu or a colour without taking this screen. A nonempty FOLDER
keeps its earlier files and receives this run in a new capture subdirectory. `--picture-arg` is
passed on, once for each: `--picture-arg=--dark`, `--picture-arg=--size`
`--picture-arg=1000x760`. The exit is then `screenshots.py`'s.

ONE PHASE INSTEAD. `--phase PHASE FIXTURE` runs `tabs_check.py FIXTURE --phase
PHASE` there, for the window phases `window_checks.py` does not carry, such as
`--phase textedit testdata/textedit-embedded.pdf`. The fixture is a path in the
clone's `testdata`. The exit is then `tabs_check.py`'s.

VOICEOVER INSTEAD. `--voiceover` runs `voiceover_check.py` there in place of
the window checks: the walk `nvda_check.py` makes on Windows, heard through
VoiceOver. The other Mac needs *Allow VoiceOver to be controlled with
AppleScript* ticked once in VoiceOver Utility. The exit is then
`voiceover_check.py`'s.

NOT A GATE. It needs a second machine that is switched on.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
MARKER = "TPDF-REMOTE-EXIT"
APP = "src-tauri/target/release/bundle/macos/tpdf Checks.app"

# What runs on the other Mac. `{...}` are filled in by `remote`; everything it
# is given is quoted there, and the paths may begin with `~`.
RECEIVE = r"""
set -u
export PATH="$HOME/.local/bin:/opt/homebrew/bin:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin"
clone={clone}
tree="$clone-check"; runs="$clone-check-run"; run="$runs/{run_id}"; sha={sha}
capture_ready() {{ [ -n "{capture}" ] && [ -d "$run/{capture}" ] && echo "TPDF-CAPTURE {capture}"; }}
stop() {{ capture_ready; echo "[FAIL] $1"; echo "{marker} 2"; exit 0; }}
[ -d "$clone/.git" ] || stop "no clone at $clone"
mkdir -p "$runs" && touch "$runs/worktree.lock" || stop "cannot open the worktree lock"
zmodload zsh/system || stop "cannot load zsh file locking"
zsystem flock -t 0 -f lockfd "$runs/worktree.lock" 2>/dev/null || stop "another check is using $tree"
mkdir "$run" || stop "cannot make $run"
cat >"$run/tree.bundle" || stop "cannot receive the bundle"
"""

REMOTE = RECEIVE + r"""
if ioreg -n Root -d1 -a | grep -A1 CGSSessionScreenIsLocked | grep -q '<true/>'; then
  stop "the session on this Mac is locked, so no window can be checked. The screen saver locks it: defaults -currentHost write com.apple.screensaver idleTime -int 0, then unlock it once"
fi
cd "$clone" || stop "cannot enter $clone"
# The remote first: the bundle holds only what the remote does not have, and
# cannot be read into a clone that is behind it.
git cat-file -e "$sha^{{commit}}" 2>/dev/null || git -c credential.helper= fetch --quiet origin 2>/dev/null
if [ -s "$run/tree.bundle" ]; then
  git fetch --quiet "$run/tree.bundle" "{ref}" || stop "cannot import the bundle"
fi
rm -f "$run/tree.bundle"
git cat-file -e "$sha^{{commit}}" 2>/dev/null || stop "this Mac does not have commit $sha"
[ -d "$tree" ] || git worktree add --quiet --detach "$tree" "$sha" || stop "cannot make the worktree $tree"
cd "$tree" || stop "cannot enter $tree"
python3 - "$clone" "$tree" <<'WORKTREE'
{verify_worktree}
WORKTREE
[ "$?" -eq 0 ] || stop "cannot verify the check worktree"
git checkout --quiet --force --detach "$sha" || stop "cannot check out $sha"
echo "[OK] $tree is at $(git rev-parse --short HEAD)"
# An explicit phase can name an external document in the clone. Stage it
# before generation so an older copy cannot override a generated fixture.
phase_input={phase_input}
if [[ "$phase_input" == testdata/* ]] &&
   ! git --literal-pathspecs ls-files --error-unmatch -- "$phase_input" >/dev/null 2>&1; then
  # Clear only this run's selected file in the reusable test worktree.
  if [[ -f "$phase_input" || -L "$phase_input" ]]; then
    git --literal-pathspecs clean -fx -- "$phase_input" || stop "cannot clear the previous phase input"
  fi
  if [[ -f "$clone/$phase_input" ]]; then
    mkdir -p "$(dirname "$phase_input")" && cp "$clone/$phase_input" "$phase_input" || stop "cannot stage the phase input"
  fi
fi
# Rebuild the window suite's fixtures from the generators in this checkout.
uv run --no-project --with-requirements scripts/fixture-tools.txt python scripts/ci_fixtures.py --signed --hostile >"$run/fixtures.log" 2>&1 &&
  uv run --no-project --with fonttools testdata/make_text_pdf.py testdata >>"$run/fixtures.log" 2>&1 &&
  python3 testdata/make_form_pdf.py >>"$run/fixtures.log" 2>&1 &&
  python3 testdata/make_redact_pages_pdf.py >>"$run/fixtures.log" 2>&1 &&
  python3 testdata/make_textedit_push.py >>"$run/fixtures.log" 2>&1 &&
  uv run --no-project --with pypdf --with fonttools testdata/make_textedit_embedded.py >>"$run/fixtures.log" 2>&1 ||
  {{ tail -15 "$run/fixtures.log"; stop "cannot generate fixtures from $sha"; }}
# This input is a real document with no generator.
if [ -f "$clone/testdata/text-heavy.pdf" ]; then
  cp "$clone/testdata/text-heavy.pdf" testdata/text-heavy.pdf || stop "cannot copy text-heavy.pdf"
else
  rm -f testdata/text-heavy.pdf || stop "cannot clear a stale external fixture"
fi
[[ -z "$phase_input" || -f "$phase_input" ]] || stop "the selected phase input is missing: $phase_input"
git diff --quiet HEAD || stop "fixture preparation changed a tracked test input"
# The engine the sent tree pins, and not the clone's: until 2026-10-10 this
# copied `vendor/pdfium` from the clone, so a tree that moved the pin was
# checked against the engine before it and nothing said so.
PATH="/opt/homebrew/bin:$PATH" python3 scripts/fetch_pdfium.py | tail -1
[ "${{pipestatus[1]}}" = 0 ] || stop "the pinned PDFium could not be installed"
# Both logs go first, so a failure shows this run's and not the last one's: on
# 2026-10-16 a build that failed before `npm ci` wrote anything printed the tail
# of a log eight hours old, which read as a finished build.
rm -f "$run/npm.log" "$run/build.log"
{wrap} sh -c 'npm ci >"$0/npm.log" 2>&1 && npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app >"$0/build.log" 2>&1' "$run" \
  || {{ tail -15 "$run/npm.log" "$run/build.log" 2>&1; stop "the checks build failed; $run/npm.log and build.log have all of it"; }}
app="$tree/{app}"
[ -x "$app/Contents/MacOS/tpdf" ] || stop "the build left no $app"
identity=$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -1)
if [ -n "$identity" ]; then
  {wrap} sh -c 'codesign -f -s "$1" --options runtime "$0/Contents/Resources/pdfium/libpdfium.dylib" && codesign -f -s "$1" --options runtime --preserve-metadata=entitlements,identifier "$0"' "$app" "$identity" >"$run/sign.log" 2>&1 \
    || {{ cat "$run/sign.log"; stop "the bundled PDFium could not be signed as $identity"; }}
  echo "[OK] the bundled PDFium is signed as the app is: $identity"
fi
# Launch this run's own executable so timeout cleanup can distinguish concurrent
# applications, including another launch of the reusable build product.
owned_app="$run/${{app:t}}"
ditto "$app" "$owned_app" || stop "cannot stage the checks app for this run"
app="$owned_app"
trap 'rm -rf -- "$owned_app"' EXIT
job="com.tpdf.windowchecks.{run_id}"
rm -f "$run/done" "$run/checks.log"
# Every other app is hidden for the run and shown again after it; the header
# says why. Hidden, never quit: a hidden app keeps its windows and its work.
cat >"$run/hide.applescript" <<'HIDE'
tell application "System Events"
  set found to name of every process whose visible is true
  set AppleScript's text item delimiters to linefeed
  return found as text
end tell
HIDE
cat >"$run/show.applescript" <<'SHOW'
on run argv
  tell application "System Events" to set visible of process (item 1 of argv) to true
end run
SHOW
hidden=""
show_again() {{
  [ -n "$hidden" ] || return 0
  print -r -- "$hidden" | while IFS= read -r one; do osascript "$run/show.applescript" "$one" >/dev/null 2>&1; done
  hidden=""
}}
owns_job=0
stop_job() {{
  if (( owns_job )); then
    launchctl bootout "gui/$(id -u)/$job" 2>/dev/null
    owns_job=0
  fi
}}
trap 'stop_job; show_again; rm -rf -- "$owned_app"' EXIT
trap 'exit 2' HUP INT TERM
# Snapshot before hiding anything, including when the first hide is interrupted.
hidden=$(osascript "$run/hide.applescript" 2>"$run/hide.log")
if [ -n "$hidden" ]; then
  print -r -- "$hidden" | while IFS= read -r one; do
    osascript -e 'on run argv' -e 'tell application "System Events" to set visible of process (item 1 of argv) to false' -e 'end run' "$one" 2>>"$run/hide.log"
  done
fi
if [ -s "$run/hide.log" ]; then
  echo "[WARN] could not hide all visible apps: $(head -1 "$run/hide.log")"
fi
printf '#!/bin/zsh\nexport PATH=%q\ncd -- %q || exit 2\nrun=%q\napp=%q\n' "$PATH" "$tree" "$run" "$app" >"$run/job.sh"
cat >>"$run/job.sh" <<'JOB'
{job} >"$run/checks.log" 2>&1
echo "EXIT $?" >>"$run/checks.log"
touch "$run/done"
JOB
chmod +x "$run/job.sh"
python3 - "$run" "$job" <<'PYPLIST'
import plistlib
import sys
from pathlib import Path, PurePosixPath
run = Path(sys.argv[1])
with (run / "job.plist").open("wb") as file:
    plistlib.dump(dict(Label=sys.argv[2], ProgramArguments=[str(run / "job.sh")], RunAtLoad=True), file)
PYPLIST
[ "$?" -eq 0 ] || {{ show_again; stop "could not write the launchd job"; }}
{start} || {{ stop "launchd did not take the job; is anybody logged in on this Mac?"; }}
waited=0
while [ ! -f "$run/done" ] && [ ! -f "$run/timed-out" ] && [ "$waited" -lt {limit} ]; do sleep 5; waited=$((waited + 5)); done
stop_job
if [[ -f "$run/timed-out" || ! -f "$run/done" ]]; then
  python3 -c 'import sys; from pathlib import Path; sys.path.insert(0, "scripts"); from stray import clear_strays; clear_strays(Path(sys.argv[1]))' "$app/Contents/MacOS/tpdf"
  show_again
  tail -8 "$run/checks.log" 2>/dev/null
  stop "the window checks did not end within {limit} s"
fi
show_again
{show}
capture_ready
code=$(sed -n 's/^EXIT //p' "$run/checks.log" | tail -1)
echo "{marker} ${{code:-2}}"
"""


def git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True,
                          check=True).stdout.strip()


def tracked_tree() -> str:
    """`git stash create`: a commit of the tracked changes, or nothing when there are none.

    Tried again for a few seconds. It takes the index lock, and on 2026-10-11
    this script and its twin for the other platform were started together
    before a release: the second ended in `git stash create` with exit 1.
    """
    for attempt in range(5):
        try:
            return git("stash", "create")
        except subprocess.CalledProcessError as error:
            if attempt == 4:
                # A file added with `git add -N` is the other cause, and retrying
                # cannot help: `stash create` refuses it as "not uptodate".
                print(f"[FAIL] git stash create: {(error.stderr or '').strip()}")
                print("[FAIL] a file added with `git add -N` cannot be stashed; stage it with `git add`")
                raise SystemExit(2) from error
            time.sleep(1)
    return ""


def tree_to_send(rev: str | None) -> str:
    """The commit to run: `rev`, or the tracked tree as it is here."""
    if rev:
        return git("rev-parse", "--verify", f"{rev}^{{commit}}")
    untracked = git("ls-files", "--others", "--exclude-standard")
    if untracked:
        print(f"[NOTE] {len(untracked.splitlines())} untracked file(s) are not sent; stage them first")
    return tracked_tree() or git("rev-parse", "HEAD")


# The controller and its desktop driver share the SSH session's AppleEvent
# permissions. Keep the controller in the foreground until its process group exits.
VOICEOVER_DRIVER = """
import os, signal, subprocess, sys
from pathlib import Path
run = Path(sys.argv[1])
driver = subprocess.Popen([run / "job.sh"], start_new_session=True)
try:
    driver.wait(timeout=float(sys.argv[2]))
except subprocess.TimeoutExpired:
    (run / "timed-out").touch()
    try:
        os.killpg(driver.pid, signal.SIGINT)
        driver.wait(timeout=3)
    except (subprocess.TimeoutExpired, ProcessLookupError):
        pass
    finally:
        try:
            os.killpg(driver.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        driver.wait()
"""


def remote(args: argparse.Namespace, sha: str, capture: str, run_id: str) -> str:
    only = " ".join(f"--only {shlex.quote(text)}" for text in args.only)
    start = 'owns_job=1\nlaunchctl bootstrap "gui/$(id -u)" "$run/job.plist"'
    if args.pictures:
        extra = " ".join(shlex.quote(text) for text in args.picture_arg)
        job = (f'mkdir "$run/{capture}" && '
               'uv run --with reportlab --with pillow scripts/screenshots.py '
               f'"$app/Contents/MacOS/tpdf" --more --out "$run/{capture}" {extra}')
        show = 'grep -v \'^EXIT\' "$run/checks.log"'
    elif args.voiceover:
        job = 'python3 scripts/voiceover_check.py "$app" --keep "$run/voiceover"'
        show = 'grep -v \'^EXIT\' "$run/checks.log"'
        # From this SSH session, not from launchd: VoiceOver answers an
        # AppleEvent from the first and lets one from a `gui/<uid>` job time
        # out (-1712), measured 2026-11-15. `open` puts the app on the
        # desktop from either.
        start = f'python3 -c {shlex.quote(VOICEOVER_DRIVER)} "$run" {float(args.timeout)}'
    elif args.phase:
        phase, fixture = args.phase
        job = (f'uv run scripts/tabs_check.py "$app/Contents/MacOS/tpdf" {shlex.quote(fixture)} '
               f'--phase {shlex.quote(phase)}')
        show = ('grep -E \'\\[FAIL\\]|\\[NOTE\\]|checks passed|Error\' "$run/checks.log" '
                '| grep -v \'^EXIT\'')
    else:
        job = f'python3 scripts/window_checks.py "$app/Contents/MacOS/tpdf" {only}'
        show = ('grep -E \'^\\[FAIL\\]|\\[NOTE\\]|not made\' "$run/checks.log"\n'
                'sed -n \'/^=== summary/,$p\' "$run/checks.log" | grep -v \'^EXIT\'')
    phase_input = PurePosixPath(args.phase[1]) if args.phase else None
    borrow = (phase_input is not None and phase_input.parts[:1] == ("testdata",)
              and phase_input.parts[:2] != ("testdata", "private") and ".." not in phase_input.parts)
    return REMOTE.format(verify_worktree=(ROOT / "scripts/check_worktree.py").read_text(), clone=remote_path(args.clone), sha=shlex.quote(sha), marker=f"{MARKER} {run_id}",
                         phase_input=shlex.quote(str(phase_input) if borrow else ""),
                         ref=f"refs/tpdf-check/{run_id}", run_id=run_id, capture=capture, wrap=args.wrap, app=APP, job=job, show=show,
                         limit=int(args.timeout), start=start)


def remote_path(path: str) -> str:
    """One shell word, expanding only a leading home directory."""
    if path == "~" or path.startswith("~/"):
        return '"$HOME"' + shlex.quote(path[1:])
    return shlex.quote(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--clone", required=True, help="the clone on the other Mac; may begin with ~")
    parser.add_argument("--wrap", default="", help="a command put before the build and the signing")
    parser.add_argument("--rev", help="run this commit, not the tree as it is here")
    parser.add_argument("--only", action="append", default=[], help="passed to window_checks.py")
    parser.add_argument("--timeout", type=float, default=900,
                        help="seconds the window checks may take, after the build")
    parser.add_argument("--pictures", type=Path,
                        help="run screenshots.py --more there and copy its pictures into this folder")
    parser.add_argument("--picture-arg", action="append", default=[],
                        help="passed to screenshots.py, once for each argument")
    parser.add_argument("--phase", nargs=2, metavar=("PHASE", "FIXTURE"),
                        help="run tabs_check.py FIXTURE --phase PHASE there instead")
    parser.add_argument("--voiceover", action="store_true",
                        help="run voiceover_check.py there instead")
    args = parser.parse_args()
    if sum(map(bool, (args.phase, args.pictures, args.voiceover))) > 1:
        parser.error("--phase, --pictures and --voiceover are different runs; pass one")

    sha = tree_to_send(args.rev)
    print(f"--- sending {sha[:10]} to {args.host} ---", flush=True)
    quiet = ["-o", "ConnectTimeout=15", "-o", "BatchMode=yes"]
    run_id = uuid.uuid4().hex
    ref = f"refs/tpdf-check/{run_id}"
    capture = "pictures" if args.pictures else ""
    with tempfile.TemporaryDirectory(prefix="tpdf-run-on-mac-") as scratch:
        bundle = Path(scratch) / "tree.bundle"
        if git("rev-list", "--max-count=1", sha, "--not", "--remotes=origin"):
            git("update-ref", ref, sha)
            try:
                git("bundle", "create", str(bundle), ref, "--not", "--remotes=origin")
            finally:
                git("update-ref", "-d", ref)
        # The remote shell reads the bundle only after acquiring the clone lock.
        # The script travels as an argument so stdin is a binary stream on every OS.
        command = "zsh -c " + shlex.quote(remote(args, sha, capture, run_id))
        try:
            with (bundle if bundle.exists() else Path(os.devnull)).open("rb") as payload:
                ran = subprocess.run(["ssh", "-T", *quiet, args.host, command], stdin=payload,
                                     capture_output=True, timeout=args.timeout + 3600, check=False)
        except subprocess.TimeoutExpired:
            print("[FAIL] the run on the other Mac did not return")
            return 2
    stdout = ran.stdout.decode("utf-8", "replace")
    stderr = ran.stderr.decode("utf-8", "replace")
    code = None
    captured = False
    for line in stdout.splitlines():
        if line.startswith(f"{MARKER} {run_id} "):
            code = line.split()[-1]
        elif capture and line == f"TPDF-CAPTURE {capture}":
            captured = True
        else:
            print(line)
    if code is None:
        print(stderr.strip()[-600:])
        print("[FAIL] the run printed no exit of its own, so it did not finish")
        return 2
    if code != "0" and stderr.strip():
        # What a step wrote to stderr outside its own log, such as the --wrap
        # command refusing to start, is otherwise lost.
        print("--- stderr on the other Mac ---")
        print(stderr.strip()[-1200:])
    if args.pictures and captured:
        args.pictures.mkdir(parents=True, exist_ok=True)
        # Keep earlier evidence intact. A reused destination gets a fresh run
        # directory so its files and count describe only the current capture.
        dest = args.pictures
        if any(dest.iterdir()):
            dest = Path(tempfile.mkdtemp(prefix="capture-", dir=dest))
        folder = f"{args.clone}-check-run/{run_id}/{capture}"
        got = subprocess.run(["scp", "-q", "-r", *quiet, f"{args.host}:{folder}/.", str(dest)],
                             check=False)
        if got.returncode != 0:
            print(f"[FAIL] the pictures could not be copied back from {folder}")
            return 2
        cleanup = 'clone=' + remote_path(args.clone) + f'; rm -r "$clone-check-run/{run_id}/{capture}"'
        if subprocess.run(["ssh", "-T", *quiet, args.host, cleanup], check=False).returncode:
            print(f"[WARN] could not remove the copied capture at {folder}")
        print(f"[{'OK' if code == '0' else 'FAIL'}] {len(list(dest.glob('*.png')))} pictures in {dest}")
    elif args.pictures and code == "0":
        print("[FAIL] the picture run returned without a capture")
        return 2
    return 0 if code == "0" else 1 if code == "1" else 2


if __name__ == "__main__":
    sys.exit(main())
