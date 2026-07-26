#!/usr/bin/env python3
"""Generates text PDFs for the Phase 0 text-object round-trip spike (0.3).

The spike asks whether one existing text object can be edited and everything
else reproduced faithfully. Answering that needs fixtures where the *font* is
the hard part, because that is what makes the real-world case hard:

  base14   Helvetica, not embedded. The easy case, and the control: if a route
           cannot round-trip this one, it cannot round-trip anything.
  truetype An embedded, subsetted TrueType simple font with WinAnsiEncoding.
           The subset contains only the glyphs already used, so a character
           outside it has no glyph to draw -- see AGENTS.md.
  cid      Type0 / Identity-H over a subsetted CIDFontType2. What essentially
           every modern producer emits, and the case where the content stream
           carries raw 2-byte glyph IDs rather than anything text-like.
  marked   The truetype fixture plus the carriers that hold a copy of the text
           somewhere other than the glyphs: a marked-content /ActualText span,
           a /Contents entry on an annotation, and document metadata. Removing
           the glyphs from a page like this does NOT remove the secret, which
           is the whole reason redaction needs a verifier rather than an edit.

Each fixture puts several text objects on the page with a rule and a filled box
around them, so a round trip has adjacent, unrelated content to damage.

The output is gitignored and never leaves the machine, which is why defaulting
to a system font is acceptable here; pass --font to override. Nothing generated
by this script may be committed or redistributed.

Usage:
    uv run --with fonttools testdata/make_text_pdf.py [outdir] [--font PATH]
"""

import argparse
import os
import sys
import zlib

WIDTH, HEIGHT = 595, 842  # A4 in points.

# The lines placed on every fixture. Kept short and distinct so a diff can name
# which one moved.
LINES = [
    "The quick brown fox jumps over the lazy dog.",
    "REDACT ME: account 4711-0815 belongs to A. Beispiel.",
    "Sphinx of black quartz, judge my vow.",
    "Widths matter: iiiii WWWWW 11111 00000",
]

# The line the harness edits. Index into LINES.
TARGET_LINE = 1


class Pdf:
    """Accumulates numbered objects and serializes them with a valid xref."""

    def __init__(self) -> None:
        """Starts an empty document."""
        self.objects: "list[bytes | None]" = []

    def reserve(self) -> int:
        """Allocates an object number to be filled in later."""
        self.objects.append(None)
        return len(self.objects)

    def put(self, number: int, body: bytes) -> int:
        """Stores the body of a previously reserved object number."""
        self.objects[number - 1] = body
        return number

    def add(self, body: bytes) -> int:
        """Allocates and fills an object in one step."""
        return self.put(self.reserve(), body)

    def stream(self, dictionary: bytes, data: bytes, compress: bool = True) -> int:
        """Adds a stream object, deflating it unless asked not to.

        `dictionary` is the stream dictionary without /Length or /Filter, which
        this method owns because both depend on the encoded bytes.
        """
        inner = dictionary.strip()
        if not (inner.startswith(b"<<") and inner.endswith(b">>")):
            raise ValueError("stream dictionary must be wrapped in << >>")
        inner = inner[2:-2].strip()
        if compress:
            data = zlib.compress(data, 9)
            inner = (inner + b" /Filter /FlateDecode").strip()
        head = b"<< " + inner + (b" /Length %d >>" % len(data))
        return self.add(head + b"\nstream\n" + data + b"\nendstream")

    def serialize(self, root: int, info: "int | None" = None) -> bytes:
        """Renders the whole file, computing xref offsets as it goes."""
        out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
        offsets = [0] * (len(self.objects) + 1)
        for index, body in enumerate(self.objects, start=1):
            if body is None:
                raise ValueError(f"object {index} was reserved but never filled")
            offsets[index] = len(out)
            out += b"%d 0 obj\n" % index + body + b"\nendobj\n"

        xref_at = len(out)
        out += b"xref\n0 %d\n" % (len(self.objects) + 1)
        out += b"0000000000 65535 f \n"
        for index in range(1, len(self.objects) + 1):
            out += b"%010d 00000 n \n" % offsets[index]
        trailer = b"<< /Size %d /Root %d 0 R" % (len(self.objects) + 1, root)
        if info is not None:
            trailer += b" /Info %d 0 R" % info
        out += b"trailer\n" + trailer + b" >>\n"
        out += b"startxref\n%d\n%%%%EOF\n" % xref_at
        return bytes(out)


