#!/usr/bin/env python3
"""Export the synthetic HTML through an installed Chromium browser, without PDF edits.

uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py <browser> <output-dir>
Uses an isolated temporary profile and Page.printToPDF's generateTaggedPDF flag.
The independent parser verifies both variants; no installed browser profile is used.
Add --flow to export one ordinary paragraph wrapping across two pages.
Use --latin1 --latin1-font Verdana for the existing-glyph accented control;
--latin1 alone exports Arial, whose leading overhang needs placement checks.
Add --rectangles for unchanged text surrounded by painted backgrounds.
Add --headings for a heading and paragraph inside article/section containers.
Add --list for an ordinary numbered list with separately tagged labels.
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


def export(browser, output, flow=False, latin1=False, latin1_font="Arial", rectangles=False, headings=False, numbered_list=False):
    output.mkdir(parents=True, exist_ok=True)
    # A failed rerun must not leave yesterday's successful export as evidence.
    for name in ("browser-tagged.pdf", "browser-untagged.pdf"):
        (output / name).write_bytes(b"")
    source = Path(__file__).with_name(
        "textedit-producer-list.html" if numbered_list else
        "textedit-producer-headings.html" if headings else
        "textedit-producer-rectangles.html" if rectangles else
        "textedit-producer-browser-flow.html" if flow else
        "textedit-producer-browser-latin1.html" if latin1 else "textedit-producer.html"
    ).resolve()
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
                    if latin1:
                        # A font control is authored before printing, never by
                        # normalizing the browser's PDF or its glyph metrics.
                        call("Runtime.evaluate", {"expression":
                             "document.body.style.fontFamily = " + json.dumps(latin1_font)}, session)
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
                        assert len(reader.pages) == (2 if flow else 1), "wrong page count"
                        for page in reader.pages:
                            first = "SYNTHETIC ÄÖÜ äöü ß" if latin1 else "SYNTHETIC FIRST"
                            expected = "1. " + first + " 2. SYNTHETIC SECOND" if numbered_list else first + " SYNTHETIC SECOND"
                            assert " ".join(page.extract_text().split()) == expected, "wrong synthetic text"
                        if (headings or numbered_list) and tagged:
                            pending = [reader.trailer["/Root"]["/StructTreeRoot"]["/K"]]
                            kinds = []
                            visited = 0
                            while pending:
                                visited += 1
                                assert visited <= 128, "unexpectedly large heading tree"
                                item = pending.pop().get_object()
                                if isinstance(item, list):
                                    pending.extend(item)
                                elif isinstance(item, dict):
                                    kinds.append(item.get("/S"))
                                    if "/K" in item:
                                        pending.append(item["/K"])
                            required = {"/L", "/LI", "/Lbl"} if numbered_list else {"/H1", "/P"}
                            assert required.issubset(kinds), "browser omitted required structure roles"
                        if flow and tagged:
                            root = reader.trailer["/Root"]["/StructTreeRoot"]
                            document = root["/K"]
                            paragraph = document["/K"]
                            leaf = paragraph["/K"]
                            assert [node["/S"] for node in (document, paragraph, leaf)] == ["/Document", "/P", "/NonStruct"], "expected one flowing paragraph"
                            items = leaf["/K"]
                            assert len(items) == 2 and items[0] == 0, "wrong flow content items"
                            assert leaf.raw_get("/Pg") == reader.pages[0].indirect_reference, "wrong first-page ownership"
                            assert items[1]["/Type"] == "/MCR" and items[1]["/MCID"] == 0, "wrong continuation item"
                            assert items[1].raw_get("/Pg") == reader.pages[1].indirect_reference, "wrong continuation page"
                            nums = root["/ParentTree"]["/Nums"]
                            assert len(nums) == 4, "wrong parent-tree page count"
                            for index, page in enumerate(reader.pages):
                                assert nums[index * 2] == page["/StructParents"] == index, "wrong page parent key"
                                assert list(nums[index * 2 + 1].get_object()) == [leaf.indirect_reference], "wrong reverse ownership"
                    print(json.dumps({"browser": version["product"], "tagged": True, "untagged": True,
                                      "pages": 2 if flow else 1, "flow": flow, "latin1": latin1, "rectangles": rectangles, "headings": headings, "numbered_list": numbered_list,
                                      "font": latin1_font if latin1 else "Arial"}))
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
    variant = parser.add_mutually_exclusive_group()
    variant.add_argument("--list", dest="numbered_list", action="store_true", help="export a numbered list with separately tagged labels")
    variant.add_argument("--headings", action="store_true", help="export nested heading and paragraph markup")
    variant.add_argument("--rectangles", action="store_true", help="export painted backgrounds around the text")
    variant.add_argument("--flow", action="store_true", help="export one naturally wrapped paragraph across two pages")
    variant.add_argument("--latin1", action="store_true", help="export accented letters using the browser's embedded font")
    parser.add_argument("--latin1-font", choices=("Arial", "Verdana"), default="Arial")
    args = parser.parse_args()
    if args.latin1_font != "Arial" and not args.latin1:
        parser.error("--latin1-font requires --latin1")
    export(args.browser, args.output, args.flow, args.latin1, args.latin1_font, args.rectangles, args.headings, args.numbered_list)
