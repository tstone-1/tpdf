#!/usr/bin/env python3
"""Generates the hostile corpus for the Phase 0 sanitized-rewrite spike (0.4).

The spike asks whether a rewrite from a garbage-collected reachable object graph
actually removes what a plain re-serialize leaves behind, and whether `lopdf` can
do it or QPDF is required (PLAN.md open question 4).

Every fixture hides a distinct needle in a different place. Half of them are
places a GC *must* clear; the other half are places a GC has no business
clearing, and they are here to keep the spike honest -- a reachability sweep is
not sanitation, and a corpus that only contains removable leaks would let the
spike claim otherwise.

  orphan        A compressed stream nothing references. The textbook case.
  orphan-cycle  Two unreachable objects referencing each other. Distinguishes a
                reachability sweep from a reference count, which would keep both.
  stale         A prior revision whose page content is replaced by an appended
                incremental update. The old bytes are still in the file -- this
                is what "drew a black box and saved" produces.
  trailing      A short file written over a longer one, so the tail of the old
                file survives past the new %%EOF. Not an object at all, which is
                why it needs a byte-level check rather than a graph walk.
  objstm        An orphan packed inside an /ObjStm behind an xref stream. Tests
                that the rewriter can see into compressed object streams at all.
  attachment    An embedded file, reachable from the catalog. Must survive.
  metadata      /Info and an XMP /Metadata stream, both reachable. Must survive.
  unused-form   A Form XObject listed in /Resources but never invoked. Reachable,
                so it survives -- finding this one needs content analysis, not a
                graph walk.
  bomb          A reachable stream that inflates to 1 GiB, with a needle at the
                very end. Nothing can decode it under a sane bound, so a verifier
                must report *not verified* rather than clean.
  filters       Ordinary page content behind /ASCIIHexDecode and an image behind
                /RunLengthDecode -- filters `lopdf` does not implement. Decides
                what a verifier does when a stream simply will not decode.
  encrypted     The orphan fixture under AES-256 with an empty user password, so
                it opens in any reader without a prompt. Encryption is the case
                PLAN.md §6 names as a hard verification failure.

Writes `hostile-manifest.json` next to the fixtures, giving each needle and
whether a GC'd rewrite is expected to remove it. The Rust harness reads that
rather than hardcoding expectations.

The output is gitignored. Usage:
    python3 testdata/make_hostile_pdf.py [outdir]
"""

import argparse
import json
import os
import subprocess
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

# Every needle starts with this, so one scan can find any of them and a hit can
# be attributed to a carrier without a second lookup.
PREFIX = "TPDF-NEEDLE"

# How far the bomb inflates. Large enough that no verifier should decode it, and
# small enough that generating the fixture stays quick.
BOMB_BYTES = 1 << 30


def needle(name: str) -> str:
    """Builds the needle string for a carrier."""
    return f"{PREFIX}-{name.upper()}-4711-0815"


def skeleton(
    pdf: Pdf,
    content: str,
    *,
    resources: bytes = b"",
    page_extra: bytes = b"",
    catalog_extra: bytes = b"",
) -> "tuple[int, int, int]":
    """Adds the font, content stream, page, page tree and catalog.

    `resources` is appended inside the page's /Resources dictionary and the two
    `*_extra` arguments inside the page and catalog dictionaries, so a fixture
    can hang a carrier off the reachable graph. Returns the object numbers of the
    catalog, the content stream and the font -- none of which can be assumed,
    because a fixture may reserve numbers before calling this.
    """
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
        b" /Encoding /WinAnsiEncoding >>"
    )
    stream = pdf.stream(b"<< >>", content.encode("latin-1"))
    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d]"
        b" /Resources << /Font << /F1 %d 0 R >>%s >> /Contents %d 0 R%s >>"
        % (pages, WIDTH, HEIGHT, font, resources, stream, page_extra),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R%s >>" % (pages, catalog_extra))
    return catalog, stream, font


def visible_content(title: str, lines: "tuple[str, ...]" = ()) -> str:
    """Draws a titled page, so a render of the rewrite has something to compare.

    A rewrite that silently drops page content has to be visible somewhere; the
    spike compares rendered pixels before and after, and an empty page would make
    that comparison vacuous.
    """
    parts = [
        "q 0.90 0.94 0.99 rg 40 700 515 90 re f Q\n",
        "q 0.10 0.35 0.75 RG 2 w 40 690 m 555 690 l S Q\n",
        "BT /F1 15 Tf 60 740 Td (%s) Tj ET\n" % escape(title).decode("latin-1"),
    ]
    for index, line in enumerate(lines):
        parts.append(
            "BT /F1 11 Tf 60 %d Td (%s) Tj ET\n"
            % (650 - index * 20, escape(line).decode("latin-1"))
        )
    return "".join(parts)


