#!/usr/bin/env python3
"""Every platform arm names its platforms, and every Linux difference is listed.

tpdf builds for macOS, Windows and Linux, and `worker.rs` refuses any other OS
at compile time. Two things follow, and this checks both.

**1. No arm is written as "not these".** Linux arrived on 2026-10-14 into a tree
whose arms said `not(any(target_os = "macos", windows))`, meaning "a platform tpdf
does not support". Linux took every one of them without anybody deciding it:
installed fonts were never found (`sysfont.rs`), "Show in folder" answered
"supported on Windows and macOS" (`commands/document.rs`), and `tpdf path`
pointed a Linux reader at a macOS menu command. A negated platform is a decision
about platforms nobody has listed, so it is refused. And with the set closed at
three, an arm naming all three is always true and decides nothing, so that is
refused too.

What is checked, in every `.rs` file under `src-tauri/src`, tests and probes
included (a test arm that a platform silently takes is the same defect):

- the predicate of every `#[cfg(...)]`, `#![cfg(...)]`, `cfg!(...)` and the
  first argument of `#[cfg_attr(...)]`;
- inside it, every `not(...)` whose argument names a platform (`target_os`,
  `target_family`, `windows`, `unix`) is refused;
- every `any(...)` that covers macOS, Windows and Linux (`unix` counts as macOS
  and Linux) is refused.

**The one exception is the guard itself**: the `not(any(macos, linux, windows))`
directly above `compile_error!` in `worker.rs`. Exactly one must exist, which is
the emptiness control for this half --- a scanner that read nothing would find
no guard. A second control asks that the scan saw at least `MIN_LINUX_ARMS`
explicit `target_os = "linux"` predicates.

What is not checked: CPU architecture (`target_arch`), `debug_assertions`,
`test` and features, which are not platforms. A platform named in a string
(`windows_subsystem = "windows"`) is not a predicate and is not read.

**2. The README's list of what differs on Linux names its code, both ways.**
Each item between `<!-- linux-differences` and `<!-- /linux-differences -->`
carries `<!-- linux-difference: <key> -->`, and the code it describes carries
`linux-difference: <key>` in a comment, in `src-tauri/src` or `src/lib`. The two
sets of keys must be equal: an item whose code is gone, or a difference in code
the README does not mention, fails.

And because a marker only helps where someone remembered to write it, the half
that can catch a forgotten one: a string literal that says a thing works on
macOS or Windows (`supported`, `available` or `implemented` ... `on` a platform,
or a platform `only`) without saying Linux fails, unless a `linux-difference:`
marker is within `MARKER_REACH` lines above it. That is the sentence Linux was
shown for "Show in folder" until 2026-10-15, and the planted self-test holds it.

Run with `--self-test` to check the planted cases before the tree.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST = ROOT / "src-tauri" / "src"
TS = ROOT / "src" / "lib"
README = ROOT / "README.md"

#: How many explicit Linux predicates a healthy tree has at least. 2026-10-15
#: counted well over a hundred; a scanner that stops reading falls far below.
MIN_LINUX_ARMS = 40
#: How far above a refusal sentence its marker may sit, in lines.
MARKER_REACH = 6

PLATFORMS = {"macos", "windows", "linux"}
HEADS = re.compile(r"#!?\[cfg\(|#!?\[cfg_attr\(|\bcfg!\(")
MARKER = re.compile(r"linux-difference:\s*([a-z0-9-]+)")
BLOCK = re.compile(r"<!-- linux-differences\b.*?-->(.*?)<!-- /linux-differences -->", re.S)
STRING = re.compile(r'"((?:[^"\\]|\\.)*)"', re.S)
REFUSAL = re.compile(
    r"\b(?:supported|available|implemented)\b[^.;]{0,40}?\bon\b[^.;]{0,20}?\b(?:macOS|Windows)\b"
    r"|\b(?:macOS|Windows)\b(?:\s+(?:and|or)\s+(?:macOS|Windows))?\s+only\b"
)


def balanced(text: str, start: int) -> int:
    """Index just past the `)` closing the `(` at `start - 1`, skipping strings."""
    depth, i = 1, start
    while i < len(text) and depth:
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        i += 1
    return i


def predicates(text: str) -> list[tuple[int, str, int]]:
    """Each cfg predicate in `text`: (offset, predicate, offset past the attribute)."""
    out = []
    for head in HEADS.finditer(text):
        start = head.end()
        end = balanced(text, start)
        inner = text[start:end - 1]
        if "cfg_attr" in head.group():
            inner = split_first(inner)
        out.append((head.start(), inner, end))
    return out


def split_first(inner: str) -> str:
    """The first top-level argument of `cfg_attr(PRED, ...)`."""
    depth = 0
    for i, c in enumerate(inner):
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        elif c == "," and depth == 0:
            return inner[:i]
    return inner


def platforms_in(pred: str) -> set[str]:
    """The platforms a predicate's atoms name, `unix` read as macOS and Linux."""
    named = set(re.findall(r'target_os\s*=\s*"(\w+)"', pred))
    pred_bare = re.sub(r'"[^"]*"', '""', pred)
    if re.search(r"\bwindows\b", pred_bare) or re.search(r'target_family\s*=\s*"windows"', pred):
        named.add("windows")
    if re.search(r"\bunix\b", pred_bare) or re.search(r'target_family\s*=\s*"unix"', pred):
        named |= {"macos", "linux"}
    return named


