#!/usr/bin/env python3
"""Generates THIRD-PARTY-NOTICES.md, and gates the licensing constraint.

Two separate jobs, deliberately in one script because they read the same data.

**The obligation.** PDFium is BSD-3-Clause and the crate tree is MIT and
Apache-2.0. All three require their notices to be reproduced in *binary*
distributions -- not merely in the source repository, which is where a
`LICENSE` file at the root satisfies nothing. Shipping an installer without
this file is the one item on the release checklist that is a legal obligation
rather than polish.

**The gate.** AGENTS.md opens with "No AGPL or GPL dependencies. Ever.", and
records that the check is `cargo metadata` over the whole tree rather than a
glance at a crate's README. That check is real and it is also *structurally
incomplete*, which is the interesting part and the reason this script exists in
this shape:

    cargo metadata sees 531 cargo packages and is blind to the fourteen C++
    libraries compiled into libpdfium.

Those fourteen -- FreeType, ICU, libjpeg-turbo, libpng, libtiff, Little CMS,
OpenJPEG, zlib, Abseil, AGG, fast_float, simdutf, llvm-libc, and PDFium itself
-- are inside the binary blob that does the actual PDF parsing, and no cargo
command can name them. A sweep that is complete over cargo and silent about
everything else passes exactly like one that covered the whole product. So this
script reads `vendor/pdfium/licenses/` too, and a new file appearing there is a
finding rather than something nobody notices.

Two GPL strings in there are known and benign, and both are recorded in
ALLOWED_COPYLEFT below with the mechanism rather than a reassurance. They are
allowlisted by *file and reason*, never inferred, and an entry naming a file
that no longer exists is a warning -- an allowlist that rots silently is worse
than no allowlist, because it goes on excusing something that has changed.

Usage:
    scripts/third_party_notices.py            # regenerate the notices file
    scripts/third_party_notices.py --check    # gate: up to date, and no copyleft
    scripts/third_party_notices.py --cross-check <other-pdfium-dir>

`--check` is what `scripts/gates.py` runs. It fails if the committed file does
not match what this script would generate right now, which is what keeps the
notices honest across a `cargo update` -- a hand-maintained notices file is
wrong the first time a dependency changes and nothing says so. On a mismatch it
prints the diff, because a gate that fails on a machine you are not sitting at
is only actionable if its message carries the evidence.

`--cross-check` renders again against another platform's PDFium install and
requires the two to be byte-identical. **Run it after any PDFium pin bump.** The
archives differ in line endings and in comment prefixes for the same upstream
licences, so this document -- which is supposed to be one document covering both
products -- can silently become one document per platform. That is not a
hypothetical: it is how the `notices` gate came to be green on macOS and red on
Windows with nothing wrong, on the first CI run that reached it.

    scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
    scripts/third_party_notices.py --cross-check /tmp/pdfium-win
"""

import argparse
import difflib
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CARGO_MANIFEST = REPO / "src-tauri" / "Cargo.toml"
PDFIUM = REPO / "vendor" / "pdfium"
OUTPUT = REPO / "THIRD-PARTY-NOTICES.md"

# The platforms tpdf ships. The notices file is the union across all of them:
# one document covering both products is simpler to ship and cannot be wrong by
# being read on the wrong platform.
TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
]

# Licence families that end the build. MPL-2.0 is deliberately absent: it is
# file-level copyleft, it reaches the tree through Servo's CSS crates via Tauri,
# and AGENTS.md records it as permitted. GPL, LGPL and AGPL are not.
FORBIDDEN = re.compile(r"\b(AGPL|LGPL|GPL-[0-9]|GPL\b|GNU General Public)", re.I)

