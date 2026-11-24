#!/usr/bin/env python3
"""Edits a document in an installed tpdf while its update is refused, held and broken.

Usage, on Windows, in a session with a desktop:
    python scripts/update_edit_check.py <tpdf_<previous>_x64-setup.exe>

WHY THIS EXISTS. `update_check.py` presses *Update to <version>* in a window
with nothing unsaved, so the code that guards unsaved work during an update
(`finishUpdate` in `src/lib/update.ts`: the question, and the hold on edits
from the question until the installer starts) had only unit tests behind it,
and every release record from 26.11.6 on lists "the hold on edits while a
Windows update downloads" as not seen in a window. This is that hold, seen.

WHAT IT DOES, in order, against the published release:
  1. starts a local proxy and the old copy with `HTTPS_PROXY` pointing at it,
     so every request of the updater goes through it, on a one-page document
     it writes itself under a folder with a space in its name;
  2. turns the page (Pages, then *Rotate page clockwise*), which is unsaved
     work, and presses the offer: the question must come and name the
     document; *Keep open* must leave the document open and the offer where
     it was;
  3. presses the offer again and answers *Discard and install*. The proxy
     passes the installer at `--rate` KB/s; while the header says
     *Downloading update*, the page is turned twice more, and both must be
     refused by a disabled control;
  4. the proxy cuts the download after `--cut-after` MB. The copy must keep
     running, must say that the update was not installed (26.11.10 and later),
     and must let the reader save: *Save* writes the file, and its page reads
     `/Rotate 90`, which is the one edit made before the question and neither
     of the two tried during the download;
  5. the proxy passes everything. The copy is closed (nothing is unsaved, so
     nothing is asked), started again, and the offer pressed: it must install
     without a question, come back as the new version, and the saved document
     must still read `/Rotate 90`.

The last line reports the first attempt (broken on purpose by the proxy) and
the eventual result separately, as `update_check.py` does for the first launch.

THE PROXY is a CONNECT tunnel: it never sees the bytes in clear, it only paces
and counts them, so the updater's signature check runs on what really came from
GitHub. That `HTTPS_PROXY` is honoured is itself a check, made before the
second press: if no request came through the proxy the download could not be
broken, the install would go ahead, and the run stops there.

NO KEY AND NO CLICK IS SENT. Every press in the window goes through UI
Automation, as `update_check.py`'s does, and the question is answered with
`TDM_CLICK_BUTTON` (see `answer`). Keys sent with `SendInput` reached the page
in two runs of five on 2026-11-17, and clicks on the question missed twice.

CONTROL. `--control edit-after` makes the two turns after the download has
failed instead of during it, when edits are allowed again. The page then reads
`/Rotate 270`, and the run ends with exit code 0 exactly when the two rotation
checks failed and no other check did that passes in a normal run.

The copy, the folder and the registry are handled as in `update_check.py`; it
refuses to start when tpdf is installed or running.

Exit codes: 0 every check passed, 1 a check failed, 2 the run could not start.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import re
import select
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from installed_check import (APP_KEY, NAME, PDF_CLASS, UNINSTALL_KEY, Checks, RanOut,
                             already_installed, bounded, processes, registry, tool, uninstall,
                             window_titles)
from live_output import stream_results
from update_check import (buttons, close, copies, offered, offers, restore_session, set_session_aside,
                          start)
from update_log import SECOND_START_S, explain, served_old

# The checks a `--control edit-after` run must fail, and the one that a copy
# before 26.11.10 fails in every run (it said nothing when a download broke).
EDIT_AFTER = ["the saved page reads /Rotate 90", "after the update the saved page still reads /Rotate 90"]
SAYS_FAILURE = "the copy says the update was not installed"
SAYS_FROM = (26, 11, 10)

# Every element under one window, as `<control type>|<name>`.
ELEMENTS = r"""
param([long]$Window, [string]$Press = "", [switch]$Expandable)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$root = $A::FromHandle([IntPtr]$Window)
foreach ($e in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                             [System.Windows.Automation.Condition]::TrueCondition)) {
    $name = $e.Current.Name
    if (-not $name) { continue }
    $type = $e.Current.ControlType.ProgrammaticName
    $off = if ($e.Current.IsEnabled) { "" } else { "|off" }
    "$type|$name$off"
    # A disabled control is reported, not pressed: that is how the hold on
    # edits shows. The question's two choices are panes without Invoke and are
    # answered by `answer()` instead.
    $pattern = $null
    # `-Expandable` takes only a control that opens something (`aria-expanded`):
    # "Pages" names the tool row's group and other controls before it.
    $fits = -not $Expandable -or $e.TryGetCurrentPattern(
        [System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)
    if ($Press -and $name -eq $Press -and $fits -and $type -in @("ControlType.Button", "ControlType.MenuItem")) {
        # A refused Rotate leaves Pages open. Reuse that menu on the next attempt.
        if ($Expandable -and $e.Current.IsEnabled -and
            $pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded) {
            "OPEN|$name|expanded"
            break
        }
        $pattern = $null
        if (-not $e.Current.IsEnabled) {
            "REFUSED|$name|disabled"
        } elseif ($e.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
            try { $pattern.Invoke(); "PRESSED|$name|invoke" } catch { "REFUSED|$name|$($_.Exception.GetType().Name)" }
        } elseif ($e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
            # A button with `aria-expanded` has ExpandCollapse and no Invoke in
            # WebView2; Expand runs its default action, which is its click.
            try { $pattern.Expand(); "PRESSED|$name|expand" } catch { "REFUSED|$name|$($_.Exception.GetType().Name)" }
        } else {
            "REFUSED|$name|no Invoke"
        }
        break
    }
}
"""


def elements(script: Path, window: int, press: str = "", expandable: bool = False) -> list[str]:
    command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script),
               "-Window", str(window)]
    if press:
        command += ["-Press", press]
    if expandable:
        command += ["-Expandable"]
    try:
        ran = subprocess.run(command, capture_output=True, text=True, encoding="utf-8",
                             errors="replace", timeout=90, check=False)
    except subprocess.TimeoutExpired as e:
        raise RanOut("the window's elements could not be read within 90 s") from e
    return [line.strip() for line in ran.stdout.splitlines() if line.strip()]


# --- The document --------------------------------------------------------------------------

def one_page_pdf() -> bytes:
    """A one-page document with no `/Rotate`, its cross-reference table computed."""
    content = b"BT /F1 24 Tf 72 700 Td (A page to turn) Tj ET"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> "
        b"/Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    out += b"".join(b"%010d 00000 n \n" % o for o in offsets)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    return bytes(out)


def rotation(pdf: Path) -> str:
    """The last `/Rotate` written to the file, which an appended save puts last; `none` without one."""
    found = re.findall(rb"/Rotate\s+(-?\d+)", pdf.read_bytes())
    return found[-1].decode() if found else "none"


# --- The proxy ----------------------------------------------------------------------------

class Tunnels(socketserver.ThreadingTCPServer):
    """A CONNECT proxy that can pace and cut what comes back."""

    daemon_threads = True
    allow_reuse_address = True

    def __init__(self) -> None:
        super().__init__(("127.0.0.1", 0), Tunnel)
        self.mode = "open"          # "open", or "slow" to pace and cut
        self.rate = 150 * 1024      # bytes a second, in "slow"
        self.cut_after = 3 << 20    # bytes back on one tunnel, in "slow"
        self.hosts: list[str] = []
        self.largest = 0            # the most bytes one tunnel carried back
        self.cut = 0                # tunnels cut
        self.lock = threading.Lock()


class Tunnel(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        server: Tunnels = self.server  # type: ignore[assignment]
        head = b""
        while b"\r\n\r\n" not in head and len(head) < 65536:
            part = self.request.recv(4096)
            if not part:
                return
            head += part
        line = head.split(b"\r\n", 1)[0].decode("latin-1")
        verb, _, rest = line.partition(" ")
        if verb != "CONNECT":
            self.request.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            return
        target = rest.split(" ", 1)[0]
        host, _, port = target.rpartition(":")
        with server.lock:
            server.hosts.append(host)
        try:
            upstream = socket.create_connection((host, int(port)), timeout=30)
        except OSError:
            self.request.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            return
        self.request.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        back = 0
        began = time.monotonic()
        pair = [self.request, upstream]
        try:
            while True:
                ready, _, _ = select.select(pair, [], [], 60)
                if not ready:
                    return
                for side in ready:
                    data = side.recv(16384)
                    if not data:
                        return
                    if side is upstream:
                        back += len(data)
                        with server.lock:
                            server.largest = max(server.largest, back)
                        if server.mode == "slow":
                            if back > server.cut_after:
                                with server.lock:
                                    server.cut += 1
                                return
                            # Pace to the rate over the life of the tunnel.
                            due = began + back / server.rate
                            if due > time.monotonic():
                                time.sleep(due - time.monotonic())
                        self.request.sendall(data)
                    else:
                        upstream.sendall(data)
        except OSError:
            return
        finally:
            upstream.close()


# --- The run ------------------------------------------------------------------------------

def header(script: Path, window: int) -> list[str]:
    """The names of the window's buttons."""
    return [line.removeprefix("BUTTON ") for line in buttons(script, window) if line.startswith("BUTTON ")]


def dialog(pid: int, title: str, wait: float) -> int:
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        shown = [hwnd for hwnd, t in window_titles(pid) if t == title]
        if shown:
            return shown[0]
        time.sleep(0.3)
    return 0


UNSAVED = "Unsaved changes"  # the tab's marker, `aria-label` in App.svelte


ROTATE = "Rotate page clockwise"  # an item of the tool row's Pages group


def turn(script: Path, window: int) -> str:
    """Opens the Pages group and presses Rotate page clockwise, through UI Automation.

    Answers what the press printed: `PRESSED|...`, `REFUSED|...` for a
    disabled control, or `not found`. No key or click is sent to the window:
    keys sent with `SendInput` landed in two runs of five on 2026-11-17, and a
    click to give the page the keyboard took the foreground elsewhere.
    """
    opened = elements(script, window, press="Pages", expandable=True)
    if not any(line.startswith(("PRESSED|", "OPEN|")) for line in opened):
        found = next((line for line in opened if line.startswith("REFUSED|")), None)
        return found or f"Pages not found among {[l for l in opened if 'Pages' in l][:6]}"
    time.sleep(0.5)
    pressed = elements(script, window, press=ROTATE)
    found = next((line for line in pressed if line.startswith(("PRESSED|", "REFUSED|"))), None)
    return found or f"{ROTATE} not found; page controls: {[l for l in pressed if 'age' in l][:12]}"


def edit(script: Path, window: int, name: str) -> tuple[bool, str]:
    """Turns the page once; whether the tab then marks it unsaved, and what the press said.

    The second run on 2026-11-17 pressed the offer with nothing unsaved and
    nothing said so. So the tab must be there first, and the marker after.
    """
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and not any(name in line for line in elements(script, window)):
        time.sleep(1)
    said = turn(script, window)
    if not said.startswith("PRESSED|"):
        return False, said
    for _ in range(10):
        time.sleep(0.5)
        if any(UNSAVED in line for line in elements(script, window)):
            return True, said
    return False, f"{said}; no '{UNSAVED}' marker came"


def save(script: Path, window: int, pdf: Path) -> tuple[bool, str]:
    """Presses the header's Save; whether the file was then written."""
    before = pdf.stat().st_mtime_ns
    pressed = elements(script, window, press="Save")
    said = next((line for line in pressed if line.startswith(("PRESSED|", "REFUSED|"))), "Save not found")
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline and pdf.stat().st_mtime_ns == before:
        time.sleep(0.3)
    return pdf.stat().st_mtime_ns != before, said


# The question is rfd's `TaskDialogIndirect` with two custom buttons, numbered
# by rfd 0.17.2 (`backend/win_cid/message_dialog.rs`): OK, here *Discard and
# install*, is 1000, and Cancel, here *Keep open*, is 1001. 26.11.8 and main
# both lock rfd 0.17.2; a new rfd is a reason to read the numbers again.
CHOICES = {"Discard and install": 1000, "Keep open": 1001}
TDM_CLICK_BUTTON = 0x0400 + 102


def answer(checks: Checks, script: Path, pid: int, asked: int, choice: str) -> bool:
    """Presses one choice of the question; True when the question is then gone.

    The two choices are panes without Invoke, and a click on their centre
    missed twice on 2026-11-17, so the dialog is told which button was pressed
    with `TDM_CLICK_BUTTON`, the message a task dialog takes for exactly that.
    The labels are read first, and both must be there: a question whose
    choices were renamed or replaced stops the run here. Which number each
    label has is not read from the dialog --- the element listing carries no
    button ids --- so the numbers rest on `CHOICES` and the rfd version it
    names, and the check after the press is what says the question went.

    Every later step assumes the question is gone. On the first run that day
    it stayed up and a check after it passed with the dialog still open; so
    the run stops here instead.
    """
    shown = elements(script, asked)
    if not all(any(line.endswith(f"|{label}") for line in shown) for label in CHOICES):
        return checks.note(False, f"{choice} closes the question", f"the choices read {shown}")
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.PostMessageW(ctypes.c_void_p(asked), TDM_CLICK_BUTTON, CHOICES[choice], 0)
    gone_by = time.monotonic() + 10
    while dialog(pid, "Install update", 0.5) and time.monotonic() < gone_by:
        pass
    return checks.note(not dialog(pid, "Install update", 0.5), f"{choice} closes the question",
                       f"button {CHOICES[choice]} posted")


def version_of(name: str) -> tuple[int, ...]:
    return tuple(int(part) for part in name.split("."))


def run(checks: Checks, proxy: Tunnels, folder: Path, pdf: Path, scratch: Path, old: str, new: str,
        wait: float, control: str | None, result: dict[str, str]) -> None:
    script = scratch / "buttons.ps1"
    from update_check import AUTOMATION
    script.write_text(AUTOMATION, encoding="utf-8")
    everything = scratch / "elements.ps1"
    everything.write_text(ELEMENTS, encoding="utf-8")
    log = scratch / "tpdf.log"
    address = f"http://127.0.0.1:{proxy.server_address[1]}"
    env = dict(os.environ, TPDF_SESSION_FILE=str(scratch / "session.json"), TPDF_LOG_FILE=str(log),
               HTTPS_PROXY=address, HTTP_PROXY=address, ALL_PROXY=address, NO_PROXY="")
    offer = f"Update to {new}"

    print(f"--- {old} opens the document, through the proxy at {address} ---")
    app, window = start(folder, pdf, env, scratch, old, wait)
    try:
        checks.note(bool(window), f"a window titled 'tpdf v{old}' is shown")
        if not window:
            return
        seen = offers(script, window, new, wait)
        if f"BUTTON {offer}" not in seen and served_old(log):
            print(f"[INFO] {old} was told it is current ({explain(log)}); started again in {SECOND_START_S} s")
            close([app.pid])
            time.sleep(SECOND_START_S)
            app, window = start(folder, pdf, env, scratch, old, wait)
            seen = offers(script, window, new, wait) if window else []
        checks.note(f"BUTTON {offer}" in seen, f"the window offers '{offer}'")
        through = sorted(set(proxy.hosts))
        if not checks.note(any("github" in h for h in through),
                           "the updater's requests went through the proxy", ", ".join(through) or "none"):
            return
        if f"BUTTON {offer}" not in seen:
            return

        print("--- an edit, then the question, answered Keep open ---")
        # Paced and cut from here on, so that a press with nothing unsaved,
        # which installs without asking, cannot finish an install either.
        proxy.mode = "slow"
        turned, said = edit(everything, window, pdf.name)
        if not checks.note(turned, f"{ROTATE} turns the page, and the tab marks unsaved changes", said):
            return
        pressed = buttons(script, window, press=offer)
        checks.note(f"PRESSED {offer}" in pressed, "the offer is pressed")
        asked = dialog(app.pid, "Install update", 20)
        if not checks.note(bool(asked), "the question 'Install update' comes"):
            return
        said = elements(everything, asked)
        checks.note(any(pdf.name in line for line in said), "the question names the document",
                    "; ".join(said[:6]))
        if not answer(checks, everything, app.pid, asked, "Keep open"):
            return
        time.sleep(1)
        checks.note(app.poll() is None and offer in header(script, window),
                    "after Keep open the copy runs and the offer is still there")

        print(f"--- the question answered Discard and install; the proxy paces to "
              f"{proxy.rate // 1024} KB/s and cuts at {proxy.cut_after >> 20} MB ---")
        buttons(script, window, press=offer)
        asked = dialog(app.pid, "Install update", 20)
        if not checks.note(bool(asked), "the question comes again, so the edit is still unsaved"):
            return
        if not answer(checks, everything, app.pid, asked, "Discard and install"):
            return
        downloading: list[str] = []
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and not downloading:
            downloading = [b for b in header(script, window) if b.startswith("Downloading update")]
            time.sleep(0.5)
        checks.note(bool(downloading), "the header says the update is downloading", ", ".join(downloading))
        if control != "edit-after":
            during = [turn(everything, window) for _ in range(2)]
            still = any(b.startswith("Downloading update") for b in header(script, window))
            checks.note(still, "the download still runs after two tries to turn the page",
                        "; ".join(during))
            # `REFUSED|<name>|disabled` and not any refusal: a press refused
            # because the control has no Invoke, or threw, is not the edit
            # commands being disabled, and would pass this with the download
            # not holding them at all.
            checks.note(all(said.startswith("REFUSED|") and said.endswith("|disabled") for said in during),
                        "the edit commands are disabled while the download runs", "; ".join(during))

        print("--- the proxy cuts the download ---")
        deadline = time.monotonic() + wait * 3
        while time.monotonic() < deadline and app.poll() is None:
            now = header(script, window)
            if not any(b.startswith("Downloading update") for b in now):
                break
            time.sleep(1)
        checks.note(proxy.cut > 0, "the proxy cut the download", f"largest tunnel {proxy.largest:,} bytes")
        checks.note(app.poll() is None, "the copy keeps running after the download broke")
        if app.poll() is not None:
            return
        result["first"] = "failed (the download was cut by this check's proxy, as intended)"
        time.sleep(2)
        shown = elements(everything, window)
        told = [line for line in shown if "could not be installed" in line]
        checks.note(bool(told), SAYS_FAILURE, told[0] if told else "nothing names the failure")
        if control == "edit-after":
            for _ in range(2):
                print(f"[INFO] control: {turn(everything, window)}")

        print("--- the reader saves ---")
        written, said = save(everything, window, pdf)
        checks.note(written, "Save writes the document", said)
        time.sleep(1)
        turned = rotation(pdf)
        checks.note(turned == "90", "the saved page reads /Rotate 90",
                    f"/Rotate {turned}: one turn kept from before the question, "
                    "the two pressed during the download refused" if turned == "90" else f"/Rotate {turned}")

        print("--- the proxy passes everything; the copy is started again and updated ---")
        proxy.mode = "open"
        # Gone is not enough: `close` kills a copy still there after 20 s,
        # which is exactly what a copy that asked would be.
        killed = close([app.pid])
        checks.note(app.poll() is not None and not killed,
                    "the copy closes without asking, nothing being unsaved",
                    f"killed after 20 s: {killed}" if killed else "")
        app, window = start(folder, pdf, env, scratch, old, wait)
        seen = offers(script, window, new, wait) if window else []
        if not checks.note(f"BUTTON {offer}" in seen, f"the second start offers '{offer}'"):
            return
        buttons(script, window, press=offer)
        checks.note(not dialog(app.pid, "Install update", 5), "nothing unsaved, so nothing is asked")
        try:
            app.wait(timeout=wait * 5)
            ended = True
        except subprocess.TimeoutExpired:
            ended = False
        checks.note(ended, "the copy ends for the installer")
        title = f"tpdf v{new}"
        windows: list[tuple[int, str]] = []
        deadline = time.monotonic() + wait * 3
        while time.monotonic() < deadline and not windows:
            windows = [t for pid in copies(folder) for t in window_titles(pid) if t[1].startswith(title)]
            time.sleep(0.5)
        checks.note(bool(windows), f"the installer starts '{title}'")
        said = tool(folder / "tpdf-cli.exe", "--version").stdout.strip()
        checks.note(said.endswith(new), f"the installed tool is version {new}", said)
        result["eventual"] = f"installed {new} on the second attempt" if windows else "not installed"
        checks.note(rotation(pdf) == "90", "after the update the saved page still reads /Rotate 90",
                    f"/Rotate {rotation(pdf)}")
    finally:
        running = copies(folder)
        close(sorted(set(running) | ({app.pid} if app.poll() is None else set())))


def main() -> int:
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("installer", help="the installer of a release before the published one")
    parser.add_argument("--wait", type=float, default=60, help="seconds to wait for a window")
    parser.add_argument("--rate", type=int, default=150, help="KB/s the proxy passes the download at")
    parser.add_argument("--cut-after", type=int, default=3, help="MB after which the proxy cuts it")
    parser.add_argument("--control", choices=["edit-after"],
                        help="break one thing on purpose; exit 0 when exactly its checks fail")
    args = parser.parse_args()

    if sys.platform != "win32":
        print("[ERROR] this check runs the Windows installer; run it on Windows")
        return 2
    installer = Path(args.installer).resolve()
    named = NAME.match(installer.name)
    if not installer.is_file() or not named:
        print(f"[ERROR] {installer} is not a tpdf installer that names its version")
        return 2
    old = named.group(1)
    if version_of(old) < (26, 11, 6):
        print(f"[ERROR] {old} has no hold on edits during an update; give 26.11.6 or later")
        return 2
    try:
        new = offered()
    except (RuntimeError, OSError, ValueError, KeyError) as e:
        print(f"[ERROR] {e}")
        return 2
    if new == old:
        print(f"[ERROR] latest.json offers {new}, which is the installer given; give an older one")
        return 2
    listing = registry(UNINSTALL_KEY)
    if listing is not None:
        print(already_installed(listing))
        return 2
    if any(image.lower() == "tpdf.exe" for _, _, image in processes()):
        print("[ERROR] tpdf is running; a second start would be handed to it")
        return 2
    folder = Path(os.environ["LOCALAPPDATA"]) / "tpdf-update-edit-check"
    if folder.exists():
        print(f"[ERROR] {folder} exists; remove it and run again")
        return 2

    proxy = Tunnels()
    proxy.rate = args.rate * 1024
    proxy.cut_after = args.cut_after << 20
    threading.Thread(target=proxy.serve_forever, daemon=True).start()

    if args.control:
        print(f"--- control '{args.control}': the two turns are pressed after the failure ---")
    checks = Checks()
    result = {"first": "not reached", "eventual": "not reached"}
    before = {key: registry(key) for key in (PDF_CLASS, APP_KEY)}
    real_session = Path(os.environ["APPDATA"]) / "com.timostein.tpdf" / "session.json"
    with tempfile.TemporaryDirectory(prefix="tpdf-update-edit-check-") as temporary:
        scratch = Path(temporary)
        kept = scratch / "session-as-it-was.json"
        set_aside = False
        try:
            set_session_aside(real_session, kept)
            set_aside = True
            spaced = scratch / "a folder with a space"
            spaced.mkdir()
            document = spaced / "turn me.pdf"
            document.write_bytes(one_page_pdf())
            print(f"--- latest.json offers {new}; installing {old} into {folder} ---")
            code = bounded(f'"{installer}" /S /D={folder}', "the installer", 300)
            checks.note(code == 0, "the installer ends with exit code 0", f"exit {code}")
            run(checks, proxy, folder, document, scratch, old, new, args.wait, args.control, result)
        except (RanOut, RuntimeError, OSError) as e:
            checks.note(False, "every step ends, and within its time limit", str(e))
        finally:
            try:
                restore_session(real_session, kept, set_aside)
            finally:
                try:
                    proxy.shutdown()
                finally:
                    uninstall(checks, folder, before, run_it=True)

    print()
    summary = f"first attempt: {result['first']}; eventually: {result['eventual']}"
    # A copy before 26.11.10 says nothing when a download breaks; that line is a
    # finding about the old copy and stays red until such a copy is the old one.
    known = [SAYS_FAILURE] if version_of(old) < SAYS_FROM else []
    if args.control:
        unexpected = [n for n in checks.failed if n not in EDIT_AFTER and n not in known]
        missing = [n for n in EDIT_AFTER if n not in checks.failed]
        if unexpected or missing:
            print(f"[FAIL] control: did not fail but should have: {missing or 'nothing'}; "
                  f"failed but should not have: {unexpected or 'nothing'}")
            return 1
        print(f"[OK] control: what it is aimed at failed ({len(EDIT_AFTER)}), and nothing else")
        return 0
    if checks.failed:
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}; {summary}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed; {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
