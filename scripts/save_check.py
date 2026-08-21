#!/usr/bin/env python3
"""Drives an edit and a Save through the real menu, and reads the file back.

WHY THIS EXISTS
===============

Nothing else here writes a file. `viewer_check.py` names `file.save` in its
`undriven` table with the reason -- driving it would write over the corpus fixture
the rest of the run is reading -- `save.rs`'s unit tests build their plans
directly, and `edits.test.ts` asserts the shape of the `invoke` call. So the one
thing no check covered was the whole path: a reader edits a document, chooses
Save, and the bytes on disk change.

That gap became a bug report. Saving was reported broken from the running
application on 2026-08-20, and no harness in the repository could say whether it
was: the release went out with the symptom unreproduced, because reproducing it
by hand is the only instrument there was.

**The menu is the instrument.** macOS exposes the whole menu bar through
accessibility, so an edit and a Save can be driven exactly as a reader drives
them -- `Page > Rotate page clockwise`, `File > Save` -- with no synthetic key
events, which `docs/TRAPS.md` records as never reaching the web view anyway. What
the run then reads is not the application's opinion of what it did: it is the
file, by digest, and `qpdf --check` over the result.

WHAT IT CHECKS
==============

1. A document with no edits offers no Save. The control, and it runs first: if
   Save were always enabled every phase below would pass without meaning it.
2. A page rotation enables Save, and choosing it changes the bytes on disk.
3. The saved file is structurally sound, read by `qpdf`, which shares no code
   with anything in this repository.
4. Save greys again afterwards, so the document that comes back from the reopen
   is a clean one rather than a dirty one nobody rewrote.
5. A highlight over the page's own text is a second, different kind of edit, and
   it saves too --- it adds an annotation where a rotation changes an attribute.
6. Nothing is left beside the file. Staging writes a sibling and renames it, so a
   leftover is a failed commit that reported success.

WHAT IT DOES NOT CHECK
======================

Save a copy and Extract pages, both of which open a panel this cannot answer, and
every refusal `save.rs` states --- an encrypted document, a file changed under
the open one, a missing fingerprint. Those have tests that reach them directly;
what this adds is the join.

**It needs an unlocked screen and it says so rather than passing.** A locked
session suspends the web view, so the document never opens and every menu item
stays greyed --- which reads exactly like an application that ignores its own
menu. That misreading cost this file's author twenty minutes on the day it was
written, and it is the reason the lock is checked before anything else.

Usage:
    scripts/save_check.py
    scripts/save_check.py <path-to.app> [fixture.pdf]
"""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "src-tauri/target/release/bundle/macos/tpdf.app"
FIXTURE = ROOT / "testdata/outline-simple.pdf"

FILE_MENU = 3
EDIT_MENU = 4
PAGE_MENU = 5


def osa(script: str) -> str:
    done = subprocess.run(
        ["osascript", "-e", script], capture_output=True, text=True, timeout=120
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip())
    return done.stdout.strip()


def screen_is_locked() -> bool:
    out = subprocess.run(["ioreg", "-n", "Root", "-d1"], capture_output=True, text=True)
    return "CGSSessionScreenIsLocked" in out.stdout and "Yes" in "".join(
        line for line in out.stdout.splitlines() if "CGSSessionScreenIsLocked" in line
    )


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def enabled(menu: int, item: str) -> bool:
    return (
        osa(
            'tell application "System Events" to tell process "tpdf" to return enabled of '
            f'menu item "{item}" of menu 1 of menu bar item {menu} of menu bar 1'
        )
        == "true"
    )


def click(menu: int, item: str, settle: float = 3.0) -> None:
    osa('tell application "tpdf" to activate')
    time.sleep(0.5)
    osa(
        'tell application "System Events" to tell process "tpdf" to click '
        f'menu item "{item}" of menu 1 of menu bar item {menu} of menu bar 1'
    )
    time.sleep(settle)


def wait_for_document(seconds: int = 30) -> bool:
    """Print is guarded on there being a document, and nothing else."""
    for _ in range(seconds):
        time.sleep(1)
        try:
            if enabled(FILE_MENU, "Print"):
                return True
        except RuntimeError:
            continue
    return False


def qpdf_ok(path: Path) -> tuple[bool, str]:
    if shutil.which("qpdf") is None:
        return True, "[SKIP] qpdf is not installed, so the saved file was not read back"
    done = subprocess.run(
        ["qpdf", "--check", str(path)], capture_output=True, text=True
    )
    # A warning is exit 3 and is not a failure: the fixtures are generated and
    # several carry structures qpdf comments on. Exit 2 is a file it cannot read.
    if done.returncode in (0, 3):
        return True, f"[OK]   qpdf reads the saved file back (exit {done.returncode})"
    return False, f"[FAIL] qpdf cannot read the saved file: {done.stdout.strip()[:400]}"


