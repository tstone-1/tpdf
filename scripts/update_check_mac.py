#!/usr/bin/env python3
"""Runs the previous release on a Mac and applies the published update to it.

Usage, on a Mac, logged in, with a session that is not locked:
    python3 scripts/update_check_mac.py <tpdf_<previous>_aarch64.dmg> <document.pdf>

WHY THIS EXISTS. It is `update_check.py` for the other platform: `BUILD.md`,
*Cutting a release*, step 12, as one command. No test fetches the real
`latest.json`, checks a real signature or replaces a real bundle; the step was
done by hand and so was skipped.

WHAT IT DOES, in order, and each line it prints is one check:
  1. reads `latest.json` as an installed copy does, for the version on offer;
  2. copies the previous release's application out of its disk image into a
     scratch folder, and starts it on a copy of the document that lies under a
     folder with a space in its name;
  3. waits for *Install update* in the application's menu to become available,
     which is the offer, and chooses it;
  4. waits for *Restart to finish update* and chooses that;
  5. waits for the application to come back: another process from the same
     bundle, the tool inside the bundle at the new version, the document open
     again, and *Install update* not available in the new copy;
  6. reads the replaced bundle's signature, which has to be the notarized
     Developer ID one;
  7. quits the application and removes the scratch folder.

On a Mac the two steps of 3 and 4 are two, where Windows has one: the plugin
replaces the bundle on disk and leaves the old process running until it is
restarted.

HOW THE MENU IS CHOSEN. Through System Events, which needs the Accessibility
permission for whatever starts this script (the terminal, or `sshd-keygen-
wrapper` for a run over SSH); without it the first read of the menu fails and
the run says so. The button in the window is not used: the accessibility tree
of the web view was measured as unreliable for it (`BUILD.md`, step 12), and
the menu is the application's own second way to the same command.

THE SESSION. As in `update_check.py`: the application is started with
`TPDF_SESSION_FILE` in the scratch folder, the real session file is moved away
for the run and put back, and after the restart the document is looked for in
whichever of the two was written.

It refuses to start while tpdf is running. It needs the network.

CONTROL. `--control no-press` waits for the offer and chooses nothing. The run
then ends with exit code 0 exactly when the checks of the update failed and no
other did.

Exit codes: 0 every check passed, 1 a check failed, 2 the run could not start.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

LATEST = "https://github.com/tstone-1/tpdf/releases/latest/download/latest.json"
NAME = re.compile(r"^tpdf_(\d+\.\d+\.\d+)_aarch64\.dmg$")
REAL_SESSION = Path.home() / "Library/Application Support/com.timostein.tpdf/session.json"

NO_PRESS = [
    "Install update is chosen",
    "Restart to finish update becomes available",
    "the application that was running ends",
    "the application comes back",
    "the tool in the bundle is version",
    "the document is open again",
    "the new copy offers no update",
]


class Checks:
    def __init__(self) -> None:
        self.failed: list[str] = []
        self.count = 0

    def note(self, ok: bool, name: str, detail: str = "") -> bool:
        self.count += 1
        if not ok:
            self.failed.append(name)
        print(f"{'[PASS]' if ok else '[FAIL]'} {name}{': ' + detail if detail else ''}", flush=True)
        return ok


def run(*command: str, timeout: float = 60) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(list(command), capture_output=True, text=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(list(command), 124, "", f"no answer within {timeout:g} s")


def offered() -> str:
    with urllib.request.urlopen(LATEST, timeout=30) as answer:
        manifest = json.load(answer)
    if "darwin-aarch64" not in manifest.get("platforms", {}):
        raise RuntimeError("latest.json offers nothing for darwin-aarch64")
    return str(manifest["version"])


def copies(binary: Path) -> list[int]:
    """The applications running this bundle's main binary.

    A worker is the same binary started by the application, so a process whose
    parent is one of these is left out (`update_check.py` has what counting
    them cost).
    """
    found: dict[int, int] = {}
    for line in run("ps", "-axo", "pid=,ppid=,args=").stdout.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and (parts[2] == str(binary) or parts[2].startswith(str(binary) + " ")):
            found[int(parts[0])] = int(parts[1])
    return [pid for pid, parent in found.items() if parent not in found]


def menu(pid: int, item: str, press: bool = False) -> str:
    """`enabled`, `disabled`, `pressed`, or what System Events said instead."""
    act = "click it" if press else "return (enabled of it) as text"
    script = (f'tell application "System Events" to tell (first process whose unix id is {pid}) '
              f'to tell menu item "{item}" of menu 1 of menu bar item 2 of menu bar 1 to {act}')
    asked = run("osascript", "-e", script, timeout=20)
    if asked.returncode != 0:
        return asked.stderr.strip() or f"exit {asked.returncode}"
    if press:
        return "pressed"
    return "enabled" if asked.stdout.strip() == "true" else "disabled"


def wait_for(pid: int, item: str, want: str, limit: float) -> str:
    said = ""
    deadline = time.monotonic() + limit
    while time.monotonic() < deadline:
        said = menu(pid, item)
        if said == want:
            break
        time.sleep(1)
    return said


def names(document: Path, sessions: list[Path], since: float) -> str:
    for where in sessions:
        if where.is_file() and where.stat().st_mtime >= since \
                and document.name in where.read_text(encoding="utf-8", errors="replace"):
            return str(where)
    return ""


def update(checks: Checks, app: Path, document: Path, scratch: Path, old: str, new: str,
           wait: float, press: bool) -> None:
    binary = app / "Contents/MacOS/tpdf"
    cli = app / "Contents/MacOS/tpdf-cli"
    session = scratch / "session.json"
    print(f"--- {old} opens the document and offers the update ---")
    said = run(str(cli), "--version").stdout.strip()
    checks.note(said.endswith(old), f"the tool in the bundle is version {old}", said)
    opened = run("open", "-n", "-a", str(app), str(document),
                 "--env", f"TPDF_SESSION_FILE={session}", "--env", f"TPDF_LOG_FILE={scratch / 'tpdf.log'}")
    first: list[int] = []
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline and not first:
        first = copies(binary)
        time.sleep(0.3)
    if not checks.note(bool(first), "the application starts", opened.stderr.strip()):
        return
    pid = first[0]
    try:
        offer = wait_for(pid, "Install update", "enabled", wait)
        checks.note(offer == "enabled", "the menu offers Install update", offer)
        if offer != "enabled":
            return
        began = time.time()
        if press:
            checks.note(menu(pid, "Install update", press=True) == "pressed", "Install update is chosen")
        else:
            checks.note(False, "Install update is chosen", "not chosen")
        ready = wait_for(pid, "Restart to finish update", "enabled", wait * 3 if press else 10)
        checks.note(ready == "enabled", "Restart to finish update becomes available", ready)
        if ready == "enabled":
            menu(pid, "Restart to finish update", press=True)

        print("--- the application comes back as the new version ---")
        gone_by = time.monotonic() + (wait if ready == "enabled" else 5)
        while time.monotonic() < gone_by and pid in copies(binary):
            time.sleep(0.3)
        checks.note(pid not in copies(binary), "the application that was running ends")
        again: list[int] = []
        named = ""
        deadline = time.monotonic() + (wait if ready == "enabled" else 5)
        while time.monotonic() < deadline:
            again = [p for p in copies(binary) if p != pid]
            named = names(document, [session, REAL_SESSION], began) if again else ""
            if again and named:
                break
            time.sleep(0.5)
        checks.note(bool(again), "the application comes back, from the same bundle")
        said = run(str(cli), "--version").stdout.strip()
        checks.note(said.endswith(new), f"the tool in the bundle is version {new}", said)
        checks.note(bool(named), "the document is open again, from under a folder with a space",
                    f"named in {named}" if named else str(document))
        if again:
            # Long enough for the new copy's own check for updates to answer.
            time.sleep(15)
            now = menu(again[0], "Install update")
            checks.note(now == "disabled", "the new copy offers no update", now)
        else:
            checks.note(False, "the new copy offers no update", "no new copy to ask")
        gate = run("spctl", "-a", "-vv", str(app))
        checks.note("Notarized Developer ID" in gate.stderr, "the bundle on disk is notarized Developer ID",
                    " ".join(gate.stderr.split())[:160])
    finally:
        for running in copies(binary):
            run("kill", str(running))
        time.sleep(1)
        for running in copies(binary):
            run("kill", "-9", str(running))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("dmg", help="the disk image of the release before the published one")
    parser.add_argument("pdf")
    parser.add_argument("--wait", type=float, default=60)
    parser.add_argument("--control", choices=["no-press"])
    args = parser.parse_args()

    if sys.platform != "darwin":
        print("[ERROR] this check runs the macOS bundle; run it on a Mac")
        return 2
    dmg, pdf = Path(args.dmg).resolve(), Path(args.pdf).resolve()
    for path in (dmg, pdf):
        if not path.is_file():
            print(f"[ERROR] no {path}")
            return 2
    named = NAME.match(dmg.name)
    if not named:
        print(f"[ERROR] {dmg.name} does not name its version")
        return 2
    old = named.group(1)
    try:
        new = offered()
    except (RuntimeError, OSError, ValueError, KeyError) as e:
        print(f"[ERROR] {e}")
        return 2
    if new == old:
        print(f"[ERROR] latest.json offers {new}, which is the image given; give the one before")
        return 2
    if run("pgrep", "-x", "tpdf").stdout.strip():
        print("[ERROR] tpdf is running; a second start would be handed to it")
        return 2

    control = args.control
    if control:
        print("--- control: the offer is waited for and nothing is chosen ---")
    checks = Checks()
    # Resolved, because `ps` prints the path without the `/var` link in it.
    scratch = Path(tempfile.mkdtemp(prefix="tpdf-update-check-")).resolve()
    kept = scratch / "session-as-it-was.json"
    if REAL_SESSION.is_file():
        shutil.move(str(REAL_SESSION), str(kept))
    mount = scratch / "image"
    try:
        spaced = scratch / "a folder with a space"
        spaced.mkdir()
        document = spaced / pdf.name
        shutil.copyfile(pdf, document)
        print(f"--- latest.json offers {new}; taking {old} out of {dmg.name} ---")
        attached = run("hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mount), str(dmg),
                       timeout=120)
        app = scratch / "tpdf.app"
        copied = attached.returncode == 0 and run("ditto", str(mount / "tpdf.app"), str(app),
                                                   timeout=120).returncode == 0
        run("hdiutil", "detach", "-quiet", str(mount))
        if checks.note(copied, "the application is copied out of the disk image",
                       "" if copied else attached.stderr.strip()):
            update(checks, app, document, scratch, old, new, args.wait, press=control != "no-press")
    finally:
        if REAL_SESSION.is_file():
            REAL_SESSION.unlink()
        if kept.is_file():
            REAL_SESSION.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(kept), str(REAL_SESSION))
        shutil.rmtree(scratch, ignore_errors=True)

    print()
    if control:
        unexpected = [n for n in checks.failed if not any(n.startswith(e) for e in NO_PRESS)]
        missing = [e for e in NO_PRESS if not any(n.startswith(e) for n in checks.failed)]
        if unexpected or missing:
            print(f"[FAIL] control '{control}': did not fail but should have: {missing or 'nothing'}; "
                  f"failed but should not have: {unexpected or 'nothing'}")
            return 1
        print(f"[OK] control '{control}': what it is aimed at failed ({len(NO_PRESS)}), and nothing else")
        return 0
    if checks.failed:
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed: {old} updated itself to {new}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