def stream_body(dictionary: bytes, data: bytes, compress: bool = True) -> bytes:
    """Renders a stream object's body without adding it to a document.

    Needed for the incremental fixture, which has to replace an object that has
    already been serialized. Borrows `Pdf.stream` rather than repeating its
    /Length and /Filter handling.
    """
    scratch = Pdf()
    scratch.stream(dictionary, data, compress)
    body = scratch.objects[0]
    assert body is not None
    return body


def append_revision(base: bytes, updates: "dict[int, bytes]", root: int, size: int) -> bytes:
    """Appends a real incremental update section to an already-serialized file.

    The original bytes are left exactly where they were, which is the property
    the `stale` fixture exists to demonstrate.
    """
    marker = base.rfind(b"startxref")
    if marker < 0:
        raise ValueError("base file has no startxref")
    previous = int(base[marker + len(b"startxref") :].split()[0])

    out = bytearray(base)
    if not out.endswith(b"\n"):
        out += b"\n"

    offsets = {}
    for number in sorted(updates):
        offsets[number] = len(out)
        out += b"%d 0 obj\n" % number + updates[number] + b"\nendobj\n"

    xref_at = len(out)
    out += b"xref\n"
    for number in sorted(updates):
        # One subsection per updated object: the numbers need not be contiguous.
        out += b"%d 1\n" % number
        out += b"%010d 00000 n \n" % offsets[number]
    out += b"trailer\n<< /Size %d /Root %d 0 R /Prev %d >>\n" % (size, root, previous)
    out += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(out)


def serialize_with_xref_stream(
    pdf: Pdf, root: int, pack: "list[int]", info: "int | None" = None
) -> bytes:
    """Serializes with an xref stream, packing `pack` objects into an /ObjStm.

    This is what essentially every modern producer emits and what the classic
    xref table in `make_text_pdf.py` cannot express. Stream objects cannot be
    packed -- the format has no room for them -- so asking to is an error rather
    than a silent skip.
    """
    bodies = list(pdf.objects)
    for index, body in enumerate(bodies, start=1):
        if body is None:
            raise ValueError(f"object {index} was reserved but never filled")

    packed = sorted(pack)
    for number in packed:
        body = bodies[number - 1]
        assert body is not None
        if b"\nstream\n" in body:
            raise ValueError(f"object {number} is a stream and cannot be packed")

    objstm_number = len(bodies) + 1
    xref_number = len(bodies) + 2
    size = xref_number + 1

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    entries: "dict[int, tuple[int, int, int]]" = {}

    for number, body in enumerate(bodies, start=1):
        if number in packed:
            continue
        assert body is not None
        entries[number] = (1, len(out), 0)
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"

    pairs = bytearray()
    data = bytearray()
    for index, number in enumerate(packed):
        body = bodies[number - 1]
        assert body is not None
        pairs += b"%d %d " % (number, len(data))
        data += body + b"\n"
        entries[number] = (2, objstm_number, index)
    first = len(pairs)
    payload = zlib.compress(bytes(pairs) + bytes(data), 9)
    entries[objstm_number] = (1, len(out), 0)
    out += (
        b"%d 0 obj\n<< /Type /ObjStm /N %d /First %d /Filter /FlateDecode"
        b" /Length %d >>\nstream\n" % (objstm_number, len(packed), first, len(payload))
    )
    out += payload + b"\nendstream\nendobj\n"

    xref_at = len(out)
    entries[xref_number] = (1, xref_at, 0)
    rows = bytearray()
    rows += bytes([0]) + (0).to_bytes(4, "big") + (65535).to_bytes(2, "big")
    for number in range(1, size):
        kind, second, third = entries[number]
        rows += bytes([kind]) + second.to_bytes(4, "big") + third.to_bytes(2, "big")
    encoded = zlib.compress(bytes(rows), 9)

    trailer = b"/Type /XRef /Size %d /W [1 4 2] /Root %d 0 R" % (size, root)
    if info is not None:
        trailer += b" /Info %d 0 R" % info
    out += b"%d 0 obj\n<< %s /Filter /FlateDecode /Length %d >>\nstream\n" % (
        xref_number,
        trailer,
        len(encoded),
    )
    out += encoded + b"\nendstream\nendobj\n"
    out += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(out)


