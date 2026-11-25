#!/usr/bin/env python3
"""Summarize dated local reviews against the files in this checkout.

    python3 scripts/review_coverage.py
    python3 scripts/review_coverage.py --record
    python3 scripts/review_coverage.py --since 2026-11-24
    python3 scripts/review_coverage.py --list remaining
    python3 scripts/review_coverage.py --list revisit

Files have equal weight. Only explicit per-path inventories earn credit. Current
coverage additionally requires a review of a clean revision with identical file
contents at the same path. Old narrative reports contribute no inferred paths.
"""

import argparse
import json
import re
import subprocess
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BLOCK = re.compile(r"<summary>([^<]+)</summary>(.*?)</details>", re.DOTALL)
PATH_ROW = re.compile(r"^- `([^`\n]+)`[ \t]*$", re.MULTILINE)
FOLDER_ROW = re.compile(r"^- `[^`\n]+/` \(([\d,]+) files?\)$", re.MULTILINE)
APP_DIRS = ("src/", "src-tauri/src/", "src-tauri/tests/", "api/python/")
APP_TYPES = {".ts", ".svelte", ".css", ".rs", ".py"}
CODE_TYPES = APP_TYPES | {".js", ".mjs", ".c", ".swift", ".ps1", ".sh", ".cmd", ".nsh", ".html"}


def paths_from_git(*args):
    output = subprocess.check_output(["git", "-C", ROOT, *args, "-z", "--"])
    return {path.decode("utf-8") for path in output.split(b"\0") if path}


def inventory(report):
    groups = {}
    for title, body in BLOCK.findall(report):
        if title.startswith("Read in full"):
            kind = "full"
        elif title.startswith("Skimmed"):
            kind = "sampled"
        else:
            continue
        count = re.search(r"\(([\d,]+)(?: files?)?\)$", title)
        paths = set(PATH_ROW.findall(body))
        # Older reports grouped some directories by count. Their membership at
        # review time is unknown, so validate those counts without crediting paths.
        grouped = sum(int(n.replace(",", "")) for n in FOLDER_ROW.findall(body))
        if kind in groups or not count or len(paths) + grouped != int(count[1].replace(",", "")):
            raise ValueError(f"paths do not match the inventory heading {title!r}")
        groups[kind] = paths
    if groups and (groups.keys() != {"full", "sampled"} or groups["full"] & groups["sampled"]):
        raise ValueError("incomplete or overlapping full and sampled inventories")
    return groups


def clean_revision(report):
    # Only the reviewed snapshot can establish current coverage. Ignore any
    # comparisons with an earlier review further down the same scope line.
    for label in ("Revision", "Scope"):
        match = re.search(rf"^\*\*{label}\*\*(.*)$", report, re.MULTILINE)
        if match:
            snapshot = re.split(r"\b(previous|prior|preceding|compared|since)\b", match[1],
                                maxsplit=1, flags=re.IGNORECASE)[0]
            commit = re.search(r"`([a-f0-9]{7,40})`", snapshot)
            dirty = re.search(r"\b(not clean|dirty|unstaged|uncommitted|modified files)\b",
                              snapshot, re.IGNORECASE)
            if commit and not dirty and re.search(r"\bclean\b", snapshot, re.IGNORECASE):
                return commit[1]
    return None


def review_history(record):
    """Merge local inventories into the portable history by report name."""
    ledger = ROOT / "docs" / "review-coverage.json"
    history = json.loads(ledger.read_text(encoding="utf-8")) if ledger.exists() else {}
    for path in sorted((ROOT / "docs/reviews").glob("????-??-??_????*codebase_deep*.md")):
        report = path.read_text(encoding="utf-8")
        try:
            groups = inventory(report)
        except ValueError as error:
            raise ValueError(f"{path.name}: {error}") from error
        entry = None
        if groups:
            entry = {"revision": clean_revision(report),
                     "full": sorted(groups["full"]), "sampled": sorted(groups["sampled"])}
        if path.name in history and history[path.name] != entry:
            raise ValueError(f"{path.name}: saved inventory differs from local report")
        history[path.name] = entry
    if record:
        from tempfile import NamedTemporaryFile
        with NamedTemporaryFile("w", encoding="utf-8", dir=ledger.parent, delete=False) as out:
            json.dump(history, out, indent=1, sort_keys=True, ensure_ascii=False)
            out.write("\n")
        Path(out.name).replace(ledger)
    return history


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--since", type=date.fromisoformat, help="include reviews from this date")
    parser.add_argument("--list", choices=["remaining", "revisit"],
                        help="list application files without a full read, or needing a fresh one")
    parser.add_argument("--record", action="store_true", help="save local inventories in the tracked coverage history")
    args = parser.parse_args()
    tracked = {p for p in paths_from_git("ls-files") if (ROOT / p).is_file()}
    app = {p for p in tracked if p.startswith(APP_DIRS) and Path(p).suffix in APP_TYPES}
    code = {p for p in tracked if Path(p).suffix in CODE_TYPES or p.startswith(".githooks/")}
    full, sampled, current = set(), set(), set()
    inventories = skipped = unknown_revision = 0
    try:
        history = review_history(args.record)
    except ValueError as error:
        parser.exit(1, f"{error}\n")
    for name, entry in history.items():
        if args.since and name[:10] < args.since.isoformat():
            continue
        if entry is None:
            skipped += 1
            continue
        inventories += 1
        full.update(entry["full"])
        sampled.update(entry["sampled"])
        revision = entry["revision"]
        if revision:
            try:
                changed = paths_from_git("diff", "--no-renames", "--name-only", revision)
            except subprocess.CalledProcessError as error:
                parser.exit(1, f"{name}: cannot compare reviewed revision {revision}: {error}\n")
            current.update(set(entry["full"]) - changed)
        else:
            unknown_revision += 1

    print(f"{inventories} dated inventories, {skipped} reports without per-file inventories. Only named files count.")
    print(f"{unknown_revision} inventories without an identifiable clean revision count only historically.")
    for label, files in [("Application and client source/tests", app),
                         ("Repository code including tooling and tests", code),
                         ("All tracked files", tracked)]:
        print(f"\n{label} ({len(files)} files)")
        for state, members in [("Fully read at least once", full), ("Full read, matching current contents", current),
                               ("Only skimmed or searched", sampled - full),
                               ("No recorded inspection", files - full - sampled)]:
            count = len(files & members)
            fraction = f"{100 * count / len(files):.1f}%" if files else "n/a"
            print(f"  {state}: {count}/{len(files)} ({fraction})")
    if args.list:
        paths = app - full if args.list == "remaining" else (app & full) - current
        print(f"\nApplication paths ({args.list}): {len(paths)}")
        for path in sorted(paths):
            print(path)


if __name__ == "__main__":
    main()
