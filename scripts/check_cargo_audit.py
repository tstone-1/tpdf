#!/usr/bin/env python3
"""`cargo audit` over each lockfile, with one named yanked crate let through.

`.cargo/audit.toml` denies every warning, so a yanked crate fails the audit
like a vulnerability does. cargo-audit can ignore an advisory by its id, but a
yank has no id: its only switch for yanked crates is `[yanked] enabled`, which
turns the check off for every crate at once.

That became a problem on 2026-10-16. The `zip` maintainers yanked every
release below 9.0.3 for security issues, some published and more announced
(zip-rs/zip2#1075), and `tauri-plugin-updater` 2.13.3 and 2.14.0 both require
`zip ^4`. Tauri's move to zip 9 (tauri-apps/plugins-workspace#3698) was open
and unreleased, so no lockfile could avoid `zip 4.6.1`, and `Audit` was red on
every push from 08:18 UTC.

What tpdf exposes to it: the updater calls `zip::ZipArchive` only in
`extract_zip`, on Windows, after `verify_signature` has checked the download
against the minisign key in `tauri.conf.json`, so the parser reads only bytes
tpdf's release signed. Read in the updater 2.13.3 source.

So this runs `cargo audit --json`, which applies the ignores in
`.cargo/audit.toml`, and fails on any vulnerability and any warning except a
yanked crate named, by name and version, in `ALLOWED_YANKED`. An entry that
matches no lockfile fails too: an exception for a crate that has left the tree
is the silent kind `.cargo/audit.toml` warns about, and here it is red.

Remove the `zip` entry when a `tauri-plugin-updater` on zip 9 is taken; this
check then fails until it is removed.

    scripts/check_cargo_audit.py LOCKFILE [LOCKFILE ...]
    scripts/check_cargo_audit.py --self-test

Run from the repository root, where cargo-audit reads `.cargo/audit.toml`.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys

#: (crate, version) -> why it may be yanked and still pass.
ALLOWED_YANKED: dict[tuple[str, str], str] = {
    ("zip", "4.6.1"): (
        "every zip below 9.0.3 is yanked and tauri-plugin-updater requires zip ^4; "
        "remove when an updater on zip 9 is taken"
    ),
}


def findings(report: dict, allowed: dict[tuple[str, str], str]) -> tuple[list[str], set[tuple[str, str]]]:
    """What fails in one cargo-audit JSON report, and which allowances it used.

    A report without the keys this reads, or with no dependencies, is a
    finding: "could not read" must not pass as "nothing found".
    """
    out: list[str] = []
    used: set[tuple[str, str]] = set()
    try:
        count = report["lockfile"]["dependency-count"]
        vulns = report["vulnerabilities"]
        warnings = report["warnings"]
    except (KeyError, TypeError):
        return ["the report does not have the shape cargo-audit 0.22 writes"], used
    if not isinstance(count, int) or count <= 0:
        out.append(f"the lockfile has no dependencies (dependency-count {count!r})")
    for v in vulns.get("list", []):
        adv = v.get("advisory") or {}
        pkg = v.get("package") or {}
        out.append(f"vulnerability {adv.get('id')} in {pkg.get('name')} {pkg.get('version')}")
    if vulns.get("found") and not vulns.get("list"):
        out.append("vulnerabilities found, none listed")
    for kind, items in warnings.items():
        for w in items:
            pkg = w.get("package") or {}
            key = (pkg.get("name"), pkg.get("version"))
            if kind == "yanked" and key in allowed:
                used.add(key)
                continue
            adv = (w.get("advisory") or {}).get("id")
            out.append(f"{kind} {key[0]} {key[1]}" + (f" ({adv})" if adv else ""))
    return out, used


def audit(lockfile: str) -> dict:
    """cargo-audit's JSON for one lockfile. It exits 1 on a denied warning, so
    the exit code says nothing here; anything but 0 or 1 is an error."""
    run = subprocess.run(
        ["cargo", "audit", "--json", "--file", lockfile],
        capture_output=True,
        text=True,
    )
    if run.returncode not in (0, 1):
        sys.stderr.write(run.stderr)
        raise SystemExit(f"cargo audit exited {run.returncode} on {lockfile}")
    try:
        return json.loads(run.stdout)
    except json.JSONDecodeError:
        sys.stderr.write(run.stderr)
        raise SystemExit(f"cargo audit printed no JSON for {lockfile}")


def check(reports: dict[str, dict], allowed: dict[tuple[str, str], str]) -> list[str]:
    failures: list[str] = []
    used: set[tuple[str, str]] = set()
    for name, report in reports.items():
        found, u = findings(report, allowed)
        used |= u
        failures += [f"{name}: {f}" for f in found]
    for key in sorted(set(allowed) - used):
        failures.append(f"ALLOWED_YANKED names {key[0]} {key[1]}, which no lockfile has yanked; remove it")
    return failures


def self_test() -> int:
    def report(vulns=(), **warnings):
        return {
            "lockfile": {"dependency-count": 10},
            "vulnerabilities": {"found": bool(vulns), "count": len(vulns), "list": list(vulns)},
            "warnings": {k: [{"package": {"name": n, "version": v}, "kind": k} for n, v in items] for k, items in warnings.items()},
        }

    allowed = {("zip", "4.6.1"): "test"}
    vuln = {"advisory": {"id": "RUSTSEC-0000-0000"}, "package": {"name": "x", "version": "1.0.0"}}
    cases = [
        ("the allowed yank alone passes", {"a": report(yanked=[("zip", "4.6.1")])}, False),
        ("another yanked crate fails", {"a": report(yanked=[("zip", "4.6.1"), ("other", "1.0.0")])}, True),
        ("the same crate at another version fails", {"a": report(yanked=[("zip", "4.6.0")])}, True),
        ("a vulnerability fails", {"a": report([vuln], yanked=[("zip", "4.6.1")])}, True),
        ("an unmaintained warning fails", {"a": report(yanked=[("zip", "4.6.1")], unmaintained=[("y", "2.0.0")])}, True),
        ("an allowance no lockfile uses fails", {"a": report()}, True),
        ("one lockfile using it is enough", {"a": report(), "b": report(yanked=[("zip", "4.6.1")])}, False),
        ("a report of another shape fails", {"a": {"error": "x"}}, True),
        ("an empty lockfile fails", {"a": {**report(yanked=[("zip", "4.6.1")]), "lockfile": {"dependency-count": 0}}}, True),
    ]
    bad = 0
    for name, reports, should_fail in cases:
        failed = bool(check(reports, allowed))
        ok = failed == should_fail
        bad += not ok
        print(f"[{'OK' if ok else 'FAIL'}] {name}")
    return 1 if bad else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("lockfiles", nargs="*")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if not args.lockfiles:
        parser.error("name at least one lockfile")
    failures = check({f: audit(f) for f in args.lockfiles}, ALLOWED_YANKED)
    for f in failures:
        print(f"[FAIL] {f}")
    if failures:
        return 1
    for (name, version), why in ALLOWED_YANKED.items():
        print(f"[OK] yanked {name} {version} allowed: {why}")
    print(f"[OK] {len(args.lockfiles)} lockfile(s): no vulnerability, no other warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