# Copyleft strings inside vendor/pdfium/licenses/ that are known and benign.
# Each entry must name the file and state the *mechanism*, because "we checked
# and it is fine" is not something the next person can re-verify.
ALLOWED_COPYLEFT = {
    "icu.txt": (
        "The GPL text covers ICU4C's autotools build scripts -- aclocal.m4, "
        "config.guess and install-sh -- each carrying the Autoconf special "
        "exception, and the file itself states the exception's condition is "
        "fulfilled because ICU4C ships a generated `configure`. None of the "
        "three is compiled into libpdfium; they are build-time files of a "
        "library we consume as a prebuilt binary."
    ),
    "llvm-libc.txt": (
        "Apache-2.0 WITH LLVM-exception. The GPLv2 mention is part of the "
        "exception itself: it *waives* Apache-2.0 clauses that a court might "
        "find conflict with GPLv2, which makes the code more permissive, not "
        "less. There is no GPL obligation here to inherit."
    ),
}

# Which npm packages ship is READ FROM THE BUILD, not declared here.
#
# The first version of this script listed them by hand -- production
# dependencies plus `svelte` and `tslib`, on the reasoning that Svelte compiles
# a runtime into the output and tslib supplies TypeScript's emit helpers. That
# list was wrong in both directions, which is why it is gone: `tslib` is *not*
# emitted into the bundle at all, and `esm-env` is, as a Svelte transitive that
# is marked `"dev": true` in the lockfile and would never have been guessed.
#
# It produced the right *count*, four, which is the part worth remembering: a
# total that matches is not evidence that the set matches, and the two were
# disjoint on half their members.
#
# Vite emits a sourcemap whose `sources` array names every module that went
# into the bundle. That is the build's own account of what it shipped, so it
# cannot drift from what shipped.
BUNDLE_SOURCEMAPS = "dist/assets/*.js.map"
NODE_MODULE_IN_SOURCEMAP = re.compile(r"node_modules/((?:@[^/]+/)?[^/]+)/")

LICENCE_FILENAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def read_text(path: Path) -> str:
    """Reads a file as UTF-8, tolerating the odd stray byte in a licence text."""
    return path.read_bytes().decode("utf-8", errors="replace").replace("\r\n", "\n")


def normalise_licence_text(body: str) -> str:
    """Strips a wholesale `//` comment prefix from a licence text.

    Not cosmetic. `bblanchon/pdfium-binaries` ships `licenses/pdfium.txt` with
    every line prefixed `// ` in the macOS archive and with none in the Windows
    one -- the same licence, packaged differently. Generating this document from
    whichever archive happens to be installed therefore produced two different
    files, and the `notices` gate could be green on one platform and red on the
    other with nothing wrong. It was, on the first CI run that got that far.

    The general lesson is worth more than the fix: **a document intended to be
    platform-independent is not, if its inputs are platform-specific.** CRLF was
    the other half of the same problem and is handled in `read_text`; that one
    was invisible because normalising it is already habit, which is exactly why
    this one was not.

    Stripping is **per line and unconditional**, and the first attempt got that
    wrong in a way worth keeping. It only stripped when 80% of a file's lines
    were commented, on the reasoning that a whole-file prefix is a safe thing to
    remove and a stray `//` is not. But `pdfium.txt` is PDFium's own licence
    followed by a dozen other projects' -- only the first block carries the
    prefix, 27 lines of 196 -- so the guard declined, the output was unchanged,
    and the verification below still failed. A threshold chosen from what one
    imagines the input looks like is a guess; this one was off by a factor of
    five.

    Per line is safe because a leading `//` in a licence text is never content.
    A URL is `https://...`, which does not start with `//` once indentation is
    removed.
    """
    return "\n".join(re.sub(r"^\s*//\s?", "", line) for line in body.split("\n"))


def licence_files(directory: Path) -> "list[Path]":
    """Returns the licence-ish files in a package directory, sorted by name."""
    if not directory.is_dir():
        return []
    found = [
        entry
        for entry in sorted(directory.iterdir())
        if entry.is_file() and entry.name.upper().startswith(LICENCE_FILENAMES)
    ]
    return found


