#!/usr/bin/env python3
"""Asserts that BUILD.md's indexes of docs/build/ name every section there.

`BUILD.md` reached 702,438 characters on 2026-10-23, three quarters of it records and
write-ups that matter once you are in their area: the record of each engine pin and
each release, the PDFium compatibility probes, and one section per probe, bench and
window check. They moved verbatim into `docs/build/`, and `BUILD.md` kept the
procedures and, where a file holds many sections, a list of their titles. A list
that has lost a title answers "is there a section about this?" with a confident
no, which is worse than no list, and nothing compared the two.

The rule. A line `<!-- build-index: docs/build/<file>.md -->` in `BUILD.md` is
followed by one `- ` bullet per `### ` heading of that file, the title verbatim and
in the file's order, up to the first line that is not a bullet. Headings inside
fenced code are not headings. And every file in `docs/build/` is linked from
`BUILD.md`, so a corpus file nothing points at is a failure rather than a file
nobody finds.

Refusals besides the diff, because a scan that examined nothing reports no
difference and so does a clean one: no marker, a marker naming a missing file, a
file with no headings, a list with no bullets, or no file in `docs/build/`.

Usage:
    scripts/check_build_index.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUILD = ROOT / "BUILD.md"
CORPUS = ROOT / "docs" / "build"
MARKER = re.compile(r"^<!-- build-index: (docs/build/[A-Za-z0-9._-]+\.md) -->$")


def headings(text: str) -> list[str]:
    """The `### ` titles of a Markdown file, outside fenced code."""
    out, fence = [], False
    for line in text.split("\n"):
        if line.startswith("```"):
            fence = not fence
        elif not fence and line.startswith("### "):
            out.append(line[4:])
    return out


def indexes(text: str) -> dict[str, list[str]]:
    """Each marker's file and the bullets that follow it."""
    lines = text.split("\n")
    found: dict[str, list[str]] = {}
    for at, line in enumerate(lines):
        m = MARKER.match(line)
        if not m:
            continue
        bullets = []
        for later in lines[at + 1:]:
            if not later.startswith("- "):
                break
            bullets.append(later[2:])
        if m.group(1) in found:
            found[m.group(1) + " (again)"] = bullets
        else:
            found[m.group(1)] = bullets
    return found


def check(build: str, corpus: dict[str, str]) -> list[str]:
    """Every finding, given BUILD.md's text and docs/build/'s files by repository path."""
    wrong = []
    if not corpus:
        return ["docs/build/ holds no file, so nothing was compared"]
    found = indexes(build)
    if not found:
        wrong.append("BUILD.md has no `<!-- build-index: ... -->` line, so no list was compared")
    for rel, bullets in found.items():
        if rel.endswith(" (again)"):
            wrong.append(f"BUILD.md indexes {rel[:-8]} twice")
            continue
        if rel not in corpus:
            wrong.append(f"BUILD.md indexes {rel}, which does not exist")
            continue
        titles = headings(corpus[rel])
        if not titles:
            wrong.append(f"{rel} has no `### ` heading, so its index compared nothing")
        if not bullets:
            wrong.append(f"BUILD.md's index of {rel} has no bullet")
        for t in titles:
            if t not in bullets:
                wrong.append(f"{rel}: `### {t}` is missing from BUILD.md's index")
        for b in bullets:
            if b not in titles:
                wrong.append(f"BUILD.md's index of {rel} names `{b}`, which is no heading there")
        if not wrong and bullets != titles:
            wrong.append(f"BUILD.md's index of {rel} lists its titles in another order or more than once")
    for rel in corpus:
        if f"]({rel})" not in build:
            wrong.append(f"{rel} is linked from nowhere in BUILD.md")
    return wrong


def self_test() -> int:
    corpus = {"docs/build/a.md": "# A\n\n### One\n\ntext\n\n```\n### not a heading\n```\n\n### Two\n",
              "docs/build/b.md": "# B\n\nrecords\n"}
    good = ("[a](docs/build/a.md) [b](docs/build/b.md)\n\n<!-- build-index: docs/build/a.md -->\n"
            "- One\n- Two\n\nmore\n")
    cases = [
        ("the index as it should be passes", good, corpus, False),
        ("a missing title fails", good.replace("- Two\n", ""), corpus, True),
        ("a title that is no heading fails", good.replace("- Two\n", "- Two\n- Three\n"), corpus, True),
        ("another order fails", good.replace("- One\n- Two\n", "- Two\n- One\n"), corpus, True),
        ("a duplicate bullet fails", good.replace("- Two\n", "- Two\n- Two\n"), corpus, True),
        ("a heading in fenced code is not indexed", good.replace("- Two\n", "- Two\n- not a heading\n"), corpus, True),
        ("a file nothing links fails", good.replace("[b](docs/build/b.md)", ""), corpus, True),
        ("no marker fails", good.replace("<!-- build-index: docs/build/a.md -->\n", ""), corpus, True),
        ("a marker naming a missing file fails", good.replace("a.md -->", "c.md -->"), corpus, True),
        ("an indexed file with no heading fails", good.replace("a.md -->", "b.md -->"), corpus, True),
        ("an empty corpus fails", good, {}, True),
    ]
    bad = 0
    for name, build, files, should_fail in cases:
        failed = bool(check(build, files))
        bad += failed != should_fail
        print(f"[{'OK' if failed == should_fail else 'FAIL'}] {name}")
    return 1 if bad else 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    corpus = {f"docs/build/{p.name}": p.read_text(encoding="utf-8") for p in sorted(CORPUS.glob("*.md"))}
    build = BUILD.read_text(encoding="utf-8")
    wrong = check(build, corpus)
    for line in wrong:
        print(f"[FAIL] {line}")
    if wrong:
        return 1
    listed = sum(len(b) for b in indexes(build).values())
    print(f"[OK] BUILD.md links all {len(corpus)} files of docs/build/ and lists all {listed} "
          "sections of the indexed ones, in order")
    return 0


if __name__ == "__main__":
    sys.exit(main())