def double_flate(size: int, tail: bytes) -> bytes:
    """Compresses `size` zero bytes followed by `tail`, twice.

    Streamed rather than materialized: the point is a stream that inflates to a
    gigabyte, not a generator that allocates one.
    """
    compressor = zlib.compressobj(9)
    chunks = []
    block = b"\0" * (1 << 20)
    for _ in range(size >> 20):
        chunks.append(compressor.compress(block))
    chunks.append(compressor.compress(tail))
    chunks.append(compressor.flush())
    return zlib.compress(b"".join(chunks), 9)


def build_orphan(path: str) -> "list[dict]":
    """A compressed stream object that nothing in the document references."""
    text = needle("orphan")
    pdf = Pdf()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "orphan",
            ("An unreachable Form XObject holds a copy of the secret.",),
        ),
    )
    # Added after the catalog and referenced by nothing at all. Compressed, so a
    # byte scan of the file will not find it -- only a decode of every stream.
    pdf.stream(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 300 40] >>",
        ("BT /F1 12 Tf 0 0 Td (%s) Tj ET" % text).encode("latin-1"),
    )
    write(path, pdf.serialize(catalog))
    return [carrier(text, "unreachable Form XObject stream", "removed")]


def build_orphan_cycle(path: str) -> "list[dict]":
    """Two unreachable objects that reference each other."""
    first, second = needle("cycle-a"), needle("cycle-b")
    pdf = Pdf()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "orphan-cycle",
            ("Two unreachable objects reference each other and nothing else.",),
        ),
    )
    left = pdf.reserve()
    right = pdf.reserve()
    pdf.put(
        left,
        b"<< /Type /TPDFOrphan /Next %d 0 R /Note (%s) >>" % (right, escape(first)),
    )
    pdf.put(
        right,
        b"<< /Type /TPDFOrphan /Next %d 0 R /Note (%s) >>" % (left, escape(second)),
    )
    write(path, pdf.serialize(catalog))
    return [
        carrier(first, "unreachable object in a two-object cycle", "removed"),
        carrier(second, "unreachable object in a two-object cycle", "removed"),
    ]