def calls(pred: str, name: str) -> list[str]:
    """The argument text of every `name(...)` inside a predicate."""
    out = []
    for m in re.finditer(rf"\b{name}\(", pred):
        out.append(pred[m.end():balanced(pred, m.end()) - 1])
    return out


def arm_problems(path: str, text: str) -> tuple[list[str], int, int]:
    """(problems, guards found, explicit Linux predicates seen) for one file."""
    problems, guards, linux = [], 0, 0
    for offset, pred, end in predicates(text):
        line = text.count("\n", 0, offset) + 1
        linux += len(re.findall(r'target_os\s*=\s*"linux"', pred))
        negated = [arg for arg in calls(pred, "not") if platforms_in(arg)]
        if negated:
            guard = (
                platforms_in(negated[0]) == PLATFORMS
                and text[end:].lstrip().startswith("]")
                and text[end:].lstrip()[1:].lstrip().startswith("compile_error!")
            )
            if guard and len(negated) == 1:
                guards += 1
            else:
                problems.append(
                    f"{path}:{line}: `not({negated[0].strip()})` -- name the platforms "
                    "the arm is for instead"
                )
        # An `any` inside a `not` is that negation's argument, judged above.
        outside = pred
        for arg in calls(pred, "not"):
            outside = outside.replace(f"not({arg})", "not()")
        for arg in calls(outside, "any"):
            if platforms_in(arg) >= PLATFORMS:
                problems.append(
                    f"{path}:{line}: `any({arg.strip()})` covers every platform tpdf builds "
                    "for, so the cfg decides nothing -- remove it"
                )
    return problems, guards, linux


def code_only(text: str) -> str:
    """`text` with every comment blanked to spaces, lines kept where they were.

    A comment quoting the old sentence is history, not a sentence a reader is
    shown. `//` inside a string literal (a URL) is not a comment.
    """
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append(re.sub(r"[^\n]", " ", text[i:j]))
            i = j
        else:
            out.append(c)
            i += 1
    return "".join(out)


def refusal_problems(path: str, text: str) -> list[str]:
    """String literals saying a thing works on macOS or Windows, not Linux, unmarked."""
    lines = text.split("\n")
    problems = []
    for m in STRING.finditer(code_only(text)):
        body = m.group(1)
        if "Linux" in body or not REFUSAL.search(body):
            continue
        line = text.count("\n", 0, m.start()) + 1
        above = lines[max(0, line - 1 - MARKER_REACH):line]
        if not any(MARKER.search(l) for l in above):
            problems.append(
                f"{path}:{line}: \"{body[:70]}\" says where this works and leaves Linux out -- "
                "make it work on Linux, or list it in the README's Linux differences and "
                "mark it `linux-difference: <key>`"
            )
    return problems


def readme_keys(text: str) -> list[str] | None:
    """The keys of the README's Linux list, or None when the list is not found."""
    block = BLOCK.search(text)
    return None if block is None else MARKER.findall(block.group(1))


def code_keys() -> dict[str, list[str]]:
    """Every `linux-difference:` marker in code, key to where."""
    found: dict[str, list[str]] = {}
    files = sorted(RUST.rglob("*.rs")) + sorted(p for p in TS.rglob("*.ts") if not p.name.endswith(".test.ts"))
    for path in files:
        for n, line in enumerate(path.read_text(encoding="utf-8").split("\n"), 1):
            for key in MARKER.findall(line):
                found.setdefault(key, []).append(f"{path.relative_to(ROOT)}:{n}")
    return found