def copyright_lines(paths: "list[Path]") -> "list[str]":
    """Extracts the copyright notices a licence requires be reproduced.

    This is the half of MIT and BSD that a bare SPDX identifier does not
    satisfy: both say the *above copyright notice* shall be included, and the
    identifier does not carry one.
    """
    seen: "dict[str, None]" = {}
    for path in paths:
        for line in read_text(path).splitlines():
            stripped = line.strip()
            if re.match(r"^(//|#|\*)?\s*Copyright\b", stripped, re.I):
                cleaned = re.sub(r"^(//|#|\*)\s*", "", stripped).strip()
                if len(cleaned) > 12:
                    seen.setdefault(cleaned, None)
    return list(seen)


def cargo_shipped_packages() -> "list[dict]":
    """Returns the crates linked into the shipped binary, across all targets.

    Only *normal* dependencies. A build-dependency runs during compilation and
    a dev-dependency builds the tests; neither ships, and including them would
    inflate the notices with crates the user never receives.
    """
    by_id: "dict[str, dict]" = {}
    for target in TARGETS:
        argv = [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            str(CARGO_MANIFEST),
            "--filter-platform",
            target,
        ]
        # `encoding="utf-8"` is not optional. `text=True` alone decodes with
        # the *locale* codec, which is cp1252 on Windows, and `cargo metadata`
        # emits UTF-8 containing byte 0x81 -- undefined in cp1252. The reader
        # thread then raises UnicodeDecodeError, `.stdout` comes back None, and
        # `json.loads(None)` fails with a TypeError about JSON types that says
        # nothing about encodings. `docs/TRAPS.md` records the identical failure,
        # same byte, in `mutate_rust.py`.
        raw = subprocess.run(
            argv,
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        ).stdout
        meta = json.loads(raw)

        packages = {pkg["id"]: pkg for pkg in meta["packages"]}
        nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
        roots = (
            [meta["resolve"]["root"]]
            if meta["resolve"].get("root")
            else list(meta["workspace_members"])
        )

        # Breadth-first over normal edges only. `dep_kinds` carries one entry
        # per kind, and a normal dependency is the one whose kind is null.
        queue = list(roots)
        reached = set(roots)
        while queue:
            current = queue.pop()
            node = nodes.get(current)
            if node is None:
                continue
            for dep in node["deps"]:
                normal = any(k.get("kind") is None for k in dep.get("dep_kinds", []))
                if normal and dep["pkg"] not in reached:
                    reached.add(dep["pkg"])
                    queue.append(dep["pkg"])

        for pid in reached:
            if pid in roots or pid in by_id:
                continue
            by_id[pid] = packages[pid]

    return sorted(by_id.values(), key=lambda p: (p["name"].lower(), p["version"]))


def npm_shipped_packages() -> "list[dict]":
    """Returns the npm packages whose code reaches the frontend bundle.

    Derived from the built sourcemaps rather than from the dependency graph,
    because the dependency graph cannot answer this question: what ships is
    decided by the bundler and by which imports the code actually reaches, and
    a package's `dev` flag in the lockfile is about how it was installed, not
    about whether its bytes end up in `dist/`.
    """
    maps = sorted(REPO.glob(BUNDLE_SOURCEMAPS))
    if not maps:
        raise FileNotFoundError(
            "no sourcemaps under dist/ -- run `npm run build` first. Deriving the "
            "npm notices without them would silently fall back to guessing, which "
            "is what this function exists to stop."
        )

    names: "set[str]" = set()
    for path in maps:
        for source in json.loads(read_text(path)).get("sources", []):
            # Separators are normalised before matching. Rollup builds these
            # with the host's path semantics, so a Windows run can emit
            # `..\node_modules\svelte\...`; a pattern written against the
            # forward slashes visible on a Mac then matches nothing, the npm
            # section comes out empty, and the only symptom is the whole file
            # comparing unequal on one platform.
            hit = NODE_MODULE_IN_SOURCEMAP.search(source.replace("\\", "/"))
            if hit:
                names.add(hit.group(1))

    lock = json.loads(read_text(REPO / "package-lock.json"))
    entries = lock.get("packages", {})
    out: "list[dict]" = []
    for name in sorted(names, key=str.lower):
        entry = entries.get(f"node_modules/{name}", {})
        out.append(
            {
                "name": name,
                "version": entry.get("version", "?"),
                "license": entry.get("license", "UNKNOWN"),
                "dir": REPO / "node_modules" / name,
            }
        )
    return out


