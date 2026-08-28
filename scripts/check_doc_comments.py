#!/usr/bin/env python3
"""Refuses a doc comment that documents nothing.

## Why this exists

`armErase`'s doc comment ran to twelve lines and documented nothing. The crop
tool had been inserted between it and the method, so the file held two `/** */`
blocks in a row --- and only the second one binds. TypeScript accepts that in
silence: two doc comments before one declaration is legal, tooling takes the
last, and the first becomes prose sitting in the middle of a class.

What made it expensive is what the orphan said. It read *"Only drawings are
erasable ... making the eraser remove whole marks of any kind would be a second,
much more destructive command wearing the same cursor"* --- a live design
argument against the feature being built, attached to nothing, in the file where
somebody would go looking for exactly that reasoning.

It was invisible three ways at once. The **diff** that caused it shows an
insertion of a whole coherent block, which is what an intentional insertion
looks like. The **file** reads correctly top to bottom, because a detached
comment and a section header are the same characters. And nothing **mechanical**
could see it: no lint, no type error, and no test can assert on a comment.

A scan found **31** of them across the frontend on 2026-08-23 --- in `viewer.ts`,
`App.svelte`, `viewercheck.ts`, `scroller.ts`, `thumbnails.ts` and seven more.
All were repaired; this is what stops the thirty-second.

## The rule, and why it can be total

A `/** */` block must be followed by code, never by another `/** */`.

The obvious objection is the group header: a block introducing several
constants, which is legitimate prose and has exactly this shape. The answer is a
spelling rather than an allowlist --- **a group header is a plain `/* */` block,
not a doc comment** --- so the rule needs no exceptions and no list to rot. There
is one such header in the tree, above `commands.ts`'s scoring weights, and it
says so in its own text.

The **module header at line 1** is the single structural exception, and it is
not an allowlist either: a file's own `/** */` header is followed by the first
declaration's doc comment in every well-formed module here, so the rule cannot
apply to it. It is recognised by position, not by name.

## What it does not catch

A doc comment on the *wrong* declaration --- one that binds, and describes
something else. Nothing mechanical can see that, and it is the failure mode this
one leaves open.

## Why it is frontend-only, which is a decision rather than an oversight

An outside review scored the narrow `SUFFIXES` below as a gap: "the
orphaned-doc-comment defect it exists for is in at least six places in Rust".
**The premise is wrong, and rustc settles it in four experiments rather than in
an argument.** The defect above is *silent loss* --- two doc blocks in a row,
only the last binds, the first becomes prose nobody will ever see rendered. Every
Rust spelling of that shape is either not a loss or not silent:

  - **Two `///` runs before one item, blank line between them: BOTH attach.**
    They are sugar for `#[doc]` attributes and rustdoc concatenates them ---
    checked by generating the HTML and finding both blocks in it, not by reading
    the reference. Nothing is lost, so there is nothing to report.
  - **A doc comment on a statement** is `unused_doc_comments`, a warning --- and
    this repository runs `cargo clippy --all-targets -- -D warnings`, which
    **denies** it. Measured by planting one in `textbox.rs` and running the gate's
    own command: exit 101, `error: unused doc comment`. That is the only one of
    the four whose fate depends on this repository's settings rather than on the
    language, which is why it is the one that was measured here rather than in a
    scratch file.
  - **A doc comment before a closing brace** is `error[E0584]`, "found a
    documentation comment that doesn't document anything".
  - **An inner `//!` where an outer `///` was meant** is `error[E0753]`.

So a `.rs` arm for *that* shape would be a check with no reachable subject, which
is the shape this repository refuses everywhere else. The review's six
line-ranges were read two days later and every one of them pointed at a doc
comment correctly bound to its item; the ranges had simply moved.

## The Rust shape that IS reachable, and it is not loss --- it is misattribution

**The four bullets above are all about a blank line between the two runs. Take
the blank line away and Rust does something else: the two runs are one comment.**
Everything the first block said --- its summary, its `# Errors` section --- is
then rendered as documentation for the item below, and the item the first block
was written for keeps whatever it had, which is usually nothing. Nothing is lost,
so the paragraph above is right; the attribution is wrong, and a confidently
wrong rustdoc page is worse than a missing one.

Measured on 2026-08-28, three live instances:

  - `render.rs` --- seventeen lines describing `close` sat on the end of
    `release_all`'s comment. `release_all` rendered with both, and `close` had no
    documentation at all.
  - `save.rs` --- `mark_sites` rendered with **two `# Errors` sections** and two
    summaries, the second describing a function that writes.
  - `lib.rs` --- introduced *in the session that wrote this check*, while fixing
    the other two, and found only by running it. That is the argument for the
    arm existing: the defect is invisible to a careful reader by construction.

The rule is narrow on purpose. A `///` line is flagged when it ends a sentence,
the line before it ends a sentence, the next line is a blank `///`, and the
paragraph after that opens with this repository's house style for the paragraph
that follows a summary --- a `**bolded claim**`. Measured over the whole tree
that is seven flags: the three above, and four ordinary paragraph-final
sentences, which are listed in `PROSE` with a reason each. A wider rule flagged
twenty-four and would have been argued down to nothing.

The same question is open for `scripts/*.py` and deliberately not answered here:
a second docstring in a row is inert rather than lost, so the Python shape is not
this defect either, but nobody has measured it. Raise it as its own proposal
rather than as an extension of this one.
"""