def escape(text: str) -> bytes:
    """Escapes a string for a PDF literal, latin-1 as WinAnsi's ASCII subset."""
    raw = text.encode("latin-1", "replace")
    for char in (b"\\", b"(", b")"):
        raw = raw.replace(char, b"\\" + char)
    return raw


def surrounding_content() -> str:
    """Draws non-text content next to the text, so damage to it is visible.

    A round trip that reflows the content stream can lose or reorder these; a
    pixel diff outside the edited text's bounds is exactly how that shows up.
    """
    return (
        "q 0.85 0.90 0.98 rg 40 640 515 120 re f Q\n"
        "q 0.10 0.35 0.75 RG 2 w 40 620 m 555 620 l S Q\n"
        "q 0.95 0.55 0.10 rg 460 300 80 80 re f Q\n"
        "q 0 0.5 0.2 RG 1 w 60 200 m 200 260 l 340 200 l 480 260 l S Q\n"
    )


def build_base14(path: str) -> None:
    """Writes the Helvetica fixture: no embedded font, no subsetting."""
    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )

    content = surrounding_content()
    for index, line in enumerate(LINES):
        y = 720 - index * 40
        content += (
            "BT /F1 14 Tf 60 %d Td (%s) Tj ET\n" % (y, escape(line).decode("latin-1"))
        )

    stream = pdf.stream(b"<< >>", content.encode("latin-1"))
    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
        % (pages, WIDTH, HEIGHT, font, stream),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))


def subset_font(font_path: str, text: str, retain_gids: bool):
    """Subsets `font_path` down to the glyphs `text` needs.

    Returns the subsetted TTFont plus its serialized bytes. Subsetting is not
    incidental to this fixture -- it is the property under test.
    """
    from fontTools import subset
    from fontTools.ttLib import TTFont

    font = TTFont(font_path, fontNumber=0)
    options = subset.Options()
    options.retain_gids = retain_gids
    options.notdef_outline = True
    options.recalc_bounds = True
    # Layout tables are dropped: PDF applies no shaping, and keeping them would
    # only inflate the fixture.
    options.layout_features = []
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text=text)
    subsetter.subset(font)

    import io

    buffer = io.BytesIO()
    font.save(buffer)
    return font, buffer.getvalue()


def font_metrics(font) -> "dict[str, float]":
    """Pulls the FontDescriptor numbers out of a TTFont, in 1000-unit space."""
    upem = font["head"].unitsPerEm
    scale = 1000.0 / upem
    head, os2, post = font["head"], font["OS/2"], font["post"]
    return {
        "scale": scale,
        "bbox": [
            round(head.xMin * scale),
            round(head.yMin * scale),
            round(head.xMax * scale),
            round(head.yMax * scale),
        ],
        "ascent": round(os2.sTypoAscender * scale),
        "descent": round(os2.sTypoDescender * scale),
        "cap_height": round(getattr(os2, "sCapHeight", 700) * scale),
        "italic_angle": post.italicAngle,
        # A fixed nominal value: real producers compute this, and nothing in
        # this spike depends on it being accurate.
        "stem_v": 80,
    }


def descriptor(pdf: Pdf, name: str, metrics: "dict[str, float]", file_ref: int, key: bytes) -> int:
    """Adds a FontDescriptor pointing at an embedded font file."""
    bbox = metrics["bbox"]
    return pdf.add(
        b"<< /Type /FontDescriptor /FontName /%s /Flags 32 "
        b"/FontBBox [%d %d %d %d] /ItalicAngle %d /Ascent %d /Descent %d "
        b"/CapHeight %d /StemV %d /%s %d 0 R >>"
        % (
            name.encode("ascii"),
            bbox[0],
            bbox[1],
            bbox[2],
            bbox[3],
            int(metrics["italic_angle"]),
            metrics["ascent"],
            metrics["descent"],
            metrics["cap_height"],
            metrics["stem_v"],
            key,
            file_ref,
        )
    )


