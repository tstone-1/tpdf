#!/usr/bin/env python3
"""Runs the window checks of this tree on a second Mac reached over SSH.

    scripts/run_on_mac.py --host HOST --clone '~/path/to/tpdf'
        [--wrap 'COMMAND --'] [--rev REV] [--only TEXT]... [--timeout SECONDS]

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
clone's checkout is never moved. `vendor/pdfium` and `testdata` are copied from
the clone, which has to have them: git carries neither. The first run is a
cold build.

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

`--wrap` is a command put in front of the build and the signing, ending in
`--`, for a host where an SSH login has neither the pinned Node nor an unlocked
keychain and a script of its own supplies both.

WHAT THE ANSWER MEANS. Exit 0 only when the run on the other Mac printed
`window_checks.py`'s own exit code and that code is 0. A connection that
fails, a build that does not finish, a locked session and a run that never
prints its exit all end in 2, never in 0.

NOT A GATE. It needs a second machine that is switched on.
"""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MARKER = "TPDF-REMOTE-EXIT"
REF = "refs/tpdf-check/tree"
APP = "src-tauri/target/release/bundle/macos/tpdf Checks.app"

# What runs on the other Mac. `{...}` are filled in by `remote`; everything it
# is given is quoted there, and the paths may begin with `~`.
REMOTE = r"""
set -u
export PATH="$HOME/.local/bin:/opt/homebrew/bin:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin"
clone={clone}; clone="${{clone/#\~/$HOME}}"
tree="$clone-check"; run="$clone-check-run"; sha={sha}
stop() {{ echo "[FAIL] $1"; echo "{marker} 2"; exit 0; }}
mkdir -p "$run" || stop "cannot make $run"
[ -d "$clone/.git" ] || stop "no clone at $clone"
if ioreg -n Root -d1 -a | grep -A1 CGSSessionScreenIsLocked | grep -q '<true/>'; then
  stop "the session on this Mac is locked, so no window can be checked. The screen saver locks it: defaults -currentHost write com.apple.screensaver idleTime -int 0, then unlock it once"
fi
cd "$clone" || stop "cannot enter $clone"
# The remote first: the bundle holds only what the remote does not have, and
# cannot be read into a clone that is behind it.
git cat-file -e "$sha^{{commit}}" 2>/dev/null || git -c credential.helper= fetch --quiet origin 2>/dev/null
[ -f "$run/tree.bundle" ] && git fetch --quiet "$run/tree.bundle" "{ref}" 2>/dev/null
git cat-file -e "$sha^{{commit}}" 2>/dev/null || stop "this Mac does not have commit $sha"
[ -d "$tree" ] || git worktree add --quiet --detach "$tree" "$sha" || stop "cannot make the worktree $tree"
cd "$tree" || stop "cannot enter $tree"
git checkout --quiet --force --detach "$sha" || stop "cannot check out $sha"
echo "[OK] $tree is at $(git rev-parse --short HEAD)"
for carried in vendor/pdfium testdata; do
  [ -d "$clone/$carried" ] || stop "the clone has no $carried, and git does not carry it"
  mkdir -p "$tree/$carried" && rsync -a "$clone/$carried/" "$tree/$carried/" || stop "cannot copy $carried"
done
{wrap} sh -c 'npm ci >"$0/npm.log" 2>&1 && npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app >"$0/build.log" 2>&1' "$run" \
  || {{ tail -15 "$run/build.log" 2>/dev/null; stop "the checks build failed; $run/build.log has all of it"; }}
app="$tree/{app}"
[ -x "$app/Contents/MacOS/tpdf" ] || stop "the build left no $app"
identity=$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -1)
if [ -n "$identity" ]; then
  {wrap} sh -c 'codesign -f -s "$1" --options runtime "$0/Contents/Resources/pdfium/libpdfium.dylib" && codesign -f -s "$1" --options runtime --preserve-metadata=entitlements,identifier "$0"' "$app" "$identity" >"$run/sign.log" 2>&1 \
    || {{ cat "$run/sign.log"; stop "the bundled PDFium could not be signed as $identity"; }}
  echo "[OK] the bundled PDFium is signed as the app is: $identity"
fi
job="com.tpdf.windowchecks.$$"
rm -f "$run/done" "$run/checks.log"
cat >"$run/job.sh" <<JOB
#!/bin/zsh
export PATH="$PATH"
cd "$tree"
python3 scripts/window_checks.py "$app/Contents/MacOS/tpdf" {only} >"$run/checks.log" 2>&1
echo "EXIT \$?" >>"$run/checks.log"
touch "$run/done"
JOB
chmod +x "$run/job.sh"
cat >"$run/job.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>$job</string>
<key>ProgramArguments</key><array><string>$run/job.sh</string></array>
<key>RunAtLoad</key><true/></dict></plist>
PLIST
launchctl bootstrap "gui/$(id -u)" "$run/job.plist" || stop "launchd did not take the job; is anybody logged in on this Mac?"
waited=0
while [ ! -f "$run/done" ] && [ "$waited" -lt {limit} ]; do sleep 5; waited=$((waited + 5)); done
launchctl bootout "gui/$(id -u)/$job" 2>/dev/null
if [ ! -f "$run/done" ]; then
  pkill -f "$app/Contents/MacOS/tpdf" 2>/dev/null
  tail -8 "$run/checks.log" 2>/dev/null
  stop "the window checks did not end within {limit} s"
fi
grep -E '^\[FAIL\]|\[NOTE\]|not made' "$run/checks.log"
sed -n '/^=== summary/,$p' "$run/checks.log" | grep -v '^EXIT'
code=$(sed -n 's/^EXIT //p' "$run/checks.log" | tail -1)
echo "{marker} ${{code:-2}}"
"""


