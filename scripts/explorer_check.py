#!/usr/bin/env python3
"""Turns tpdf's Explorer integration on, looks at what Explorer gets, and turns it off.

Usage, on Windows, in the logged-on session (it opens windows and reads the screen):
    python scripts/explorer_check.py <tpdf_<version>_x64-setup.exe> <explorer_probe.exe>
        [--shots FOLDER] [--control NAME]

The installer must be built with the Explorer overlay
(`npm run tauri build -- --config src-tauri/tauri.explorer.conf.json --bundles nsis`)
and the probe with `cargo build --release -p tpdf-shell --example explorer_probe`.

WHY THIS EXISTS. `docs/PLAN.md` 7.8 ends on three observations, and nothing on a
Mac can make any of them: a folder of the hostile corpus shows thumbnails or
generic icons while Explorer stays responsive; no Explorer process has
`pdfium.dll` loaded; and turning the integration off leaves no tpdf key. This
is those three, plus the preview pane, against the installer a reader gets.

WHAT IT DOES, in order; each line it prints is one check:
  1. installs silently into a folder of its own and reads the user's classes;
  2. `tpdf-cli explorer --add --replace`, and reads both slots back;
  3. asks Windows' thumbnail cache, with forced extraction, for each file of
     the hostile corpus, a document that renders, a file of random bytes and a
     password-protected one, through `explorer_probe thumbs`: every answer
     within the handler's 10 s limit and a margin, the good document drawn,
     the provider loaded in `dllhost.exe`, and PDFium in no Explorer process;
  4. hosts the preview handler in `prevhost.exe` through `explorer_probe
     preview`: the first page painted, Page Down showing another, the handler
     loaded there and PDFium not;
  5. opens an Explorer window on the corpus in large icons and, for 15 s,
     asks it for an answer every 200 ms; the slowest answer must be under a
     second. A picture of the window goes to `--shots` when given;
  6. `tpdf-cli explorer --remove`: the user's classes and preview list read
     exactly as they did before step 2;
  7. turns it on again and uninstalls: no key of tpdf's class ids or DLL is
     left, and the folder holds nothing but files Windows deletes at restart.

`--control NAME` breaks one thing on purpose and exits 0 exactly when the
checks aimed at it failed and no other did: `no-add` skips step 2, `no-remove`
skips the `--remove` in step 6.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from installed_check import (  # noqa: E402
    UNINSTALL_KEY, Checks, RanOut, bounded, processes, registry, tool,
)

ROOT = Path(__file__).resolve().parent.parent
THUMBNAIL = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A21}"
PREVIEW = "{7D5A3B9E-2C41-4E8F-9A6B-1F0C3D5E7A22}"
CLASSES = r"HKCU\Software\Classes"
PREVIEW_LIST = r"HKCU\Software\Microsoft\Windows\CurrentVersion\PreviewHandlers"
LIMIT_MS = 10_000 + 3_000

CONTROLS: dict[str, tuple[str, list[str]]] = {
    "no-add": ("the integration is not turned on", [
        "turning it on", "both slots read tpdf",
        "the document that renders gets a drawn thumbnail",
        "the thumbnail provider ran in dllhost.exe",
        "the preview handler paints the first page",
        "Page Down shows another page",
        "the preview handler ran in prevhost.exe"]),
    "no-remove": ("the integration is not turned off before the classes are compared", [
        "the user's classes read as before it was turned on",
        "the user's preview list reads as before it was turned on"]),
}
# Checks a control may take with it without being wrong: with nothing turned
# on, the uninstall has nothing of tpdf's to find either way.
MAY_ALSO_FAIL: dict[str, list[str]] = {"no-add": ["turning it off"]}


def snapshot() -> dict[str, str | None]:
    return {key: registry(key) for key in (CLASSES, PREVIEW_LIST)}


def blocks(text: str | None) -> dict[str, list[str]]:
    """`reg query /s` output as {key: its value lines}, without Explorer's own view state.

    An Explorer window writes how it was shown (`Bags`, `BagMRU`) under
    `Software\\Classes\\Local Settings`, and step 5 opens one, so that subtree
    changes whatever tpdf does and is left out of the comparison.
    """
    found: dict[str, list[str]] = {}
    key = ""
    for line in (text or "").splitlines():
        if line.startswith("HKEY_"):
            key = line.strip()
            found.setdefault(key, [])
        elif line.strip() and key:
            found[key].append(line.strip())
    return {k: v for k, v in found.items() if "\\LOCAL SETTINGS\\" not in k.upper() + "\\"}


def differences(before: str | None, after: str | None) -> list[str]:
    old, new = blocks(before), blocks(after)
    said = []
    for key in sorted(set(old) | set(new)):
        if old.get(key) != new.get(key):
            said.append(f"{key}: {old.get(key)} -> {new.get(key)}")
    return said


def ours_left() -> list[str]:
    """Every line of the user's classes and preview list naming tpdf's handlers."""
    found = []
    for key in (CLASSES, PREVIEW_LIST):
        text = registry(key) or ""
        for line in text.splitlines():
            upper = line.upper()
            if THUMBNAIL in upper or PREVIEW in upper or "TPDF_SHELL" in upper:
                found.append(line.strip())
    return found


def corpus(folder: Path) -> tuple[list[Path], Path]:
    """The hostile corpus, a document that renders, random bytes, a locked file."""
    folder.mkdir(parents=True)
    files = []
    for source in sorted((ROOT / "testdata").glob("hostile-*.pdf")):
        files.append(Path(shutil.copy(source, folder)))
    good = Path(shutil.copy(ROOT / "testdata" / "text-heavy.pdf", folder / "good.pdf"))
    files.append(good)
    files.append(Path(shutil.copy(ROOT / "testdata" / "incr-encrypted-pw.pdf", folder)))
    junk = folder / "random-bytes.pdf"
    junk.write_bytes(random.Random(7).randbytes(65536))
    files.append(junk)
    return files, good


SHOTS: Path | None = None


def probe(binary: Path, *args: str, timeout: float) -> list[list[str]]:
    env = dict(os.environ, **({"TPDF_PROBE_SHOTS": str(SHOTS)} if SHOTS else {}))
    run = subprocess.run([str(binary), *args], capture_output=True, text=True, timeout=timeout,
                         check=False, env=env)
    if run.stderr.strip():
        print(f"  probe stderr: {run.stderr.strip()[:400]}")
    return [line.split("\t") for line in run.stdout.splitlines() if "\t" in line]


def modules_say(rows: list[list[str]]) -> tuple[bool, bool, bool, set[str]]:
    """(PDFium anywhere, provider in dllhost, handler in prevhost, processes with the DLL)."""
    pdfium = any(r[0] == "modules" and r[4] == "pdfium=true" for r in rows)
    holding = {r[1] for r in rows if r[0] == "modules" and r[3] == "tpdf_shell=true"}
    return pdfium, "dllhost.exe" in holding, "prevhost.exe" in holding, holding


def explorer_window(folder: Path, shots: Path | None, checks: Checks) -> None:
    script = f"""
