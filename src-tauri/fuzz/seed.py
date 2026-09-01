#!/usr/bin/env python3
"""Fills `src-tauri/fuzz/corpus/<target>/` from `testdata/`.

**The seeds are generated, never committed**, and that is the same rule the
fixtures follow: `testdata/*.pdf` is gitignored and written by
`scripts/ci_fixtures.py`, so a committed corpus derived from it would be a
second copy of generated content -- one that goes stale silently, because
nothing compares the two. `fuzz/corpus/` is gitignored for the same reason,
with the addition that libFuzzer *writes* into it: every input reaching new
coverage is saved there, so a tracked corpus would grow by thousands of
unreviewable files per run.

A coverage-guided fuzzer will eventually reach a PDF header on its own. It will
not do so within an hour: `%PDF-`, a cross-reference table and a page tree are
several hundred bytes of exact structure before any of the code under test runs
at all. Seeding is what makes the difference between fuzzing `lopdf`'s "is this
a PDF" check and fuzzing the walkers behind it.

Usage:
    src-tauri/fuzz/seed.py             # fill every target's corpus
    src-tauri/fuzz/seed.py --target ber_definite
    src-tauri/fuzz/seed.py --list      # what would be written, and from what

It is idempotent and additive: an existing corpus directory is added to, never
cleared, because the fuzzer's own accumulated inputs are worth more than the
seeds and a script that tidied them away would throw that out on every run.
"""

import argparse
import binascii
import re
import sys
import zlib
from pathlib import Path

FUZZ = Path(__file__).resolve().parent
ROOT = FUZZ.parent.parent
TESTDATA = ROOT / "testdata"
CORPUS = FUZZ / "corpus"

# A signature's `/Contents`, as every fixture here writes it: a hex string in
# the signature dictionary. Read out of the raw bytes rather than through a PDF
# library on purpose -- the seeds for a BER walk should be what the file holds,
# including the writer's zero padding, which is precisely the half
# `ber::to_definite_length` exists to get right.
CONTENTS = re.compile(rb"/Contents\s*<([0-9A-Fa-f\s]+)>")

# Largest seed worth writing.
#
# libFuzzer executes every corpus file whatever `-max_len` says -- that bound is
# on inputs it *generates* -- so a 337 MB scanned document really is executed,
# once, and really does contribute coverage. It also has to be held in memory
# for the run and read at every start, and `testdata/` has three fixtures past
# 300 MB. Keeping them cost 4.4 GB across nine corpora and put a third of a
# gigabyte into every target's resident set before the first mutation.
#
# It is also what stops three targets running out of memory, which is how the
# cap came to exist. Measured over the whole grown corpus, one input at a time:
# `incr-scan-40p.pdf` costs `links_scan` **1,019 MB** resident and
# `incr-scan-20p.pdf` **537 MB**, against **188 MB** for the heaviest of the
# other 3,243 entries. Process RSS is a high-water mark, so a run that meets
# those two looks exactly like a slow leak -- and libFuzzer without a sanitizer
# cannot say otherwise, since it blames whichever input was current when its
# sampler fired. One such report named `da39a3ee...`, the SHA-1 of the empty
# string.
#
# The trade is stated rather than hidden: what is lost is one execution of one
# very large document per target. What these walkers are fuzzed for is
# structural -- a cyclic page tree, a font dictionary that lies, a destination
# naming a page that is not there -- and none of that needs size.
MAX_SEED_BYTES = 8 * 1024 * 1024

# Text strings in the encodings a PDF may use for one, as `decode_text_string`
# reads them. Hand-written rather than extracted, because what stresses that
# function is the encoding boundary -- a truncated surrogate pair, a lone
# byte-order mark, a byte with no PDFDocEncoding meaning -- and no fixture in
# `testdata/` contains a broken one by construction.
TEXT_STRINGS = {
    "utf16-plain": b"\xfe\xff\x00H\x00e\x00l\x00l\x00o",
    "utf16-astral": b"\xfe\xff\xd8=\xde\x00",
    "utf16-lone-high-surrogate": b"\xfe\xff\xd8=",
    "utf16-odd-length": b"\xfe\xff\x00H\x00",
    "utf16-bom-only": b"\xfe\xff",
    "utf8-bom": b"\xef\xbb\xbfStra\xc3\x9fe",
    "pdfdoc": b"Stra\xe1e \x18 fi",
    "pdfdoc-controls": b"a\x00b\x07c\x1fd",
    "empty": b"",
}


def signature_blobs() -> list[tuple[str, bytes]]:
    """Every `/Contents` blob in `testdata/`, named after the file it came from."""
    found: list[tuple[str, bytes]] = []
    for pdf in sorted(TESTDATA.glob("*.pdf")):
        raw = pdf.read_bytes()
        for index, match in enumerate(CONTENTS.finditer(raw)):
            hexed = re.sub(rb"\s+", b"", match.group(1))
            if len(hexed) % 2:
                # A hex string with an odd digit count is legal PDF -- the last
                # digit is padded with a zero -- and the same rule applies here.
                hexed += b"0"
            try:
                found.append((f"{pdf.stem}-{index}", binascii.unhexlify(hexed)))
            except binascii.Error:
                continue
    return found