def pdfium_components() -> "list[tuple[str, Path]]":
    """Returns (label, path) for PDFium's own licence and each bundled library."""
    out: "list[tuple[str, Path]]" = []
    root_licence = PDFIUM / "LICENSE"
    if root_licence.is_file():
        out.append(("pdfium-binaries (packaging)", root_licence))
    directory = PDFIUM / "licenses"
    if directory.is_dir():
        for entry in sorted(directory.iterdir()):
            if entry.is_file():
                out.append((entry.stem, entry))
    return out


def scan_copyleft(
    cargo: "list[dict]", npm: "list[dict]", pdfium: "list[tuple[str, Path]]"
) -> "tuple[list[str], list[str]]":
    """Returns (problems, warnings) for the licensing constraint.

    Three populations, and the third is the one a cargo-only sweep cannot see.
    """
    problems: "list[str]" = []
    warnings: "list[str]" = []

    for pkg in cargo:
        spdx = pkg.get("license") or ""
        if FORBIDDEN.search(spdx):
            problems.append(f"crate {pkg['name']} {pkg['version']}: {spdx}")
        if not spdx and not pkg.get("license_file"):
            warnings.append(f"crate {pkg['name']} {pkg['version']} declares no licence")

    for pkg in npm:
        spdx = str(pkg.get("license") or "")
        if FORBIDDEN.search(spdx):
            problems.append(f"npm {pkg['name']} {pkg['version']}: {spdx}")

    seen_files = set()
    for label, path in pdfium:
        seen_files.add(path.name)
        hits = sorted(set(m.group(0) for m in FORBIDDEN.finditer(read_text(path))))
        if not hits:
            continue
        reason = ALLOWED_COPYLEFT.get(path.name)
        if reason is None:
            problems.append(
                f"pdfium/{path.name}: copyleft string(s) {hits} with no allowlist entry"
            )

    # An allowlist entry for a file that is gone excuses nothing and hides that
    # the population changed. Warn rather than fail: the file being absent is
    # not itself a licence problem.
    for name in ALLOWED_COPYLEFT:
        if name not in seen_files:
            warnings.append(
                f"ALLOWED_COPYLEFT names {name}, which is not in vendor/pdfium/licenses/"
            )

    return problems, warnings


