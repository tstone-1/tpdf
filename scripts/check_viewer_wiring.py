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

THREE INTERFACES, AND WHY NOT MORE
==================================

`ViewerOptions` was the first. `ThumbnailOptions` and `ScrollerOptions` joined it
on 2026-08-28, for exactly the same reason and after measuring which interfaces
can actually carry the defect. The frontend's exported interfaces hold **19**
optional `on*` callbacks in total, and those three hold all of them: 16, 2 and 1.

An outside review scored this gate as covering "21 of `App.svelte`'s ~80 callback
fields" and proposed extending it to `AppActions`' 51. **Measured, that would be
a check with no reachable subject.** Every one of those 51 members is
*required* --- `grep -c '?:'` inside the interface is zero --- so
`const appActions: AppActions = { ... }` in `App.svelte` fails `npm run check`
the moment one is missing. The type checker is a stronger instrument than this
script and it already owns them. What it cannot see is an **optional** property,
which is silently absent by design; that, and only that, is what belongs here.

The same measurement is the entry criterion for anything added below: an
interface earns a row when a missing key is silent at every layer. If TypeScript
refuses it, leave it to TypeScript.

ONE CALLBACK IS A SPECIAL CASE, AND THE MESSAGE SAYS THE WRONG THING
====================================================================

`ScrollerOptions` declares exactly one optional callback. Remove it from the
literal and the emptiness control fires first --- *"no callbacks found ... the
scan is broken"* --- rather than the specific *"onGone is declared and not
wired"*. The gate refuses either way, which is what matters, but the reason it
prints is wrong. With one callback, "nobody wired it" and "the scan is reading
the wrong block" are the same observation, and no message can separate them.
Worth knowing before chasing a broken regex that is fine.

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

#: One row per interface whose optional callbacks a caller can silently omit.
#:
#: `(interface file, interface header, construction file, construction marker,
#: literal terminator)`. The construction is bounded rather than scanned
#: whole-file for the reason `viewer_literal` gives: these files build several
#: objects with `on`-prefixed keys, and a file-wide pattern would count a
#: sidebar's callbacks as the viewer's.
WIRINGS: list[tuple[str, str, str, str, str]] = [
    (
        "src/lib/viewer.ts",
        "export interface ViewerOptions {",
        "src/App.svelte",
        "new Viewer(",
        "\n      });\n",
    ),
    (
        "src/lib/scroller.ts",
        "export interface ScrollerOptions {",
        # Constructed by the viewer rather than by the application: the scroller
        # is the viewer's own, and `App.svelte` has never seen it.
        "src/lib/viewer.ts",
        "new Scroller(this.surfaceHost, {",
        "\n    });\n",
    ),
    (
        "src/lib/thumbnails.ts",
        "export interface ThumbnailOptions {",
        # `Sidebar` constructs the strip from `opts.pages`, which the application
        # fills in; so the literal that can omit a callback is the `pages:` block
        # in `App.svelte`, not the `new Thumbnails(` line in `sidebar.ts`.
        "src/App.svelte",
        "pages: {",
        "\n        },\n",
    ),
]

#: Callbacks the application deliberately does not supply, with the reason.
#:
#: An entry here is a claim that the application has nothing to do when the
#: viewer reports something, which is rarely true and always worth writing down
#: -- the same rule `viewer_sweep.py` applies to a fixture it does not open.
#:
#: **Empty since 2026-08-23, and the one entry it held was a finding rather
#: than a decision.** This check was written for `onDrawn` and turned up
#: `onNavigate` on its first run -- declared so that a Back and Forward
#: affordance could be re-enabled after a jump, and consumed by nothing,
#: because both commands were guarded on `withDocument` alone and neither ever
#: greyed. The entry said as much and said the repair was the same piece of
#: work as making them grey. That work is done: `Viewer.canGoBack` and
#: `canGoForward` are `History`'s own answers, the two commands read them, and
#: `App.svelte` refreshes the pushed menu map on every history change.
#:
#: It stays as an empty table on purpose. The alternative is deleting the
#: mechanism and rebuilding it the next time a callback is genuinely not
#: needed, at which point the reason would be written from scratch rather than
#: against this one -- and an exemption with no reason is what the rule exists
#: to prevent.
NOT_WIRED: dict[str, str] = {}

