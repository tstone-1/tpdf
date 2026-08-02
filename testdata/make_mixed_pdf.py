#!/usr/bin/env python3
"""Generates a document whose pages are not all the same size.

Every other fixture in this corpus is uniform, and that is not a coincidence --
`make_rotated_pdf.py` says so out loud, and builds a second, uniform file
precisely because the viewer cannot lay the mixed one out. The consequence is
that the corpus cannot currently fail on the largest assumption the frontend
makes: `App.svelte` hands the scroller `doc.pages[0]` and nothing else, and
`Scroller.computeGeometry` derives the tile grid and every page offset from that
single size.

What that costs on a document like this one, and why it is not a scrollbar
problem:

  * A page **wider** than page 1 is only ever *requested* as far as page 1
    reaches. The tiles beyond that column are never asked for, so the right-hand
    side of the page is not drawn -- silently, with no error and nothing on
    screen to say anything is missing.
  * A page **shorter or taller** than page 1 shifts everything after it. Offsets
    are page-1's height multiplied by the page index, so one differing page puts
    every later page somewhere it is not.

So the fixture carries one of each, and controls for both:

  1. `p1` A4 portrait   -- the size the whole document will be laid out at.
  2. `p2` A4 portrait   -- control: identical to page 1, before anything unusual.
  3. `p3` A3 landscape  -- **wider**, same height. Isolates the cropping half:
     nothing about the vertical axis changes, so a failure here is the tile grid
     and cannot be an offset.
  4. `p4` A5 portrait   -- **shorter** and narrower. Isolates the offset half in
     the other direction: a page smaller than the box it is given is not cropped,
     it is misplaced, and so is everything after it.
  5. `p5` A4 portrait   -- control after both anomalies. Its offset is wrong if
     and only if offsets accumulate per page rather than per page-1.

A property with one value present is the same as none (`docs/TRAPS.md`), which is
why there are three A4 pages rather than a single one beside the two odd sizes:
two distinct widths and two distinct heights are both present, and each anomaly
has a control on the same axis.

Every page carries eight markers at its own edges -- the four corners, and the
midpoint of each side -- each one a distinct string naming its page and its
corner. That is what makes cropping *detectable* rather than merely present: a
check can ask which markers a rendered page covers, and a missing marker names
where the page was cut. Page 3 additionally carries `p3-just-past-a4`, placed a
few points beyond page 1's width, so a layout that is generous rather than
correct still misses something.

Deliberately **not** written as `mixed-manifest.json`: `scripts/viewer_check.py`
binds any `<fixture>-manifest.json` to `TPDF_READING_MANIFEST`, which the reading
-order check then asserts page by page. This fixture makes no claim about reading
order -- its markers are placed at corners, not in a sentence -- so a manifest
under that name would enrol it in a check it was not built for and cannot pass.
The geometry goes to `mixed-geometry.json` instead, and the naming is the whole
reason.

Base-14 Helvetica, so this needs no font file and nothing it writes is anyone
else's to redistribute. The output is gitignored.

Usage: python3 make_mixed_pdf.py <outdir>
"""

import json
import os
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import Pdf, escape  # noqa: E402

#: Distance from every edge that a marker's own edge is placed at, in points.
#: Large enough to survive a renderer's edge antialiasing, small enough that a
#: page cropped anywhere inside its own bounds loses one.
MARGIN = 24

#: Type size for the markers.
SIZE = 12

#: A nominal upper bound on Helvetica's advance width, as a fraction of the type
#: size. Only used to right-align the right-hand markers so they stay on the
#: page: the fixture asserts *which side of page 1's width* a marker falls on,
#: never its exact x, so an approximation is the right tool and an exact one
#: would need the font metrics this file exists to avoid needing.
ADVANCE = 0.6