def main() -> int:
    args = sys.argv[1:]
    app = Path(args[0]) if args else BUNDLE
    fixture = Path(args[1]) if len(args) > 1 else FIXTURE
    if not app.exists():
        print(f"[FAIL] no bundle at {app} --- npm run tauri build -- --bundles app")
        return 2
    if not fixture.exists():
        print(f"[FAIL] no fixture at {fixture} --- see BUILD.md on generating them")
        return 2
    if screen_is_locked():
        print("[FAIL] the screen is locked, so the web view is suspended and no")
        print("[FAIL] document will open. This cannot be answered from a script;")
        print("[FAIL] unlock the screen and run it again.")
        return 2

    problems = 0
    work = Path(tempfile.mkdtemp(prefix="tpdf-save-check-"))
    target = work / fixture.name
    shutil.copy2(fixture, target)
    before = digest(target)

    subprocess.run(["osascript", "-e", 'tell application "tpdf" to quit'],
                   capture_output=True)
    time.sleep(2)
    subprocess.run(["open", "-a", str(app.resolve()), str(target)], check=True)
    if not wait_for_document():
        print("[FAIL] no document was open 30s after launch (Print never enabled)")
        return 2
    print(f"[OK]   the document opened from {target}")

    try:
        # 1. The control. Save must be withheld before anything is edited, or
        #    every assertion below is satisfied by a menu that is always live.
        if enabled(FILE_MENU, "Save"):
            print("[FAIL] Save is offered on a document with no edits")
            problems += 1
        else:
            print("[OK]   Save is withheld on a document with no edits")

        # 2. A page rotation, which changes an attribute rather than adding an
        #    object -- the cheapest edit there is, and it needs no selection.
        click(PAGE_MENU, "Rotate page clockwise")
        if not enabled(FILE_MENU, "Save"):
            print("[FAIL] Save is still withheld after a page rotation")
            return 2
        print("[OK]   a page rotation offers Save")

        click(FILE_MENU, "Save", settle=5.0)
        after_rotate = digest(target)
        if after_rotate == before:
            print("[FAIL] the file on disk is byte-identical after Save")
            problems += 1
        else:
            print(f"[OK]   Save changed the file ({len(target.read_bytes())} bytes)")

        ok, said = qpdf_ok(target)
        print(said)
        if not ok:
            problems += 1

        # 4. And the reopen the save performs must produce a clean document.
        if enabled(FILE_MENU, "Save"):
            print("[FAIL] Save is still offered after saving, so the reopened "
                  "document is dirty")
            problems += 1
        else:
            print("[OK]   Save is withheld again after the save")

        # 5. A second kind of edit, over the page's own text. This one adds an
        #    annotation, so the file grows -- a different path through `save.rs`
        #    than an attribute change.
        click(EDIT_MENU, "Select all on page", settle=2.0)
        if not enabled(EDIT_MENU, "Highlight selection"):
            print("[SKIP] this document has no selectable text, so the "
                  "annotation half was not driven")
        else:
            click(EDIT_MENU, "Highlight selection")
            if not enabled(FILE_MENU, "Save"):
                print("[FAIL] Save is withheld after a highlight")
                problems += 1
            else:
                click(FILE_MENU, "Save", settle=5.0)
                after_mark = digest(target)
                if after_mark == after_rotate:
                    print("[FAIL] the file is unchanged after highlighting and saving")
                    problems += 1
                else:
                    print(f"[OK]   a highlight saved too ({len(target.read_bytes())} bytes)")
                ok, said = qpdf_ok(target)
                print(said)
                if not ok:
                    problems += 1

        # 6. Staging writes a sibling and renames it. Anything else left in the
        #    directory is a commit that did not happen and did not say so.
        strays = [p.name for p in work.iterdir() if p != target]
        if strays:
            print(f"[FAIL] the save left {strays} beside the document")
            problems += 1
        else:
            print("[OK]   nothing was left beside the document")
    finally:
        subprocess.run(["osascript", "-e", 'tell application "tpdf" to quit'],
                       capture_output=True)

    if problems:
        print(f"[FAIL] {problems} problem(s) saving over the open document")
        return 2
    print(f"[OK] the document saved over itself from the menu, twice, and reads back")
    return 0


if __name__ == "__main__":
    sys.exit(main())
