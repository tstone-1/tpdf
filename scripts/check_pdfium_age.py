#!/usr/bin/env python3
"""Says when the pinned PDFium has fallen behind the one Chrome ships.

    scripts/check_pdfium_age.py            # asks the network
    scripts/check_pdfium_age.py --self-test

WHY THIS EXISTS. PDFium has no releases of its own. Chrome cuts a branch of it
for each milestone, `chromium/<number>`, and merges fixes for that milestone
into the branch afterwards. On 2026-10-10 the pin was `chromium/8066`, Chrome
stable had been on 8078 for some days, and a fix in the rasterizer this build
compiles in had been merged into 8078 the day before. Nothing here said so; it
was found by somebody asking.

WHAT IT ASKS, and both are questions about the world, not about the commit:

  1. Is Chrome stable on a later branch than the pinned one? chromiumdash
     answers with a version, whose third number is the branch.
  2. Has the pinned branch gained commits since the pin? Its head, read from
     PDFium's repository, is then not the pinned commit. A commit merged into
     a branch Chrome already ships is how a security fix arrives.

Either is exit 1, with what to build. The pin is read from
`scripts/pdfium_build.json`, which is what the build uses.

NOT A GATE, for the reason `audit.yml` gives for itself: the answer changes
when upstream moves and not when the code does, and `gates.py` has to pass
offline. `audit.yml` runs it weekly and on a push.

WHAT THE ANSWER MEANS. Exit 0 only when both questions were answered and
neither found anything. A source that does not answer, or answers in a shape
this does not read, is exit 2 and never 0: a check that could not ask has not
found the pin current.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PINS = ROOT / "scripts" / "pdfium_build.json"
STABLE = "https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Windows&num=1"
HEAD = "https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/{branch}?format=JSON"


class Unanswered(Exception):
    """A source did not answer, or not in a shape this reads."""


def pinned(text: str) -> tuple[int, str]:
    """The branch and the commit `pdfium_build.json` names."""
    pins = json.loads(text)
    branch = re.fullmatch(r"(\d+)-tpdf\.\d+", str(pins.get("version", "")))
    commit = str(pins.get("pdfium", ""))
    if not branch or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise Unanswered(f"{PINS.name} names no <branch>-tpdf.<n> version and 40-digit commit")
    return int(branch[1]), commit


def stable_branch(text: str) -> tuple[int, str]:
    """Chrome stable's branch and its version, from chromiumdash's answer."""
    try:
        version = str(json.loads(text)[0]["version"])
    except (ValueError, LookupError, TypeError) as error:
        raise Unanswered(f"chromiumdash's answer has no version in it: {error!r}") from error
    parts = version.split(".")
    if len(parts) != 4 or not all(part.isdigit() for part in parts):
        raise Unanswered(f"chromiumdash's version {version!r} is not four numbers")
    return int(parts[2]), version


def branch_head(text: str) -> str:
    """The commit a branch ends at, from gitiles' answer."""
    # Gitiles puts `)]}'` in front of its JSON, against a page that includes it.
    body = text.split("\n", 1)[1] if text.startswith(")]}'") else text
    try:
        commit = str(json.loads(body)["commit"])
    except (ValueError, LookupError, TypeError) as error:
        raise Unanswered(f"PDFium's repository named no commit: {error!r}") from error
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise Unanswered(f"PDFium's repository named {commit!r}, which is not a commit")
    return commit


def findings(pin: tuple[int, str], stable: tuple[int, str], head: str) -> list[str]:
    """What is behind, as sentences; empty when nothing is."""
    branch, commit = pin
    found = []
    if stable[0] > branch:
        found.append(
            f"Chrome stable is {stable[1]}, on chromium/{stable[0]}; the pin is chromium/{branch}. "
            f"Build the head of chromium/{stable[0]}."
        )
    if head != commit:
        found.append(
            f"chromium/{branch} now ends at {head[:12]}; the pin is {commit[:12]}. "
            "What was merged since is what Chrome took for that milestone."
        )
    return found


def ask(url: str) -> str:
    try:
        with urllib.request.urlopen(url, timeout=30) as answer:
            return answer.read().decode("utf-8")
    except (urllib.error.URLError, TimeoutError, UnicodeDecodeError) as error:
        raise Unanswered(f"{url} did not answer: {error}") from error


def self_test() -> int:
    """The reading and the rule, on answers written out here."""
    commit, later = "a" * 40, "b" * 40
    pins = json.dumps({"version": "8078-tpdf.1", "pdfium": commit})
    dash = json.dumps([{"version": "156.0.8078.12"}])
    cases = [
        ("the pin is read", pinned(pins) == (8078, commit)),
        ("chromiumdash's branch is read", stable_branch(dash) == (8078, "156.0.8078.12")),
        ("gitiles' head is read past its prefix", branch_head(")]}'\n" + json.dumps({"commit": later})) == later),
        ("a current pin finds nothing", findings((8078, commit), (8078, "156.0.8078.12"), commit) == []),
        ("a pin ahead of stable finds nothing", findings((8086, commit), (8078, "156.0.8078.12"), commit) == []),
        ("a later stable branch is found", len(findings((8066, commit), (8078, "156.0.8078.12"), commit)) == 1),
        ("a branch that moved is found", len(findings((8078, commit), (8078, "156.0.8078.12"), later)) == 1),
        ("both are found together", len(findings((8066, commit), (8078, "156.0.8078.12"), later)) == 2),
    ]
    for name, text, reader in (
        ("a pin with no branch is not read", json.dumps({"version": "latest", "pdfium": commit}), pinned),
        ("an empty chromiumdash answer is not read", "[]", stable_branch),
        ("a version of three numbers is not read", json.dumps([{"version": "156.0.8078"}]), stable_branch),
        ("a page that is not JSON is not read", "<html>", branch_head),
        ("a head that is not a commit is not read", json.dumps({"commit": "main"}), branch_head),
    ):
        try:
            reader(text)
            cases.append((name, False))
        except Unanswered:
            cases.append((name, True))
    for name, ok in cases:
        print(f"[{'OK' if ok else 'FAIL'}] {name}")
    return 0 if all(ok for _, ok in cases) else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--self-test", action="store_true", help="check the reading and the rule, offline")
    if parser.parse_args().self_test:
        return self_test()
    try:
        pin = pinned(PINS.read_text(encoding="utf-8"))
        stable = stable_branch(ask(STABLE))
        head = branch_head(ask(HEAD.format(branch=pin[0])))
    except (Unanswered, OSError) as error:
        print(f"[FAIL] could not ask, so the pin is not known to be current: {error}")
        return 2
    found = findings(pin, stable, head)
    for line in found:
        print(f"[FAIL] {line}")
    if found:
        print("       BUILD.md has how an engine is built, published and pinned.")
        return 1
    print(f"[OK] the pin is the head of chromium/{pin[0]} ({pin[1][:12]}), and Chrome stable is {stable[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