#: The pages, as (tag, name, width, height). A4 portrait is page 1 and therefore
#: the size everything is laid out at; the other two exist to differ from it on
#: one axis each.
PAGES = (
    ("p1", "a4-first", 595, 842),
    ("p2", "a4-control", 595, 842),
    ("p3", "a3-landscape", 1191, 842),
    ("p4", "a5-portrait", 420, 595),
    ("p5", "a4-after", 595, 842),
)

#: Where on a page each marker goes, as (spot, horizontal, vertical). The
#: horizontal and vertical terms are resolved against the page's *own* size, so
#: "right" means this page's right edge and not page 1's.
SPOTS = (
    ("topleft", "left", "top"),
    ("topmid", "centre", "top"),
    ("topright", "right", "top"),
    ("midleft", "left", "middle"),
    ("midright", "right", "middle"),
    ("botleft", "left", "bottom"),
    ("botmid", "centre", "bottom"),
    ("botright", "right", "bottom"),
)


def text_width(text: str) -> float:
    """Nominal width of a marker at {@link SIZE}, per {@link ADVANCE}."""
    return ADVANCE * SIZE * len(text)


def place(
    text: str, horizontal: str, vertical: str, width: int, height: int
) -> "tuple[float, float]":
    """Where a marker's text object starts, given a side and a page size."""
    if horizontal == "left":
        x = float(MARGIN)
    elif horizontal == "right":
        x = width - MARGIN - text_width(text)
    else:
        x = (width - text_width(text)) / 2.0

    if vertical == "top":
        # The baseline, so the cap height sits inside the margin rather than on
        # top of it.
        y = float(height - MARGIN - SIZE)
    elif vertical == "bottom":
        y = float(MARGIN)
    else:
        y = (height - SIZE) / 2.0
    return x, y


def markers(tag: str, width: int, height: int) -> "list[dict]":
    """Every marker on one page, with the position each was drawn at."""
    placed = []
    for spot, horizontal, vertical in SPOTS:
        text = f"{tag}-{spot}"
        x, y = place(text, horizontal, vertical, width, height)
        placed.append(
            {
                "text": text,
                "x": round(x, 2),
                "y": round(y, 2),
                "width_pt": round(text_width(text), 2),
            }
        )

    if tag == "p3":
        # Just past page 1's right edge, so a layout that is merely generous --
        # one that rounds a column up, or adds a tile of slack -- still fails to
        # ask for it. The far-right markers above sit 500 points beyond A4 and
        # are lost by any wrong answer at all, which makes them the easy case;
        # this one is lost by a layout that is nearly right.
        text = "p3-just-past-a4"
        x = float(PAGES[0][2] + 8)
        y = (height - SIZE) / 2.0 - 2 * SIZE
        placed.append(
            {"text": text, "x": x, "y": round(y, 2), "width_pt": round(text_width(text), 2)}
        )
    return placed


def show(x: float, y: float, text: str) -> str:
    """One text object at a point."""
    return "BT /F1 %d Tf %.2f %.2f Td (%s) Tj ET\n" % (
        SIZE,
        x,
        y,
        escape(text).decode("latin-1"),
    )


def border(width: int, height: int) -> str:
    """A hairline just inside the page's own edges.

    Drawn so a human opening the fixture to see why a check failed can see where
    each page *claims* to end, which on a cropped render is the fact that is
    otherwise invisible: the missing content leaves no gap, it leaves a page
    that looks like a smaller page.
    """
    return "q 0.6 0.6 0.6 RG 0.5 w %d %d %d %d re S Q\n" % (
        MARGIN // 2,
        MARGIN // 2,
        width - MARGIN,
        height - MARGIN,
    )