from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
#: Frontend only *for the orphan rule*, and the docstring above says why with the
#: measurements: every Rust spelling of a silently **lost** doc comment is caught
#: by rustc or by the clippy gate. Rust has its own shape, `fused` below.
SUFFIXES = {".ts", ".svelte"}

RUST = ROOT / "src-tauri" / "src"

#: Lines the fused-run rule flags that are ordinary prose: a paragraph's last
#: sentence, after another sentence, before a bolded paragraph. Keyed by file and
#: by the line's own text, so an edit elsewhere in the file does not move them,
#: and each carries why it is not a summary. An entry whose text is no longer in
#: its file fails the check rather than being ignored --- an exemption nobody can
#: see is how a check stops covering what it was written for.
PROSE: dict[tuple[str, str], str] = {
    (
        "lib.rs",
        "`docs/THREAT-MODEL.md` §T6.9 carries what holding it costs.",
    ): "closes the paragraph about holding the password, and points at the section",
    (
        "save.rs",
        "`docs/THREAT-MODEL.md` §T6 and residual risk 18 carry what that changes.",
    ): "the same shape: a pointer closing the paragraph about where the parse runs",
    (
        "worker_shm.rs",
        "`examples/rewrite_probe.rs` measures exactly that, deterministically.",
    ): "names the probe that measures the SIGBUS the paragraph describes",
    (
        "workers.rs",
        "That is the point of restarting.",
    ): "a four-word conclusion to the paragraph above it, not a summary of an item",
}