def build_stale(path: str) -> "list[dict]":
    """A prior revision, superseded by an appended incremental update.

    Two carriers, because an incremental update leaves a secret behind in two
    quite different states and only one of them is visible to a parser:

    * The overwritten stream keeps its old bytes at their old offset, but the
      newest cross-reference table points past them. No parser will ever hand
      them to a verifier -- they are reachable only by reading the file as bytes,
      which is why this one is left uncompressed. Compressed, it would be
      invisible to everything.
    * The orphaned stream was never superseded, only dropped from the page. It is
      still in the cross-reference table, so a parser does see it, as an object
      nothing references.
    """
    overwritten = needle("stale-overwritten")
    orphaned = needle("stale-orphaned")

    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
        b" /Encoding /WinAnsiEncoding >>"
    )
    first = pdf.stream(
        b"<< >>",
        visible_content("stale", ("Secret: %s" % overwritten,)).encode("latin-1"),
        compress=False,
    )
    second = pdf.stream(
        b"<< >>",
        ("BT /F1 11 Tf 60 600 Td (%s) Tj ET\n" % escape(orphaned).decode("latin-1")).encode(
            "latin-1"
        ),
    )
    page = pdf.reserve()
    pages = pdf.reserve()
    page_dict = (
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d]"
        b" /Resources << /Font << /F1 %d 0 R >> >> /Contents [%s] >>"
    )
    pdf.put(
        page,
        page_dict
        % (pages, WIDTH, HEIGHT, font, b"%d 0 R %d 0 R" % (first, second)),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    base = pdf.serialize(catalog)

    updates = {
        first: stream_body(
            b"<< >>",
            visible_content(
                "stale",
                ("Both secrets were removed by an incremental update.",),
            ).encode("latin-1"),
            compress=False,
        ),
        page: page_dict % (pages, WIDTH, HEIGHT, font, b"%d 0 R" % first),
    }
    write(path, append_revision(base, updates, catalog, len(pdf.objects) + 1))
    return [
        carrier(
            overwritten,
            "content stream overwritten by the update, old bytes still in the file",
            "removed",
        ),
        carrier(
            orphaned,
            "content stream the update dropped, still in the xref",
            "removed",
        ),
    ]


def build_trailing(path: str) -> "list[dict]":
    """A short file written in place over a longer one, leaving its tail."""
    text = needle("trailing")

    short = Pdf()
    catalog, _, _ = skeleton(
        short,
        visible_content("trailing", ("Everything past %%EOF is not this file.",)),
    )
    short_bytes = short.serialize(catalog)

    long = Pdf()
    long_catalog, _, _ = skeleton(
        long,
        visible_content("trailing", ("The longer file this one was written over.",)),
    )
    # Padding first, then the needle, so the needle is guaranteed to land in the
    # tail rather than inside the part the shorter file overwrote.
    # Small on purpose. A reader looks for `startxref` only in the last kilobyte
    # or so, and qpdf gives up and reconstructs the xref once the tail pushes it
    # out of that window -- which would make this a damaged file rather than a
    # valid one that happens to be carrying a secret.
    long.stream(b"<< /Type /TPDFPadding >>", b" " * 192, compress=False)
    long.add(b"<< /Type /TPDFRemnant /Note (%s) >>" % escape(text))
    long_bytes = long.serialize(long_catalog)

    # Only the old file's *object* region is kept. Carrying its xref table over
    # too would leave the last startxref pointing at offsets that mean nothing in
    # the combined file, and every reader would then report damage -- which makes
    # a different point (a broken file) than the one this fixture is for (a
    # perfectly valid file with a secret sitting past its %%EOF).
    cut = long_bytes.rfind(b"\nxref\n")
    if cut <= len(short_bytes):
        raise ValueError("the padded file must be longer than the short one")

    write(path, short_bytes + long_bytes[len(short_bytes) : cut] + b"\n")
    return [carrier(text, "bytes past %%EOF from an overwritten longer file", "removed")]


def build_objstm(path: str) -> "list[dict]":
    """An orphan packed inside an /ObjStm behind an xref stream."""
    text = needle("objstm")
    pdf = Pdf()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "objstm",
            ("An orphan hides inside a compressed object stream.",),
        ),
    )
    orphan = pdf.add(b"<< /Type /TPDFOrphan /Note (%s) >>" % escape(text))
    # Everything except the content stream, which cannot be packed.
    packable = [
        number
        for number, body in enumerate(pdf.objects, start=1)
        if body is not None and b"\nstream\n" not in body
    ]
    if orphan not in packable:
        raise ValueError("the orphan should be packable")
    write(path, serialize_with_xref_stream(pdf, catalog, packable))
    return [carrier(text, "unreachable object inside an /ObjStm", "removed")]


def build_attachment(path: str) -> "list[dict]":
    """An embedded file, reachable from the catalog's name tree."""
    text = needle("attachment")
    pdf = Pdf()
    names = pdf.reserve()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "attachment",
            ("An embedded file carries the secret. A GC keeps it, correctly.",),
        ),
        catalog_extra=b" /Names << /EmbeddedFiles %d 0 R >>" % names,
    )
    embedded = pdf.stream(
        b"<< /Type /EmbeddedFile /Subtype /text#2Fplain >>",
        ("%s\n" % text).encode("latin-1"),
    )
    filespec = pdf.add(
        b"<< /Type /Filespec /F (secret.txt) /UF (secret.txt)"
        b" /EF << /F %d 0 R >> >>" % embedded
    )
    pdf.put(names, b"<< /Names [(secret.txt) %d 0 R] >>" % filespec)
    write(path, pdf.serialize(catalog))
    return [carrier(text, "embedded file stream reachable from /Names", "survives")]


def build_metadata(path: str) -> "list[dict]":
    """Document information and an XMP metadata stream, both reachable."""
    info_text, xmp_text = needle("docinfo"), needle("xmp")
    pdf = Pdf()
    metadata = pdf.reserve()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "metadata",
            ("/Info and XMP both hold the secret, and both are reachable.",),
        ),
        catalog_extra=b" /Metadata %d 0 R" % metadata,
    )
    xmp = (
        '<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>\n'
        '<x:xmpmeta xmlns:x="adobe:ns:meta/">\n'
        ' <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">\n'
        '  <rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        "   <dc:title><rdf:Alt><rdf:li>%s</rdf:li></rdf:Alt></dc:title>\n"
        "  </rdf:Description>\n"
        " </rdf:RDF>\n"
        "</x:xmpmeta>\n"
        '<?xpacket end="w"?>\n' % xmp_text
    )
    # Uncompressed on purpose: the spec wants an XMP packet readable by a tool
    # that does not parse PDF at all.
    pdf.put(
        metadata,
        stream_body(
            b"<< /Type /Metadata /Subtype /XML >>",
            xmp.encode("utf-8"),
            compress=False,
        ),
    )
    info = pdf.add(
        b"<< /Title (%s) /Producer (tpdf spike 0.4 fixture) >>" % escape(info_text)
    )
    write(path, pdf.serialize(catalog, info=info))
    return [
        carrier(info_text, "/Info /Title", "survives"),
        carrier(xmp_text, "XMP /Metadata stream", "survives"),
    ]