def build(path: str) -> "list[dict]":
    """Writes the fixture and returns what each page is and where its ink is."""
    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    pages_ref = pdf.reserve()

    geometry = []
    page_refs = []
    for tag, name, width, height in PAGES:
        placed = markers(tag, width, height)
        content = border(width, height)
        for marker in placed:
            content += show(marker["x"], marker["y"], marker["text"])
        stream = pdf.stream(b"<< >>", content.encode("latin-1"))
        page_refs.append(
            pdf.add(
                b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
                b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
                % (pages_ref, width, height, font, stream)
            )
        )
        geometry.append(
            {
                "page": len(page_refs) - 1,
                "name": name,
                "tag": tag,
                "width_pt": width,
                "height_pt": height,
                "markers": placed,
            }
        )

    kids = b" ".join(b"%d 0 R" % ref for ref in page_refs)
    pdf.put(
        pages_ref,
        b"<< /Type /Pages /Count %d /Kids [%s] >>" % (len(page_refs), kids),
    )
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_ref)

    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))
    return geometry


def verify(geometry: "list[dict]") -> None:
    """Asserts the fixture still discriminates what it was built to discriminate.

    Asserted **per page, by name**, never as one invariant over all of them. A
    blanket rule over a set of deliberately different cases is wrong for at least
    one of them, and the one it is wrong for is the control -- `docs/TRAPS.md`
    records a generator whose own self-check refused to write the fixture on the
    strength of its own finding. Here the A4 pages having no ink beyond A4 is not
    a weakness to be flagged; it is what makes them controls, and it is asserted
    as such.
    """
    first = geometry[0]
    first_width, first_height = first["width_pt"], first["height_pt"]

    widths = {page["width_pt"] for page in geometry}
    heights = {page["height_pt"] for page in geometry}
    if len(widths) < 2:
        raise SystemExit("[FAIL] every page is the same width: nothing to crop")
    if len(heights) < 2:
        raise SystemExit("[FAIL] every page is the same height: no offset can drift")

    by_name = {page["name"]: page for page in geometry}

    # The oversized page: its ink has to reach past page 1's width, or a viewer
    # that crops to page 1 loses nothing and the fixture proves nothing.
    wide = by_name["a3-landscape"]
    if wide["width_pt"] <= first_width:
        raise SystemExit("[FAIL] a3-landscape is not wider than page 1")
    if wide["height_pt"] != first_height:
        raise SystemExit(
            "[FAIL] a3-landscape differs from page 1 on both axes; a failure "
            "there could be the offset rather than the crop"
        )
    # The marker just past page 1's right edge. Asserted rather than assumed,
    # and asserted *near the boundary* on purpose: "has ink beyond page 1's
    # width" would read like the check that matters here and could not fail ---
    # this page is 596 points wider than page 1, so its own right-edge markers
    # satisfy that the moment the two asserts above pass. A guard nothing can
    # break is a guard that can silently become wrong (`docs/TRAPS.md`). What is
    # genuinely independent is whether anything sits in the first tile past the
    # boundary, which is what a layout that is generous rather than correct
    # would still cover.
    near = [
        m
        for m in wide["markers"]
        if first_width < m["x"] <= first_width + 256 and m["x"] + m["width_pt"] < wide["width_pt"]
    ]
    if not near:
        raise SystemExit(
            "[FAIL] a3-landscape has no ink just past page 1's width; only a "
            "layout that is wrong by 500 points would be caught"
        )

    # And at all four of its own edges, so a crop on any side is detectable
    # rather than only a crop on the right.
    edges = {
        "left": min(m["x"] for m in wide["markers"]) <= MARGIN,
        "right": max(m["x"] + m["width_pt"] for m in wide["markers"])
        >= wide["width_pt"] - MARGIN - 1,
        "bottom": min(m["y"] for m in wide["markers"]) <= MARGIN,
        "top": max(m["y"] for m in wide["markers"]) >= wide["height_pt"] - MARGIN - SIZE - 1,
    }
    missing = sorted(side for side, reached in edges.items() if not reached)
    if missing:
        raise SystemExit(f"[FAIL] a3-landscape has no ink at its {', '.join(missing)} edge(s)")

    # The undersized page: shorter, which is the axis that moves every later
    # page's offset. Its width differing too is fine here -- unlike the wide
    # page, a smaller page is not cropped by a larger box, so there is no second
    # effect to confuse it with.
    small = by_name["a5-portrait"]
    if small["height_pt"] >= first_height:
        raise SystemExit("[FAIL] a5-portrait is not shorter than page 1")

    # The controls, asserted as controls. Two A4 pages besides page 1, one on
    # each side of the anomalies: without the trailing one, "offsets are wrong
    # after a differing page" has nothing after the differing pages to be wrong.
    controls = [name for name in ("a4-control", "a4-after") if name in by_name]
    if len(controls) < 2:
        raise SystemExit("[FAIL] the fixture needs a control page before and after")
    for name in controls:
        page = by_name[name]
        if (page["width_pt"], page["height_pt"]) != (first_width, first_height):
            raise SystemExit(f"[FAIL] {name} is not page 1's size, so it is not a control")
        if any(m["x"] + m["width_pt"] > first_width for m in page["markers"]):
            raise SystemExit(f"[FAIL] {name} has ink beyond page 1's width")

    texts = [m["text"] for page in geometry for m in page["markers"]]
    if len(set(texts)) != len(texts):
        raise SystemExit("[FAIL] two markers share a string, so neither names where it is")


