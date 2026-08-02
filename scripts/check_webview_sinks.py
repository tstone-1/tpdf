#!/usr/bin/env python3
"""Asserts the frontend contains no way to put a document's bytes into markup.

`docs/THREAT-MODEL.md` T8 says every document-derived string --- outline titles,
search results, form field labels --- is attacker-controlled and must reach the
DOM as **data**. Until 2026-08-02 the document justified that with "none of it
reaches the UI at all", which the sidebar and search had already falsified: an
outline title goes to `title.textContent` and every search result to
`results.ts`. The mitigation survived the change and is a better one than the
sentence it replaced --- `textContent` sets character data and never parses
markup --- but it survived by **convention**, and a convention is not a control.

This is the control. The invariant it pins is deliberately the narrow, checkable
one:

    there is no markup-parsing sink anywhere in the frontend

That is sufficient rather than merely necessary, and the reason is worth stating
because it is what makes a one-pattern grep a complete answer. If no sink exists,
then a string reaching the DOM has only `textContent`, `createTextNode`, `value`
and `setAttribute` left to travel by, and none of those parses markup. So this
check does not need to know *which* strings came from a document, which is the
part no grep could ever decide.

Two things bound that argument, and both were checked by hand on 2026-08-02
rather than assumed:

- **`setAttribute` can be a sink** when the attribute is `href`, `src` or an
  event handler and the value is attacker-controlled. Every `setAttribute` call
  in this frontend passes a **constant** attribute name --- `role`, `aria-label`,
  `aria-selected`, `tabindex` --- so there is no URL-bearing attribute to poison.
- **No document-derived URL crosses the boundary at all.** `outline.rs` refuses
  `/URI`, `/Launch` and `/GoToR` actions and reports them as
  `Target::Refused { action }`, so an outline entry can never carry a
  `javascript:` destination into the UI.

If either of those changes --- a `href` built from document text, or the outline
starting to follow URI actions --- this check goes on passing and stops being
sufficient. That is the one way it can be wrong, and it is recorded here rather
than left to be discovered.

## Why a script and not a vitest test

Reading the source tree needs `node:fs`, and this project deliberately ships no
Node type declarations (see the comment in `vite.config.ts`). A gate script costs
nothing and keeps `npm run check` honest.

Usage:
    scripts/check_webview_sinks.py
"""

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Where the frontend is. `index.html` and `shell.html` sit at the repo root and
# are both Vite inputs, so they are frontend even though they are not under src/.
ROOTS = [REPO / "src"]
ROOT_FILES = [REPO / "index.html", REPO / "shell.html"]
SUFFIXES = {".ts", ".js", ".svelte", ".html"}

# Every way to hand a string to a markup parser that is reachable from this
# stack. `{@html` is Svelte's; the rest are the DOM's.
#
# `eval` and `new Function` are not markup sinks and are here anyway: the CSP
# blocks them at runtime, so one shipping would be a bug that only appears in
# production, which is the worst place to find it.
SINKS = [
    "innerHTML",
    "outerHTML",
    "insertAdjacentHTML",
    "document.write",
    "createContextualFragment",
    "srcdoc",
    "{@html",
    "eval(",
    "new Function(",
]

# A line carrying this marker is exempt, and every exemption is counted and
# printed. Explicit and greppable rather than a heuristic about what "looks
# like" a comment: `AGENTS.md` records a checker that was satisfied by the note
# tracking the very gap it was looking for, and a rule about prose would let
# this one be silenced by a doc comment that merely *mentions* `innerHTML`.
EXEMPT = "webview-sink-ok:"

# `setAttribute` with a computed name. Dotted, so this matches a *call* and not
# the method definition in `testdom.ts`.
ATTRIBUTE_CALL = ".setAttribute("


def indices(haystack: str, needle: str) -> "list[int]":
    """Returns every start offset of `needle`, so two calls on one line both count."""
    found: "list[int]" = []
    at = haystack.find(needle)
    while at != -1:
        found.append(at)
        at = haystack.find(needle, at + 1)
    return found


