#!/usr/bin/env python3
"""Pastes the page copy on the clipboard into PowerPoint and lists what the slide holds.

*Copy page for PowerPoint* is only finished when PowerPoint has taken it apart:
`tests/cli/pagecopy.rs` reads the metafile's records, and this reads the slide.
It drives PowerPoint through COM: a new presentation, `Shapes.PasteSpecial` of
the enhanced metafile, `Ungroup` until no picture or group is left, the text of
every shape, a save, a reopen, and the text again. The slide is exported as a
PNG beside the saved file.

**Why the shapes are counted and not only the picture looked at** (2026-11-07):
a word drawn as outlines under a white box looks hidden in the exported slide,
but *Ungroup* makes the box a shape of its own, and moving it shows the word.
26.11.4 passed the picture check and failed this one with 36 shapes where the
page shows 7. A hidden word is hidden only if no shape carries it.

Usage, on Windows with PowerPoint installed, after a copy is on the clipboard:

    python scripts/powerpoint_check.py --text "Visible note" --text "Inside page clip" \\
        --text "Struck words" --shapes 7

which is what page 1 of the hidden-words fixture of `tests/cli/pagecopy.rs` must
give. That fixture is written to `%TEMP%\\tpdf-cli-it-<pid>-pagecopy\\hidden.pdf`
by every `cargo test --test cli`. Copy it with the installed application
(*Copy page for PowerPoint* from the palette, page 1 in view), or with the
test's own route: `TPDF_PAGECOPY_FROM=<that file> TPDF_PAGECOPY_CLIPBOARD=1
cargo test --locked --test cli`. Exit 0 when the slide holds exactly the texts
given, before and after the reopen, and the number of shapes given.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

SLIDE = r"""
param([string]$Out)
$ErrorActionPreference = 'Stop'
$pp = New-Object -ComObject PowerPoint.Application
$pp.DisplayAlerts = 1
function Texts($shapes) {
    foreach ($s in $shapes) {
        if ($s.Type -eq 6) { Texts $s.GroupItems; continue }
        if ($s.HasTextFrame -and $s.TextFrame.HasText) { "TEXT " + $s.TextFrame.TextRange.Text }
    }
}
function List($shapes) {
    foreach ($s in $shapes) {
        $fill = if ($s.Fill.Visible) { '{0:X6}' -f $s.Fill.ForeColor.RGB } else { 'none' }
        'SHAPE type={0} x={1:N1} y={2:N1} w={3:N1} h={4:N1} fill={5}' -f `
            $s.Type, $s.Left, $s.Top, $s.Width, $s.Height, $fill
    }
}
$pres = $pp.Presentations.Add(0)
try {
    $slide = $pres.Slides.Add(1, 12)
    $null = $slide.Shapes.PasteSpecial(2)
    for ($round = 0; $round -lt 4; $round++) {
        $todo = @($slide.Shapes | Where-Object { $_.Type -eq 13 -or $_.Type -eq 6 })
        if ($todo.Count -eq 0) { break }
        foreach ($s in $todo) { try { $null = $s.Ungroup() } catch { "NOTE $($_.Exception.Message)" } }
    }
    "COUNT $($slide.Shapes.Count)"
    List $slide.Shapes
    Texts $slide.Shapes
    $pres.SaveAs("$Out\slide.pptx")
    $slide.Export("$Out\slide.png", "PNG")
} finally { $pres.Close() }
$again = $pp.Presentations.Open("$Out\slide.pptx", -1, 0, 0)
try {
    "REOPENED $($again.Slides.Item(1).Shapes.Count)"
    Texts $again.Slides.Item(1).Shapes | ForEach-Object { "AGAIN " + $_ }
} finally { $again.Close() }
if ($pp.Presentations.Count -eq 0) { $pp.Quit() }
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--text", action="append", default=[], help="a text the slide must hold")
    parser.add_argument("--shapes", type=int, required=True, help="how many shapes the slide must hold")
    parser.add_argument("--keep", help="a folder to keep slide.pptx and slide.png in")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="tpdf-powerpoint-check-") as temporary:
        out = Path(args.keep).resolve() if args.keep else Path(temporary)
        out.mkdir(parents=True, exist_ok=True)
        script = Path(temporary) / "slide.ps1"
        script.write_text(SLIDE, encoding="utf-8")
        ran = subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(script), "-Out", str(out)],
            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300, check=False,
        )
        lines = [line.rstrip() for line in ran.stdout.splitlines() if line.strip()]
        for line in lines:
            print(f"[INFO] {line}")
        if ran.returncode != 0:
            print(f"[FAIL] PowerPoint did not finish (exit {ran.returncode}): {ran.stderr.strip()}")
            return 1
    texts = sorted(line[5:] for line in lines if line.startswith("TEXT "))
    again = sorted(line[11:] for line in lines if line.startswith("AGAIN TEXT "))
    count = next((int(line[6:]) for line in lines if line.startswith("COUNT ")), -1)
    expected = sorted(args.text)
    checks = [
        (texts == expected, f"the slide's text boxes are {expected}", f"{texts}"),
        (again == expected, "the same after saving and reopening", f"{again}"),
        (count == args.shapes, f"the slide holds {args.shapes} shapes", f"{count}"),
    ]
    for passed, what, seen in checks:
        print(f"[{'PASS' if passed else 'FAIL'}] {what}" + ("" if passed else f": {seen}"))
    return 0 if all(passed for passed, _, _ in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
