#!/usr/bin/env python3
"""Refuses a new collection held at the top level of `App.svelte`.

WHY THIS EXISTS
===============

`App.svelte` is the layer no test reaches: nothing imports it, and what is
checked of it is read as text. So `AGENTS.md` makes state be born outside it:
anything shaped like a walk, a set, a cache or a map starts life as a `src/lib`
module with its own unit tests, and the component keeps the wiring. Three
shipped defects sat at exactly that join, one of them an "already asked" set
of comments keyed by page slot, which the next page deletion renumbered (the
trap "An 'already asked' set keyed by a slot is renumbered by the next
deletion"). The rule was written down and nothing enforced it, so a new
`const seen = new Set()` in the component was a review comment at best.

THE RULE
========

In the component's script, a declaration at the top level, with no brace or
parenthesis open around it, whose value is `new Map(`, `new Set(`,
`new WeakMap(` or `new WeakSet(`, bare or inside `$state(` / `$state.raw(`,
is a finding. One inside a function, a `$derived`, an object literal or a class
is not: it is made per call, and the rule is about what the component *holds*.
A comment or a string naming one is not seen (`blanked` from
`check_view_after_await.py` removes them first, offsets kept).

Collections that predate the rule are in `KNOWN` with what keeps them there.
The ledger is checked both ways: an entry the component no longer has is a
finding, so the ledger cannot outlive the move that empties it.

WHAT IS REFUSED rather than passed: a script with fewer than `MIN_TOP_LEVEL`
top-level declarations found (the scan has stopped seeing the top level, and
would then pass anything), and a `KNOWN` entry with no reason.

WHAT IT CANNOT SEE: a collection made by a call (`makeSeen()` returning a Set),
an array used as a set, or a class instance that wraps one. Those are the
module the rule asks for, or the rule's spirit broken by a different spelling;
this checks the spelling that has been written each time so far.

Usage:
    scripts/check_app_state.py                    # the planted cases, then App.svelte
    scripts/check_app_state.py --self-test        # the planted cases alone
    scripts/check_app_state.py --app <file>       # another copy of the component
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_view_after_await import blanked, script_of  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "src" / "App.svelte"

#: Fewer top-level declarations than this means the scan is not seeing them.
#: 154 on the day this was written.
MIN_TOP_LEVEL = 100

#: Collections the component may still hold, each with why. Empty since the five
#: the rule was made with (`covered`, `wordsAsked`, `commentWords`, `redactionWords`,
#: `redactionPlans`) became `CoveredWords`, `CommentWords` and `RegionWords` in
#: `src/lib`, each a field of `LiveDocument`. A new entry needs a reason a reviewer
#: accepts; the default is the module.
KNOWN: dict[str, str] = {}

DECLARATION = re.compile(r"\b(?:let|const|var)\s+([A-Za-z_$][\w$]*)")
COLLECTION = re.compile(r"\s*(?::[^=;]*)?=\s*(?:\$state(?:\.raw)?\s*(?:<[^(]*>)?\s*\(\s*)?new\s+(Map|Set|WeakMap|WeakSet)\b")


def top_level(code: str) -> list[tuple[str, int, str | None]]:
    """Each top-level declaration: its name, offset, and the collection it makes or None."""
    found: list[tuple[str, int, str | None]] = []
    depth = 0
    at = 0
    for match in DECLARATION.finditer(code):
        for c in code[at:match.start()]:
            if c in "{([":
                depth += 1
            elif c in "})]":
                depth -= 1
        at = match.start()
        if depth != 0:
            continue
        made = COLLECTION.match(code, match.end())
        found.append((match.group(1), match.start(), made.group(1) if made else None))
    return found


def check(script: str, known: dict[str, str], first_line: int = 1) -> list[str]:
    """The findings for one component script."""
    code = blanked(script)
    declared = top_level(code)
    failures: list[str] = []
    if len(declared) < MIN_TOP_LEVEL:
        failures.append(
            f"only {len(declared)} top-level declarations found (at least {MIN_TOP_LEVEL} expected): "
            "the scan is not reading the component's top level"
        )
    held = {name for name, _, made in declared if made}
    for name, offset, made in declared:
        if made and name not in known:
            line = first_line + code.count("\n", 0, offset)
            failures.append(
                f"App.svelte:{line}: `{name}` is a top-level `new {made}`: state shaped like a set or a "
                "map starts in a `src/lib` module with its own tests (AGENTS.md, Quality gates)"
            )
    for name, why in known.items():
        if not why.strip():
            failures.append(f"KNOWN names `{name}` with no reason")
        if name not in held:
            failures.append(f"KNOWN names `{name}`, which App.svelte no longer holds: take it out of the ledger")
    return failures


def _padding(lines: int = MIN_TOP_LEVEL) -> str:
    return "".join(f"  let filler{n} = {n};\n" for n in range(lines))


#: (name, script, known, expected number of findings)
CASES: list[tuple[str, str, dict[str, str], int]] = [
    ("a top-level Map is refused", _padding() + "  const seen = new Map<number, string>();\n", {}, 1),
    ("a top-level Set held as state is refused", _padding() + "  let asked = $state(new Set<number>());\n", {}, 1),
    ("a typed WeakMap is refused", _padding() + "  const cache: WeakMap<object, number> = new WeakMap();\n", {}, 1),
    ("one inside a function is not", _padding() + "  function f() {\n    const seen = new Set();\n  }\n", {}, 0),
    ("one inside a derived is not", _padding() + "  const by = $derived(new Map(rows));\n", {}, 0),
    ("one in a comment is not", _padding() + "  // const seen = new Set();\n  /* let m = new Map(); */\n", {}, 0),
    ("one in a string is not", _padding() + '  const label = "let m = new Map()";\n', {}, 0),
    ("a known one with its reason passes", _padding() + "  let covered = new Map();\n", {"covered": "a slot"}, 0),
    ("a known one that has gone is refused", _padding(), {"covered": "a slot"}, 1),
    ("a known one with no reason is refused", _padding() + "  let covered = new Map();\n", {"covered": " "}, 1),
    ("a script whose top level is not seen is refused", "  const seen = 1;\n", {}, 1),
]


def self_test(quiet: bool = False) -> int:
    failed = 0
    for name, script, known, expected in CASES:
        got = len(check(script, known))
        ok = got == expected
        failed += not ok
        if not quiet or not ok:
            print(f"[{'OK' if ok else 'FAIL'}] self-test: {name} ({got} finding(s), {expected} expected)")
    return failed


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return 1 if self_test() else 0
    if self_test(quiet=True):
        print("[FAIL] the planted cases did not come out as expected; run with --self-test")
        return 2
    app = APP
    if "--app" in argv:
        at = argv.index("--app")
        if at + 1 >= len(argv):
            print("[FAIL] --app needs a file")
            return 2
        app = Path(argv[at + 1])
    try:
        source = app.read_text(encoding="utf-8")
        script, start = script_of(source)
    except (OSError, ValueError) as why:
        print(f"[FAIL] could not read {app} ({why})")
        return 2
    failures = check(script, KNOWN, 1 + source.count("\n", 0, start))
    for failure in failures:
        print(f"[FAIL] {failure}")
    if failures:
        return 1
    print(
        f"[OK] no top-level Map or Set in App.svelte beyond the {len(KNOWN)} in the ledger "
        f"({len(top_level(blanked(script)))} top-level declarations read, {len(CASES)} planted cases)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
