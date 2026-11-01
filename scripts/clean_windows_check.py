#!/usr/bin/env python3
"""Signs with each listed timestamp authority on a clean Windows, in Windows Sandbox.

Usage, on Windows with the Windows Sandbox feature on, in a session with a desktop:
    python scripts/clean_windows_check.py <tpdf_<version>_x64-setup.exe> <document.pdf>
        [--tool <tpdf-cli.exe>] [--expect-refused]

WHY THIS EXISTS. A clean Windows holds none of the roots the three authorities
chain to, and tpdf refuses a timestamp from an authority it cannot trust. Every
machine that builds or tests tpdf has those roots by now, so no gate can see the
failure: 26.10.28 shipped with DigiCert and GlobalSign refused on a clean machine
(`docs/TRAPS.md`, *A clean Windows holds none of the timestamp authorities' roots,
and HTTPS hid it for one of them*).

WHAT IT DOES. Installs the release in a new sandbox, makes a throwaway signing
certificate there, and signs once per authority, reading the root store around
each. All three are asked over plain `http://`, Sectigo included: tpdf's HTTPS
client has Windows check the server's certificate, which fetches the same root
as a side effect and would hide a broken lookup.

THEN READING, in a second new sandbox. A document signed with the throwaway
certificate and a DigiCert timestamp kept without judging it is verified three
times: as it is, where the authority must read as not trusted and nothing may be
fetched; with `--fetch-roots`, where it must read as trusted; and as it is again,
where it must stay trusted. The signer must read as not trusted all three times:
the fetch brings only a root Microsoft distributes.

THE CONTROLS. The store must start without any of the roots, or the sandbox was
not clean and the run proves nothing. `--expect-refused` turns the signing verdict
over and skips reading: run a release from before the lookup, 26.10.28 or older,
and every authority must be refused, which shows the check can fail. `--tool` runs
that `tpdf-cli.exe` over the installed one, for a tree not yet released.

No HTTPS and no browser run in the sandbox before the signings. Windows fetches
roots for either. Installing the release fetches Certum's, which is none of these.
Nothing is changed outside the sandbox and a temporary folder.
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from xml.sax.saxutils import escape

SANDBOX = Path(r"C:\Windows\System32\WindowsSandbox.exe")
LIMIT_SECONDS = 300
# The authorities, as `tpdf sign --timestamp` takes them: all without TLS.
AUTHORITIES = ("http://timestamp.sectigo.com", "digicert", "globalsign")

SCRIPT = r"""
$kit = 'C:\kit'
function Say($t) { $t | Add-Content -Encoding ASCII "$kit\report.txt" }
function Roots { Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\AuthRoot, Cert:\CurrentUser\Root |
    ForEach-Object { $_.Subject } | Sort-Object -Unique }