def build_unused_form(path: str) -> "list[dict]":
    """A Form XObject listed in /Resources but never invoked by the content."""
    text = needle("unusedform")
    pdf = Pdf()
    form = pdf.reserve()
    catalog, _, font = skeleton(
        pdf,
        visible_content(
            "unused-form",
            ("A resource nothing draws. Reachable, so a graph walk keeps it.",),
        ),
        resources=b" /XObject << /Fx1 %d 0 R >>" % form,
    )
    pdf.put(
        form,
        stream_body(
            b"<< /Type /XObject /Subtype /Form /BBox [0 0 300 40]"
            b" /Resources << /Font << /F1 %d 0 R >> >> >>" % font,
            ("BT /F1 12 Tf 0 0 Td (%s) Tj ET" % text).encode("latin-1"),
        ),
    )
    write(path, pdf.serialize(catalog))
    return [carrier(text, "Form XObject in /Resources that nothing invokes", "survives")]


def build_bomb(path: str) -> "list[dict]":
    """A reachable stream that inflates to a gigabyte, with a needle at the end."""
    text = needle("bomb")
    pdf = Pdf()
    image = pdf.reserve()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "bomb",
            (
                "A reachable image inflates to %d MiB." % (BOMB_BYTES >> 20),
                "Nothing can decode it under a sane bound, so nothing can verify it.",
            ),
        ),
        resources=b" /XObject << /Ix1 %d 0 R >>" % image,
    )
    payload = double_flate(BOMB_BYTES, text.encode("latin-1"))
    pdf.put(
        image,
        stream_body(
            b"<< /Type /XObject /Subtype /Image /Width 32768 /Height 32768"
            b" /ColorSpace /DeviceGray /BitsPerComponent 8"
            b" /Filter [/FlateDecode /FlateDecode] >>",
            payload,
            compress=False,
        ),
    )
    write(path, pdf.serialize(catalog))
    return [carrier(text, "tail of a stream that inflates to 1 GiB", "unverifiable")]


def run_length_encode(data: bytes) -> bytes:
    """Encodes with PDF's /RunLengthDecode filter.

    Only the repeat form is emitted, which is all the fixture's flat image needs.
    A run of `n` identical bytes (2..128) is written as `257 - n` then the byte;
    128 ends the stream.
    """
    out = bytearray()
    index = 0
    while index < len(data):
        byte = data[index]
        run = 1
        while index + run < len(data) and data[index + run] == byte and run < 128:
            run += 1
        if run == 1:
            out += bytes([0, byte])
        else:
            out += bytes([257 - run, byte])
        index += run
    out.append(128)
    return bytes(out)


def build_filters(path: str) -> "list[dict]":
    """Reachable streams behind filters the rewriter cannot decode.

    Neither carrier is hidden. Both are ordinary page content, drawn on the page,
    in filters the PDF specification has always had. They are here because a
    verifier that decodes every stream and calls what is left clean has to say
    what it does when a stream will not decode -- and the answer decides whether
    redaction can certify a scanned document at all.
    """
    hex_text, image_text = needle("asciihex"), needle("runlength")

    pdf = Pdf()
    form = pdf.reserve()
    image = pdf.reserve()
    catalog, _, font = skeleton(
        pdf,
        visible_content(
            "filters",
            (
                "Both the box below and a second text run are drawn from streams",
                "behind filters this rewriter does not implement.",
            ),
        )
        + "q 120 0 0 40 60 560 cm /Ix1 Do Q\nq 1 0 0 1 60 520 cm /Fx1 Do Q\n",
        resources=b" /XObject << /Fx1 %d 0 R /Ix1 %d 0 R >>" % (form, image),
    )

    drawn = ("BT /F1 11 Tf 0 0 Td (%s) Tj ET" % hex_text).encode("latin-1")
    pdf.put(
        form,
        stream_body(
            b"<< /Type /XObject /Subtype /Form /BBox [0 0 400 20]"
            b" /Resources << /Font << /F1 %d 0 R >> >>"
            b" /Filter /ASCIIHexDecode >>" % font,
            drawn.hex().encode("ascii") + b">",
            compress=False,
        ),
    )

    # A flat 8x8 grey square, plus the needle as trailing bytes the image data
    # never reaches. Real image filters carry real secrets in their pixels; this
    # fixture only has to be undecodable, and pixels are not searchable anyway.
    pixels = bytes([0x80]) * 64 + image_text.encode("latin-1")
    pdf.put(
        image,
        stream_body(
            b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8"
            b" /ColorSpace /DeviceGray /BitsPerComponent 8"
            b" /Filter /RunLengthDecode >>",
            run_length_encode(pixels),
            compress=False,
        ),
    )

    write(path, pdf.serialize(catalog))
    return [
        carrier(hex_text, "page content behind /ASCIIHexDecode", "unverifiable"),
        carrier(image_text, "image data behind /RunLengthDecode", "unverifiable"),
    ]


