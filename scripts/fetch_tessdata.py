#!/usr/bin/env python3
"""Installs the pinned Tesseract language data into `vendor/tessdata`.

Linux reads text with Tesseract (`src-tauri/src/ocr_tesseract.rs`), and tpdf
ships the language data rather than relying on whatever the distribution has
installed: English and German from tessdata_fast 4.1.0, Apache-2.0. The files
are 5.6 MB together and gitignored, like `vendor/pdfium/`; a Linux bundle takes
them from here (`src-tauri/tauri.linux.conf.json`).

Each file is fetched to a temporary name, hashed, and moved into place only if
its digest is the one pinned below. The upstream tag could be moved; the digest
cannot.

Usage:
    scripts/fetch_tessdata.py            # install what is absent or wrong
    scripts/fetch_tessdata.py --check    # verify only, exit 1 if wrong
"""

from __future__ import annotations

import argparse
import hashlib
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / "vendor" / "tessdata"
RELEASE = "4.1.0"
URL = "https://github.com/tesseract-ocr/tessdata_fast/raw/{release}/{name}"

# file -> sha256, measured 2026-10-14 from the 4.1.0 tag (a8ba5063).
PINS = {
    "eng.traineddata": "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2",
    "deu.traineddata": "19d219bbb6672c869d20a9636c6816a81eb9a71796cb93ebe0cb1530e2cdb22d",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def wrong(name: str) -> str | None:
    """Why the installed file is not the pinned one, or None if it is."""
    path = DEST / name
    if not path.is_file():
        return "missing"
    found = sha256(path)
    return None if found == PINS[name] else f"sha256 {found}, pinned {PINS[name]}"


def fetch(name: str) -> None:
    DEST.mkdir(parents=True, exist_ok=True)
    part = DEST / f"{name}.part"
    url = URL.format(release=RELEASE, name=name)
    with urllib.request.urlopen(url, timeout=120) as response, part.open("wb") as out:  # noqa: S310
        for block in iter(lambda: response.read(1 << 20), b""):
            out.write(block)
    found = sha256(part)
    if found != PINS[name]:
        part.unlink()
        raise SystemExit(f"[FAIL] {name}: {url} has sha256 {found}, pinned {PINS[name]}")
    part.replace(DEST / name)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--check", action="store_true", help="verify only")
    args = parser.parse_args()
    failed = False
    for name in PINS:
        why = wrong(name)
        if why is None:
            print(f"[OK]   {name}")
            continue
        if args.check:
            print(f"[FAIL] {name}: {why}")
            failed = True
            continue
        fetch(name)
        print(f"[OK]   {name} installed from tessdata_fast {RELEASE}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
