"""Own one macOS UI-check instance without touching an already running app.

Pass a separately identified .app when the installed TPDF is open. LaunchServices
still launches the bundle, while Accessibility and cleanup target its owned PID.
"""
from __future__ import annotations

import atexit
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import time


def osa(script: str, timeout: float = 15) -> str:
    result = subprocess.run(["osascript", "-e", script], capture_output=True,
                            text=True, timeout=timeout, check=True)
    return result.stdout.strip()


class MacCheckApp:
    def __init__(self, bundle: Path):
        self.bundle = bundle.resolve()
        with (self.bundle / "Contents/Info.plist").open("rb") as source:
            info = plistlib.load(source)
        self.identifier = json.dumps(info["CFBundleIdentifier"])
        self.name = info["CFBundleName"]
        self.binary = self.bundle / "Contents/MacOS" / info["CFBundleExecutable"]
        self.pid: int | None = None
        self.started = ""

    def _pids(self) -> list[int]:
        answer = osa('tell application "System Events" to get unix id of '
                     f'(every process whose bundle identifier is {self.identifier})')
        return [int(part.strip()) for part in answer.split(",") if part.strip()]

    def start(self, document: Path | None = None) -> None:
        if self._pids():
            raise RuntimeError("An application with this bundle identifier is already running; "
                               "use a separately identified test bundle.")
        args = ["open", "-a", str(self.bundle)]
        if document is not None:
            args.append(str(document.resolve()))
        subprocess.run(args, check=True)
        for _ in range(60):
            pids = self._pids()
            if len(pids) > 1:
                raise RuntimeError("More than one instance appeared; refusing ambiguous ownership")
            if pids:
                candidate = pids[0]
                executable = subprocess.check_output(
                    ["ps", "-p", str(candidate), "-o", "comm="], text=True).strip()
                if executable != str(self.binary):
                    raise RuntimeError("LaunchServices opened a different bundle; refusing to drive it")
                self.pid = candidate
                self.started = self._stamp()
                if not self.started:
                    raise RuntimeError("Test process exited during launch")
                atexit.register(self.quit)
                return
            time.sleep(0.25)
        raise RuntimeError("No test application appeared after launch")

    def _stamp(self) -> str:
        result = subprocess.run(["ps", "-p", str(self.pid), "-o", "lstart=", "-o", "comm="],
                                capture_output=True, text=True)
        return result.stdout.strip() if result.returncode == 0 else ""

    @property
    def process(self) -> str:
        if self.pid is None:
            raise RuntimeError("No owned test process")
        return f'(first process whose unix id is {self.pid})'

    def activate(self) -> None:
        osa(f'tell application "System Events" to tell {self.process} to set frontmost to true')

    def kill(self) -> None:
        if self.pid is not None and self._stamp() == self.started:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

    def quit(self) -> None:
        if self.pid is None or self._stamp() != self.started:
            return
        try:
            osa('use framework "AppKit"\n'
                "set targetApp to current application's NSRunningApplication's "
                f'runningApplicationWithProcessIdentifier:{self.pid}\n'
                "targetApp's terminate()", timeout=5)
        except (subprocess.SubprocessError, OSError):
            self.kill()
        for _ in range(20):
            if self._stamp() != self.started:
                return
            time.sleep(0.1)
        self.kill()
