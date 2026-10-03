#!/usr/bin/env python3
"""Runs the Rust tests of this tree on a Windows machine reached over SSH.

    scripts/run_on_windows.py --host HOST --clone 'C:\\Users\\me\\tpdf'
        [--suite cli|lib|all] [--rev REV] [--timeout SECONDS]

WHY THIS EXISTS. `check_windows.py` type-checks the Windows tree from a Mac and
runs nothing, so behaviour that exists only on Windows is first exercised by
CI, after the push. On 2026-10-03 that cost a red `main`: two redaction checks
failed on the runner because `Windows.Media.Ocr` does not read a 16 px control
word reliably, which no check on a Mac can see. The same run on a Windows
machine takes a few minutes and would have said so before the push. It also
found something CI structurally cannot: two OCR checks that pass on an English
runner and fail on a German Windows.

Run it before pushing anything that touches redaction, OCR, printing, signing
with the certificate store, or any `#[cfg(windows)]` code.

WHAT IT SENDS. The tree as it is here, unpushed and uncommitted work included:
`git stash create` makes a commit of the tracked changes without touching the
working tree or the stash list, and the commits the remote does not have
travel as a `git bundle` on standard input. **Untracked files are not sent**;
stage a new file first, and the script says so when there are any. `--rev`
runs a commit instead, which is how a result is compared with `origin/main`.

WHERE IT RUNS. In a worktree of its own beside the clone, `<clone>-check`,
made on the first run and kept, with its own `target`, `node_modules` and
`vendor`. The clone's checkout is never moved and its files are never
changed; only its object store gains the commits sent. The first run is a cold
build and takes a quarter of an hour or more. Later runs are incremental.

WHAT THE HOST HAS TO BE. A Windows machine with git, Rust, Node and Python on
its `PATH`, whose SSH login shell is WSL's bash. That shell is why everything
below goes in on standard input: `wsl.exe` as a login shell takes no command,
and a Windows program started from it reads the rest of standard input ---
which is the script --- unless its own is redirected. Its output is written to
a file and read back by a second connection for the same reason.

WHAT THE ANSWER MEANS. Exit 0 only when the remote run printed its own exit
marker and that marker is 0. A connection that fails, a build that does not
finish and a run that prints no marker all exit 2, never 0: a check whose
failure looks like silence is one that cannot fail.

NOT A GATE. It needs a second machine that is switched on, so `gates.py` does
not run it, for the reason it does not run `check_windows.py`.
"""

from __future__ import annotations

import argparse
import base64
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MARKER = "TPDF-REMOTE-EXIT"
REF = "refs/tpdf-remote/check"

SUITES = {
    "cli": ["cargo test --locked --test cli"],
    "lib": ["cargo test --locked --lib"],
    "all": ["cargo test --locked --lib", "cargo test --locked --test cli"],
}

# What a reader needs from a test run: every failure and skip, and each
# suite's own total. The rest stays in `cargo.txt` on the host.
SHOWN = r"\[FAIL\]|\[SKIP\]|^test result|^cli: |^error|panicked at|FAILED|could not compile"


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def wsl_path(windows: str) -> str:
    """`C:\\Users\\me\\tpdf` as WSL mounts it: `/mnt/c/Users/me/tpdf`."""
    match = re.fullmatch(r"([A-Za-z]):[\\/](.*)", windows)
    if not match:
        raise SystemExit(f"[FAIL] --clone is a Windows path with a drive letter, not {windows!r}")
    return f"/mnt/{match[1].lower()}/" + match[2].replace("\\", "/").rstrip("/")


def tree_to_send(rev: str | None) -> tuple[str, str]:
    """The commit to run and how to describe it."""
    if rev:
        return git("rev-parse", "--verify", f"{rev}^{{commit}}"), rev
    untracked = [line[3:] for line in git("status", "--porcelain").splitlines() if line.startswith("??")]
    if untracked:
        print(f"[WARN] {len(untracked)} untracked file(s) are not sent; stage them to include them:")
        for name in untracked[:5]:
            print(f"       {name}")
    made = git("stash", "create")
    if made:
        return made, "the working tree"
    return git("rev-parse", "HEAD"), "HEAD"


