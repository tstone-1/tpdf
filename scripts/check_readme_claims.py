#!/usr/bin/env python3
"""Refuses a README that says a shipped command is not built.

WHY THIS EXISTS. On 2026-08-22 an outside review compared the public README with
the command registry and found the README describing a materially less capable
product than the binary: it said editing had *just begun*, it said *the open file
is never modified in place* -- false since Save in place shipped in `26.8.5` --
and its "Not built yet" list still named ink, shapes, text boxes and squiggly,
all four of which were registered commands with keyboard shortcuts.

Nothing could have caught that. Prose is not checkable in general, and the
alternative on offer -- another hand-maintained inventory -- is the thing that
drifted in the first place.

WHAT IS CHECKED, and it is deliberately one narrow claim rather than the whole
document: **a bullet under "Not built yet" carries a comment naming the command
that would exist if it were built, and none of those may be registered.** To
claim a feature is absent you have to name its absence in a form the registry can
contradict. That is the exact error that occurred, and it is the one with a
consequence -- a reader deciding whether to download this.

WHAT IS NOT CHECKED. Everything else in the README, including the status
paragraph, which is the sentence that was most wrong. There is no honest
mechanical test for "does this paragraph describe the product", and inventing a
keyword list to approximate one would be a second inventory to maintain and
therefore a second thing to drift. `BUILD.md`'s release checklist carries that
half, and it is a checklist rather than a gate on purpose: `docs/TRAPS.md`
records that a checklist is the weaker instrument, and naming which half is
weak beats implying both are strong.

Usage: scripts/check_readme_claims.py
"""

from __future__ import annotations

import pathlib
import re
import sys

#: Repository root, taken from this file rather than the working directory.
ROOT = pathlib.Path(__file__).resolve().parent.parent

README = ROOT / "README.md"
REGISTRY = ROOT / "src" / "lib" / "appcommands.ts"

#: The heading whose bullets carry the claims.
SECTION = "## Not built yet"

#: `<!-- not-built: edit.foo edit.bar -->`
CLAIM = re.compile(r"<!--\s*not-built:\s*([^>]*?)\s*-->")

#: `id: "edit.foo"` in the registry.
REGISTERED = re.compile(r'id:\s*"([^"]+)"')


def main() -> int:
    """Compares what the README says is missing with what is registered."""
    if not README.exists() or not REGISTRY.exists():
        print("[FAIL] README.md or src/lib/appcommands.ts is missing")
        return 1

    readme = README.read_text(encoding="utf-8")
    if SECTION not in readme:
        print(f"[FAIL] README.md has no '{SECTION}' section -- this check found nothing to read")
        return 1
    section = readme.split(SECTION, 1)[1].split("\n## ", 1)[0]

    claimed = [name for line in CLAIM.findall(section) for name in line.split()]
    if not claimed:
        # An empty scan reads exactly like a clean one. If the markers are ever
        # stripped -- by a rewrite, or by somebody who thought they were noise --
        # this has to say so rather than pass in silence.
        print(f"[FAIL] no 'not-built:' claims under '{SECTION}' -- every bullet needs one")
        return 1

    registered = set(REGISTERED.findall(REGISTRY.read_text(encoding="utf-8")))
    if not registered:
        print("[FAIL] no commands found in src/lib/appcommands.ts -- the scan found nothing")
        return 1

    shipped = sorted(name for name in claimed if name in registered)
    if shipped:
        for name in shipped:
            print(f"[FAIL] README.md lists '{name}' as not built, and it is a registered command")
        return 1

    duplicated = sorted({name for name in claimed if claimed.count(name) > 1})
    if duplicated:
        # Two bullets claiming one command is one of them saying nothing, and it
        # is how a bullet comes to look covered while its own claim went missing.
        for name in duplicated:
            print(f"[FAIL] README.md claims '{name}' is not built more than once")
        return 1

    print(f"[OK]   README.md: none of the {len(claimed)} unbuilt commands it names is registered")
    print(f"       (checked against {len(registered)} registered commands)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
