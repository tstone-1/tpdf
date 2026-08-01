#!/usr/bin/env python3
"""Generates a document whose character mappings are absent, broken or predefined.

`multilingual.pdf` is the other half of this: correct documents in scripts that
are not Latin. Every page there carries a well-formed `/ToUnicode` CMap, which is
the well-behaved case. `docs/PLAN.md` Phase 1 names the rest --- *"malformed
encodings and custom CMaps"* --- and this is it.

A separate corpus rather than three more pages there, because the subject is
different: those pages ask whether search reads a *correct* document in another
script, and these ask what happens when the document's own statement of what its
bytes mean is missing or wrong. `examples/search-probe` reads both, unchanged,
because they share a manifest shape.

## The three pages, and what each one established

  0 `no-mapping`   Identity-H over an embedded subset with **no `/ToUnicode` at
                   all**. Common in real files --- some LaTeX setups and some
                   scanner output emit it --- and the interesting part is that
                   PDFium does not fail. It returns eighteen characters of
                   plausible-looking garbage for eighteen drawn characters:
                   `Encoding probe ABC` extracts as `(QFRGLQJSUREH$%&`, because
                   the glyph ids are used as if they were character codes and
                   this subset's happen to sit a constant offset below ASCII.

                   That is the failure mode worth having a fixture for. The page
                   is **not** textless, so nothing tells a reader anything is
                   wrong: search finds nothing and reports no matches, copy
                   yields nonsense, and a screen reader reads the nonsense out.

  1 `broken-map`   A `/ToUnicode` whose entries are individually legal and
                   jointly wrong: one CID maps to a lone **high** surrogate and
                   another to a lone **low** one. This is the only fixture that
                   reaches `text.rs`'s replacement-character path, which until
                   now was covered by unit tests alone.

                   It also produced the result nobody would have predicted. The
                   two spaces both map to a high surrogate; the first is followed
                   by `p` and becomes U+FFFD, and the second is followed by the
                   `A`, which maps to a low surrogate --- so **two unrelated
                   characters pair into one astral character** and the page comes
                   back seventeen characters long. That is what decoding UTF-16
                   means, the box is the union of the two, and no interpretation
                   of a broken file is more correct; it is recorded so that the
                   count is not mistaken for a defect later.

  2 `predefined`   `/Encoding /UniJIS-UCS2-H` over a **non-embedded**
                   `KozMinPro-Regular`, which needs PDFium's own bundled
                   Adobe-Japan1 tables and a substituted font to draw with. It
                   extracts correctly, so **the vendored build has the predefined
                   CMaps** --- a fact about the `chromium/7881` pin rather than
                   about our code, and one that would have to be re-established
                   if the pin moved.

## What the manifest states

The same three kinds `multilingual-manifest.json` uses. Nearly everything here is
`measured`: what a broken document extracts as is a property of PDFium, not
something this program can decide, and writing it as though it were a fact about
the file is how a measurement comes to read as a specification. The `lines` a page
records are therefore what it **extracts** as, with `written` beside it holding
what was actually laid out --- and on this corpus those differ on every page,
which is the point of it.

The output is gitignored. Nothing generated here may be committed or
redistributed --- it embeds a subset of a system font.

Usage:
    uv run --with fonttools testdata/make_encodings_pdf.py [outdir]
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import (  # noqa: E402
    HEIGHT,
    WIDTH,
    Pdf,
    descriptor,
    font_metrics,
    subset_font,
)

#: Left edge of every line.
TEXT_X = 60
#: Baselines, spread down the page so a check that reads a position has something
#: at each height --- see the note in `make_multilingual_pdf.py` about a corpus
#: whose pages are mostly blank.
TOP_Y = HEIGHT - 90
BOTTOM_Y = 120

SIZE = 18

#: Drawn on the two Identity-H pages. Latin on purpose: the question is what
#: happens to the *mapping*, and a reader of the output has to be able to see at a
#: glance that what came back is not what went in.
PROBE = "Encoding probe ABC"

#: Drawn on the predefined-CMap page. Japanese, because `UniJIS-UCS2-H` is a
#: Japanese encoding and using it for Latin would prove nothing about it.
JAPANESE = "日本語の符号"

FONT_CANDIDATES = [
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "C:\\Windows\\Fonts\\arialuni.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]


def latin_font() -> str:
    """The first candidate font present, for the two Identity-H pages."""
    for candidate in FONT_CANDIDATES:
        if os.path.exists(candidate):
            return candidate
    raise SystemExit("[FAIL] no candidate font found; see FONT_CANDIDATES")


def cmap_of(entries: "list[bytes]") -> bytes:
    """A ToUnicode CMap holding exactly the bfchar entries given."""
    return (
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        b"/CMapName /TPDF-Encodings def /CMapType 2 def\n"
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        b"%d beginbfchar\n%s\nendbfchar\n"
        b"endcmap CMapName currentdict /CMap defineresource pop end end"
        % (len(entries), b"\n".join(entries))
    )


class Identity:
    """An embedded Identity-H font, with whatever ToUnicode it is given."""

    def __init__(self, pdf: Pdf, tag: str, font_path: str, text: str):
        """Subsets `font_path` for `text` and adds the descendant font."""
        font, font_bytes = subset_font(font_path, text, retain_gids=True)
        metrics = font_metrics(font)
        scale = metrics["scale"]
        self._cmap = font.getBestCmap()
        order = font.getGlyphOrder()
        self._gid_of = {name: index for index, name in enumerate(order)}
        hmtx = font["hmtx"]

        used = sorted({self.gid(char) for char in text if self.gid(char)})
        widths = b" ".join(
            b"%d [%d]" % (gid, round(hmtx[order[gid]][0] * scale)) for gid in used
        )
        file_ref = pdf.stream(b"<< /Length1 %d >>" % len(font_bytes), font_bytes)
        desc = descriptor(pdf, f"TPDF{tag}+Enc", metrics, file_ref, b"FontFile2")
        self._cid = pdf.add(
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /TPDF%s+Enc "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor %d 0 R /DW 1000 /W [%s] /CIDToGIDMap /Identity >>"
            % (tag.encode("ascii"), desc, widths)
        )
        self._tag = tag
        self._pdf = pdf

    def gid(self, char: str) -> int:
        """The glyph id for a character, or 0 if the subset has none."""
        glyph = self._cmap.get(ord(char))
        return self._gid_of.get(glyph, 0) if glyph else 0

    def encode(self, text: str) -> bytes:
        """A string as the hex 2-byte CIDs Identity-H addresses glyphs by."""
        return b"".join(b"%04X" % self.gid(char) for char in text)

    def font(self, to_unicode: "bytes | None") -> int:
        """The Type0 font object, with the given ToUnicode or none at all."""
        extra = b""
        if to_unicode is not None:
            extra = b" /ToUnicode %d 0 R" % self._pdf.stream(b"<< >>", to_unicode)
        return self._pdf.add(
            b"<< /Type /Font /Subtype /Type0 /BaseFont /TPDF%s+Enc "
            b"/Encoding /Identity-H /DescendantFonts [%d 0 R]%s >>"
            % (self._tag.encode("ascii"), self._cid, extra)
        )


def predefined_font(pdf: Pdf) -> int:
    """A non-embedded `KozMinPro-Regular` under `/Encoding /UniJIS-UCS2-H`.

    No `FontFile`, no `/ToUnicode`, and deliberately neither: the encoding is one
    PDFium ships tables for, and the whole question is whether it uses them. A
    reader has to substitute a font to draw the page, which is the other half of
    what a non-embedded CJK font asks of a viewer.
    """
    desc = pdf.add(
        b"<< /Type /FontDescriptor /FontName /KozMinPro-Regular /Flags 6 "
        b"/FontBBox [-437 -340 1147 1317] /ItalicAngle 0 /Ascent 1317 "
        b"/Descent -349 /CapHeight 742 /StemV 80 >>"
    )
    cid = pdf.add(
        b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /KozMinPro-Regular "
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >> "
        b"/FontDescriptor %d 0 R /DW 1000 >>" % desc
    )
    return pdf.add(
        b"<< /Type /Font /Subtype /Type0 /BaseFont /KozMinPro-Regular "
        b"/Encoding /UniJIS-UCS2-H /DescendantFonts [%d 0 R] >>" % cid
    )


def broken_entries(font: Identity, text: str) -> "list[bytes]":
    """A ToUnicode where two entries are legal syntax and illegal Unicode.

    Three entries, and it takes three to reach every branch of the rule. In
    `Encoding probe ABC`:

      * the **space** maps to a lone high surrogate. The first space is followed by
        `p`, so it has nothing to pair with and becomes U+FFFD; the second is
        followed by the `A`, so the two pair into one astral character.
      * `A` maps to a lone **low** surrogate, which is what makes that pair happen.
      * `B` maps to a low surrogate as well --- and by then the pair before it has
        been consumed, so `B` is a low surrogate with nothing in front of it, which
        is the *other* replacement branch.

    The third was added because a mutation proved it had to be. Replacing the lone
    **low** path with a different character survived: every low surrogate in the
    fixture was preceded by a high one, so that branch was never taken and the
    corpus could not tell the two replacement paths apart.
    """
    entries = []
    for char in sorted(set(text)):
        gid = font.gid(char)
        if not gid:
            continue
        if char == " ":
            entries.append(b"<%04X> <D840>" % gid)
        elif char == "A":
            entries.append(b"<%04X> <DC00>" % gid)
        elif char == "B":
            entries.append(b"<%04X> <DC01>" % gid)
        else:
            entries.append(b"<%04X> <%04X>" % (gid, ord(char)))
    return entries


#: What each page extracts as, measured with `text-probe --mode order` against the
#: `chromium/7881` pin. Written here rather than computed, because computing it
#: would mean re-implementing PDFium's fallback and then asserting the two agree.
EXTRACTED = {
    # Glyph ids read as character codes. This subset's sit 0x1D below ASCII, which
    # is an accident of subsetting and is exactly why the result looks like text.
    #
    # Note the two U+0003s where the spaces were: the space's glyph id is 3, so the
    # fallback yields a control character. They are written as escapes because the
    # first version of this constant was **transcribed off a terminal**, which
    # printed them as nothing --- so a sixteen-character expectation was written for
    # an eighteen-character result, and the probe's own count said 18 two lines
    # above the string it was compared against.
    "no-mapping": "(QFRGLQJ\u0003SUREH\u0003$%&",
    # `Encoding` + U+FFFD + `probe` + U+20000 + U+FFFD + `C`: seventeen characters
    # for eighteen drawn, because the second space and the `A` paired. Two
    # replacement characters, from the two different branches --- an unpaired high
    # surrogate and a low one with nothing before it.
    "broken-map": "Encoding\ufffdprobe\U00020000\ufffdC",
    "predefined": JAPANESE,
}


def pages() -> "list[dict]":
    """Each page's name, purpose, and what was drawn on it."""
    return [
        {
            "name": "no-mapping",
            "purpose": "a CID font with no /ToUnicode at all",
            "written": PROBE,
        },
        {
            "name": "broken-map",
            "purpose": "a /ToUnicode with lone surrogates in it",
            "written": PROBE,
        },
        {
            "name": "predefined",
            "purpose": "a predefined CMap over a non-embedded CJK font",
            "written": JAPANESE,
        },
    ]