def bundle_of(sha: str) -> bytes | None:
    """The commits `origin/main` does not have, or `None` when it has them all."""
    base = git("merge-base", sha, "origin/main")
    if base == sha:
        return None
    git("update-ref", REF, sha)
    try:
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "tree.bundle"
            git("bundle", "create", str(path), f"{base}..{REF}")
            return path.read_bytes()
    finally:
        git("update-ref", "-d", REF)


def powershell(clone: str, sha: str, suite: str) -> str:
    tree = clone.rstrip("\\/") + "-check"
    run = clone.rstrip("\\/") + "-check-run"
    tests = "\n".join(
        f'{command} 2>&1 | Tee-Object -Append "$run\\cargo.txt" | '
        f'Select-String -Pattern \'{SHOWN}\' | ForEach-Object {{ $_.Line.Substring(0, [Math]::Min(600, $_.Line.Length)) }}\n'
        f"if ($LASTEXITCODE -ne 0) {{ $failed = 1 }}"
        for command in SUITES[suite]
    )
    return f"""$ErrorActionPreference = "Continue"
$clone = '{clone}'; $tree = '{tree}'; $run = '{run}'; $sha = '{sha}'
function Stop-Run($why, $code) {{ "[FAIL] $why"; "{MARKER} $code"; exit }}
if (-not (Test-Path "$clone\\.git")) {{ Stop-Run "there is no clone at $clone" 2 }}
Set-Location $clone
git fetch --quiet origin 2>&1 | Out-Null
if (Test-Path "$run\\tree.bundle") {{ git fetch --quiet "$run\\tree.bundle" "{REF}" 2>&1 | Out-Null }}
git cat-file -e "$sha^{{commit}}" 2>&1 | Out-Null
if ($LASTEXITCODE) {{ Stop-Run "the commit $sha did not arrive in the clone" 2 }}
if (-not (Test-Path $tree)) {{ git worktree add --quiet --detach $tree $sha 2>&1 | Out-Null }}
Set-Location $tree
git checkout --quiet --force --detach $sha 2>&1 | Out-Null
if ((git rev-parse HEAD) -ne $sha) {{ Stop-Run "the check worktree is not at $sha" 2 }}
"[OK] $tree is at " + (git rev-parse --short HEAD)
$lock = (Get-FileHash package-lock.json).Hash
if (-not (Test-Path node_modules) -or -not (Test-Path "$run\\lock.txt") -or ((Get-Content "$run\\lock.txt") -ne $lock)) {{
  npm ci --no-audit --no-fund 2>&1 | Out-Null
  if ($LASTEXITCODE) {{ Stop-Run "npm ci failed" 2 }}
  Set-Content "$run\\lock.txt" $lock
}}
npm run build 2>&1 | Out-Null
if ($LASTEXITCODE) {{ Stop-Run "the frontend did not build" 2 }}
python scripts\\fetch_pdfium.py 2>&1 | Select-Object -Last 1
if ($LASTEXITCODE) {{ Stop-Run "PDFium could not be fetched" 2 }}
# What CI generates, by the script CI calls. Without it most checks skip, and
# a suite that skipped everything reads like one that passed.
python -u scripts\\ci_fixtures.py --signed --hostile 2>&1 | Select-Object -Last 1
if ($LASTEXITCODE) {{ Stop-Run "the fixtures could not be generated; python -m pip install -r scripts\\fixture-tools.txt installs what that needs" 2 }}
# The fixtures CI cannot generate embed a system font. The clone has them if
# they were ever made there, and without them the OCR checks skip. Every file
# and not only the documents: a fixture without its manifest fails a control.
$borrowed = 0
Get-ChildItem "$clone\\testdata" -File -ErrorAction SilentlyContinue | ForEach-Object {{
  if (-not (Test-Path "testdata\\$($_.Name)")) {{ Copy-Item $_.FullName "testdata\\$($_.Name)"; $borrowed++ }}
}}
"[OK] fixtures: CI's generated here, $borrowed more copied from the clone's testdata"
$env:CARGO_BUILD_JOBS = "2"
Set-Location src-tauri
Remove-Item "$run\\cargo.txt" -ErrorAction SilentlyContinue
$failed = 0
{tests}
"{MARKER} $failed"
"""


