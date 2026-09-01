#!/usr/bin/env python3
"""Generates the document that ends the process which parses it.

A cross-reference stream declares the byte widths of its own fields in `/W`.
`lopdf` multiplies them out and asks for a zeroed buffer of the result without
checking it, so `/W [1 4 W2]` with a large `W2` reaches `handle_alloc_error` --
which is an **abort**, not a panic. `catch_unwind` cannot see it, and there is no
point in the tpdf code where a check could go, because the code that would have
to check is `lopdf`'s own cross-reference parser. `docs/THREAT-MODEL.md` residual
risk 21 has the account.

Found 2026-09-01 by coverage-guided fuzzing (`src-tauri/fuzz/`), independently by
two targets, and generated here rather than committed for the reason every
fixture in this directory is: `testdata/*.pdf` is gitignored, and a corpus that
is a program rather than a pile of binaries can say WHY each file is the shape it
is.

**The width is the whole fixture.** The threshold measured that day is sharp --
`W[2] = 2**45` completes and `2**46` aborts -- so the value below is far enough
past it to be unambiguous on any machine, and the rest of the file is the
smallest well-formed document that will carry it. Everything else here is
deliberately ordinary: a reader that refuses this for some *other* reason would
be a fixture that tests nothing, which is why the object graph is complete and
the page is real.

**It lives in `testdata/abort/`, not beside the other fixtures, and that is
structural rather than tidy.** Two tests in `save.rs` sweep `testdata/*.pdf` and
`Document::load` every entry; `scripts/viewer_sweep.py` opens each one in a
window. `read_dir` is not recursive, so a subdirectory takes this file out of all
three at once and out of whatever sweep is written next --- which an exemption
list in each of them would not. Measured the hard way: dropping it beside the
others aborted the whole Rust test binary, and 1,150 passing tests then reported
nothing.

⚠ **Do not open this with anything you mind losing.** It is not a hostile file in
the memory-safety sense -- the allocation is refused rather than made, so there is
nothing to exploit -- but any process that parses it with `lopdf` dies where it
stands. `worker-probe` hands it to a *worker* on purpose and asserts the
coordinator is told rather than taken with it.
"""

import pathlib
import sys

#: The third field's width, in bytes. `2**45` completes and `2**46` aborts, so
#: this is chosen well past the threshold rather than at it: a fixture that sat
#: on the boundary would be a measurement of the machine's allocator.
BOMB_WIDTH = 3333333333333333332


def build() -> bytes:
    """The smallest document carrying the width, as bytes."""
    objects: list[bytes] = [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n",
    ]

    out = bytearray(b"%PDF-1.5\n")
    offsets = [0]
    for body in objects:
        offsets.append(len(out))
        out += body

    # The cross-reference stream itself. Its /W is the finding; its /Length and
    # payload are honest, so nothing refuses the file before reaching the widths.
    xref_at = len(out)
    payload = b"\x00" * 16
    stream = (
        b"4 0 obj\n<< /Type /XRef /Size 5 "
        b"/W [1 4 " + str(BOMB_WIDTH).encode("ascii") + b"] "
        b"/Root 1 0 R /Length " + str(len(payload)).encode("ascii") + b" >>\nstream\n"
        + payload
        + b"\nendstream\nendobj\n"
    )
    out += stream
    out += b"startxref\n" + str(xref_at).encode("ascii") + b"\n%%EOF\n"
    return bytes(out)


def main(argv: list[str]) -> int:
    where = pathlib.Path(argv[1]) if len(argv) > 1 else pathlib.Path("testdata")
    at = where if where.suffix == ".pdf" else where / "abort" / "xref-bomb.pdf"
    at.parent.mkdir(parents=True, exist_ok=True)
    bytes_written = build()
    at.write_bytes(bytes_written)
    print(f"[OK] wrote {at} ({len(bytes_written)} bytes, /W [1 4 {BOMB_WIDTH}])")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