def fused(path: pathlib.Path) -> list[tuple[int, str]]:
    """Lines that look like a second summary inside one `///` run.

    See the module docstring: two `///` runs with no blank line between them are
    one comment, so the first block's summary and `# Errors` are rendered as the
    documentation of the item below it.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    out: list[tuple[int, str]] = []

    def doc(at: int) -> str | None:
        if 0 <= at < len(lines) and lines[at].strip().startswith("///"):
            return lines[at].strip()[3:].strip()
        return None

    for i in range(len(lines)):
        here, before = doc(i), doc(i - 1)
        if not here or not before:
            continue
        if not here.endswith(".") or not before.endswith("."):
            continue
        if doc(i + 1) != "":
            continue
        after = doc(i + 2)
        if after is None or not after.startswith("**"):
            continue
        if (path.name, here) in PROSE:
            continue
        out.append((i + 1, here))
    return out


def blocks(lines: list[str]) -> list[tuple[int, int]]:
    """Every `/** */` block, as (first line, last line), zero-based."""
    found: list[tuple[int, int]] = []
    i = 0
    while i < len(lines):
        if lines[i].strip().startswith("/**"):
            start = i
            while i < len(lines) and "*/" not in lines[i]:
                i += 1
            found.append((start, min(i, len(lines) - 1)))
        i += 1
    return found


def orphans(path: pathlib.Path) -> list[tuple[int, str]]:
    """Doc blocks in `path` that are followed by another doc block."""
    lines = path.read_text(encoding="utf-8").splitlines()
    out: list[tuple[int, str]] = []
    for start, end in blocks(lines):
        # The module header. A file's own `/** */` is followed by the first
        # declaration's doc in every well-formed module, so the rule cannot
        # apply to it -- recognised by position rather than by name.
        if start == 0:
            continue
        after = end + 1
        while after < len(lines) and lines[after].strip() == "":
            after += 1
        if after < len(lines) and lines[after].strip().startswith("/**"):
            first = (
                lines[start].strip()[3:].strip().removesuffix("*/").strip()
                if start == end
                else lines[start + 1].strip().lstrip("* ").strip()
            )
            out.append((start + 1, first))
    return out


def main() -> int:
    files = sorted(p for p in SRC.rglob("*") if p.suffix in SUFFIXES and p.is_file())
    # **The emptiness control.** A scan that found no files, or a tree with no
    # doc comments in it, passes exactly like a clean one -- which is the shape
    # this repository records as a check that cannot fail.
    if not files:
        print(f"[FAIL] no .ts or .svelte files under {SRC}", file=sys.stderr)
        return 1
    total = sum(len(blocks(p.read_text(encoding="utf-8").splitlines())) for p in files)
    if total == 0:
        print("[FAIL] scanned the frontend and found no doc comments at all", file=sys.stderr)
        return 1

    findings: list[str] = []
    for path in files:
        for line, first in orphans(path):
            findings.append(f"{path.relative_to(ROOT)}:{line}: {first[:80]}")

    # The Rust arm. A different rule for a different failure -- see the module
    # docstring: not a doc comment that is lost, one that binds to the wrong item.
    rust = sorted(p for p in RUST.rglob("*.rs") if p.is_file())
    if not rust:
        print(f"[FAIL] no .rs files under {RUST}", file=sys.stderr)
        return 1
    by_name = {p.name: p for p in rust}
    for name, text in PROSE:
        path = by_name.get(name)
        if path is None:
            print(f"[FAIL] the PROSE entry for {name} names a file that is gone", file=sys.stderr)
            return 1
        if text not in path.read_text(encoding="utf-8"):
            print(
                f"[FAIL] {name} no longer contains the PROSE line {text!r}, "
                "so the exemption is stale",
                file=sys.stderr,
            )
            return 1

    fusions: list[str] = []
    for path in rust:
        for line, text in fused(path):
            fusions.append(f"{path.relative_to(ROOT)}:{line}: {text[:80]}")

    print(f"scanned {len(files)} frontend files ({total} doc comments) and {len(rust)} Rust files")
    if fusions:
        print(
            f"[FAIL] {len(fusions)} Rust doc comment(s) look like a second summary inside "
            "one `///` run. Two runs with no blank line between them are ONE comment, so "
            "everything above binds to the item below and the item it was written for has "
            "nothing.",
            file=sys.stderr,
        )
        for line in fusions:
            print(f"       {line}", file=sys.stderr)
        print(
            "       Move the block above the item it describes. If it really is the last "
            "sentence of a paragraph, add it to PROSE with a reason.",
            file=sys.stderr,
        )
        return 1
    if findings:
        print(
            f"[FAIL] {len(findings)} doc comment(s) document nothing: each is followed by "
            "another doc comment, and only the last one binds.",
            file=sys.stderr,
        )
        for line in findings:
            print(f"       {line}", file=sys.stderr)
        print(
            "       Move it above the declaration it describes. If it introduces a "
            "*group* rather than one declaration, spell it `/* */` -- a group header "
            "is not a doc comment.",
            file=sys.stderr,
        )
        return 1
    print(
        "[OK] every doc comment is followed by code, and no Rust `///` run carries a "
        "second summary."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