def bash(clone: str, sha: str, suite: str, bundle: bytes | None) -> str:
    run = wsl_path(clone) + "-check-run"
    windows_run = clone.rstrip("\\/") + "-check-run"
    lines = [f'mkdir -p "{run}" && cd "{run}" || exit 1', "rm -f result.txt tree.bundle"]
    if bundle is not None:
        lines += [
            "base64 -d > tree.bundle <<'TPDF_PAYLOAD'",
            base64.encodebytes(bundle).decode().rstrip("\n"),
            "TPDF_PAYLOAD",
        ]
    lines += [
        "cat > run.ps1 <<'TPDF_SCRIPT'",
        powershell(clone, sha, suite).rstrip("\n"),
        "TPDF_SCRIPT",
        # Its standard input is redirected, or it reads the rest of this script.
        f"powershell.exe -NoProfile -ExecutionPolicy Bypass -File '{windows_run}\\run.ps1' "
        "> result.txt 2>&1 < /dev/null",
    ]
    return "\n".join(lines) + "\n"


def ssh(host: str, script: str, timeout: float) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["ssh", "-T", "-o", "ConnectTimeout=15", "-o", "BatchMode=yes",
         "-o", "ServerAliveInterval=30", host],
        input=script, capture_output=True, text=True, timeout=timeout,
    )


def verdict(output: str) -> int:
    """The remote run's own exit, or 2 when it never said one."""
    found = re.findall(rf"^{MARKER} (\d+)\s*$", output, flags=re.MULTILINE)
    if not found:
        return 2
    return min(int(found[-1]), 2)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--host", default=os.environ.get("TPDF_WINDOWS_HOST"),
                        help="the SSH host; TPDF_WINDOWS_HOST when not given")
    parser.add_argument("--clone", default=os.environ.get("TPDF_WINDOWS_CLONE"),
                        help="the tpdf clone on that host, as a Windows path; TPDF_WINDOWS_CLONE")
    parser.add_argument("--suite", choices=sorted(SUITES), default="cli")
    parser.add_argument("--rev", help="run this commit, not the working tree")
    parser.add_argument("--timeout", type=float, default=5400.0)
    args = parser.parse_args()
    if not args.host or not args.clone:
        print("[FAIL] name the machine: --host HOST --clone 'C:\\Users\\me\\tpdf', "
              "or TPDF_WINDOWS_HOST and TPDF_WINDOWS_CLONE")
        return 2

    sha, what = tree_to_send(args.rev)
    bundle = bundle_of(sha)
    sent = "already on origin/main" if bundle is None else f"{len(bundle):,} bytes of commits sent"
    print(f"[..] {what} ({sha[:9]}, {sent}) on {args.host}: suite {args.suite}")
    try:
        ran = ssh(args.host, bash(args.clone, sha, args.suite, bundle), args.timeout)
        if ran.returncode != 0:
            print(f"[FAIL] the connection to {args.host} ended with {ran.returncode}: "
                  f"{(ran.stderr or ran.stdout).strip()[-400:]}")
            return 2
        read = ssh(args.host, f'cat "{wsl_path(args.clone)}-check-run/result.txt"\n', 120)
    except subprocess.TimeoutExpired:
        print(f"[FAIL] no answer from {args.host} within {args.timeout:.0f} s")
        return 2
    output = read.stdout.replace("\r", "")
    for line in output.splitlines():
        if not line.startswith(MARKER):
            print(f"     {line}")
    code = verdict(output)
    if code == 0:
        print(f"[OK] the {args.suite} suite passes on {args.host}")
    elif code == 1:
        print(f"[FAIL] the {args.suite} suite fails on {args.host}; "
              "the full output is in <clone>-check-run\\cargo.txt there")
    else:
        print(f"[FAIL] the run on {args.host} did not finish, so nothing was tested")
    return code


if __name__ == "__main__":
    sys.exit(main())
