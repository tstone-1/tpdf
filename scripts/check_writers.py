#!/usr/bin/env python3
"""Every registered command that writes a file is named in the threat model's §3 list.

`docs/THREAT-MODEL.md` §3 is the one place that answers *how many ways can the
webview cause a write*, and it went wrong three times in two weeks --- always in
the direction that under-claims, and never in a way anything could report. The
row said "two", then "four", then "six" against a list of five, while the true
count was eight: `split_document`, `redact_copy` and `redact_document` were each
disclosed in their own §T6 entry and absent from the summary.

That section's own instruction --- *the list is the claim and the number follows
it* --- is a rule with nobody applying it, which is the shape `docs/TRAPS.md`
calls a rule you wrote down and do not enforce. This is the enforcement.

**The set is derived from the terminal writers, not from the command names.** A
command writes a file exactly when it reaches one of the six functions in
`save.rs` that create or replace one. Keying on the callers would be a list
maintained by hand beside a list maintained by hand; keying on the callee is a
property of the code.

**Both directions, and a control.** A command reaching a writer and missing from
the list is the failure that happened; a name in the list reaching no writer is
the mirror, and is what a rename leaves behind. The control is that every
terminal name still exists in `save.rs`: a renamed helper would otherwise make
the scan select nothing, and an empty set agreeing with an empty set is the
`docs/TRAPS.md` entry about a query whose predicate the data does not use.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The functions in `save.rs` that create or replace a file. Anything reaching one
# of these has written to a path; anything reaching none of them has not.
TERMINAL = [
    "write_copy",
    "write_split",
    "write_merged",
    "commit_in_place",
    "append_in_place",
    "print_bytes",
]

NUMBERS = {
    2: "two", 3: "three", 4: "four", 5: "five", 6: "six",
    7: "seven", 8: "eight", 9: "nine", 10: "ten", 11: "eleven", 12: "twelve",
}


def say(ok: bool, text: str) -> bool:
    print(f"[{'OK' if ok else 'FAIL'}]   {text}")
    return ok


def commands_that_write(source: str) -> set[str]:
    """Every `#[tauri::command]` function whose body reaches a terminal writer."""
    found = set()
    for match in re.finditer(r"#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", source):
        name = match.group(1)
        # The body runs to the first line that is a bare closing brace, which is
        # how every function in this file ends.
        rest = source[match.end():]
        end = re.search(r"^\}", rest, re.M)
        body = rest[: end.start()] if end else rest
        if any(f"save::{terminal}" in body for terminal in TERMINAL):
            found.add(name)
    return found


def main() -> int:
    lib = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")
    save = (ROOT / "src-tauri" / "src" / "save.rs").read_text(encoding="utf-8")
    model = (ROOT / "docs" / "THREAT-MODEL.md").read_text(encoding="utf-8")

    ok = True

    # The control, first: a renamed terminal makes every set below empty, and two
    # empty sets agree.
    missing = [t for t in TERMINAL if f"fn {t}" not in save]
    ok &= say(
        not missing,
        f"all {len(TERMINAL)} terminal writers still exist in save.rs"
        + (f" -- missing {missing}" if missing else ""),
    )
    if missing:
        return 1

    marker = re.search(r"<!--\s*writers:\s*([^>]*?)-->", model)
    if not marker:
        print("[FAIL]   docs/THREAT-MODEL.md has no <!-- writers: ... --> marker")
        return 1
    listed = set(marker.group(1).split())
    writes = commands_that_write(lib)

    ok &= say(len(writes) > 0, f"{len(writes)} registered command(s) reach a writer")

    unlisted = sorted(writes - listed)
    ok &= say(
        not unlisted,
        "every command that writes a file is in the §3 list"
        + (f" -- {unlisted} are not" if unlisted else ""),
    )
    stale = sorted(listed - writes)
    ok &= say(
        not stale,
        "every name in the §3 list reaches a writer"
        + (f" -- {stale} do not" if stale else ""),
    )

    # The count in the boundary row, which is the number that went stale twice.
    row = re.search(r"\*\*Webview\*\* \(Svelte\).*?--- (\w+) of which write files", model)
    if not row:
        print("[FAIL]   the §3 boundary row no longer states a count of writing commands")
        return 1
    want = NUMBERS.get(len(writes), str(len(writes)))
    ok &= say(
        row.group(1) == want,
        f"the §3 boundary row says '{row.group(1)}' and there are {len(writes)}",
    )

    if ok:
        print(f"[OK] the §3 writer list and the registry agree on all {len(writes)}.")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