def build_truetype(path: str, font_path: str) -> None:
    """Writes the embedded-subset simple-font fixture (WinAnsiEncoding)."""
    text = "".join(LINES)
    font, font_bytes = subset_font(font_path, text, retain_gids=False)
    metrics = font_metrics(font)
    scale = metrics["scale"]

    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    first, last = 32, 126
    widths = []
    for code in range(first, last + 1):
        glyph = cmap.get(code)
        widths.append(round(hmtx[glyph][0] * scale) if glyph else 0)

    pdf = Pdf()
    file_ref = pdf.stream(
        b"<< /Length1 %d >>" % len(font_bytes), font_bytes, compress=True
    )
    desc = descriptor(pdf, "TPDFAA+Embedded", metrics, file_ref, b"FontFile2")
    font_obj = pdf.add(
        b"<< /Type /Font /Subtype /TrueType /BaseFont /TPDFAA+Embedded "
        b"/FirstChar %d /LastChar %d /Widths [%s] /Encoding /WinAnsiEncoding "
        b"/FontDescriptor %d 0 R >>"
        % (first, last, b" ".join(b"%d" % w for w in widths), desc)
    )

    content = surrounding_content()
    for index, line in enumerate(LINES):
        y = 720 - index * 40
        content += (
            "BT /F1 14 Tf 60 %d Td (%s) Tj ET\n" % (y, escape(line).decode("latin-1"))
        )

    stream = pdf.stream(b"<< >>", content.encode("latin-1"))
    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
        % (pages, WIDTH, HEIGHT, font_obj, stream),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))


def build_cid(path: str, font_path: str) -> None:
    """Writes the Type0 / Identity-H fixture over a subsetted CIDFontType2.

    Glyph IDs are retained through subsetting so the content stream can address
    them directly, which is what Identity-H means and what a producer that keeps
    original GIDs emits.
    """
    text = "".join(LINES)
    font, font_bytes = subset_font(font_path, text, retain_gids=True)
    metrics = font_metrics(font)
    scale = metrics["scale"]

    cmap = font.getBestCmap()
    glyph_order = font.getGlyphOrder()
    gid_of = {name: index for index, name in enumerate(glyph_order)}
    hmtx = font["hmtx"]

    def encode(line: str) -> bytes:
        """Maps a line to the hex string of 2-byte glyph IDs Identity-H wants."""
        out = bytearray()
        for char in line:
            glyph = cmap.get(ord(char))
            gid = gid_of.get(glyph, 0) if glyph else 0
            out += b"%04X" % gid
        return bytes(out)

    used = sorted(
        {gid_of[cmap[ord(c)]] for c in text if ord(c) in cmap and cmap[ord(c)] in gid_of}
    )
    widths = b" ".join(
        b"%d [%d]" % (gid, round(hmtx[glyph_order[gid]][0] * scale)) for gid in used
    )

    # A ToUnicode CMap: without it the text is unextractable, and text
    # extraction is half of what this spike has to preserve.
    to_unicode_entries = []
    for char in sorted(set(text)):
        glyph = cmap.get(ord(char))
        if glyph and glyph in gid_of:
            to_unicode_entries.append(b"<%04X> <%04X>" % (gid_of[glyph], ord(char)))
    to_unicode = (
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        b"/CMapName /TPDF-Identity-H def /CMapType 2 def\n"
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        b"%d beginbfchar\n%s\nendbfchar\n"
        b"endcmap CMapName currentdict /CMap defineresource pop end end"
        % (len(to_unicode_entries), b"\n".join(to_unicode_entries))
    )

    pdf = Pdf()
    file_ref = pdf.stream(
        b"<< /Length1 %d >>" % len(font_bytes), font_bytes, compress=True
    )
    desc = descriptor(pdf, "TPDFBB+Embedded", metrics, file_ref, b"FontFile2")
    cid_font = pdf.add(
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /TPDFBB+Embedded "
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
        b"/FontDescriptor %d 0 R /DW 1000 /W [%s] /CIDToGIDMap /Identity >>"
        % (desc, widths)
    )
    to_unicode_ref = pdf.stream(b"<< >>", to_unicode)
    font_obj = pdf.add(
        b"<< /Type /Font /Subtype /Type0 /BaseFont /TPDFBB+Embedded "
        b"/Encoding /Identity-H /DescendantFonts [%d 0 R] /ToUnicode %d 0 R >>"
        % (cid_font, to_unicode_ref)
    )

    content = surrounding_content()
    for index, line in enumerate(LINES):
        y = 720 - index * 40
        content += "BT /F1 14 Tf 60 %d Td <%s> Tj ET\n" % (
            y,
            encode(line).decode("ascii"),
        )

    stream = pdf.stream(b"<< >>", content.encode("latin-1"))
    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
        % (pages, WIDTH, HEIGHT, font_obj, stream),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))