def build_encrypted(path: str) -> "list[dict]":
    """The orphan fixture, encrypted with AES-256 and an empty user password.

    Encryption is the case PLAN.md §6 names as a hard verification failure, and
    it is worth a fixture rather than an assumption: the file opens without a
    password in any reader, so a user has every reason to expect a rewrite to
    work on it.
    """
    text = needle("encrypted")
    pdf = Pdf()
    catalog, _, _ = skeleton(
        pdf,
        visible_content(
            "encrypted",
            ("AES-256, empty user password, with an unreachable orphan inside.",),
        ),
    )
    pdf.stream(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 300 40] >>",
        ("BT /F1 12 Tf 0 0 Td (%s) Tj ET" % text).encode("latin-1"),
    )

    plain = path + ".plain"
    write(plain, pdf.serialize(catalog))
    try:
        subprocess.run(
            [
                "qpdf",
                "--encrypt",
                "",
                "spike04-owner",
                "256",
                "--",
                # Without this qpdf collects the orphan on the way in, and the
                # fixture would arrive already sanitized.
                "--preserve-unreferenced",
                plain,
                path,
            ],
            check=True,
            capture_output=True,
        )
    finally:
        os.remove(plain)
    return [carrier(text, "unreachable object in an encrypted file", "removed")]


def carrier(text: str, where: str, expect: str) -> dict:
    """Describes one hidden needle for the manifest.

    `expect` is what a garbage-collected rewrite should do about it:

      removed       Unreachable. A collection sweep must clear it.
      survives      Reachable. Clearing it would be data loss, not sanitation;
                    removing it needs a policy, not a graph walk.
      unverifiable  Nothing can decide either way under a sane resource bound, so
                    the only honest verdict is *not verified*.
    """
    if expect not in ("removed", "survives", "unverifiable"):
        raise ValueError(f"unknown expectation {expect}")
    return {"needle": text, "where": where, "expect": expect}


def write(path: str, data: bytes) -> None:
    """Writes a fixture to disk."""
    with open(path, "wb") as handle:
        handle.write(data)


BUILDERS = {
    "hostile-orphan.pdf": build_orphan,
    "hostile-orphan-cycle.pdf": build_orphan_cycle,
    "hostile-stale.pdf": build_stale,
    "hostile-trailing.pdf": build_trailing,
    "hostile-objstm.pdf": build_objstm,
    "hostile-attachment.pdf": build_attachment,
    "hostile-metadata.pdf": build_metadata,
    "hostile-unused-form.pdf": build_unused_form,
    "hostile-bomb.pdf": build_bomb,
    "hostile-filters.pdf": build_filters,
    "hostile-encrypted.pdf": build_encrypted,
}


def main() -> int:
    """Writes every fixture and the manifest the Rust harness reads."""
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()
    os.makedirs(args.outdir, exist_ok=True)

    fixtures = []
    for name, builder in BUILDERS.items():
        path = os.path.join(args.outdir, name)
        carriers = builder(path)
        fixtures.append({"file": name, "carriers": carriers})
        print(f"[OK] {path} ({os.path.getsize(path)} bytes)")

    manifest = os.path.join(args.outdir, "hostile-manifest.json")
    with open(manifest, "w", encoding="utf-8") as handle:
        json.dump({"prefix": PREFIX, "fixtures": fixtures}, handle, indent=2)
        handle.write("\n")
    print(f"[OK] {manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
