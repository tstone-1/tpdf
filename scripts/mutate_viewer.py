#!/usr/bin/env python3
"""Breaks the application's command wiring on purpose, one edit at a time.

`mutate_rust.py` and `mutate_frontend.py` drive `cargo test` and `vitest`. The
command list and the window shortcuts are covered by neither: they are asserted
by `viewer_check.py`, which needs a built bundle and a real webview. So this is
the third harness, and it exists for the same reason as the other two -- a check
that has only ever passed looks exactly like one that cannot fail, and the
command list was covered by nothing at all until 2026-08-01.

It inherits the properties `AGENTS.md` records the absence of:

**A run with no summary line is a broken run, not a survivor.** A crash, a
timeout, a bad mutation that fails to build and an occluded window all produce
no `[FAIL]` lines, which is exactly what a surviving mutation looks like.

**The failure count is derived two ways** -- by counting `[FAIL]` lines and by
reading the summary's own arithmetic -- and a disagreement is reported as a
broken run rather than as either answer.

**It refuses to start if a mutation names a check the suite does not define.**
The names come from a clean baseline run, and a name that cannot go red reports
SURVIVED and reads as a gap in the checks rather than as a typo here.

**Expected names are matched as a prefix of the line after the marker**, never
by slicing the padded name column. `viewercheck.ts` pads names to 40 and a name
*longer* than that is followed by a single space, so a column parse silently
stops matching the day a check gets a long name -- which has happened here once
already, in the direction that looks like good news.

**Files are restored by bytes and verified by digest**, not by `write_text`,
whose locale codec rewrites every line ending on Windows and compares equal
afterwards because both sides read through the same translation.

Usage:
    scripts/mutate_viewer.py                     # every mutation
    scripts/mutate_viewer.py --list
    scripts/mutate_viewer.py --only "Cmd-K"      # substring of the name
"""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf"
FIXTURE = ROOT / "testdata/outline-simple.pdf"


@dataclass(frozen=True)
class Mutation:
    """One edit, and the check whose job it is to notice."""

    name: str
    path: str
    before: str
    after: str
    expect: str


MUTATIONS = [
    Mutation(
        "Cmd-K reaches no arm at all",
        "src/lib/appcommands.ts",
        '  if ((event.metaKey || event.ctrlKey) && event.key === "k") {',
        "  if (false) {",
        "Cmd-K opens the palette",
    ),
    Mutation(
        "Cmd-K needs no modifier",
        "src/lib/appcommands.ts",
        '  if ((event.metaKey || event.ctrlKey) && event.key === "k") {',
        '  if (event.key === "k") {',
        "a bare k does not open the palette",
    ),
    Mutation(
        "anticlockwise turns clockwise",
        "src/lib/appcommands.ts",
        "      run: () => actions.viewer()?.rotateBy(-1),",
        "      run: () => actions.viewer()?.rotateBy(1),",
        "view.rotateCounterClockwise runs from the palette",
    ),
    Mutation(
        "a command leaves the list",
        "src/lib/appcommands.ts",
        """    {
      id: "view.showThumbnails",
      title: "Show page thumbnails",
      enabled: withDocument,
      run: () => actions.showTab("pages"),
    },
""",
        "",
        "every registered command is classified",
    ),
    Mutation(
        "print is wired to the open dialog",
        "src/lib/appcommands.ts",
        "      run: () => actions.printDocument(),",
        "      run: () => actions.openDocument(),",
        "file.print runs from the palette",
    ),
    Mutation(
        "Zoom to ignores what was typed",
        "src/lib/appcommands.ts",
        "          if (zoom !== null) actions.viewer()?.setZoomFixed(zoom);",
        "          if (zoom !== null) actions.viewer()?.setZoomFixed(1);",
        "view.zoomTo runs from the palette",
    ),
    Mutation(
        "Cmd-F opens find with no document",
        "src/lib/appcommands.ts",
        '  } else if (matches("find.open", event) && title) {',
        '  } else if (matches("find.open", event)) {',
        "Cmd-P prints with no document open",
    ),
    Mutation(
        "Cmd-O stacks file dialogs",
        "src/lib/appcommands.ts",
        "    if (!actions.busyOpening()) actions.openDocument();",
        "    actions.openDocument();",
        "Cmd-O opens one dialog at a time",
    ),
    Mutation(
        "zoom in is offered with no document",
        "src/lib/appcommands.ts",
        """      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
      enabled: withDocument,
""",
        """      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
""",
        "with no document only Open document is offered",
    ),
    Mutation(
        "a page break is not looked across at all",
        "src-tauri/src/search.rs",
        "    let tail = carry_for(text, page, query, options);",
        "    let tail = None;",
        "a phrase is found across a page break",
    ),
    Mutation(
        "the page break is not whitespace",
        "src-tauri/src/search.rs",
        """        if index == split {
            items.push((BREAK, '\\n'));
        }
""",
        "",
        "a phrase is found across a page break",
    ),
    Mutation(
        "a hit that starts outside the selection is kept",
        "src/lib/search.ts",
        "            m.start >= startsIn.from &&",
        "            true &&",
        "a scoped search looks only inside the selection",
    ),
    Mutation(
        "a hit that ends outside the selection is kept",
        "src/lib/search.ts",
        "            m.end <= endsIn.to",
        "            true",
        "a scoped search looks only inside the selection",
    ),
    Mutation(
        "a broken pattern reports no problem",
        "src-tauri/src/search.rs",
        "            problem: Some(problem),",
        "            problem: None,",
        "a pattern that does not compile says so instead of finding nothing",
    ),
    Mutation(
        "the phase does not put the rotation back",
        "src/lib/viewercheck.ts",
        "  viewer.rotateBy(entry.rotation - viewer.rotation);",
        "",
        "leaves the viewer as the phase before it did",
    ),
]

