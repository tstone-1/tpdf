#!/usr/bin/env python3
"""Regenerate the README's screenshots and its animation in docs/img/.

    uv run --with reportlab --with pillow scripts/screenshots.py <checks-binary>

Five stills and `demo.gif`, from one run: every state the application holds is
a frame of the animation, and five of those frames are also the stills.

macOS only, and it needs a visible, unlocked desktop: it opens a tpdf window
for about half a minute and photographs that window, and nothing else on the
screen, each time the application reports a state is ready. The window does not
need to be in front and nothing is typed, so the machine stays usable.

`<checks-binary>` is a check build, as for `tabs_check.py` (BUILD.md has the
command): a release binary ignores TPDF_OPENCHECK and would sit there until the
timeout. The terminal running this needs the Screen Recording permission, or
`screencapture` writes a picture of the desktop with no window in it --- which
is why the size of every picture is checked against the window's.

What it shows comes from two documents, both generated: `testdata/demo.pdf`,
written here by `testdata/make_demo_pdf.py`, and `testdata/incr-two-signers.pdf`
for the signature cards (`testdata/make_incremental_pdf.py` writes that one).
The states themselves are `src/lib/screenshotcheck.ts`.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "testdata"))

from PIL import Image  # noqa: E402

from harness_launch import report  # noqa: E402
from make_demo_pdf import build  # noqa: E402

#: Every state `screenshotcheck.ts` announces. A run that produced fewer fails.
SHOTS = ("reading", "palette", "annotate", "redact", "signatures")

#: How wide a committed picture is. The window is 1200 points, so this is 1.33x.
WIDTH_PX = 1600

#: How wide the animation is. A GIF has 256 colours and no compression between
#: frames worth the name, so it is the size that decides the file's weight.
GIF_WIDTH_PX = 960

#: Fewer frames than this is a run that stopped early, whatever else was green.
FEWEST_FRAMES = 12


def animate(frames: list[tuple[Path, int]], out: Path) -> None:
    """Writes the frames as one looping GIF, each shown for its own time."""
    pictures = []
    for path, _ in frames:
        with Image.open(path) as full:
            height = round(full.height * GIF_WIDTH_PX / full.width)
            pictures.append(full.convert("RGB").resize((GIF_WIDTH_PX, height), Image.LANCZOS))
    # One palette for every frame, taken from all of them side by side: a
    # palette per frame makes the unchanged toolbar shimmer between frames.
    strip = Image.new("RGB", (GIF_WIDTH_PX, pictures[0].height * len(pictures)))
    for at, picture in enumerate(pictures):
        strip.paste(picture, (0, at * picture.height))
    # Octree at 256, measured against median cut at 128: that one turned the
    # red box brown and the traffic lights grey, for 6 KB a frame.
    palette = strip.quantize(colors=256, method=Image.FASTOCTREE, dither=Image.Dither.NONE)
    indexed = [p.quantize(palette=palette, dither=Image.Dither.NONE) for p in pictures]
    indexed[0].save(out, save_all=True, append_images=indexed[1:], loop=0, optimize=True,
                    duration=[shown for _, shown in frames], disposal=1)

#: The document window a process owns, by its CoreGraphics number. Swift
#: because the window list is a CoreGraphics call and Python has no binding for
#: it without a dependency; run once per launch, since the number does not change.
WINDOW_OF = """
import CoreGraphics
let pid = Int(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
var best = (area: 0.0, number: 0)
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid
    && (w[kCGWindowLayer as String] as? Int) == 0 {
    let b = w[kCGWindowBounds as String] as! [String: Double]
    // The process also owns a strip the width of the screen and 33 points
    // tall, which is its menu bar; the document window is the tall one.
    if b["Height"]! > 300 && b["Width"]! * b["Height"]! > best.area {
        best = (b["Width"]! * b["Height"]!, w[kCGWindowNumber as String] as! Int)
    }
}
if best.number != 0 { print(best.number) }
"""


def window_of(pid: int, script: Path, deadline: float) -> int:
    """The window's number, once the application has made one."""
    while time.monotonic() < deadline:
        found = subprocess.run(["swift", str(script), str(pid)], capture_output=True, text=True)
        if found.stdout.strip().isdigit():
            return int(found.stdout.strip())
        time.sleep(0.5)
    raise SystemExit("[FAIL] the application opened no window")


def png_size(path: Path) -> tuple[int, int]:
    """Width and height from a PNG's header."""
    with path.open("rb") as file:
        head = file.read(24)
    return struct.unpack(">II", head[16:24])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--out", type=Path, default=ROOT / "docs" / "img")
    parser.add_argument("--signed", type=Path, default=ROOT / "testdata" / "incr-two-signers.pdf")
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--dark", action="store_true",
                        help="take the pictures with the window in its dark appearance; "
                             "needs --out, because the README's pictures are the light ones")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise SystemExit("[FAIL] screenshots are taken on macOS only")
    if args.dark and args.out == ROOT / "docs" / "img":
        raise SystemExit("[FAIL] --dark needs --out: docs/img holds the README's light pictures")
    if not args.signed.is_file():
        raise SystemExit(f"[FAIL] {args.signed} is missing; testdata/make_incremental_pdf.py writes it")
    args.out.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="tpdf-shots-") as directory:
        room = Path(directory)
        demo, signed = room / "Harbour Tide Survey 2026.pdf", room / "Signed agreement.pdf"
        regions = build(demo)
        # Kept beside the others as well, so the same document can be opened by hand.
        shutil.copyfile(demo, ROOT / "testdata" / "demo.pdf")
        shutil.copyfile(args.signed, signed)
        swift = room / "window.swift"
        swift.write_text(WINDOW_OF)
        env = dict(os.environ,
                   TPDF_OPENCHECK=f"screenshots:{demo}|{signed}|{json.dumps(regions, separators=(',', ':'))}",
                   TPDF_SESSION_FILE=str(room / "session.json"),
                   # Pinned both ways, so the README's pictures do not depend on
                   # how the machine that takes them happens to be set.
                   TPDF_THEME="dark" if args.dark else "light")
        log = room / "transcript.log"
        taken: list[str] = []
        frames: list[tuple[Path, int]] = []
        (room / "frames").mkdir()
        with log.open("wb") as output:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=output,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            deadline = time.monotonic() + args.timeout
            try:
                window = window_of(process.pid, swift, deadline)
                while process.poll() is None and time.monotonic() < deadline:
                    for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
                        # `FRAME <index> <milliseconds shown> [<still name>]`
                        parts = line.split()
                        if not line.startswith("FRAME ") or int(parts[1]) < len(frames):
                            continue
                        picture = room / "frames" / f"{int(parts[1]):03}.png"
                        # -o: no drop shadow, so the picture is the window. -x: no sound.
                        subprocess.run(["screencapture", "-x", "-o", f"-l{window}", str(picture)],
                                       check=True)
                        frames.append((picture, int(parts[2])))
                        if len(parts) > 3:
                            # Two thirds of a Retina capture: five pictures at
                            # full size are 2.2 MB in the repository on every regeneration.
                            subprocess.run(["sips", "--resampleWidth", str(WIDTH_PX), str(picture),
                                            "--out", str(args.out / f"{parts[3]}.png")],
                                           check=True, capture_output=True)
                            taken.append(parts[3])
                    time.sleep(0.1)
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    print("[FAIL] the application did not finish in time")
                code = process.wait(timeout=10)
        passed = report(log.read_text(encoding="utf-8", errors="replace"), code, phase="screenshots")
        if len(frames) >= FEWEST_FRAMES:
            animate(frames, args.out / "demo.gif")
            print(f"[OK]   demo.gif  {len(frames)} frames, "
                  f"{sum(shown for _, shown in frames) / 1000:.1f} s, "
                  f"{(args.out / 'demo.gif').stat().st_size // 1024} KB")
        else:
            print(f"[FAIL] only {len(frames)} frames, so no animation was written")
            passed = False

    sizes = {name: png_size(args.out / f"{name}.png") for name in taken}
    for name in SHOTS:
        if name not in sizes:
            print(f"[FAIL] no picture of {name}")
            passed = False
        else:
            print(f"[OK]   {name}.png  {sizes[name][0]} x {sizes[name][1]}")
    # One window, so one size. A picture of another size is a picture of
    # something else --- the desktop, when Screen Recording is not granted.
    if len(set(sizes.values())) > 1:
        print(f"[FAIL] the pictures differ in size: {sorted(set(sizes.values()))}")
        passed = False
    # And a window, not a strip: the first version of this photographed the
    # menu bar five times, 3024 x 66, and every check above was green.
    if any(height < 600 for _, height in sizes.values()):
        print("[FAIL] a picture is too short to be the document window")
        passed = False
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