def metadata_bomb(payload: int) -> bytes:
    """A PDF whose catalog `/Metadata` declares `payload` bytes of nothing.

    Synthetic for the reason [`TEXT_STRINGS`] is: no fixture in `testdata/`
    puts a compression bomb behind the *catalog's* metadata, and that is the
    one place `docinfo::read_xmp` decompresses without a limit.
    `hostile-bomb.pdf` has the same shape behind an image XObject, which
    `docinfo` never looks at, so a corpus seeded only from `testdata/` cannot
    reach this at all.

    It is not expected to crash anything. What it does is make the allocation
    proportional to a number the file states, from a file of a couple of
    kilobytes -- so it belongs in the corpus as a shape to mutate from rather
    than as a regression case.
    """
    blob = zlib.compress(zlib.compress(b"\x00" * payload, 9), 9)
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
        4: b"<< /Type /Metadata /Subtype /XML /Filter [/FlateDecode /FlateDecode] "
        b"/Length " + str(len(blob)).encode() + b" >>\nstream\n" + blob + b"\nendstream",
    }
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    for number in sorted(objects):
        offsets[number] = len(out)
        out += str(number).encode() + b" 0 obj\n" + objects[number] + b"\nendobj\n"
    start = len(out)
    out += b"xref\n0 " + str(len(objects) + 1).encode() + b"\n0000000000 65535 f \n"
    for number in sorted(objects):
        out += ("%010d 00000 n \n" % offsets[number]).encode()
    out += (
        b"trailer\n<< /Size " + str(len(objects) + 1).encode() + b" /Root 1 0 R >>\n"
        b"startxref\n" + str(start).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def documents() -> list[tuple[str, bytes]]:
    """Every generated fixture, plus anything under the private corpus.

    `testdata/private/` is where real customer documents live when there are
    any. It is gitignored, usually absent, and worth far more than the
    synthetic fixtures when it is not -- a generated PDF is written by one
    writer and exercises the shapes that writer produces.
    """
    found = [(pdf.stem, pdf.read_bytes()) for pdf in sorted(TESTDATA.glob("*.pdf"))]
    private = TESTDATA / "private"
    if private.is_dir():
        found += [
            (f"private-{pdf.stem}", pdf.read_bytes())
            for pdf in sorted(private.rglob("*.pdf"))
        ]
    return found


def planned(document: bytes) -> bytes:
    """A document wrapped in the layout `save_rewrite_update` reads.

    Four bytes of length, the document, then the bytes the plan is built from.
    The tail is a short fixed pattern rather than zeros: `arbitrary` reads
    collection lengths from the *end* of the buffer, so an all-zero tail makes
    every list empty and every seed the identity plan -- which is one input, not
    a corpus.
    """
    tail = bytes(range(1, 33))
    return len(document).to_bytes(4, "little") + document + tail


def corpora() -> dict[str, list[tuple[str, bytes]]]:
    """What each target is seeded with, and from where."""
    blobs = signature_blobs()
    docs = documents()
    # Small on purpose: a corpus entry is executed on every run, and the point
    # of the shape is the ratio rather than the absolute figure.
    bombs = [("metadata-bomb-16mib", metadata_bomb(16 * 1024 * 1024))]
    return {
        "ber_definite": blobs,
        "ber_certificate": blobs,
        "lopdf_load": docs + bombs,
        "annots_scan": docs,
        "links_scan": docs,
        "docinfo_scan": docs + bombs,
        "encoding_scan": docs,
        "annots_text": sorted(TEXT_STRINGS.items()),
        "save_rewrite_update": [(name, planned(raw)) for name, raw in docs],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", help="seed only this target")
    parser.add_argument(
        "--list",
        action="store_true",
        help="report what would be written without writing it",
    )
    args = parser.parse_args()

    if not TESTDATA.is_dir():
        print(f"[FAIL] no {TESTDATA}", file=sys.stderr)
        return 1

    plan = corpora()
    if args.target:
        if args.target not in plan:
            print(
                f"[FAIL] no target named {args.target}; known: "
                f"{', '.join(sorted(plan))}",
                file=sys.stderr,
            )
            return 1
        plan = {args.target: plan[args.target]}

    # A target with nothing to seed is reported rather than skipped quietly.
    # `testdata/*.pdf` is generated, so an empty answer here usually means the
    # fixtures have not been built -- `scripts/ci_fixtures.py` is what builds
    # them -- and that reads exactly like a corpus that is simply small.
    empty = [name for name, seeds in plan.items() if not seeds]
    if empty:
        print(
            f"[WARN] nothing to seed for {', '.join(empty)}; "
            f"has scripts/ci_fixtures.py been run?"
        )

    total = 0
    skipped = 0
    for name, seeds in sorted(plan.items()):
        directory = CORPUS / name
        if not args.list:
            directory.mkdir(parents=True, exist_ok=True)
        written = 0
        for seed, raw in seeds:
            if len(raw) > MAX_SEED_BYTES:
                skipped += 1
                continue
            path = directory / f"{seed}.bin"
            if args.list:
                print(f"  {path.relative_to(ROOT)}  {len(raw)} bytes")
                written += 1
                continue
            if path.exists() and path.read_bytes() == raw:
                continue
            path.write_bytes(raw)
            written += 1
        existing = 0 if args.list else len(list(directory.glob("*")))
        print(f"[OK] {name}: {written} seed(s) written, {existing} file(s) in corpus")
        total += written

    if skipped:
        print(
            f"[INFO] {skipped} seed(s) skipped as larger than "
            f"{MAX_SEED_BYTES // (1024 * 1024)} MiB; see MAX_SEED_BYTES"
        )
    print(f"[OK] {total} seed(s) written in total")
    return 0


if __name__ == "__main__":
    sys.exit(main())