def git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True,
                          check=True).stdout.strip()


def tree_to_send(rev: str | None) -> str:
    """The commit to run: `rev`, or the tracked tree as it is here."""
    if rev:
        return git("rev-parse", "--verify", f"{rev}^{{commit}}")
    untracked = git("ls-files", "--others", "--exclude-standard")
    if untracked:
        print(f"[NOTE] {len(untracked.splitlines())} untracked file(s) are not sent; stage them first")
    return git("stash", "create") or git("rev-parse", "HEAD")


def remote(args: argparse.Namespace, sha: str) -> str:
    only = " ".join(f"--only {shlex.quote(text)}" for text in args.only)
    return REMOTE.format(clone=shlex.quote(args.clone), sha=shlex.quote(sha), marker=MARKER,
                         ref=REF, wrap=args.wrap, app=APP, only=only, limit=int(args.timeout))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--clone", required=True, help="the clone on the other Mac; may begin with ~")
    parser.add_argument("--wrap", default="", help="a command put before the build and the signing")
    parser.add_argument("--rev", help="run this commit, not the tree as it is here")
    parser.add_argument("--only", action="append", default=[], help="passed to window_checks.py")
    parser.add_argument("--timeout", type=float, default=900,
                        help="seconds the window checks may take, after the build")
    args = parser.parse_args()

    sha = tree_to_send(args.rev)
    print(f"--- sending {sha[:10]} to {args.host} ---", flush=True)
    quiet = ["-o", "ConnectTimeout=15", "-o", "BatchMode=yes"]
    with tempfile.TemporaryDirectory(prefix="tpdf-run-on-mac-") as scratch:
        bundle = Path(scratch) / "tree.bundle"
        git("update-ref", REF, sha)
        try:
            made = subprocess.run(["git", "-C", str(ROOT), "bundle", "create", str(bundle), REF,
                                   "--not", "--remotes=origin"], capture_output=True, text=True,
                                  check=False)
        finally:
            git("update-ref", "-d", REF)
        # A commit the other Mac can fetch from the remote needs no bundle, and
        # `git bundle` refuses to write an empty one.
        folder = f"{args.clone}-check-run"
        # Nothing is made on the other Mac until the clone is seen to be there.
        prepare = f'test -d {args.clone}/.git && mkdir -p {folder} && rm -f {folder}/tree.bundle'
        asked = subprocess.run(["ssh", "-T", *quiet, args.host, prepare], check=False).returncode
        if asked != 0:
            print(f"[FAIL] {args.host} does not answer" if asked == 255
                  else f"[FAIL] no clone at {args.clone} on {args.host}")
            return 2
        if made.returncode == 0 and bundle.is_file():
            sent = subprocess.run(["scp", "-q", *quiet, str(bundle), f"{args.host}:{folder}/tree.bundle"],
                                  check=False)
            if sent.returncode != 0:
                print("[FAIL] the tree could not be sent")
                return 2
    try:
        ran = subprocess.run(["ssh", "-T", *quiet, args.host, "zsh", "-s"], input=remote(args, sha),
                             capture_output=True, text=True, timeout=args.timeout + 3600, check=False)
    except subprocess.TimeoutExpired:
        print("[FAIL] the run on the other Mac did not return")
        return 2
    code = None
    for line in ran.stdout.splitlines():
        if line.startswith(MARKER):
            code = line.split()[-1]
        else:
            print(line)
    if code is None:
        print(ran.stderr.strip()[-600:])
        print("[FAIL] the run printed no exit of its own, so it did not finish")
        return 2
    return 0 if code == "0" else 1 if code == "1" else 2


if __name__ == "__main__":
    sys.exit(main())