try {
    $wanted = 'USERTrust RSA Certification Authority', 'DigiCert Trusted Root G4',
        'DigiCert Assured ID Root CA', 'GlobalSign Root CA - R6'
    $roots = @(Roots)
    Say "ROOTS $($roots.Count)"
    foreach ($w in $wanted) { if ($roots -like "*$w*") { Say "NOT-CLEAN $w" } }
    $folder = "$env:LOCALAPPDATA\tpdf"
    $i = Start-Process "$kit\setup.exe" -Wait -PassThru -ArgumentList '/S', "/D=$folder"
    Say "INSTALL $($i.ExitCode)"
    $cli = "$folder\tpdf-cli.exe"
    if (Test-Path "$kit\tpdf-cli.exe") { Copy-Item "$kit\tpdf-cli.exe" $cli -Force }
    $name = 'tpdf clean machine check'
    $null = New-SelfSignedCertificate -Subject "CN=$name" -CertStoreLocation Cert:\CurrentUser\My `
        -KeyUsage DigitalSignature, NonRepudiation -KeyExportPolicy NonExportable `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.4') -NotAfter (Get-Date).AddDays(2)
    if (Test-Path "$kit\read.txt") {
        # Kept without judging its authority, which is what fetches nothing.
        $signed = "$env:TEMP\stamped.pdf"
        $p = Start-Process $cli -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$env:TEMP\out.txt" `
            -RedirectStandardError "$env:TEMP\err.txt" -ArgumentList @('sign', "`"$kit\document.pdf`"", '-o', "`"$signed`"",
            '--identity', "`"$name`"", '--timestamp', 'digicert', '--allow-untrusted-timestamp')
        Say "STAMPED exit=$($p.ExitCode) roots=$(@(Roots).Count)"
        foreach ($step in @(@('before', @()), @('fetched', @('--fetch-roots')), @('after', @()))) {
            $p = Start-Process $cli -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$env:TEMP\out.txt" `
                -RedirectStandardError "$env:TEMP\err.txt" -ArgumentList (@('verify', "`"$signed`"", '--json') + $step[1])
            $s = (Get-Content -Raw "$env:TEMP\out.txt" | ConvertFrom-Json).files[0].signatures[0]
            $hint = [bool](Select-String -Quiet -SimpleMatch '--fetch-roots' "$env:TEMP\err.txt")
            Say "READ $($step[0]) signer=$($s.trust.standing) authority=$($s.timestamp.trust.standing) hint=$hint roots=$(@(Roots).Count)"
        }
        return
    }
    foreach ($a in (Get-Content "$kit\authorities.txt")) {
        $o = "$env:TEMP\signed.pdf"
        Remove-Item $o -ErrorAction SilentlyContinue
        $p = Start-Process $cli -NoNewWindow -Wait -PassThru -RedirectStandardError "$env:TEMP\err.txt" `
            -RedirectStandardOutput "$env:TEMP\out.txt" `
            -ArgumentList @('sign', "`"$kit\document.pdf`"", '-o', "`"$o`"", '--identity', "`"$name`"", '--timestamp', $a)
        $err = (Get-Content "$env:TEMP\err.txt" -ErrorAction SilentlyContinue) -join ' '
        Say "SIGN $a exit=$($p.ExitCode) written=$(Test-Path $o) roots=$(@(Roots).Count) $err"
    }
} catch {
    Say "ERROR $_"
} finally {
    Say 'END'
}
"""


def running_sandboxes() -> list[str]:
    """The ids `wsb list` prints. Windows Sandbox runs one at a time, and starting
    another attaches to it: its configuration is ignored and its logon command
    never runs. Ending the sandbox's processes leaves the machine behind running."""
    listed = subprocess.run(["wsb", "list"], capture_output=True, text=True, check=True)
    return [line.strip() for line in listed.stdout.splitlines() if line.strip()]


def stop_sandboxes() -> None:
    """Stops what `wsb list` names, which lets go of the mapped folder."""
    for sandbox in running_sandboxes():
        subprocess.run(["wsb", "stop", "--id", sandbox], capture_output=True, check=False)


def configuration(kit: Path) -> str:
    """The sandbox's configuration: the kit mapped in, and the script run at logon."""
    return f"""<Configuration>
  <vGPU>Disable</vGPU>
  <Networking>Enable</Networking>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>{escape(str(kit))}</HostFolder>
      <SandboxFolder>C:\\kit</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\\kit\\check.ps1</Command>
  </LogonCommand>
</Configuration>
"""


def common_faults(lines: list[str]) -> list[str]:
    """What makes a report worthless whatever it says about tpdf."""
    wrong = [f"the sandbox was not clean: it holds {l[10:]}" for l in lines if l.startswith("NOT-CLEAN ")]
    wrong += [f"the script failed: {l[6:]}" for l in lines if l.startswith("ERROR ")]
    if "END" not in lines:
        wrong.append("the report has no end")
    return wrong


def read_verdict(lines: list[str]) -> list[str]:
    """Reading fetches nothing until asked; then the authority's real root arrives
    and the throwaway signer, whose root nobody distributes, stays not trusted."""
    wrong = common_faults(lines)
    expected = {
        "before": "signer=untrusted authority=untrusted hint=True",
        "fetched": "signer=untrusted authority=trusted hint=False",
        "after": "signer=untrusted authority=trusted hint=True",
    }
    for step, want in expected.items():
        rows = [l for l in lines if l.startswith(f"READ {step} ")]
        if len(rows) != 1 or want not in rows[0]:
            wrong.append(f"reading, {step}: wanted {want}, read {rows}")
    return wrong


def verdict(lines: list[str], expect_refused: bool) -> list[str]:
    """What is wrong with a report, as one sentence each; empty when it passes."""
    wrong = common_faults(lines)
    signed = [l for l in lines if l.startswith("SIGN ")]
    for authority in AUTHORITIES:
        rows = [l for l in signed if l.startswith(f"SIGN {authority} ")]
        if len(rows) != 1:
            wrong.append(f"{authority}: {len(rows)} results")
            continue
        accepted = "exit=0 written=True" in rows[0]
        refused = "exit=3 written=False" in rows[0]
        if not (refused if expect_refused else accepted):
            wrong.append(f"{authority}: {rows[0]}")
    return wrong


def one_sandbox(args: argparse.Namespace, reading: bool) -> list[str]:
    """Runs the check in one new sandbox and answers its report, printed; empty
    when the sandbox never ran it. Each run needs its own clean store."""
    with tempfile.TemporaryDirectory(prefix="tpdf-clean-", ignore_cleanup_errors=True) as folder:
        kit = Path(folder)
        shutil.copy(args.installer, kit / "setup.exe")
        shutil.copy(args.document, kit / "document.pdf")
        if args.tool:
            shutil.copy(args.tool, kit / "tpdf-cli.exe")
        if reading:
            (kit / "read.txt").write_text("", encoding="ascii")
        (kit / "check.ps1").write_text(SCRIPT, encoding="ascii")
        (kit / "authorities.txt").write_text("\n".join(AUTHORITIES) + "\n", encoding="ascii")
        (kit / "clean.wsb").write_text(configuration(kit), encoding="utf-8")
        subprocess.Popen([str(SANDBOX), str(kit / "clean.wsb")])
        report = kit / "report.txt"
        deadline = time.monotonic() + LIMIT_SECONDS
        lines: list[str] = []
        while time.monotonic() < deadline:
            time.sleep(2)
            lines = report.read_text(encoding="ascii").splitlines() if report.exists() else []
            if "END" in lines:
                break
        stop_sandboxes()
    for line in lines:
        print(line)
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("installer", type=Path)
    parser.add_argument("document", type=Path)
    parser.add_argument("--tool", type=Path)
    parser.add_argument("--expect-refused", action="store_true")
    args = parser.parse_args()
    if not SANDBOX.exists():
        print("[FAIL] Windows Sandbox is not installed: enable the feature Containers-DisposableClientVM")
        return 2
    already = running_sandboxes()
    if already:
        print(f"[FAIL] a sandbox is running and this one would attach to it: wsb stop --id {already[0]}")
        return 2
    signing = one_sandbox(args, reading=False)
    if not signing:
        print(f"[FAIL] no report within {LIMIT_SECONDS} s: the sandbox did not run the check")
        return 2
    wrong = verdict(signing, args.expect_refused)
    said = f"{len(AUTHORITIES)} authorities {'refused' if args.expect_refused else 'signed'} on a clean store"
    if not args.expect_refused:
        reading = one_sandbox(args, reading=True)
        if not reading:
            print(f"[FAIL] no report within {LIMIT_SECONDS} s: the sandbox did not run the reading check")
            return 2
        wrong += read_verdict(reading)
        said += "; reading fetched nothing until asked, then the authority's root"
    for sentence in wrong:
        print(f"[FAIL] {sentence}")
    if wrong:
        return 1
    print(f"[OK] {said}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