MARKER = re.compile(r"^\[(OK|FAIL|SKIP)\]\s")
SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed")


def build() -> tuple[bool, str]:
    """Rebuilds the bundle. A mutation that will not compile is a broken run."""
    done = subprocess.run(
        ["npm", "run", "tauri", "build", "--", "--bundles", "app"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        errors="replace",
    )
    return done.returncode == 0, done.stderr[-2000:]


def run_check() -> tuple[list[str], str, str]:
    """Runs the viewer check. Returns its result lines, its stdout, and its stderr.

    Only **stdout** carries check results. `viewer_check.py` writes its own
    verdicts on the run -- `[FAIL] exit 1`, a timeout, the loaded-module audit --
    to stderr in the same `[FAIL] ` shape, and the first version of this harness
    read both streams and counted that wrapper line as an eleventh failing check.
    Every one of ten mutations then came back off by exactly one and was reported
    as a broken run. That is the cross-check working: the count from the lines
    and the count from the summary disagreed, and it refused to answer rather
    than picking the wrong one. The stream split is the fix.
    """
    done = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/viewer_check.py"),
            str(APP),
            str(FIXTURE),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        errors="replace",
    )
    lines = [line for line in done.stdout.splitlines() if MARKER.match(line)]
    return lines, done.stdout, done.stderr


def verdict(lines: list[str], text: str, stderr: str) -> tuple[set[str], str | None]:
    """Failing check names, or a reason the run cannot be read at all.

    The two counts are the point. `failed` comes from the lines and `stated`
    from the summary's own arithmetic; they answer the same question through
    different code, and a run where they disagree is thrown away rather than
    resolved in either direction.
    """
    summary = next((m for line in text.splitlines() if (m := SUMMARY.match(line))), None)
    if summary is None:
        # The wrapper's own stderr says which silence this was: a timeout, a
        # page that never ran, an occluded window.
        said = " / ".join(line for line in stderr.splitlines() if line.startswith("[FAIL]"))
        return set(), f"no summary line: the run did not finish ({said or 'nothing on stderr'})"
    failed = {line[7:] for line in lines if line.startswith("[FAIL]")}
    passed, ran = int(summary.group(1)), int(summary.group(2))
    stated = ran - passed
    if stated != len(failed):
        return set(), f"summary says {stated} failed, {len(failed)} [FAIL] lines"
    return failed, None


