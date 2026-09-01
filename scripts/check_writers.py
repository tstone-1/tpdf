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
`save.rs` --- and, since the 2026-09-01 split, its submodules --- that create or
replace one. Keying on the callers would be a list
maintained by hand beside a list maintained by hand; keying on the callee is a
property of the code.

**Both directions, and a control.** A command reaching a writer and missing from
the list is the failure that happened; a name in the list reaching no writer is
the mirror, and is what a rename leaves behind. The control is that every
terminal name still exists in `save.rs`: a renamed helper would otherwise make
the scan select nothing, and an empty set agreeing with an empty set is the
`docs/TRAPS.md` entry about a query whose predicate the data does not use.

**Two corrections, both made 2026-09-01, and the first is why this file's own
verdict could not be trusted.** A substring scan over a function body reads
prose as code, and it was doing so: `print_document` was classified as a writer
because its body contains the words `save::print_bytes` in a **comment**
explaining that it does not call it for a print job. Strip the comments and the
count was zero --- so the one command whose classification the gate most needed
to get right was held there by a sentence, and rewording that sentence would
have dropped it out of the set and turned *every name in the list reaches a
writer* red, for a command that had not changed. A gate passing for the wrong
reason is worse than a gate that is absent, because it is trusted.

So comments are stripped, with a scanner that tracks string literals rather than
a regex --- `//` occurs inside `https://` and a blind strip would delete whatever
followed it on that line, which is a false *negative* in a security gate.

**And the call chain is followed one level, because the property is about the
code and not about where a line is written.** Stripping the comments alone leaves
`print_document` reaching no writer, which is false: it calls `print_job`, a free
function in the same file, which calls `save::print_bytes`. Keying on where a
call is *written* rather than on what a command *reaches* means a refactor that
moves a call into a helper silently declassifies the command --- and this
repository already has that entry, from a review of this very gate. One level is
what the file's own shape needs and is where it stops: deeper needs a call graph,
and a bound stated is better than a bound discovered.
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
    # Added 2026-09-01 with the page-range print's move. `print_document` already
    # reaches `print_bytes`, so no command was misclassified without it -- but a
    # terminal writer this list does not know about is precisely the gap that
    # produced two findings that day, and the next command to reach only this one
    # would have been missed. It creates a file for the same reason
    # `print_bytes` does: a print job comes back as bytes, so it is built in a
    # scratch file this process makes.
    "print_range_bytes",
]

NUMBERS = {
    2: "two", 3: "three", 4: "four", 5: "five", 6: "six",
    7: "seven", 8: "eight", 9: "nine", 10: "ten", 11: "eleven", 12: "twelve",
}


def say(ok: bool, text: str) -> bool:
    print(f"[{'OK' if ok else 'FAIL'}]   {text}")
    return ok


def without_comments(source: str) -> str:
    """`source` with `//` comments removed and string literals left alone.

    A regex cannot do this. `//` appears inside `https://` in half the doc
    comments in `lib.rs`, and deleting from there to end of line would remove any
    real call written after it --- a false negative, which in this gate means a
    writing command reported as writing nothing.

    Block comments are not handled, deliberately: `lib.rs` has none, and a
    stripper for a construct that does not occur is a branch with no test.
    """
    out = []
    in_string = False
    escaped = False
    i = 0
    while i < len(source):
        char = source[i]
        if in_string:
            out.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            i += 1
            continue
        if char == '"':
            in_string = True
            out.append(char)
            i += 1
            continue
        if char == "/" and source.startswith("//", i):
            while i < len(source) and source[i] != "\n":
                i += 1
            continue
        out.append(char)
        i += 1
    return "".join(out)


def body_of(source: str, start: int) -> str:
    """The function body beginning at `start`, to the first bare closing brace.

    Which is how every function in `lib.rs` ends, and is the same rule this file
    has always used --- kept as one helper now that two callers need it.
    """
    rest = source[start:]
    end = re.search(r"^\}", rest, re.M)
    return rest[: end.start()] if end else rest


def free_functions(source: str) -> dict[str, str]:
    """Every non-command `fn` in the file, by name, with its body.

    The one level of indirection this gate follows. Commands are excluded: a
    command is not a helper, and including them would let one command's writing
    classify another that merely names it.
    """
    commands = {
        m.group(1)
        for m in re.finditer(
            r"#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", source
        )
    }
    bodies = {}
    for match in re.finditer(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)", source, re.M):
        name = match.group(1)
        if name not in commands:
            bodies[name] = body_of(source, match.end())
    return bodies


def commands_that_write(source: str) -> set[str]:
    """Every `#[tauri::command]` function that reaches a terminal writer.

    *Reaches*, not *contains*: comments are stripped first, and a call to a free
    function in the same file is followed one level. Both are corrections made
    2026-09-01 --- see the module docstring for what each one was hiding.
    """
    source = without_comments(source)
    helpers = free_functions(source)

    def writes(body: str, depth: int) -> bool:
        if any(f"save::{terminal}" in body for terminal in TERMINAL):
            return True
        if depth == 0:
            return False
        return any(
            re.search(rf"\b{re.escape(name)}\s*\(", body) and writes(helper, depth - 1)
            for name, helper in helpers.items()
        )

    found = set()
    for match in re.finditer(r"#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", source):
        if writes(body_of(source, match.end()), 1):
            found.add(match.group(1))
    return found


def main() -> int:
    lib = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")
    # `save.rs` and every submodule under `src/save/`, concatenated. It was one
    # file until 2026-09-01, and reading only that one would have quietly stopped
    # covering a terminal writer the moment a split moved it -- the control below
    # would still pass, on a smaller file, which is the shape this gate exists to
    # refuse. `sorted` so a failure names the same file twice in a row.
    save_files = [ROOT / "src-tauri" / "src" / "save.rs"] + sorted(
        (ROOT / "src-tauri" / "src" / "save").rglob("*.rs")
    )
    save = "\n".join(f.read_text(encoding="utf-8") for f in save_files)
    model = (ROOT / "docs" / "THREAT-MODEL.md").read_text(encoding="utf-8")

    ok = True

    # The control, first: a renamed terminal makes every set below empty, and two
    # empty sets agree.
    missing = [t for t in TERMINAL if f"fn {t}" not in save]
    ok &= say(
        not missing,
        f"all {len(TERMINAL)} terminal writers still exist across {len(save_files)}"
        " file(s) of save"
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