def build_marked(path: str, font_path: str) -> None:
    """Writes the carrier fixture: the same page, plus copies of the secret.

    Every carrier here is legitimate PDF that real producers emit, and every one
    of them survives an edit that only touches page objects:

    * `/ActualText` on a marked-content span -- what a screen reader reads, and
      what a well-behaved extractor prefers over the glyphs.
    * A text annotation's `/Contents` -- a comment, not page content at all.
    * `/Info` document metadata.
    """
    text = "".join(LINES)
    font, font_bytes = subset_font(font_path, text, retain_gids=False)
    metrics = font_metrics(font)
    scale = metrics["scale"]

    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    first, last = 32, 126
    widths = []
    for code in range(first, last + 1):
        glyph = cmap.get(code)
        widths.append(round(hmtx[glyph][0] * scale) if glyph else 0)

    pdf = Pdf()
    file_ref = pdf.stream(
        b"<< /Length1 %d >>" % len(font_bytes), font_bytes, compress=True
    )
    desc = descriptor(pdf, "TPDFCC+Embedded", metrics, file_ref, b"FontFile2")
    font_obj = pdf.add(
        b"<< /Type /Font /Subtype /TrueType /BaseFont /TPDFCC+Embedded "
        b"/FirstChar %d /LastChar %d /Widths [%s] /Encoding /WinAnsiEncoding "
        b"/FontDescriptor %d 0 R >>"
        % (first, last, b" ".join(b"%d" % w for w in widths), desc)
    )

    secret = LINES[TARGET_LINE]
    content = surrounding_content()
    for index, line in enumerate(LINES):
        y = 720 - index * 40
        drawn = "BT /F1 14 Tf 60 %d Td (%s) Tj ET\n" % (
            y,
            escape(line).decode("latin-1"),
        )
        if index == TARGET_LINE:
            # The span's /ActualText is a second, independent copy of the line.
            drawn = (
                "/Span << /ActualText (%s) >> BDC\n%s EMC\n"
                % (escape(line).decode("latin-1"), drawn)
            )
        content += drawn

    stream = pdf.stream(b"<< >>", content.encode("latin-1"))

    annotation = pdf.add(
        b"<< /Type /Annot /Subtype /Text /Rect [500 700 520 720] "
        b"/Contents (%s) /F 2 >>" % escape("Note: " + secret)
    )

    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R "
        b"/Annots [%d 0 R] >>"
        % (pages, WIDTH, HEIGHT, font_obj, stream, annotation),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    info = pdf.add(
        b"<< /Title (%s) /Producer (tpdf spike 0.3 fixture) >>" % escape(secret)
    )
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root, info=info))


def default_font() -> str:
    """Picks a Latin TrueType present on this platform.

    Deliberately a *serif* face. The base14 fixture asks for Helvetica, and
    PDFium's font mapper aliases Helvetica to Arial, so embedding Arial would
    make a substituted render pixel-identical to an embedded one -- the fixture
    would then be unable to tell the two apart, which is the one thing it is
    for. A serif embedded font makes substitution obvious on sight and in a
    pixel diff.
    """
    candidates = [
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/Library/Fonts/Georgia.ttf",
        "C:\\Windows\\Fonts\\georgia.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
    ]
    for candidate in candidates:
        if os.path.exists(candidate):
            return candidate
    raise SystemExit("[FAIL] no default font found; pass --font PATH")


def main() -> int:
    """Writes all three fixtures into the output directory."""
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir", nargs="?", default="testdata")
    parser.add_argument("--font", default=None)
    args = parser.parse_args()

    font_path = args.font or default_font()
    os.makedirs(args.outdir, exist_ok=True)

    base14 = os.path.join(args.outdir, "text-base14.pdf")
    truetype = os.path.join(args.outdir, "text-truetype.pdf")
    cid = os.path.join(args.outdir, "text-cid.pdf")
    marked = os.path.join(args.outdir, "text-marked.pdf")

    build_base14(base14)
    build_truetype(truetype, font_path)
    build_cid(cid, font_path)
    build_marked(marked, font_path)

    print(f"font: {font_path}")
    for path in (base14, truetype, cid, marked):
        print(f"[OK] {path} ({os.path.getsize(path)} bytes)")
    print(f"target line for the spike is index {TARGET_LINE}: {LINES[TARGET_LINE]!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
