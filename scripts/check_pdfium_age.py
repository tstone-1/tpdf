#!/usr/bin/env python3
"""Says when the pinned PDFium has fallen behind the one Chrome ships.

    scripts/check_pdfium_age.py            # asks the network
    scripts/check_pdfium_age.py --self-test
    scripts/check_pdfium_age.py --output FILE          # and says what to build
    scripts/check_pdfium_age.py --repin VERSION COMMIT # writes the two pins

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

WHAT IT PROPOSES. When the pin is behind, `--output` appends `version=` and
`pdfium=` to the named file: the head of Chrome stable's branch as build 1 of
that branch, or the new head of the pinned branch as its next build. `audit.yml`
gives the two to `pdfium.yml`, which writes them with `--repin` into its own
checkout and builds a candidate, and opens an issue. Nothing here changes the
pin of the repository; `BUILD.md` has what a person does with the candidate.
`--pins` reads another file than the repository's, which is how `audit.yml`
rehearses the whole path against a pin that is known to be behind.

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


def proposal(version: str, pin: tuple[int, str], stable: tuple[int, str],
             head: str, stable_head: str | None) -> tuple[str, str] | None:
    """The version and commit to build next, or None when the pin is current.

    `stable_head` is the head of Chrome stable's branch, asked for only when
    that branch is later than the pinned one.
    """
    branch, commit = pin
    if stable[0] > branch:
        if stable_head is None:
            raise Unanswered(f"the head of chromium/{stable[0]} was not asked for")
        return f"{stable[0]}-tpdf.1", stable_head
    if head != commit:
        build = int(version.rpartition(".")[2])
        return f"{branch}-tpdf.{build + 1}", head
    return None


def repinned(text: str, version: str, commit: str) -> str:
    """`pdfium_build.json` with its version and commit replaced, and nothing else."""
    if not re.fullmatch(r"\d+-tpdf\.\d+", version) or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise Unanswered(f"{version!r} and {commit!r} are not a <branch>-tpdf.<n> version and a 40-digit commit")
    pinned(text)
    for key, value in (("version", version), ("pdfium", commit)):
        text, count = re.subn(rf'("{key}":\s*")[^"]*(")', rf"\g<1>{value}\g<2>", text)
        if count != 1:
            raise Unanswered(f"{PINS.name} has {count} lines for {key!r}")
    return text


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
    old = (8066, commit)
    stable = (8078, "156.0.8078.12")
    cases += [
        ("a later stable branch proposes its head as build 1",
         proposal("8066-tpdf.3", old, stable, commit, later) == ("8078-tpdf.1", later)),
        ("a branch that moved proposes its next build",
         proposal("8078-tpdf.1", (8078, commit), stable, later, None) == ("8078-tpdf.2", later)),
        ("a current pin proposes nothing", proposal("8078-tpdf.1", (8078, commit), stable, commit, None) is None),
        ("a repin changes the two pins and keeps the rest",
         json.loads(repinned(json.dumps({"version": "8066-tpdf.1", "pdfium": commit, "builder": "x"}, indent=2),
                             "8078-tpdf.1", later)) == {"version": "8078-tpdf.1", "pdfium": later, "builder": "x"}),
    ]
    for name, text, reader in (
        ("a repin to a branch name is refused", pins, lambda text: repinned(text, "8078-tpdf.1", "main")),
        ("a repin to a version with no build is refused", pins, lambda text: repinned(text, "8078", later)),
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
    parser.add_argument("--pins", type=Path, default=PINS, help="read or write this file, not the repository's")
    parser.add_argument("--output", type=Path, help="append version= and pdfium= of what to build to this file")
    parser.add_argument("--repin", nargs=2, metavar=("VERSION", "COMMIT"), help="write these two pins and stop")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    try:
        text = args.pins.read_text(encoding="utf-8")
        if args.repin:
            args.pins.write_text(repinned(text, *args.repin), encoding="utf-8", newline="")
            print(f"[OK] {args.pins.name} now names {args.repin[0]} at {args.repin[1][:12]}")
            return 0
        pin = pinned(text)
        stable = stable_branch(ask(STABLE))
        head = branch_head(ask(HEAD.format(branch=pin[0])))
        stable_head = branch_head(ask(HEAD.format(branch=stable[0]))) if stable[0] > pin[0] else None
        build = proposal(str(json.loads(text)["version"]), pin, stable, head, stable_head)
    except (Unanswered, OSError) as error:
        print(f"[FAIL] could not ask, so the pin is not known to be current: {error}")
        return 2
    found = findings(pin, stable, head)
    for line in found:
        print(f"[FAIL] {line}")
    if build and args.output:
        with args.output.open("a", encoding="utf-8") as out:
            out.write(f"version={build[0]}\npdfium={build[1]}\n")
    if build:
        print(f"       To build: {build[0]} at {build[1]}")
    if found:
        print("       BUILD.md has how an engine is built, published and pinned.")
        return 1
    print(f"[OK] the pin is the head of chromium/{pin[0]} ({pin[1][:12]}), and Chrome stable is {stable[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
