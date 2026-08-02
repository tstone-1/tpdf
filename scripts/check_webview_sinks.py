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

One thing bounds that argument: a string that cannot become **markup** can still
become a **navigation or a script** by reaching `href`, `src` or an event
handler. So five more rules close the three routes those have --- a computed
attribute name, a dangerous literal one, and a direct property assignment ---
plus the blunt one that makes the others nearly moot: **no URL-bearing element is
ever created**, whether named by a literal or computed. With no `<a>`, `<img>` or
`<iframe>` in existence there is nothing for a URL to be assigned to.

Each of those reads the namespaced form too, and it took a review to notice that
it did not: `.setAttribute(` does not match `.setAttributeNS(`, so a single
missing letter pair moved a call out of the check's sight entirely. The
namespaced calls take the name one argument later, which is why the patterns
below carry the extra `[^,]*,` rather than a shared prefix -- and why a
namespaced call whose arguments this file cannot parse is **flagged** rather than
skipped. A rule that quietly stops matching is the failure this whole file is
about.

The first version of this gate, shipped earlier the same day, enforced only that
the attribute *name* be a literal, and `docs/THREAT-MODEL.md` justified
sufficiency with "every `setAttribute` passes a constant name, so there is no
URL-bearing attribute to poison". That is an observation about the attributes
that happen to be used, not a property the check enforced:
`setAttribute("href", row.title)` names a constant and would have passed. The
weaker version was correct about the tree in front of it and wrong about what it
guaranteed, which is the distinction this whole file exists to hold.

**What remains outside the check**, and is therefore residual risk 7 rather than
a claim: no document-derived URL crosses the boundary at all, because
`outline.rs` refuses `/URI`, `/Launch` and `/GoToR` and reports them as
`Target::Refused { action }` whose string is one of five literals we choose. That
is enforced in Rust --- `outline.rs`'s `no_target_variant_may_carry_a_url` fails
to *compile* if a variant is added --- but nothing links the two, so a Rust
change cannot turn this check red. Read the two together.

## Why a script and not a vitest test

Reading the source tree needs `node:fs`, and this project deliberately ships no
Node type declarations (see the comment in `vite.config.ts`). A gate script costs
nothing and keeps `npm run check` honest.

Usage:
    scripts/check_webview_sinks.py
