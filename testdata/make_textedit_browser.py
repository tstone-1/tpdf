#!/usr/bin/env python3
"""Export the synthetic HTML through an installed Chromium browser, without PDF edits.

uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py <browser> <output-dir>
Uses an isolated temporary profile and Page.printToPDF's generateTaggedPDF flag.
The independent parser verifies both variants; no installed browser profile is used.
https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-printToPDF
"""
import argparse
import base64
import json
from pathlib import Path
import subprocess
import tempfile
import time

from pypdf import PdfReader
import websocket


def export(browser, output):
    output.mkdir(parents=True, exist_ok=True)
    # A failed rerun must not leave yesterday's successful export as evidence.
    for name in ("browser-tagged.pdf", "browser-untagged.pdf"):
        (output / name).write_bytes(b"")
    source = Path(__file__).with_name("textedit-producer.html").resolve()
    with tempfile.TemporaryDirectory(prefix="tpdf-browser-") as profile:
        with (output / "browser.log").open("wb") as log:
            process = subprocess.Popen([str(browser.resolve()), "--headless",
                "--remote-debugging-port=0", "--disable-background-networking",
                "--no-first-run", f"--user-data-dir={profile}", "about:blank"],
                stdout=log, stderr=log)
            try:
                port_file = Path(profile) / "DevToolsActivePort"
                deadline = time.monotonic() + 30
                while not port_file.exists():
                    if process.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError("browser debugging endpoint did not start")
                    time.sleep(0.1)
                port, endpoint = port_file.read_text().splitlines()
                connection = websocket.create_connection(f"ws://127.0.0.1:{int(port)}{endpoint}",
                                                          timeout=30, suppress_origin=True)
                serial = 0

                def call(method, params=None, session=None):
                    nonlocal serial
                    serial += 1
                    message = {"id": serial, "method": method, "params": params or {}}
                    if session:
                        message["sessionId"] = session
                    connection.send(json.dumps(message))
                    until = time.monotonic() + 30
                    while time.monotonic() < until:
                        reply = json.loads(connection.recv())
                        if reply.get("id") == serial:
                            if "error" in reply:
                                raise RuntimeError(reply["error"])
                            return reply["result"]
                    raise RuntimeError("browser command timed out")

                try:
                    version = call("Browser.getVersion")
                    target = call("Target.createTarget", {"url": "about:blank"})["targetId"]
                    session = call("Target.attachToTarget", {"targetId": target, "flatten": True})["sessionId"]
                    call("Page.enable", session=session)
                    navigation = call("Page.navigate", {"url": source.as_uri()}, session)
                    if navigation.get("errorText"):
                        raise RuntimeError(navigation["errorText"])
                    until = time.monotonic() + 30
                    while time.monotonic() < until:
                        ready = call("Runtime.evaluate", {"expression": "document.readyState"}, session)
                        if ready["result"].get("value") == "complete":
                            break
                        time.sleep(0.1)
                    else:
                        raise RuntimeError("synthetic page did not finish loading")
                    fonts = call("Runtime.evaluate", {"expression": "document.fonts.ready.then(() => true)",
                                 "awaitPromise": True}, session)
                    if fonts.get("exceptionDetails") or fonts["result"].get("value") is not True:
                        raise RuntimeError("synthetic fonts did not finish loading")
                    for tagged in (True, False):
                        pdf = call("Page.printToPDF", {"generateTaggedPDF": tagged,
                            "preferCSSPageSize": True, "displayHeaderFooter": False,
                            "printBackground": True}, session)
                        path = output / ("browser-tagged.pdf" if tagged else "browser-untagged.pdf")
                        path.write_bytes(base64.b64decode(pdf["data"], validate=True))
                        reader = PdfReader(path)
                        assert ("/StructTreeRoot" in reader.trailer["/Root"]) == tagged, "browser ignored tagging request"
                        assert len(reader.pages) == 1, "wrong page count"
                        assert " ".join(reader.pages[0].extract_text().split()) == "SYNTHETIC FIRST SYNTHETIC SECOND", "wrong synthetic text"
                    print(json.dumps({"browser": version["product"], "tagged": True, "untagged": True}))
                finally:
                    connection.close()
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("browser", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    export(args.browser, args.output)