def sources() -> "list[Path]":
    """Returns every frontend source file, sorted for a stable report."""
    found = [p for p in ROOT_FILES if p.is_file()]
    for root in ROOTS:
        if not root.is_dir():
            continue
        found += [p for p in root.rglob("*") if p.is_file() and p.suffix in SUFFIXES]
    return sorted(set(found))


def main() -> int:
    """Scans the frontend and refuses any markup sink."""
    files = sources()

    # A scan that examined nothing reports no findings, which is exactly what a
    # clean scan reports. `AGENTS.md`: an empty filter is not a pass, and a
    # structural audit silently covers nothing where its pattern does not occur.
    if not files:
        print(
            f"[FAIL] no frontend sources found under {REPO} -- the scan covered "
            "nothing, which is not the same as finding nothing.",
            file=sys.stderr,
        )
        return 1

    hits: "list[str]" = []
    exempted = 0
    lines_scanned = 0
    attribute_calls = 0

    for path in files:
        # Bytes, then UTF-8: `read_text()` decodes with the locale codec, which
        # is cp1252 on Windows, and this tree holds characters it cannot read.
        # That is the trap that stopped `mutate_rust.py` running on Windows at
        # all, and it is one line away from repeating here.
        text = path.read_bytes().decode("utf-8")
        rel = path.relative_to(REPO).as_posix()
        for number, line in enumerate(text.splitlines(), start=1):
            lines_scanned += 1
            found = [s for s in SINKS if s in line]

            # The second rule, and the one that keeps the first rule
            # *sufficient*: `setAttribute` is a sink when the attribute is
            # `href`, `src` or an event handler, so the argument that decides
            # whether it is one must not be computed. Matched on the dotted
            # form, which is a call --- the bare form is `testdom.ts`'s own
            # method definition, and excluding it by shape beats excusing it by
            # name.
            for index in indices(line, ATTRIBUTE_CALL):
                attribute_calls += 1
                rest = line[index + len(ATTRIBUTE_CALL) :].lstrip()
                if not rest.startswith(('"', "'")):
                    found.append(ATTRIBUTE_CALL.lstrip("."))

            if not found:
                continue
            if EXEMPT in line:
                exempted += len(found)
                continue
            for sink in found:
                hits.append(f"{rel}:{number}: {sink}  |  {line.strip()[:100]}")

    # Print the population before the verdict, so a run that scanned almost
    # nothing cannot read as a clean bill.
    print(
        f"scanned {len(files)} files, {lines_scanned} lines, "
        f"{len(SINKS)} sink patterns, {attribute_calls} setAttribute call(s), "
        f"{exempted} exemption(s) honoured"
    )

    # The setAttribute rule is the one that can silently stop covering anything:
    # it only ever fires on a call, so a refactor that renames the helper leaves
    # it scanning a pattern that no longer occurs, reporting the same clean bill.
    # `AGENTS.md`: a structural audit must report the population it found, and a
    # zero means unexamined rather than clean.
    if attribute_calls == 0:
        print(
            "[FAIL] no `.setAttribute(` call found anywhere in the frontend. That rule "
            "now covers\n       nothing -- either the DOM is built some other way, or "
            "this check needs\n       re-aiming. A pattern that does not occur passes "
            "exactly like a clean one.",
            file=sys.stderr,
        )
        return 1

    if hits:
        print(
            f"[FAIL] {len(hits)} markup sink(s) in the frontend. Document text is "
            "attacker-controlled\n"
            "       (docs/THREAT-MODEL.md T8): assign it with textContent, which "
            "sets character\n"
            "       data and never parses markup. If a sink is genuinely safe, "
            f"mark the line\n       `{EXEMPT} <why>` -- exemptions are counted "
            "and printed above.",
            file=sys.stderr,
        )
        for hit in hits:
            print(f"       {hit}", file=sys.stderr)
        return 1

    print("[OK] no markup sink in the frontend; document text can only be data.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
