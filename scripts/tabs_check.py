#!/usr/bin/env python3
"""Exercise document tabs in a built application, using disposable PDF copies.

Usage: uv run scripts/tabs_check.py src-tauri/target/release/tpdf.exe testdata/links.pdf
Requires a visible, unlocked desktop. The check edits and saves only its copies.
Use --phase tabs-position with a three-page-or-longer PDF to check nonzero scroll
restoration and the final page in all zoom modes. A mixed-size regression input
can be generated with pypdf: add_blank_page for (600, 800), (1200, 1600), (600, 400).
For --phase tabs-rotation, use (600, 800), (1200, 400), (600, 800) to expose a
preceding sheet expanding under the old scroll offset.
--phase textedit-grow types past the box the editor opens, on a line with room after
it, and reads the popup's own status line: a longer draft has to preview, a much longer
one has to say the line is full rather than name a box, and typing a width has to hand
the box back to the reader. Growth is only visible in the running application.
It needs a fixture whose "SYNTHETIC FIRST" line has room for four more of its own
characters. testdata/textedit-embedded.pdf does: sixteen more of its own round-trip
there against a 108 pt box. The draft is built from the run's own characters on
purpose -- " AND MORE" is nine characters that do not fit that line, which is a
refusal about those glyphs rather than about growth.
--phase textedit-push types past the room the line has and reads what the application
does about it: a draft longer than the room has to preview rather than refuse, because
the run after it on the line moves along; a draft of 1,000 characters has to say the
page edge stopped it, since that run cannot move any further; applying the first one
has to leave both the replacement and the run it moved on the page, and the line below
untouched. It needs testdata/textedit-push.pdf, which is the only fixture in testdata
whose first line has a second run on it -- both runs of textedit-embedded.pdf are on
lines of their own, so a longer draft there grows the box and moves nothing. Like every
testdata PDF it is generated rather than committed:
  python3 testdata/make_textedit_push.py
  uv run scripts/tabs_check.py <app> testdata/textedit-push.pdf --phase textedit-push
For --phase import, pass --other with a second PDF of at least three pages whose
pages differ in their text from each other and from the first PDF's first page. Past
the file dialog, the palette's page question is dismissed (nothing inserted, the file
released), answered with 2-N (exactly those pages, in order), and left blank (every
page), which is then read, searched, undone, redone and saved into the disposable copy:
  uv run scripts/tabs_check.py <app> testdata/text-base14.pdf --phase import --other testdata/links.pdf
--phase redact-pages reads the sentence a reader is shown after a redaction, off
the message area rather than out of the reply that produced it. It needs
testdata/redact-pages.pdf, whose four marked words each make the report answer
differently -- one survives on the page that was marked, one only on a page that
was not, one in a form object both pages draw, and one is genuinely removed and
must be named nowhere. A fourth pass marks only that last word and must come back
verified. Four passes, so this phase gets a third and a fourth disposable copy;
each pass opens one, marks regions on page 1 through the real IPC, runs
file.redactDocument, confirms at its warning and reads what is left on screen.
file.redactCopy is the sibling command and is not used: it opens a native save
panel, which no phase can answer, and both report through the same sentence.
  python3 testdata/make_redact_pages_pdf.py
  uv run scripts/tabs_check.py <app> testdata/redact-pages.pdf --phase redact-pages
--saved-copy keeps the first pass's redacted output, which is a real redacted
file an independent reader can be pointed at.
--phase recognise runs Recognise text and save as in the window: the palette, the
save panel's suggestion, the toolbar's page line and Stop button, the sentence
afterwards, the copy opened and searched, the refusal of unsaved changes, and a
stopped run. The scan is made here from the fixture's first page, with tpdf-cli
beside the binary; the fixture must show the word "quartz" once.
  uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase recognise
--phase sign signs with a certificate through the window a reader signs in:
Sign document... from the palette, the chooser, a DigiCert timestamp, the saved
copy's properties, Sectigo with long-term data refused, Sign without long-term
data, the remembered choice, and Cancel. NEVER in the gates or in CI: it signs
with a real key from the keychain, so macOS asks the person at the machine to
allow it (three times with "Allow", once with "Always Allow"), and it asks two
timestamp authorities over the network. It refuses to run without --identity,
the SHA-256 of the signing certificate; there is no default. BUILD.md has the
test identity, the command and what the person clicks.
  uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase sign --identity <sha256>
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
    parser.add_argument("--phase", choices=("tabs", "tabs-position", "tabs-rotation", "forms", "signatures", "textedit", "textedit-dash", "textedit-cff-unicode", "textedit-cff-ligatures", "textedit-passport", "textedit-agenda", "textedit-agenda-page2", "textedit-factsheet", "textedit-factsheet-body", "textedit-w3c", "textedit-latin1", "textedit-cid-latin1", "textedit-overhang", "textedit-multipage", "textedit-wrapped", "textedit-wide-spacing", "textedit-list-child", "textedit-grow", "textedit-push", "textedit-w9", "textedit-centred", "import", "redact-pages", "sign", "recognise"), default="tabs")
    parser.add_argument("--other", type=Path, help="The file --phase import inserts pages from")
    parser.add_argument("--identity", help="--phase sign only: the SHA-256 of the signing certificate")
    # 90 s by default; the signing phase waits on a person answering the
    # keychain's prompt three times and on two authorities, so it gets 900.
    parser.add_argument("--timeout", type=float, default=None)
    parser.add_argument("--saved-copy", type=Path, help="Keep the first saved PDF for independent readback")
    args = parser.parse_args()
    if (args.phase == "sign") != (args.identity is not None):
        parser.error("--identity is required by --phase sign, and taken by nothing else")
    if args.timeout is None:
        args.timeout = 900 if args.phase == "sign" else 90
    if args.saved_copy:
        args.saved_copy.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="tpdf-tabs-") as directory:
        room = Path(directory)
        first, second = room / "first.pdf", room / "second.pdf"
        shutil.copyfile(args.pdf, first)
        copies = [first, second]
        if args.phase == "import":
            if not args.other:
                parser.error("--phase import needs --other, a second PDF of three or more pages with different text")
            # A copy as well, so a save that went wrong could not touch the input.
            second = room / "other.pdf"
            copies = [first, second]
            shutil.copyfile(args.other, second)
        else:
            shutil.copyfile(args.pdf, second)
        if args.phase == "redact-pages":
            # A third and a fourth, because a redaction spends the file it is
            # applied to and the phase makes four of them. Each case needs a
            # document with all four of the fixture's words still in it.
            for extra in ("third.pdf", "fourth.pdf"):
                shutil.copyfile(args.pdf, room / extra)
                copies.append(room / extra)
        if args.phase == "recognise":
            # A picture of the fixture's first page, the same picture six times
            # over, and a directory for the copies. Nothing here is the input.
            copied = room / "copies"
            copied.mkdir()
            scan, long = room / "scan.pdf", room / "long.pdf"
            picture = room / "page.png"
            tool = args.binary.resolve().parent / ("tpdf-cli.exe" if os.name == "nt" else "tpdf-cli")
            made = subprocess.run([str(tool), "render", str(first), "-o", str(picture), "--dpi", "200"],
                                  capture_output=True, text=True, timeout=120, check=False)
            if made.returncode != 0:
                print(f"[FAIL] {tool} could not render the fixture: {made.stderr.strip()}")
                return 1
            scan.write_bytes(scan_pdf(picture.read_bytes(), 200, 1))
            long.write_bytes(scan_pdf(picture.read_bytes(), 200, 6))
            copies = [scan, long, copied]
        if args.phase == "sign":
            # The fixture, a directory of its own for the signed copies, and the
            # identity; the copy itself is signed and never written.
            signed = room / "signed"
            signed.mkdir()
            copies = [first, signed]
            copies.append(args.identity)
        joined = "|".join(str(copy) for copy in copies)
        env = dict(os.environ, TPDF_OPENCHECK=f"{args.phase}:{joined}",
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
        if args.phase == "sign":
            passed = signed_files(room / "signed") and passed
        if args.phase == "recognise":
            passed = recognised_files(room / "copies") and passed
        if passed and args.saved_copy:
            shutil.copyfile(first, args.saved_copy)
        return 0 if passed else 1


def scan_pdf(png: bytes, dpi: int, pages: int) -> bytes:
    """A PDF of `pages` pages, each showing the PNG and holding no text.

    Written here because a scan is what the phase needs and `testdata/` has none
    with words a recogniser reads. The PNG is the tool's own render: 8-bit RGBA,
    not interlaced. Its rows are unfiltered and the alpha dropped, since a PDF
    image has no use for either.
    """
    import struct
    import zlib

    assert png[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    at, data, width, height = 8, b"", 0, 0
    while at < len(png):
        (length,) = struct.unpack(">I", png[at:at + 4])
        kind, body = png[at + 4:at + 8], png[at + 8:at + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, colour, interlace) == (8, 6, 0), "expected 8-bit RGBA, not interlaced"
        elif kind == b"IDAT":
            data += body
        at += 12 + length
    raw, stride, rows, before = zlib.decompress(data), width * 4, [], bytes(width * 4)
    for y in range(height):
        kind, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for x in range(stride):
            left = line[x - 4] if x >= 4 else 0
            up, corner = before[x], before[x - 4] if x >= 4 else 0
            if kind == 1:
                line[x] = (line[x] + left) & 255
            elif kind == 2:
                line[x] = (line[x] + up) & 255
            elif kind == 3:
                line[x] = (line[x] + (left + up) // 2) & 255
            elif kind == 4:
                p = left + up - corner
                nearest = min((abs(p - left), 0, left), (abs(p - up), 1, up), (abs(p - corner), 2, corner))[2]
                line[x] = (line[x] + nearest) & 255
        before = bytes(line)
        rows.append(bytes(b for i, b in enumerate(line) if i % 4 != 3))
    pixels = zlib.compress(b"".join(rows))
    w_pt, h_pt = width * 72 / dpi, height * 72 / dpi
    content = f"q {w_pt} 0 0 {h_pt} 0 0 cm /Im0 Do Q".encode()
    kids = " ".join(f"{5 + n} 0 R" for n in range(pages))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Kids [{kids}] /Count {pages} >>".encode(),
        f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB"
        f" /BitsPerComponent 8 /Filter /FlateDecode /Length {len(pixels)} >>\nstream\n".encode()
        + pixels + b"\nendstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
    ] + [
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w_pt} {h_pt}] /Contents 4 0 R"
        f" /Resources << /XObject << /Im0 3 0 R >> >> >>".encode()
        for _ in range(pages)
    ]
    out, offsets = bytearray(b"%PDF-1.7\n"), []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    start = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    out += b"".join(f"{offset:010} 00000 n \n".encode() for offset in offsets)
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode()
    return bytes(out)


def recognised_files(directory: Path) -> bool:
    """What the recognition phase left on disk, read from outside the app.

    The copy that was read to the end is there; the one that was stopped is
    not, and neither is anything staged beside either of them.
    """
    ok = True
    for name, wanted in (("copy.pdf", True), ("stopped.pdf", False)):
        path = directory / name
        present = path.is_file() and path.read_bytes()[:5] == b"%PDF-"
        good = present == wanted
        ok = ok and good
        print(f"[{'OK' if good else 'FAIL'}]   {name}: {'written' if present else 'not written'}")
    extra = sorted(p.name for p in directory.iterdir() if p.name != "copy.pdf")
    print(f"[{'OK' if not extra else 'FAIL'}]   nothing else is left in the directory: {extra}")
    return ok and not extra


def signed_files(directory: Path) -> bool:
    """What the signing phase left on disk, read from outside the app.

    The app's own checks read the sentences; whether each file exists is a fact
    about the disk that the webview cannot see. Written by the first two
    signings, and nothing at all by the cancelled one.
    """
    ok = True
    for name, wanted in (("digicert.pdf", True), ("sectigo.pdf", True), ("cancelled.pdf", False)):
        path = directory / name
        present = path.is_file() and path.read_bytes()[:5] == b"%PDF-"
        good = present == wanted
        ok = ok and good
        verdict = "written" if present else "not written"
        print(f"[{'OK' if good else 'FAIL'}]   the {name} signing: {verdict}"
              f" ({path.stat().st_size if path.exists() else 0} bytes)")
    return ok


if __name__ == "__main__":
    raise SystemExit(main())
