#!/usr/bin/env python3
"""Every callback the viewer can fire is wired in the application.

WHY THIS EXISTS
===============

`Viewer` reports what it cannot decide through optional callbacks on
`ViewerOptions` -- a note the reader typed, a mark they took off, a box they
drew. `App.svelte` supplies them. Nothing checked that it supplied all of them,
and on 2026-08-19 it did not: `onDrawn` was added to the interface, the viewer
fired it, and the object literal in `App.svelte` never gained the key. The box
tool armed, drew its preview, and silently reached no model.

**Three layers of tests passed while the feature was inert.** `viewerdraw.test.ts`
constructs its own viewer and supplies its own `onDrawn`, so it tested the
viewer's half. `viewer_check.py`'s command probe drives a recorder, so it tested
the command's half. And `appcommands.test.ts` sweeps every registered command for
an action, which `drawBox` had. The one thing none of them looks at is the object
literal that joins the two, because it lives in a `.svelte` file that no unit test
imports and no harness constructs.

Every callback is optional by design -- the check harness builds a viewer with
none of them -- so a missing key is not a type error either. That is what makes
this worth a gate rather than a convention: the failure is silent at every layer
that could otherwise have caught it.

WHAT IT DOES NOT CHECK
======================

That the wired function does the right thing. This is a wiring check: the key is
present and it is not `undefined`. What each one should do is the business of the
tests around it.

Usage:
    scripts/check_viewer_wiring.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VIEWER = ROOT / "src/lib/viewer.ts"
APP = ROOT / "src/App.svelte"

#: Callbacks the application deliberately does not supply, with the reason.
#:
#: An entry here is a claim that the application has nothing to do when the
#: viewer reports something, which is rarely true and always worth writing down
#: -- the same rule `viewer_sweep.py` applies to a fixture it does not open.
#:
#: The one entry is a finding rather than a decision: this check was written for
#: `onDrawn` and turned up `onNavigate` on its first run, which is the argument
#: for a set diff over a spot fix.
NOT_WIRED: dict[str, str] = {
    "onNavigate": (
        "it exists so a Back and Forward affordance can be re-enabled after a "
        "jump, and there is no such affordance: both commands are guarded on "
        "`withDocument` alone, so neither greys when there is nowhere to go. "
        "Wiring this is the same piece of work as making them grey, and it "
        "belongs with that rather than as an empty callback here"
    ),
}

#: `onX?: ...` inside the exported options interface.
DECLARED = re.compile(r"^  (on[A-Za-z0-9]+)\?:", re.M)
#: `onX: ...` inside the object literal handed to `new Viewer(`.
WIRED = re.compile(r"^\s+(on[A-Za-z0-9]+):", re.M)


def options_block(source: str) -> str:
    """The body of `export interface ViewerOptions`, and only that.

    Scanned rather than taken whole-file, because `Viewer` has methods whose
    names start with `on` -- `onWheel`, `onSelectStart` -- and a file-wide
    pattern would demand the application wire its private handlers.
    """
    start = source.index("export interface ViewerOptions {")
    # The first line that is a bare closing brace at column zero ends it.
    end = source.index("\n}\n", start)
    return source[start:end]


def viewer_literal(source: str) -> str:
    """The object literal passed to `new Viewer(`, and only that.

    Bounded the same way and for the same reason: `App.svelte` builds several
    objects with `on`-prefixed keys, and a file-wide scan would count a
    sidebar's callbacks as the viewer's.
    """
    start = source.index("new Viewer(")
    # The construction ends at the first line that closes it at that indent.
    end = source.index("\n      });\n", start)
    return source[start:end]


def main() -> int:
    viewer = VIEWER.read_text(encoding="utf-8")
    app = APP.read_text(encoding="utf-8")

    declared = set(DECLARED.findall(options_block(viewer)))
    wired = set(WIRED.findall(viewer_literal(app)))

    problems = 0
    # Both directions refuse an empty scan. A regex that stops matching passes
    # exactly like a tree with nothing wrong in it, which is the failure this
    # repository records about every check built on a pattern.
    if not declared:
        print("[FAIL] no callbacks found on ViewerOptions -- the scan is broken")
        problems += 1
    if not wired:
        print("[FAIL] no callbacks found in App.svelte's `new Viewer(` -- the scan is broken")
        problems += 1
    if problems:
        return 2

    missing = sorted(declared - wired - set(NOT_WIRED))
    for name in missing:
        print(f"[FAIL] {name} is declared on ViewerOptions and not wired in App.svelte")
        problems += 1

    # An option wired but no longer declared. Harmless to the compiler under
    # excess-property checking only if the type still has it, so this is really a
    # rename that half landed.
    extra = sorted(wired - declared)
    for name in extra:
        print(f"[FAIL] {name} is wired in App.svelte and not declared on ViewerOptions")
        problems += 1

    # An exemption naming something that no longer exists. A warning rather than
    # a failure, for the reason the webview-sink gate gives about its markers: a
    # list that quietly stops applying rots into a blanket permission.
    for name in sorted(set(NOT_WIRED) - declared):
        print(f"[WARN] {name} is excused from wiring and is not declared any more")

    for name in sorted(set(NOT_WIRED) & declared):
        print(f"[INFO] {name} is deliberately not wired: {NOT_WIRED[name]}")

    if problems:
        print(f"[FAIL] {problems} viewer callback(s) are not joined up")
        return 2
    print(
        f"[OK] all {len(declared)} viewer callbacks are wired in App.svelte "
        f"({len(NOT_WIRED)} deliberately not)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