$shell = New-Object -ComObject Shell.Application
Start-Process explorer.exe -ArgumentList '"{folder}"'
$w = $null
for ($i = 0; $i -lt 60 -and -not $w; $i++) {{
  Start-Sleep -Milliseconds 250
  $w = $shell.Windows() | Where-Object {{ $_.Document.Folder.Self.Path -eq '{folder}' }} | Select-Object -First 1
}}
if (-not $w) {{ exit 3 }}
$w.Document.CurrentViewMode = 1
$w.Document.IconSize = 256
Write-Output $w.HWND
"""
    opened = subprocess.run(["powershell", "-NoProfile", "-Command", script], capture_output=True,
                            text=True, timeout=60, check=False)
    hwnd = int(opened.stdout.strip().splitlines()[-1]) if opened.returncode == 0 and opened.stdout.strip() else 0
    if not checks.note(hwnd != 0, "an Explorer window opens on the corpus in large icons",
                       opened.stderr.strip()[:200]):
        return
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.SendMessageTimeoutW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM,
                                           wintypes.LPARAM, wintypes.UINT, wintypes.UINT,
                                           ctypes.POINTER(ctypes.c_size_t)]
    slowest, failed, asked = 0.0, 0, 0
    until = time.monotonic() + 15
    while time.monotonic() < until:
        result = ctypes.c_size_t()
        started = time.monotonic()
        ok = user32.SendMessageTimeoutW(hwnd, 0, 0, 0, 0x0002, 5000, ctypes.byref(result))
        took = time.monotonic() - started
        asked += 1
        slowest = max(slowest, took)
        failed += 0 if ok else 1
        time.sleep(0.2)
    checks.note(failed == 0 and slowest < 1.0, "Explorer answers within a second throughout",
                f"{asked} asked, slowest {slowest * 1000:.0f} ms, {failed} unanswered")
    if shots:
        shots.mkdir(parents=True, exist_ok=True)
        picture = shots / "explorer-corpus.png"
        grab = f"""
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W {{ [StructLayout(LayoutKind.Sequential)] public struct R {{ public int L, T, Ri, B; }}
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r); }}
'@
$r = New-Object W+R
[W]::GetWindowRect([IntPtr]{hwnd}, [ref]$r) | Out-Null
$b = New-Object System.Drawing.Bitmap ($r.Ri - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size)
$b.Save('{picture}')
"""
        subprocess.run(["powershell", "-NoProfile", "-Command", grab], timeout=60, check=False)
        print(f"  picture: {picture}")
    user32.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("installer")
    parser.add_argument("probe")
    parser.add_argument("--shots", type=Path)
    parser.add_argument("--control", choices=sorted(CONTROLS))
    args = parser.parse_args()
    if sys.platform != "win32":
        print("[ERROR] this check runs on Windows")
        return 2
    installer, binary = Path(args.installer).resolve(), Path(args.probe).resolve()
    for path in (installer, binary):
        if not path.is_file():
            print(f"[ERROR] no {path}")
            return 2
    if registry(UNINSTALL_KEY) is not None:
        print("[ERROR] tpdf is installed on this machine; this check installs and removes it")
        return 2
    if any(image.lower() == "tpdf.exe" for _, _, image in processes()):
        print("[ERROR] tpdf is running")
        return 2
    folder = Path(os.environ["LOCALAPPDATA"]) / "tpdf-explorer-check"
    if folder.exists() or " " in str(folder):
        print(f"[ERROR] {folder} exists or has a space in it; remove it and run again")
        return 2
    global SHOTS
    SHOTS = args.shots
    if SHOTS:
        SHOTS.mkdir(parents=True, exist_ok=True)
    control = args.control
    if control:
        print(f"--- control: {CONTROLS[control][0]} ---")
    checks = Checks()
    cli = folder / "tpdf-cli.exe"
    with tempfile.TemporaryDirectory(prefix="tpdf-explorer-check-") as temporary:
        files, good = corpus(Path(temporary) / "corpus")
        try:
            print(f"--- installing {installer.name} into {folder} ---")
            code = bounded(f'"{installer}" /S /D={folder}', "the installer", 300)
            checks.note(code == 0 and (folder / "tpdf_shell.dll").is_file(),
                        "the installer installs tpdf_shell.dll", f"exit {code}")
            before = snapshot()
            said = tool(cli, "explorer")
            print(f"  before: {said.stdout.strip()}")

            print("--- turning it on ---")
            if control == "no-add":
                checks.note(False, "turning it on", "not run")
            else:
                said = tool(cli, "explorer", "--add", "--replace")
                checks.note(said.returncode == 0, "turning it on", (said.stdout + said.stderr).strip())
            said = tool(cli, "explorer")
            checks.note("thumbnails with tpdf" in said.stdout and "pane with tpdf" in said.stdout,
                        "both slots read tpdf", said.stdout.strip())

            print("--- thumbnails ---")
            rows = probe(binary, "thumbs", *map(str, files), timeout=len(files) * 20 + 30)
            thumbs = {r[1]: r for r in rows if r[0] == "thumb"}
            for path in files:
                row = thumbs.get(str(path))
                print(f"  {path.name}: {row[2:] if row else 'no answer'}")
            checks.note(len(thumbs) == len(files), "every file gets an answer",
                        f"{len(thumbs)} of {len(files)}")
            late = [Path(r[1]).name for r in thumbs.values() if int(r[5]) > LIMIT_MS]
            checks.note(not late, "every answer comes within the handler's limit", ", ".join(late))
            row = thumbs.get(str(good))
            checks.note(bool(row) and row[2] == "ok" and int(row[4]) >= 50,
                        "the document that renders gets a drawn thumbnail", str(row[2:] if row else None))
            pdfium, in_dllhost, _, holding = modules_say(rows)
            checks.note(in_dllhost, "the thumbnail provider ran in dllhost.exe", ", ".join(sorted(holding)))
            checks.note(not pdfium, "no Explorer process has pdfium.dll mapped while thumbnails are made")
            checks.note("explorer.exe" not in holding, "explorer.exe itself did not load tpdf_shell.dll")

            print("--- the preview pane ---")
            rows = probe(binary, "preview", str(ROOT / "testdata" / "outline-simple.pdf"), timeout=60)
            for r in rows:
                if r[0] == "preview":
                    print(f"  {r[1:]}")
            values = {r[1]: r[2:] for r in rows if r[0] == "preview"}
            first = float(values.get("page1", ["0"])[0])
            changed = float(values.get("page2", ["0", "0"])[1]) if len(values.get("page2", [])) > 1 else 0.0
            checks.note(first > 0.2, "the preview handler paints the first page", f"{first:.3f} white")
            # The fixture's pages differ only in their digits and the label, which is
            # 0.2% of the pane; a pane that did not turn changes nothing.
            checks.note(changed > 0.0005, "Page Down shows another page", f"{changed:.4f} changed")
            pdfium, _, in_prevhost, holding = modules_say(rows)
            checks.note(in_prevhost, "the preview handler ran in prevhost.exe", ", ".join(sorted(holding)))
            checks.note(not pdfium, "no Explorer process has pdfium.dll mapped while previewing")

            print("--- an Explorer window on the corpus ---")
            explorer_window(files[0].parent, args.shots, checks)

            print("--- turning it off ---")
            if control != "no-remove":
                said = tool(cli, "explorer", "--remove")
                checks.note(said.returncode == 0, "turning it off", (said.stdout + said.stderr).strip())
            after = snapshot()
            changed = differences(before[CLASSES], after[CLASSES])
            checks.note(not changed, "the user's classes read as before it was turned on",
                        "; ".join(changed)[:600])
            changed = differences(before[PREVIEW_LIST], after[PREVIEW_LIST])
            checks.note(not changed, "the user's preview list reads as before it was turned on",
                        "; ".join(changed)[:600])
            print(f"  after: {tool(cli, 'explorer').stdout.strip()}")
            if control != "no-add":
                tool(cli, "explorer", "--add")
        except (RanOut, OSError, subprocess.TimeoutExpired, ValueError, IndexError) as e:
            checks.note(False, "every step ends, and within its time limit", str(e))
        finally:
            print("--- uninstalling ---")
            remover = folder / "uninstall.exe"
            if remover.is_file():
                bounded(f'"{remover}" /S _?={folder}', "the uninstaller", 180)
            left = ours_left()
            checks.note(not left, "after uninstalling, no key names tpdf's handlers", "; ".join(left)[:300])
            stray = sorted(p.name for p in folder.rglob("*") if p.is_file()) if folder.exists() else []
            unexpected = [n for n in stray if n != "uninstall.exe" and not re.fullmatch(r"ns\w+\.tmp", n)]
            checks.note(not unexpected, "the folder holds nothing but the uninstaller and files "
                        "Windows deletes at restart", ", ".join(stray))
            shutil.rmtree(folder, ignore_errors=True)

    print()
    if control:
        expected = CONTROLS[control][1]
        allowed = expected + MAY_ALSO_FAIL.get(control, [])
        unexpected = [n for n in checks.failed if not any(n.startswith(e) for e in allowed)]
        missing = [e for e in expected if not any(n.startswith(e) for n in checks.failed)]
        if unexpected or missing:
            print(f"[FAIL] control '{control}': did not fail but should have: {missing or 'nothing'}; "
                  f"failed but should not have: {unexpected or 'nothing'}")
            return 1
        print(f"[OK] control '{control}': what it is aimed at failed ({len(expected)}), and nothing else")
        return 0
    if checks.failed:
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