def render(
    cargo: "list[dict]", npm: "list[dict]", pdfium: "list[tuple[str, Path]]"
) -> str:
    """Builds the notices document. Deterministic -- no dates, everything sorted."""
    lines: "list[str]" = []
    add = lines.append

    add("# Third-party notices")
    add("")
    add(
        "tpdf is MIT licensed (see `LICENSE`). The binaries additionally contain the "
        "software below, whose licences require that these notices accompany a binary "
        "distribution."
    )
    add("")
    add(
        "This file is generated by `scripts/third_party_notices.py` and checked by "
        "`scripts/gates.py`. Do not edit it by hand -- a hand-maintained notices file "
        "is wrong the first time a dependency changes, and nothing says so."
    )
    add("")

    add("## PDFium, and the libraries compiled into it")
    add("")
    add(
        "PDFium is consumed as a prebuilt binary from `bblanchon/pdfium-binaries`, "
        "pinned by digest. It statically contains the following libraries, none of "
        "which appears in `Cargo.lock` -- `cargo metadata` cannot see inside a "
        "compiled blob, which is why they are enumerated from the licence files that "
        "ship beside the library rather than from the dependency graph."
    )
    add("")
    for label, path in pdfium:
        add(f"- **{label}** — `vendor/pdfium/licenses/{path.name}`")
    add("")
    for label, path in pdfium:
        add(f"### {label}")
        add("")
        add("```")
        add(normalise_licence_text(read_text(path)).strip())
        add("```")
        add("")

    add("## Rust crates")
    add("")
    add(
        f"{len(cargo)} crates are linked into the application binary. Build- and "
        "dev-dependencies are excluded: they run at compile time and are not "
        "distributed."
    )
    add("")
    add("| Crate | Version | Licence |")
    add("|---|---|---|")
    for pkg in cargo:
        add(f"| {pkg['name']} | {pkg['version']} | {pkg.get('license') or '(none declared)'} |")
    add("")

    add("### Copyright notices")
    add("")
    add(
        "MIT and the BSD family require the copyright notice itself to be reproduced, "
        "which an SPDX identifier does not carry. These are collected from the licence "
        "files the crates ship."
    )
    add("")
    notices: "dict[str, list[str]]" = {}
    for pkg in cargo:
        directory = Path(pkg["manifest_path"]).parent
        for line in copyright_lines(licence_files(directory)):
            notices.setdefault(line, []).append(pkg["name"])
    for line in sorted(notices):
        add(f"- {line}")
    add("")

    add("### Licence texts")
    add("")
    add(
        "One copy of each distinct licence text found in the crate tree, taken from a "
        "crate that ships it rather than from a template."
    )
    add("")
    texts: "dict[str, str]" = {}
    for pkg in cargo:
        spdx = pkg.get("license") or ""
        directory = Path(pkg["manifest_path"]).parent
        for path in licence_files(directory):
            body = normalise_licence_text(read_text(path)).strip()
            key = f"{spdx} — {path.name}"
            texts.setdefault(key, body)
    for key in sorted(texts):
        add(f"#### {key}")
        add("")
        add("```")
        add(texts[key])
        add("```")
        add("")

    add("## npm packages in the frontend bundle")
    add("")
    add(
        "Packages whose code is compiled into `dist/`. This list is read from the "
        "build's own sourcemaps rather than from the dependency graph: what ships is "
        "decided by the bundler and by which imports the code actually reaches, so a "
        "package's `dev` flag in the lockfile does not answer it. One of these is a "
        "devDependency's transitive that nonetheless ends up in the bundle."
    )
    add("")
    add("| Package | Version | Licence |")
    add("|---|---|---|")
    for pkg in npm:
        add(f"| {pkg['name']} | {pkg['version']} | {pkg['license']} |")
    add("")
    npm_notices: "dict[str, None]" = {}
    for pkg in npm:
        for line in copyright_lines(licence_files(pkg["dir"])):
            npm_notices.setdefault(line, None)
    if npm_notices:
        add("### Copyright notices")
        add("")
        for line in sorted(npm_notices):
            add(f"- {line}")
        add("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    """Regenerates or checks the notices file."""
    global PDFIUM
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the committed file is stale or a forbidden licence appears",
    )
    parser.add_argument(
        "--cross-check",
        metavar="OTHER_PDFIUM_DIR",
        help=(
            "render again against another platform's PDFium install and require "
            "the two to be byte-identical. Stage one with "
            "`scripts/fetch_pdfium.py --platform win-x64 --dest <dir>`. Run it "
            "after any PDFium pin bump: the archives differ in line endings and "
            "in comment prefixes, and this document is supposed to be one "
            "document rather than one per platform."
        ),
    )
    args = parser.parse_args()

    if not PDFIUM.is_dir():
        print(
            "[FAIL] vendor/pdfium is absent -- run scripts/fetch_pdfium.py first.\n"
            "       Generating notices without it would silently omit the fourteen "
            "libraries inside libpdfium, which is the population this script exists "
            "to cover.",
            file=sys.stderr,
        )
        return 1

    cargo = cargo_shipped_packages()
    npm = npm_shipped_packages()
    pdfium = pdfium_components()

    print(
        f"crates={len(cargo)}  npm={len(npm)}  pdfium components={len(pdfium)}",
        flush=True,
    )
    # A population of zero reads exactly like a clean sweep. Say so out loud.
    for label, count in (("crates", len(cargo)), ("pdfium components", len(pdfium))):
        if count == 0:
            print(f"[FAIL] found 0 {label} -- that is a broken scan, not a clean one",
                  file=sys.stderr)
            return 1

    problems, warnings = scan_copyleft(cargo, npm, pdfium)
    for warning in warnings:
        print(f"[WARN] {warning}", file=sys.stderr)
    if problems:
        for problem in problems:
            print(f"[FAIL] forbidden licence: {problem}", file=sys.stderr)
        print(
            "\nAGENTS.md: no AGPL or GPL dependencies, ever. This is load-bearing --\n"
            "it is what makes tpdf MIT rather than AGPL, and it cannot be revisited\n"
            "later. Raise it as a decision rather than allowlisting it here.",
            file=sys.stderr,
        )
        return 1

    rendered = render(cargo, npm, pdfium)

    if args.cross_check:
        other = Path(args.cross_check)
        if not (other / "licenses").is_dir():
            print(
                f"[FAIL] {other} has no licenses/ directory -- that is not a "
                "PDFium install, and rendering against it would compare this "
                "document against a version of itself with the whole PDFium "
                "section missing, which would 'differ' for the wrong reason.",
                file=sys.stderr,
            )
            return 1
        PDFIUM = other
        against = render(cargo, npm, pdfium_components())
        if against != rendered:
            diff = list(
                difflib.unified_diff(
                    rendered.splitlines(),
                    against.splitlines(),
                    fromfile="rendered from vendor/pdfium",
                    tofile=f"rendered from {other}",
                    lineterm="",
                    n=1,
                )
            )
            print(
                f"[FAIL] the two archives produce different documents "
                f"({len(diff)} diff lines); first 30:",
                file=sys.stderr,
            )
            for line in diff[:30]:
                print(f"  {line}", file=sys.stderr)
            return 1
        print(f"[OK] byte-identical against {other}")
        return 0

    if args.check:
        if not OUTPUT.is_file():
            print(f"[FAIL] {OUTPUT.name} does not exist -- run this script without "
                  "--check", file=sys.stderr)
            return 1
        committed = read_text(OUTPUT)
        if committed != rendered:
            print(
                f"[FAIL] {OUTPUT.name} is stale. Re-run "
                "scripts/third_party_notices.py and commit the result.",
                file=sys.stderr,
            )
            # Say *how* it differs, not merely that it does. A gate that fails
            # on a machine you are not sitting at -- a CI runner, the other
            # platform -- is only actionable if its message carries the
            # evidence, and "stale" carries none. This was written after a
            # Windows CI run reported exactly that and the cause had to be
            # guessed at from a Mac.
            diff = list(
                difflib.unified_diff(
                    committed.splitlines(),
                    rendered.splitlines(),
                    fromfile=f"{OUTPUT.name} (committed)",
                    tofile=f"{OUTPUT.name} (regenerated here)",
                    lineterm="",
                    n=1,
                )
            )
            print(f"\n{len(diff)} diff line(s); first 40:", file=sys.stderr)
            for line in diff[:40]:
                print(f"  {line}", file=sys.stderr)
            return 1
        print(f"[OK] {OUTPUT.name} is current, and no forbidden licence appears.")
        return 0

    OUTPUT.write_bytes(rendered.encode("utf-8"))
    print(f"[OK] wrote {OUTPUT.name} ({len(rendered):,} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