"""

import re
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
#
# The marker counts on the flagged line **or on the line immediately above it**.
# The one exemption in the tree today needs three lines to say why it is safe,
# and a justification that has to fit on the end of the line it justifies gets
# written as "safe" -- which is not a reason. The cost is that a marker can drift
# off its line, so an exemption that matched nothing is reported below rather
# than being silently carried: an allowlist entry naming something that no longer
# exists is how an allowlist rots.
EXEMPT = "webview-sink-ok:"

# `setAttribute` calls, of both spellings. Dotted, so this matches a *call* and
# not the method definition in `testdom.ts`. Used for the population count; the
# rules below are what decide whether a call is a finding.
ATTRIBUTE_CALL = re.compile(r"\.setAttribute(?:NS)?\(")

# `createElement` calls, of both spellings, likewise for the population count.
ELEMENT_CALL = re.compile(r"createElement(?:NS)?\(")

# Attributes whose *value* is a URL or a script, so a literal name is no defence.
#
# The first version of this gate required only that the attribute **name** be a
# literal, and the threat model justified sufficiency with "every setAttribute
# passes a constant name, so there is no URL-bearing attribute to poison". That
# is a statement about the attributes that happen to be used, not one the check
# enforced --- `setAttribute("href", row.title)` names a constant and would have
# passed. Closed 2026-08-02, hours after shipping the weaker version.
DANGEROUS_ATTRS = r"href|src|srcdoc|action|formaction|xlink:href|data|ping|on\w+"

# Elements that carry a URL or execute. The frontend creates none of them, which
# is a stronger invariant than policing their attributes: with no `<a>`, `<img>`
# or `<iframe>` in existence there is nothing for a URL to be assigned *to*.
URL_ELEMENTS = r"a|iframe|img|object|embed|form|script|link|base|source|track|video|audio"

# DOM properties that navigate or execute when assigned. Lowercase and exact, on
# purpose: `this.onChange = onChange` in `search.ts` is an ordinary field, and a
# rule matching `.on[a-z]+` in any case flags it. Real DOM handlers are all
# lowercase, so case-sensitivity is what separates the two without an allowlist.
URL_PROPS = (
    r"href|src|srcdoc|action|formaction|"
    r"onclick|onload|onerror|onmouseover|onmouseenter|onfocus|onblur|onsubmit|"
    r"onchange|oninput|onkeydown|onkeyup|onkeypress|onauxclick|onpointerdown"
)

# The namespaced calls put the name one argument later --- `setAttributeNS(ns,
# name, value)`, `createElementNS(ns, tag)` --- so each rule below carries a
# second alternative with `[^,]*,` for that first argument. The negative
# lookahead is placed so that a namespaced call whose arguments do not fit that
# simple shape (a namespace expression containing a comma, say) is **flagged as
# computed** rather than falling through unmatched.
ATTR_NAME = r"\.setAttribute\(\s*|\.setAttributeNS\(\s*[^,]*,\s*"
TAG_NAME = r"createElement\(\s*|createElementNS\(\s*[^,]*,\s*"

RULES = [
    (
        "setAttribute(<computed name>)",
        re.compile(r"\.setAttribute\(\s*(?![\"'])|\.setAttributeNS\((?![^,]*,\s*[\"'])"),
    ),
    (
        "setAttribute(<url/script attribute>)",
        re.compile(rf"(?:{ATTR_NAME})[\"'](?:{DANGEROUS_ATTRS})[\"']", re.I),
    ),
    (
        "createElement(<url-bearing element>)",
        re.compile(rf"(?:{TAG_NAME})[\"'](?:{URL_ELEMENTS})[\"']", re.I),
    ),
    (
        # The one rule with an exemption in the tree. `a11y.ts` builds a heading
        # or a paragraph from the document's own structure tag, through a total
        # whitelist --- which is safe, and is exactly the shape this rule cannot
        # decide for itself, since the string it is handed comes from a PDF.
        "createElement(<computed name>)",
        re.compile(r"createElement\(\s*(?![\"'])|createElementNS\((?![^,]*,\s*[\"'])"),
    ),
    (
        "assignment to a navigating property",
        re.compile(rf"\.(?:{URL_PROPS})\s*=(?!=)"),
    ),
]


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
    element_calls = 0
    idle_markers: "list[str]" = []

    for path in files:
        # Bytes, then UTF-8: `read_text()` decodes with the locale codec, which
        # is cp1252 on Windows, and this tree holds characters it cannot read.
        # That is the trap that stopped `mutate_rust.py` running on Windows at
        # all, and it is one line away from repeating here.
        text = path.read_bytes().decode("utf-8")
        rel = path.relative_to(REPO).as_posix()
        lines = text.splitlines()
        lines_scanned += len(lines)

        flagged: "dict[int, list[str]]" = {}
        markers: "dict[int, str]" = {}

        for number, line in enumerate(lines, start=1):
            found = [s for s in SINKS if s in line]

            # The rules that keep the sink list *sufficient*. A string that
            # cannot become markup can still become a navigation or a script if
            # it reaches `href`, `src` or an event handler, by any of the three
            # routes those have: a computed attribute name, a dangerous literal
            # one, or a direct property assignment. The element rules are the
            # blunt ones and the strongest --- with no `<a>` or `<iframe>` ever
            # created, by a literal name or a computed one, there is nothing for
            # a URL to be assigned to.
            attribute_calls += len(ATTRIBUTE_CALL.findall(line))
            element_calls += len(ELEMENT_CALL.findall(line))
            found += [name for name, pattern in RULES if pattern.search(line)]

            if found:
                flagged[number] = found
            if EXEMPT in line:
                markers[number] = line

        # A marker exempts its own line or the one below it. Both directions are
        # then checked: a finding with no marker is a hit, and a marker with no
        # finding is reported as idle.
        for number, found in sorted(flagged.items()):
            if number in markers or number - 1 in markers:
                exempted += len(found)
                continue
            for sink in found:
                hits.append(f"{rel}:{number}: {sink}  |  {lines[number - 1].strip()[:100]}")

        for number, line in sorted(markers.items()):
            if number not in flagged and number + 1 not in flagged:
                idle_markers.append(f"{rel}:{number}: {line.strip()[:100]}")

    # Print the population before the verdict, so a run that scanned almost
    # nothing cannot read as a clean bill.
    print(
        f"scanned {len(files)} files, {lines_scanned} lines, "
        f"{len(SINKS)} sink patterns, {len(RULES)} rules, "
        f"{attribute_calls} setAttribute call(s), "
        f"{element_calls} createElement call(s), "
        f"{exempted} exemption(s) honoured",
        flush=True,
    )

    # These two rules are the ones that can silently stop covering anything:
    # each fires only on a call, so a refactor that renames the helper or moves
    # the DOM building elsewhere leaves them scanning a pattern that no longer
    # occurs, reporting the same clean bill. `AGENTS.md`: a structural audit must
    # report the population it found, and a zero means unexamined rather than
    # clean.
    populations = (("setAttribute", attribute_calls), ("createElement", element_calls))
    for call, population in populations:
        if population == 0:
            print(
                f"[FAIL] no `{call}(` call found anywhere in the frontend. The rules "
                "keyed on it\n       now cover nothing -- either the DOM is built some "
                "other way, or this check\n       needs re-aiming. A pattern that does "
                "not occur passes exactly like a clean one.",
                file=sys.stderr,
            )
            return 1

    # An exemption that no longer sits beside a finding. Not a failure -- the
    # tree is still clean, which is what this gate is for -- but a marker nobody
    # re-reads is how an allowlist rots into a blanket permission, so it is named
    # rather than carried.
    for marker in idle_markers:
        print(f"[WARN] exemption marks no finding: {marker}", file=sys.stderr)

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