#: `onX?: ...` inside the exported options interface --- the **optional** ones,
#: which are the only ones a caller can leave out in silence.
DECLARED = re.compile(r"^  (on[A-Za-z0-9]+)\?:", re.M)
#: `onX: ...` or `onX?: ...`, optional or not. Used only for the reverse
#: direction: a key wired under a name the interface does not have at all is a
#: rename that half landed, and a *required* callback is still a declared one.
#: Comparing that direction against the optional set alone reports every wired
#: required callback as undeclared --- which it did, on `ThumbnailOptions`.
KNOWN = re.compile(r"^  (on[A-Za-z0-9]+)\??:", re.M)
#: `onX: ...` inside the object literal handed to `new Viewer(`.
WIRED = re.compile(r"^\s+(on[A-Za-z0-9]+):", re.M)


def declared_block(source: str, header: str) -> str:
    """The body of one exported interface, and only that.

    Scanned rather than taken whole-file, because these classes have methods
    whose names start with `on` --- `onWheel`, `onSelectStart` --- and a file-wide
    pattern would demand the application wire their private handlers.
    """
    start = source.index(header)
    end = source.index("\n}\n", start)
    return source[start:end]


def wired_block(source: str, marker: str, terminator: str) -> str:
    """The object literal handed to one constructor, and only that.

    Bounded the same way and for the same reason: these files build several
    objects with `on`-prefixed keys, and a file-wide scan would count a
    sidebar's callbacks as the viewer's.
    """
    start = source.index(marker)
    end = source.index(terminator, start)
    return source[start:end]


def check(
    declared_in: str,
    header: str,
    wired_in: str,
    marker: str,
    terminator: str,
) -> tuple[int, int]:
    """One row of `WIRINGS`. Returns (problems, callbacks checked)."""
    what = header.removeprefix("export interface ").removesuffix(" {")
    try:
        source = (ROOT / declared_in).read_text(encoding="utf-8")
        app = (ROOT / wired_in).read_text(encoding="utf-8")
        block = declared_block(source, header)
        declared = set(DECLARED.findall(block))
        known = set(KNOWN.findall(block))
        wired = set(WIRED.findall(wired_block(app, marker, terminator)))
    except (OSError, ValueError) as why:
        print(f"[FAIL] {what}: could not read the blocks to compare ({why})")
        return (1, 0)

    problems = 0
    # Both directions refuse an empty scan. A regex that stops matching passes
    # exactly like a tree with nothing wrong in it, which is the failure this
    # repository records about every check built on a pattern.
    if not declared:
        print(f"[FAIL] no optional callbacks found on {what} -- the scan is broken")
        problems += 1
    if not wired:
        print(f"[FAIL] no callbacks found in {wired_in}'s `{marker}` -- the scan is broken")
        problems += 1
    if problems:
        return (problems, 0)

    for name in sorted(declared - wired - set(NOT_WIRED)):
        print(f"[FAIL] {what}.{name} is declared and not wired in {wired_in}")
        problems += 1

    # An option wired but no longer declared. Harmless to the compiler under
    # excess-property checking only if the type still has it, so this is really a
    # rename that half landed.
    for name in sorted(wired - known):
        print(f"[FAIL] {name} is wired in {wired_in} and not declared on {what}")
        problems += 1

    for name in sorted(set(NOT_WIRED) & declared):
        print(f"[INFO] {what}.{name} is deliberately not wired: {NOT_WIRED[name]}")

    return (problems, len(declared))


def main() -> int:
    problems = 0
    total = 0
    for row in WIRINGS:
        found, checked = check(*row)
        problems += found
        total += checked

    # An exemption naming something that no longer exists. A warning rather than
    # a failure, for the reason the webview-sink gate gives about its markers: a
    # list that quietly stops applying rots into a blanket permission.
    everything: set[str] = set()
    for declared_in, header, _, _, _ in WIRINGS:
        try:
            source = (ROOT / declared_in).read_text(encoding="utf-8")
            everything |= set(DECLARED.findall(declared_block(source, header)))
        except (OSError, ValueError):
            continue
    for name in sorted(set(NOT_WIRED) - everything):
        print(f"[WARN] {name} is excused from wiring and is not declared any more")

    if problems:
        print(f"[FAIL] {problems} callback(s) are not joined up")
        return 2
    print(
        f"[OK] all {total} optional callbacks across {len(WIRINGS)} interfaces are wired "
        f"({len(NOT_WIRED)} deliberately not)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