def queries(described: "list[dict]") -> "list[dict]":
    """The searches to run, and how many hits each should find."""
    index = {page["name"]: at for at, page in enumerate(described)}
    return [
        {
            "name": "unmapped-text-is-not-found",
            "query": "Encoding",
            "options": {},
            "page": index["no-mapping"],
            "hits": 0,
            "measured": (
                "0. The word is plainly on the page and cannot be found, because "
                "the font declares no character mapping and PDFium's fallback "
                "returns different characters. This is the failure the corpus "
                "exists to pin: the page is not textless, so nothing tells a "
                "reader that a search of it means nothing."
            ),
        },
        {
            "name": "unmapped-garbage-is-found",
            "query": EXTRACTED["no-mapping"][:8],
            "options": {},
            "page": index["no-mapping"],
            "hits": 2,
            "measured": (
                "2 --- each line is drawn twice, once at each height --- and it is "
                "the control for the query above: the text really is "
                "there and really is searchable, so the gap is the mapping rather "
                "than extraction failing or the page being empty."
            ),
        },
        {
            "name": "the-readable-part-of-a-broken-map-survives",
            "query": "Encoding",
            "options": {},
            "page": index["broken-map"],
            "hits": 2,
            "measured": (
                "2, one per copy of the line. A broken entry costs the characters it covers and nothing "
                "else, so the rest of the page stays searchable. A decoder that "
                "gave up on the whole page at the first bad entry would also "
                "return zero here, which is why this is worth stating."
            ),
        },
        {
            "name": "a-lone-surrogate-becomes-a-replacement",
            "query": "\ufffd",
            "options": {},
            "page": index["broken-map"],
            "hits": 4,
            "measured": (
                "4: two per copy of the line, from the two different branches --- an "
                "unpaired high surrogate and a low one with nothing before it. The "
                "only fixture that reaches `text.rs`'s replacement path. "
                "Dropping the character instead would shorten the page and shift "
                "every box after it, and keeping the raw surrogate would leave a "
                "number no consumer can decode."
            ),
        },
        {
            "name": "two-broken-entries-can-pair",
            "query": "\U00020000",
            "options": {},
            "page": index["broken-map"],
            "hits": 2,
            "measured": (
                "2, one per copy of the line, and nobody would have predicted it. The second space maps to a "
                "high surrogate and the `A` after it to a low one, so two unrelated "
                "characters decode to one astral character with a box spanning "
                "both. That is what UTF-16 means; it is pinned so the page's "
                "seventeen-character length is not read as a defect later."
            ),
        },
        {
            "name": "a-predefined-cmap-is-searchable",
            "query": "符号",
            "options": {},
            "page": index["predefined"],
            "hits": 2,
            "measured": (
                "2, one per copy of the line. `/UniJIS-UCS2-H` over a non-embedded KozMinPro needs PDFium's "
                "own bundled Adobe-Japan1 tables, so this is a fact about the "
                "`chromium/7881` pin rather than about our code --- and one to "
                "re-establish if the pin moves."
            ),
        },
        {
            "name": "a-word-that-is-not-there",
            "query": "zzzznotpresent",
            "options": {},
            "page": index["predefined"],
            "hits": 0,
            "why": (
                "the control over the whole corpus: with five of six queries "
                "expecting a hit, a matcher that matched everything would look "
                "healthy"
            ),
        },
    ]


