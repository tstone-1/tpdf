#!/usr/bin/env python3
"""Refuses a registered command the window harness does not classify.

## Why this exists

`viewercheck.ts` already asserts it, and the assertion is a good one:

    "every registered command is classified, and every classification is
     registered"

Every command must be in the harness's `probes` list, which drives it from the
palette and reads the result, or in its `undriven` table, which names it with a
written reason for not driving it. There is no third state, so a command cannot
arrive unnoticed.

**That check has one problem and it is not the check.** `viewercheck.ts` drives
a real window: it needs an unlocked, unoccluded screen and it is run by hand.
So a command registered without a classification does not turn anything red ---
it turns red the next time somebody happens to run the harness, which may be
days later, and in the meantime every gate is green.

That is not hypothetical. `edit.editForeignMark` and `edit.replyToComment` were
registered on 2026-08-29 and left out of both tables; the classification check
was red from that moment and nothing said so, because nothing ran it. It was
found on 2026-08-30 while adding a third command to the same family --- by
grepping, not by any instrument. `docs/TRAPS.md` records the general shape: a
manual-only harness is where a born-red check survives.

## What this does

The same set comparison, statically. The command ids are string literals in two
files, so both sides can be read without running anything:

- **Registered**: `id: "..."` in `appcommands.ts`, plus the four families built
  from a table with `` id: `edit.color.${...}` ``, which are covered by the
  family prefix rather than one id at a time --- the harness classifies them the
  same way, from the same tables.
- **Classified**: `id: "..."` inside `viewercheck.ts`'s `probes` array, its
  template families, and the keys of its `undriven` table.

It is deliberately **not** a replacement for the harness's own check. This one
reads source text and can be fooled by an id built some third way; the harness
reads the live registry and cannot. What this buys is the day, not the
certainty: a missing classification goes red on the commit that causes it
instead of on whichever later day somebody runs a window check.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "src" / "lib" / "appcommands.ts"
HARNESS = ROOT / "src" / "lib" / "viewercheck.ts"

ID = re.compile(r'id:\s*"([a-z]+\.[A-Za-z0-9.]+)"')
FAMILY = re.compile(r'id:\s*`([a-z]+\.[A-Za-z0-9.]+)\$\{')
UNDRIVEN_KEY = re.compile(r'"([a-z]+\.[A-Za-z0-9.]+)":\s*"')


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def main() -> int:
    registry = read(REGISTRY)
    harness = read(HARNESS)

    # The harness's two tables sit between the `probes` literal and the check
    # that reads them. Sliced rather than scanned whole, because the file also
    # holds an actions stub full of quoted strings that are not command ids.
    start = harness.find("const probes: Probe[] = [")
    end = harness.find("const registered = registry.all()")
    if start < 0 or end <= start:
        print(
            "[FAIL] could not find viewercheck.ts's probes list -- this checker reads "
            "source text and the text moved.",
            file=sys.stderr,
        )
        return 1
    region = harness[start:end]

    registered = set(ID.findall(registry))
    reg_families = set(FAMILY.findall(registry))
    classified = set(ID.findall(region)) | set(UNDRIVEN_KEY.findall(region))
    seen_families = set(FAMILY.findall(region))

    if not registered:
        print("[FAIL] no command ids found in appcommands.ts", file=sys.stderr)
        return 1

    missing = sorted(i for i in registered if i not in classified)
    stale = sorted(
        i
        for i in classified
        if i not in registered and not any(i.startswith(f) for f in reg_families)
    )
    unfamilied = sorted(reg_families - seen_families)

    if missing or stale or unfamilied:
        print(
            f"[FAIL] {len(missing)} registered command(s) the window harness does not "
            f"classify, {len(stale)} classification(s) naming nothing registered, "
            f"{len(unfamilied)} command family built from a table and not classified.",
            file=sys.stderr,
        )
        for one in missing:
            print(f"       unclassified: {one}", file=sys.stderr)
        for one in stale:
            print(f"       stale: {one}", file=sys.stderr)
        for one in unfamilied:
            print(f"       family absent from the harness: {one}", file=sys.stderr)
        print(
            "       Put it in viewercheck.ts's `probes` with a way to drive it, or in "
            "its `undriven` table with a written reason. There is no third state.",
            file=sys.stderr,
        )
        return 1

    print(
        f"[OK] all {len(registered)} registered commands and {len(reg_families)} "
        "command families are classified by the window harness."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