def check_tree() -> list[str]:
    problems: list[str] = []
    guards = linux = 0
    for path in sorted(RUST.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        rel = str(path.relative_to(ROOT))
        found, g, l = arm_problems(rel, text)
        problems += found
        guards += g
        linux += l
        problems += refusal_problems(rel, text)
    if guards != 1:
        problems.append(
            f"found {guards} compile_error! guard(s) refusing other platforms, expected exactly 1 "
            "(worker.rs) -- the scan read nothing, or the guard moved"
        )
    if linux < MIN_LINUX_ARMS:
        problems.append(
            f"saw {linux} explicit target_os = \"linux\" predicate(s), expected at least "
            f"{MIN_LINUX_ARMS} -- the scan is not reading the arms"
        )
    listed = readme_keys(README.read_text(encoding="utf-8"))
    if listed is None:
        problems.append("README.md has no <!-- linux-differences --> ... <!-- /linux-differences --> list")
        listed = []
    if not listed:
        problems.append("the README's Linux differences carry no linux-difference keys -- the scan found nothing")
    marked = code_keys()
    for key in sorted(set(listed) - set(marked)):
        problems.append(f"README lists the Linux difference `{key}` and no code is marked with it")
    for key in sorted(set(marked) - set(listed)):
        problems.append(f"code marks the Linux difference `{key}` ({', '.join(marked[key])}) and the README does not list it")
    for key in sorted({k for k in listed if listed.count(k) > 1}):
        problems.append(f"README lists the Linux difference `{key}` twice")
    if not problems:
        print(
            f"[OK]   every platform arm names its platforms ({linux} explicit Linux predicates, "
            "one compile-time guard)"
        )
        print(f"[OK]   the README's {len(listed)} Linux differences and the code's markers agree")
    return problems


# Planted cases: (what, Rust text, how many arm problems, how many refusal problems, guards).
PLANTED = [
    ("a catch-all Linux silently takes", '#[cfg(not(any(target_os = "macos", windows)))]\nfn f() {}', 1, 0, 0),
    ("a negated single platform", "#[cfg(not(windows))]\nfn f() {}", 1, 0, 0),
    ("a negation inside all()", '#[cfg(all(test, not(target_os = "linux")))]\nfn f() {}', 1, 0, 0),
    ("a negation in cfg_attr's predicate", "#[cfg_attr(not(unix), allow(dead_code))]\nfn f() {}", 1, 0, 0),
    ("a negation in cfg!", 'fn f() -> bool { cfg!(not(target_os = "macos")) }', 1, 0, 0),
    ("an arm naming all three", '#[cfg(any(target_os = "macos", windows, target_os = "linux"))]\nfn f() {}', 1, 0, 0),
    ("an arm naming all three through unix", "#[cfg(any(unix, windows))]\nfn f() {}", 1, 0, 0),
    ("the guard", '#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]\ncompile_error!("x");', 0, 0, 1),
    ("a dead arm shaped like the guard", '#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]\nfn f() {}', 1, 0, 0),
    ("explicit arms", '#[cfg(target_os = "linux")]\nfn f() {}\n#[cfg(any(windows, target_os = "linux"))]\nfn g() {}\n#[cfg(unix)]\nfn h() {}', 0, 0, 0),
    ("not a platform", '#[cfg(not(test))]\nfn f() {}\n#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]', 0, 0, 0),
    ("the sentence Linux was shown for Show in folder", 'fn f() -> &str { "Showing a file is supported on Windows and macOS" }', 0, 1, 0),
    ("a platform only", 'fn f() -> &str { "pre-spawned workers are macOS and Windows only" }', 0, 1, 0),
    ("one that names Linux", 'fn f() -> &str { "available on macOS, Windows and Linux" }', 0, 0, 0),
    ("a marked difference", '// linux-difference: x\nfn f() -> &str { "implemented on macOS only" }', 0, 0, 0),
    ("a marker too far above", "// linux-difference: x\n" + "\n" * 8 + 'fn f() -> &str { "implemented on macOS only" }', 0, 1, 0),
]


def self_test() -> list[str]:
    failures = []
    for what, text, arms, refusals, guards in PLANTED:
        found, g, _ = arm_problems("planted.rs", text)
        said = refusal_problems("planted.rs", text)
        if (len(found), len(said), g) != (arms, refusals, guards):
            failures.append(
                f"self-test `{what}`: {len(found)} arm, {len(said)} refusal problem(s), {g} guard(s); "
                f"expected {arms}, {refusals}, {guards}"
            )
    block = "x\n<!-- linux-differences: y -->\n- a <!-- linux-difference: one -->\n<!-- /linux-differences -->\n"
    if readme_keys(block) != ["one"]:
        failures.append(f"self-test README list: read {readme_keys(block)!r}, expected ['one']")
    if readme_keys("no list here") is not None:
        failures.append("self-test README list: a README with no list read as one")
    if not failures:
        print(f"[OK]   self-test: {len(PLANTED)} planted cases and the README list reader")
    return failures


def main() -> int:
    problems = self_test()
    if "--self-test" not in sys.argv:
        problems += check_tree()
    for line in problems:
        print(f"[FAIL] {line}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