def build(path: str) -> "dict":
    """Writes the document and returns its manifest."""
    pdf = Pdf()
    described = pages()
    pages_ref = pdf.reserve()
    page_refs = [pdf.reserve() for _ in described]
    font_path = latin_font()

    identity = Identity(pdf, "AA", font_path, PROBE)
    broken = Identity(pdf, "BB", font_path, PROBE)
    fonts = [
        identity.font(None),
        broken.font(cmap_of(broken_entries(broken, PROBE))),
        predefined_font(pdf),
    ]
    encoders = [identity.encode, broken.encode, lambda text: b"".join(
        # `UniJIS-UCS2-H` takes UTF-16BE code units directly, which is what makes
        # a predefined CMap usable without knowing any Adobe-Japan1 CID.
        b"%04X" % ord(char) for char in text
    )]

    for page_ref, page, font_ref, encode in zip(page_refs, described, fonts, encoders):
        content = "BT /F1 %d Tf %d %d Td <%s> Tj ET\n" % (
            SIZE,
            TEXT_X,
            TOP_Y,
            encode(page["written"]).decode("ascii"),
        )
        # A second copy lower down, so a check that drags at a fixed height finds
        # text at more than one of them. Same string: this corpus is about the
        # mapping, and a second distinct line would only add expectations.
        content += "BT /F1 %d Tf %d %d Td <%s> Tj ET\n" % (
            SIZE,
            TEXT_X,
            BOTTOM_Y,
            encode(page["written"]).decode("ascii"),
        )
        stream = pdf.stream(b"<< >>", content.encode("latin-1"))
        pdf.put(
            page_ref,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
            % (pages_ref, WIDTH, HEIGHT, font_ref, stream),
        )

    pdf.put(
        pages_ref,
        b"<< /Type /Pages /Count %d /Kids [%s] >>"
        % (len(page_refs), b" ".join(b"%d 0 R" % ref for ref in page_refs)),
    )
    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_ref)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(catalog))

    manifest: "dict" = {"pages": [], "queries": []}
    for at, page in enumerate(described):
        extracted = EXTRACTED[page["name"]]
        manifest["pages"].append(
            {
                "page": at,
                "name": page["name"],
                # Two copies of the line, so two lines come back.
                "lines": [extracted, extracted],
                "purpose": page["purpose"],
                "written": {"line": page["written"]},
                # Every page here shows something other than what it extracts as,
                # which is the corpus's whole subject --- so no consumer may assert
                # a render of any of it.
                "standin": True,
            }
        )
    manifest["queries"] = queries(described)
    return manifest


