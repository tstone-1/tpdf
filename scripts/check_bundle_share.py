#!/usr/bin/env python3
"""Require a normal build to contain no optional check/benchmark implementation.

Run after `npm run build`. For the native test build use `--checks` after
`npm run build:checks`: every entry must then be present and within the size
ceilings. Both modes examine ALL JavaScript chunks, so lazy loading cannot hide
shipped harness code. Sourcemap spans measure UTF-16 code units, not source size.
"""

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Glob rather than a hash. Vite fingerprints the filename, so a hard-coded name
# is stale the next time anything in the bundle changes.
BUNDLE_GLOB = "dist/assets/index-*.js"

# Entry points reached through the compile-time guarded harness wrapper.
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
# Helpers reached through those entry points, including the signature workflow.
SHARED = {"checkreport.ts", "signaturecheck.ts"}

FAMILY = ENTRY_POINTS | SHARED

# Dynamic runtime imports in the wrapper; type-only references do not count.
ENTRY_IMPORT = re.compile(r'await import\("\./(\w+)"\)')

# Cost ceilings apply only to the explicit native-check build.
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
        width = len(lines[line_no].encode("utf-16-le")) // 2 if line_no < len(lines) else 0
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

    return per, unmapped, len(body.encode("utf-16-le")) // 2


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
    """The runtime imports in the optional harness wrapper, as bare filenames."""
    text = (ROOT / "src" / "lib" / "harness.ts").read_text(encoding="utf-8")
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
    """Measure every shipped chunk and assert the selected build's contract."""
    checks = sys.argv[1:] == ["--checks"]
    if sys.argv[1:] and not checks:
        print("usage: check_bundle_share.py [--checks]", file=sys.stderr)
        return 2
    try:
        sole_bundle()  # A missing or stale main artifact must never read as zero.
        per, unmapped, generated, disk = {}, 0, 0, 0
        bundles = sorted(ROOT.glob("dist/assets/*.js"))
        for bundle in bundles:
            sourcemap = bundle.with_suffix(".js.map")
            # Vite's Babel helper chunk has no source map. Admit only the
            # inspected generated helper by digest, never an arbitrary missing map.
            if (not sourcemap.exists() and bundle.name.startswith("defineProperty-")
                    and hashlib.sha256(bundle.read_bytes()).hexdigest() == "c2ae68c1ad7462e777ccd4e1e3f84b3bc07ab81458f0dd0ea042a9e1b4d23afb"):
                found, unknown, units = {}, bundle.stat().st_size, bundle.stat().st_size
            else:
                found, unknown, units = attribute(bundle, sourcemap)
            if units == 0:
                raise ValueError(f"empty bundle: {bundle}")
            for source, count in found.items():
                per[source] = per.get(source, 0) + count
            unmapped += unknown
            generated += units
            disk += bundle.stat().st_size
    except (ValueError, OSError, KeyError, IndexError) as exc:
        print(f"[FAIL] {exc}", file=sys.stderr)
        return 1

    family = {}
    for source, units in per.items():
        name = source.replace("\\", "/").rsplit("/", 1)[-1]
        if name in FAMILY and "/src/lib/" in source.replace("\\", "/"):
            family[name] = family.get(name, 0) + units
    total = sum(family.values())
    share = 100.0 * total / generated
    print(f"       {len(bundles)} chunks: {disk:,} bytes, {generated:,} units, {unmapped:,} unmapped")
    print(f"       harness: {total:,} units ({share:.2f}%)")
    problems = []
    declared = declared_entry_points()
    if declared != ENTRY_POINTS:
        problems.append(f"harness entry inventory differs: extra {sorted(declared - ENTRY_POINTS)}, missing {sorted(ENTRY_POINTS - declared)}")
    borrowed = shared_importers()
    if borrowed:
        problems.append("production imports checkreport: " + ", ".join(borrowed))
    if checks:
        missing = sorted(name for name in FAMILY if not family.get(name))
        if missing:
            problems.append("no bytes in the check build for: " + ", ".join(missing))
        if total > BYTES_CEILING or share > SHARE_CEILING:
            problems.append("check harness exceeds its size ceilings")
    elif total:
        problems.append("normal build contains optional harness implementation")
    if problems:
        for problem in problems:
            print(f"[FAIL] {problem}", file=sys.stderr)
        return 1
    print("[OK] " + ("check harness present and bounded" if checks else "normal build excludes the check harness"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
