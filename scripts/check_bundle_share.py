#!/usr/bin/env python3
"""Measures how much of the shipped frontend bundle is the unattended harness.

`App.svelte` statically imports every webview entry point, so the functional
checks and the benchmarks are inside the bundle `frontendDist` embeds whole into
the binary. That is a decision rather than an oversight -- `AGENTS.md` and
`docs/RATIONALE.md` both argue it, on the ground that the checks have to observe
the artifact that ships and that the payload is not what decides cold start.

**The decision is not what this gate exists for. Its cost basis is.** The
argument was written on 2026-08-02 against `77.1 kB of a 221.2 kB bundle`, and
that figure sat in two documents as a bare number for a month while the bundle
doubled. Nobody could compute the current share from the documents, on a project
whose first stated property is cold start, in a repository whose own convention
is that no measured count lives in prose -- the authority is a command. This is
that command, so the two paragraphs can state the claim and point here.

Measured 2026-08-31, `chromium/7881` tree at `a730973` + this wave:

    bundle 447,082 units; harness family 151,362 units, 33.86%
    viewercheck 128,210 / scrollbench 6,093 / markcheck 4,439 /
    sessioncheck 3,944 / startup 3,284 / autobench 2,223 /
    opencheck 1,921 / checkreport 1,248

Worth reading off that: the **share** is where it was (34.9% in the 2026-08-02
measurement) while the **absolute** has doubled, so a check on either one alone
is half blind -- a share ceiling cannot see both halves growing together, and an
absolute ceiling condemns a bundle that grew for reasons that are not the
harness. Both are asserted, and both numbers are printed whichever way the
verdict goes, so the next reader never has to derive them.

## Method, and its limit

Vite emits a sourcemap whose `mappings` field says which source each run of
generated characters came from. Decoding it and summing each segment's span
attributes the *minified* output per module, which is the quantity the decision
is about; source bytes are not, because `viewercheck.ts` is heavily commented
and comments do not ship. `third_party_notices.py` reads the same artifact for
the same reason: it is the build's own account of what it shipped, so it cannot
drift from what shipped.

**The limit, stated because a proxy with an unstated limit is worse than no
proxy.** Sourcemap columns are UTF-16 code units, not bytes, so the totals here
are code units. On today's bundle they reconcile to within 205 of the file's
447,245 bytes on disk (0.05%), which is the non-ASCII content; the run prints that
reconciliation, and a large gap means the assumption has stopped holding rather
than that the harness moved. Bytes a segment does not cover -- a line the
bundler emitted from nothing -- are counted as unmapped and printed, never
silently folded into a module.

## What can go red, and why each one is here

An absent input that passes is this repository's most-documented defect class,
so nothing here has a quiet path:

  1. **No sourcemap, or more than one.** A renamed or missing artifact fails
     loudly. A run with nothing to measure must not report a small share.
  2. **A family member with no bytes in the bundle.** Rename `viewercheck.ts`
     and a hard-coded family list measures zero and passes with a share of
     nothing. Every named member must be attributed a non-zero span.
  3. **An entry point `App.svelte` imports that is not on the list.** The
     component's `run*IfRequested` imports and the family's entry points are
     diffed as sets, both ways, so a seventh harness cannot join the bundle
     without joining the measurement. `markcheck.ts` did exactly that after the
     prose was written, which is why the prose also said "six".
  4. **A production module importing `checkreport.ts`.** It is counted as
     harness because only the entry points reach it; that is a fact about the
     tree, so it is checked rather than assumed.
  5. **Either ceiling exceeded.**
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Glob rather than a hash. Vite fingerprints the filename, so a hard-coded name
# is stale the next time anything in the bundle changes.
BUNDLE_GLOB = "dist/assets/index-*.js"

# The entry points `App.svelte` imports, each of which replaces or drives the
# application under an environment variable and is reachable no other way.
ENTRY_POINTS = {
    "viewercheck.ts",
    "markcheck.ts",
    "sessioncheck.ts",
    "opencheck.ts",
    "autobench.ts",
    "scrollbench.ts",
    "startup.ts",
}

# Reached only from the entry points -- asserted below, not assumed. It is the
# shared printer every unattended check writes through, so it ships for exactly
# the same reason they do and belongs in the same total.
SHARED = {"checkreport.ts"}

FAMILY = ENTRY_POINTS | SHARED

# The import shape the entry points are wired with, in `App.svelte`.
ENTRY_IMPORT = re.compile(r'import\s*\{\s*run\w*IfRequested\s*\}\s*from\s*"\./lib/(\w+)"')

# Ceilings, with headroom rather than at the measurement.
#
# The share is the number the decision rests on: a third of what ships is the
# harness, and the argument is that what ships is not the lever on cold start.
# It has moved about one point in a month (34.9% -> 33.84%) while the absolute
# doubled, so 40% leaves room for a real harness increment and still refuses the
# shape the recorded decision could not survive -- the harness becoming the
# majority of the bundle is a different decision, not a larger version of this
# one.
#
# The absolute is the half the share cannot see. 200,000 is ~32% above today,
# which `viewercheck.ts` alone would reach if it grew again the way it grew
# between 2026-07-31 and 2026-08-31 (3,337 -> 10,898 lines). That is the growth
# nobody could see, so that is the growth this number is aimed at.
#
# Both are deadlines, not targets. When one fires the question is whether the
# 2026-08-02 argument still holds at the new size -- re-measure the `blank`
# variant against the payload before raising either.
SHARE_CEILING = 40.0
BYTES_CEILING = 200_000

BASE64 = {c: i for i, c in enumerate(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
)}


def decode_vlq(segment: str) -> "list[int]":
    """Decodes one base64 VLQ segment into its signed fields."""
    out: "list[int]" = []
    value = 0
    shift = 0
    for char in segment:
        digit = BASE64[char]
        value += (digit & 31) << shift
        shift += 5
        if not digit & 32:
            out.append(-(value >> 1) if value & 1 else value >> 1)
            value = 0
            shift = 0
    return out


def attribute(bundle: Path, sourcemap: Path) -> "tuple[dict, int, int]":
    """Returns (units per source, unmapped units, generated file length).

    Each mapping segment owns the generated characters from its own column up to
    the next segment's column, and the last segment on a line owns the rest of
    that line. Anything ahead of the first segment on a line, and any line with
    no segments at all, is unmapped -- reported, never attributed.
    """
    body = bundle.read_text(encoding="utf-8")
    lines = body.split("\n")
    spec = json.loads(sourcemap.read_text(encoding="utf-8"))
    sources = spec.get("sources", [])
    if not sources:
        raise ValueError(f"{sourcemap} names no sources")

    per: "dict[str, int]" = {}
    unmapped = 0
    source_index = 0

    for line_no, encoded in enumerate(spec.get("mappings", "").split(";")):
        width = len(lines[line_no]) if line_no < len(lines) else 0
        column = 0
        segments: "list[tuple[int, int | None]]" = []
        for piece in encoded.split(","):
            if not piece:
                continue
            fields = decode_vlq(piece)
            column += fields[0]
            if len(fields) >= 4:
                source_index += fields[1]
                segments.append((column, source_index))
            else:
                segments.append((column, None))

        if not segments:
            unmapped += width + 1
            continue

        unmapped += segments[0][0]
        for i, (start, source) in enumerate(segments):
            end = segments[i + 1][0] if i + 1 < len(segments) else width
            span = max(0, end - start)
            if source is None:
                unmapped += span
            else:
                name = sources[source]
                per[name] = per.get(name, 0) + span
        unmapped += 1  # the newline

    return per, unmapped, len(body)


def sole_bundle() -> "tuple[Path, Path]":
    """Finds the one built bundle and its sourcemap, or says why it cannot."""
    bundles = sorted(ROOT.glob(BUNDLE_GLOB))
    if not bundles:
        raise FileNotFoundError(
            f"no bundle matching {BUNDLE_GLOB} -- run `npm run build` first. "
            "Reporting a harness share with nothing to measure would be a pass "
            "for a run that examined nothing, which is what this refusal exists "
            "to stop."
        )
    if len(bundles) > 1:
        names = ", ".join(p.name for p in bundles)
        raise FileNotFoundError(
            f"{len(bundles)} bundles match {BUNDLE_GLOB} ({names}) -- `dist/` "
            "holds output from more than one build, so no share computed from "
            "it describes what ships. Delete `dist/` and rebuild."
        )
    bundle = bundles[0]
    sourcemap = bundle.with_suffix(".js.map")
    if not sourcemap.is_file():
        raise FileNotFoundError(
            f"{bundle.name} has no sourcemap beside it ({sourcemap.name}). The "
            "share is attributed from the map; without it there is no way to "
            "tell the harness from the viewer, and guessing is what this check "
            "exists instead of."
        )
    return bundle, sourcemap


def declared_entry_points() -> "set[str]":
    """The `run*IfRequested` modules `App.svelte` imports, as bare filenames."""
    text = (ROOT / "src" / "App.svelte").read_text(encoding="utf-8")
    return {f"{name}.ts" for name in ENTRY_IMPORT.findall(text)}


def shared_importers() -> "list[str]":
    """Modules under `src/` that import `checkreport` and are not the family."""
    out: "list[str]" = []
    for path in sorted(ROOT.glob("src/**/*")):
        if path.suffix not in (".ts", ".svelte") or path.name.endswith(".test.ts"):
            continue
        if path.name in FAMILY:
            continue
        if re.search(r'from\s*"\.{1,2}/(?:lib/)?checkreport"', path.read_text(encoding="utf-8")):
            out.append(str(path.relative_to(ROOT)))
    return out


def main() -> int:
    """Measures the harness family's share of the bundle and asserts the ceilings."""
    try:
        bundle, sourcemap = sole_bundle()
        per, unmapped, generated = attribute(bundle, sourcemap)
    except (FileNotFoundError, ValueError, OSError) as exc:
        print(f"[FAIL] {exc}", file=sys.stderr)
        return 1

    family: "dict[str, int]" = {}
    for source, units in per.items():
        name = source.replace("\\", "/").rsplit("/", 1)[-1]
        if name in FAMILY and "/src/lib/" in source.replace("\\", "/"):
            family[name] = family.get(name, 0) + units

    total = sum(family.values())
    share = 100.0 * total / generated if generated else 0.0

    print(f"       bundle {bundle.name}: {generated:,} units")
    attributed = sum(per.values())
    print(
        f"       attributed {attributed:,} + unmapped {unmapped:,} = "
        f"{attributed + unmapped:,}, against {bundle.stat().st_size:,} bytes on disk"
    )
    for name in sorted(FAMILY, key=lambda n: -family.get(n, 0)):
        got = family.get(name, 0)
        print(f"       {got:>8,}  {name}" + ("" if got else "   <- absent from the bundle"))
    print(
        f"       family {total:,} units of {generated:,} = {share:.2f}%  "
        f"(ceilings {BYTES_CEILING:,} and {SHARE_CEILING:.1f}%)"
    )

    problems: "list[str]" = []

    missing = sorted(name for name in FAMILY if not family.get(name))
    if missing:
        problems.append(
            "no bytes in the bundle for: " + ", ".join(missing) + " -- renamed, or "
            "no longer imported. A family member measured as zero makes the share "
            "smaller for the one reason that is not a smaller harness."
        )

    declared = declared_entry_points()
    if declared != ENTRY_POINTS:
        extra = sorted(declared - ENTRY_POINTS)
        gone = sorted(ENTRY_POINTS - declared)
        if extra:
            problems.append(
                "App.svelte imports an entry point this check does not count: "
                + ", ".join(extra)
            )
        if gone:
            problems.append(
                "counted as an entry point but App.svelte no longer imports it: "
                + ", ".join(gone)
            )

    borrowed = shared_importers()
    if borrowed:
        problems.append(
            "checkreport is counted as harness because only the entry points "
            "reach it, and it is now imported by: " + ", ".join(borrowed)
        )

    if total > BYTES_CEILING:
        problems.append(
            f"the harness family is {total:,} units, over the {BYTES_CEILING:,} "
            "ceiling"
        )
    if share > SHARE_CEILING:
        problems.append(
            f"the harness family is {share:.2f}% of the bundle, over the "
            f"{SHARE_CEILING:.1f}% ceiling"
        )

    if problems:
        print(
            f"[FAIL] {len(problems)} problem(s) with the shipped harness share.\n"
            "       The harness ships on purpose -- see AGENTS.md and "
            "docs/RATIONALE.md. What is\n"
            "       bounded here is its cost basis, which aged unread for a month "
            "the last time\n"
            "       it lived in prose. A ceiling firing is a question about whether "
            "the 2026-08-02\n"
            "       argument still holds at this size, not a licence to raise the "
            "number.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"       {problem}", file=sys.stderr)
        return 1

    print(
        f"[OK] the shipped harness is {total:,} units, {share:.2f}% of "
        f"{generated:,} -- under both ceilings."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