def reparse(path: str, geometry: "list[dict]") -> None:
    """Re-reads the written file with a parser that did not write it.

    A writer and its own reader agree about a document that is wrong, so the
    only structural check worth running here goes through qpdf -- via `pikepdf`
    if it is importable, otherwise the `qpdf` binary. Neither is a dependency of
    tpdf and neither is required to build the fixture: when both are absent this
    says so and names what went unchecked, rather than passing quietly.
    """
    try:
        import pikepdf  # noqa: PLC0415
    except ImportError:
        pikepdf = None

    if pikepdf is not None:
        with pikepdf.open(path) as document:
            if len(document.pages) != len(geometry):
                raise SystemExit(
                    f"[FAIL] qpdf reads {len(document.pages)} pages, wrote {len(geometry)}"
                )
            for page, expected in zip(document.pages, geometry):
                box = [float(value) for value in page.mediabox]
                got = (box[2] - box[0], box[3] - box[1])
                want = (float(expected["width_pt"]), float(expected["height_pt"]))
                if got != want:
                    raise SystemExit(
                        f"[FAIL] qpdf reads {expected['name']} as {got}, wrote {want}"
                    )
        print(f"[OK] qpdf reads {len(geometry)} pages at the sizes written")
        return

    binary = shutil.which("qpdf")
    if binary is None:
        print("[SKIP] no pikepdf and no qpdf on PATH: page sizes unverified by any other parser")
        return
    result = subprocess.run(
        [binary, "--check", path], capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise SystemExit(
            f"[FAIL] qpdf --check refused the fixture:\n{result.stdout}{result.stderr}"
        )
    print("[OK] qpdf --check accepts the fixture (page sizes unverified: no pikepdf)")


if __name__ == "__main__":
    outdir = sys.argv[1] if len(sys.argv) > 1 else "."

    pdf_path = os.path.join(outdir, "mixed.pdf")
    written = build(pdf_path)
    verify(written)
    reparse(pdf_path, written)

    sizes = ", ".join(f"{p['name']} {p['width_pt']}x{p['height_pt']}" for p in written)
    print(f"[OK] wrote {pdf_path} with {len(written)} pages: {sizes}")

    geometry_path = os.path.join(outdir, "mixed-geometry.json")
    first = {"width_pt": written[0]["width_pt"], "height_pt": written[0]["height_pt"]}
    with open(geometry_path, "w", encoding="utf-8") as handle:
        json.dump(
            {"first_page": first, "pages": written},
            handle,
            ensure_ascii=False,
            indent=2,
        )
        handle.write("\n")
    print(f"[OK] wrote {geometry_path}")
