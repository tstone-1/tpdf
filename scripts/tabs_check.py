#!/usr/bin/env python3
"""Exercise document tabs in a built application, using disposable PDF copies.

Usage: uv run scripts/tabs_check.py src-tauri/target/release/tpdf.exe testdata/links.pdf
Requires a visible, unlocked desktop. The check edits and saves only its copies.
Use --phase tabs-position with a three-page-or-longer PDF to check nonzero scroll
restoration and the final page in all zoom modes. A mixed-size regression input
can be generated with pypdf: add_blank_page for (600, 800), (1200, 1600), (600, 400).
For --phase tabs-rotation, use (600, 800), (1200, 400), (600, 800) to expose a
preceding sheet expanding under the old scroll offset.
For --phase import, pass --other with a second PDF of at least three pages whose
pages differ in their text from each other and from the first PDF's first page. Past
the file dialog, the palette's page question is dismissed (nothing inserted, the file
released), answered with 2-N (exactly those pages, in order), and left blank (every
page), which is then read, searched, undone, redone and saved into the disposable copy:
  uv run scripts/tabs_check.py <app> testdata/text-base14.pdf --phase import --other testdata/links.pdf
"""

import argparse
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile

from harness_launch import report
from win_worker_exit import check_worker_exit


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("pdf", type=Path)
    parser.add_argument("--phase", choices=("tabs", "tabs-position", "tabs-rotation", "forms", "signatures", "textedit", "textedit-dash", "textedit-cff-unicode", "textedit-cff-ligatures", "textedit-passport", "textedit-agenda", "textedit-agenda-page2", "textedit-w3c", "textedit-latin1", "textedit-cid-latin1", "textedit-overhang", "textedit-multipage", "textedit-wrapped", "textedit-wide-spacing", "textedit-list-child", "import"), default="tabs")
    parser.add_argument("--other", type=Path, help="The file --phase import inserts pages from")
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--saved-copy", type=Path, help="Keep the first saved PDF for independent readback")
    args = parser.parse_args()
    if args.saved_copy:
        args.saved_copy.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="tpdf-tabs-") as directory:
        room = Path(directory)
        first, second = room / "first.pdf", room / "second.pdf"
        shutil.copyfile(args.pdf, first)
        if args.phase == "import":
            if not args.other:
                parser.error("--phase import needs --other, a second PDF of three or more pages with different text")
            # A copy as well, so a save that went wrong could not touch the input.
            second = room / "other.pdf"
            shutil.copyfile(args.other, second)
        else:
            shutil.copyfile(args.pdf, second)
        env = dict(os.environ, TPDF_OPENCHECK=f"{args.phase}:{first}|{second}",
                   TPDF_SESSION_FILE=str(room / "session.json"))
        if os.name == "nt":
            # Keep this unattended check running behind other windows without
            # changing the flags used by the shipped application.
            env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
                env.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
                + " --disable-background-timer-throttling --disable-renderer-backgrounding"
                + " --disable-backgrounding-occluded-windows"
            )
        # A worker can inherit stdout. A file keeps an orphaned worker from
        # holding communicate() open after the timeout has killed its parent.
        # WebView2 can retain its inherited log handle briefly after the app
        # exits. Keep the diagnostic artifact outside the disposable PDF folder.
        logs = Path(__file__).resolve().parent.parent / "scratch"
        logs.mkdir(exist_ok=True)
        with tempfile.NamedTemporaryFile(mode="wb", prefix="tabs-check-", suffix=".log",
                                         dir=logs, delete=False) as output:
            log = Path(output.name)
            process = subprocess.Popen(
                [str(args.binary.resolve())], env=env, stdout=output,
                stderr=subprocess.STDOUT, start_new_session=os.name != "nt",
            )
            try:
                code = process.wait(timeout=args.timeout)
            except subprocess.TimeoutExpired:
                if os.name == "nt":
                    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                   capture_output=True, timeout=10, check=False)
                else:
                    os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
                print("[FAIL] tab check timed out")
                code = 1
        passed = report(log.read_text(encoding="utf-8", errors="replace"), code, phase=args.phase)
        passed = check_worker_exit(process, args.binary) and passed
        if passed and args.saved_copy:
            shutil.copyfile(first, args.saved_copy)
        return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