def check(manifest: "dict") -> "list[str]":
    """Complaints about the manifest. Empty means it discriminates."""
    problems = []
    by_name = {page["name"]: page for page in manifest["pages"]}

    # Per page, not blanket. A first version asserted that *every* page extracts
    # as something other than what it was written as --- which is true of the two
    # Identity-H pages and is the whole subject of the corpus, and is exactly
    # backwards for the predefined one, where extracting correctly is the finding.
    # The check refused to write the fixture on the strength of its own result.
    for name in ("no-mapping", "broken-map"):
        page = by_name[name]
        if page["lines"][0] == page["written"]["line"]:
            problems.append(f"{name} extracts as what it was written as, so it tests nothing")

    if "\ufffd" not in by_name["broken-map"]["lines"][0]:
        problems.append("the broken page carries no replacement character")
    if not any(ord(char) > 0xFFFF for char in by_name["broken-map"]["lines"][0]):
        problems.append("the broken page's two lone surrogates no longer pair")
    if by_name["predefined"]["lines"][0] != JAPANESE:
        problems.append("the predefined page is expected to extract correctly")

    kinds = {"why": 0, "measured": 0, "decided": 0}
    for query in manifest["queries"]:
        named = [kind for kind in kinds if kind in query]
        if len(named) != 1:
            problems.append(f"query {query['name']} needs exactly one of why/measured/decided")
        else:
            kinds[named[0]] += 1
        if not 0 <= query["page"] < len(manifest["pages"]):
            problems.append(f"query {query['name']} names page {query['page']}")
    if not any(query["hits"] == 0 for query in manifest["queries"]):
        problems.append("no query expects zero hits, so a matcher that matches all passes")
    return problems


def main() -> int:
    """Writes `encodings.pdf` and its manifest into the given directory."""
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    path = os.path.join(args.outdir, "encodings.pdf")
    manifest = build(path)

    problems = check(manifest)
    stem = os.path.splitext(path)[0]
    with open(f"{stem}-manifest.json", "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    if problems:
        for problem in problems:
            print(f"[FAIL] {problem}", file=sys.stderr)
        return 1

    print(f"[OK] wrote {path}")
    for page in manifest["pages"]:
        print(
            f"[OK]  page {page['page']} {page['name']:<11}"
            f" wrote {page['written']['line']!r} -> reads {page['lines'][0]!r}"
        )
    measured = sum(1 for query in manifest["queries"] if "measured" in query)
    print(
        f"[OK]  {len(manifest['queries'])} queries, {measured} of them measured of"
        f" PDFium rather than stated of the file"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