def caught(failures: set[str], expect: str) -> bool:
    return any(line.startswith(expect) for line in failures)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="print the pairs and stop")
    parser.add_argument("--only", default="", help="run mutations whose name contains this")
    args = parser.parse_args()

    chosen = [m for m in MUTATIONS if args.only.lower() in m.name.lower()]
    if args.list:
        for m in chosen:
            print(f"{m.name}\n    {m.path}\n    expects: {m.expect}")
        return 0
    if not chosen:
        print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    if not FIXTURE.exists():
        print(f"[FAIL] {FIXTURE} is missing --- testdata is generated, not committed")
        return 1

    print(f"Baseline: building and running {FIXTURE.name}")
    ok, err = build()
    if not ok:
        print(f"[FAIL] the unmutated tree does not build\n{err}")
        return 1
    lines, out, err = run_check()
    failures, broken = verdict(lines, out, err)
    if broken:
        print(f"[FAIL] baseline unreadable: {broken}")
        return 1
    if failures:
        print(f"[FAIL] baseline is not green: {sorted(failures)[:3]}")
        return 1

    # Refuse to start on a name that cannot go red, and on one that is ambiguous.
    # A prefix matching two checks would report the wrong one as the catcher.
    problems = []
    for m in chosen:
        hits = [line for line in lines if line[7:].startswith(m.expect)]
        if len(hits) != 1:
            problems.append(f"{m.name!r} expects {m.expect!r}, which matches {len(hits)} checks")
    if problems:
        print("[FAIL] " + "\n[FAIL] ".join(problems))
        return 1
    print(f"Baseline green: {len(lines)} check names, every expectation names exactly one\n")

    survived, unreadable = [], []
    for index, m in enumerate(chosen, 1):
        path = ROOT / m.path
        original = path.read_bytes()
        digest = hashlib.sha256(original).hexdigest()
        source = original.decode("utf-8")
        if source.count(m.before) != 1:
            print(f"[FAIL] {m.name}: its anchor appears {source.count(m.before)} times")
            unreadable.append(m.name)
            continue

        started = time.monotonic()
        path.write_bytes(source.replace(m.before, m.after).encode("utf-8"))
        try:
            built, err = build()
            if not built:
                # A mutation that will not compile is not a caught mutation: the
                # checks never ran, so they said nothing about it either way.
                print(f"[BROKEN] {m.name}: does not build\n{err[-400:]}")
                unreadable.append(m.name)
                continue
            lines, out, err = run_check()
            failures, broken = verdict(lines, out, err)
        finally:
            path.write_bytes(original)
            restored = hashlib.sha256(path.read_bytes()).hexdigest() == digest
        if not restored:
            print(f"[FAIL] {m.path} was not restored byte for byte")
            return 1

        took = time.monotonic() - started
        if broken:
            print(f"[BROKEN] {m.name}: {broken} ({took:.0f}s)")
            unreadable.append(m.name)
        elif caught(failures, m.expect):
            print(f"[CAUGHT] {index}/{len(chosen)} {m.name} -> {len(failures)} red ({took:.0f}s)")
        else:
            print(f"[SURVIVED] {index}/{len(chosen)} {m.name}")
            print(f"    expected {m.expect!r} to fail; {len(failures)} did: {sorted(failures)[:3]}")
            survived.append(m.name)

    # Restored, and rebuilt, so the tree is not left serving a mutated bundle --
    # a stale binary is the other way a later run reports a defect that is not
    # there.
    print("\nRebuilding the clean tree")
    build()
    print(
        f"\n{len(chosen) - len(survived) - len(unreadable)}/{len(chosen)} caught, "
        f"{len(survived)} survived, {len(unreadable)} unreadable"
    )
    for name in survived:
        print(f"  SURVIVED: {name}")
    for name in unreadable:
        print(f"  UNREADABLE: {name}")
    return 1 if survived or unreadable else 0


if __name__ == "__main__":
    sys.exit(main())
