#!/usr/bin/env python3
"""Generates a document whose text is not Latin, for the search path.

`docs/PLAN.md` Phase 1 names a multilingual corpus as one of two items *not* to
be estimated as viewer polish, and until this file existed every fixture the
search code had ever seen was English, written by us, in one script. A search
that works on those is a search that works on the documents we generated
ourselves, which is a weaker claim than it reads as.

## What each page is for

Every page isolates one property, so a failing check can name it rather than
naming "the multilingual page".

  0 `japanese`   Kanji and Hiragana, no spaces anywhere, with a Latin token
                 embedded in the run. Scripts without word separators are where
                 the two ideas "a word" and "a run of word characters" come
                 apart: `\\b` puts a boundary between a word character and a
                 non-word one, and in Japanese there are none, so a whole-word
                 search over a page of it can only match the entire run.
  1 `arabic`     The same sentence twice: once in base letters in logical order,
                 which is what a producer that stores text and shapes at render
                 time emits, and once in **presentation forms** (U+FBxx-U+FExx),
                 which is what a producer that shapes at layout time emits. They
                 are different code points for the same words, and a reader
                 types the first.
  2 `folding`    Pairs that a reader would call the same text and Unicode does
                 not: `café` precomposed against `café` decomposed, `Istanbul`
                 with a Turkish dotted capital, Greek with a final sigma, and
                 `Strasse` against `STRASSE`. `search.rs` documents its fold as
                 case plus whitespace plus soft hyphens and *deliberately* no
                 accent stripping; this page is where that decision becomes
                 visible instead of being a sentence in a doc comment.
  3 `astral`     One code point above the BMP, so a surrogate pair rides the
                 whole pipeline: a ToUnicode CMap entry that is two UTF-16 units
                 wide, a `u32` in `PageText::codes`, JSON, and JavaScript's
                 UTF-16 strings. Three index spaces meet here and two of them
                 count this character differently.

## The astral page draws a stand-in glyph, deliberately

No font on either platform has a CJK Extension B ideograph --- checked, rather
than assumed --- so the astral page maps a rare *available* glyph's CID to
U+20000 in its ToUnicode CMap. The page therefore shows one character and
extracts another.

That is not a trick, it is what the fixture is about: this page tests the
**encoding**, and in PDF the ToUnicode CMap is what a producer says its bytes
mean. It does mean no check may assert a *render* of this page, which is why the
manifest marks it `standin: true` --- a pixel comparison there would be asserting
that we drew the wrong ideograph correctly.

## Fonts

Each page picks the first system font that covers every code point it needs, and
says which one it used. A page whose code points nothing covers is a hard error
naming the page and the missing character, because the alternative is a fixture
that silently drops to `.notdef` and extracts nothing --- which looks exactly
like a search defect.

Identity-H over a subsetted CIDFontType2 throughout, the same embedding
`make_text_pdf.py` uses for `text-cid.pdf`, because that is what essentially
every real producer emits and because a base-14 font cannot express any of this.

## What the manifest states, and what it refuses to

`multilingual-manifest.json` carries the text of every line, the reading-order
fields the viewer harness expects of any corpus, and a list of **queries** with
the number of hits each should find.

Those counts are stated from what this program *wrote* --- it inserted the
substring, so it knows how many times --- and never computed by folding and
matching, which would be comparing an implementation with itself.

Three kinds of count, deliberately distinguished, because conflating them is how
a measurement comes to look like a specification:

  `why`       stated from what this program wrote. It inserted the substring, so
              it knows how many times, and nothing about the code can change it.
  `measured`  a property of **PDFium** this corpus established. That presentation
              forms come back as base letters is not our decision and not a fact
              about the file; it was assumed to be the opposite and the corpus
              said otherwise.
  `decided`   a product decision. A fixture cannot settle whether a whole-word
              search should match inside Japanese, or whether `strasse` should
              find `Straße`; it can only make the current answer visible so that
              changing it has to be argued for.

The output is gitignored. Nothing generated here may be committed or
redistributed --- it embeds a subset of a system font.

Usage:
    uv run --with fonttools testdata/make_multilingual_pdf.py [outdir]
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

#: Left edge of every line, in points.
TEXT_X = 60
#: Baseline of the first line on a page.
TOP_Y = HEIGHT - 90
#: Baseline of the last line on a page.
#:
#: Lines are spread **evenly between the two** rather than set at a fixed leading,
#: and that is not cosmetic. At a 44 pt leading a three-line page occupied the top
#: fifth of an A4 sheet, and `viewer_check.py`'s drag check --- which drags near the
#: top and again lower down and compares where the text came from --- selected
#: eighteen characters and then nothing, because there was nothing down there to
#: select. A corpus whose pages are mostly blank silently narrows any check that
#: reads a position.
BOTTOM_Y = 120
#: Point size. Larger than the Latin fixtures: CJK at 12pt is unreadable when a
#: render of one of these pages has to be looked at by a human.
SIZE = 18

#: The code point the astral page claims to carry. CJK Extension B, chosen
#: because it is a plausible thing to find in a real East Asian document and is
#: outside the BMP.
ASTRAL = 0x20000

#: The glyph actually drawn for it. U+3007 IDEOGRAPHIC NUMBER ZERO: present in
#: the CJK fonts, used nowhere else in this corpus so re-labelling its CID cannot
#: disturb another line, and --- the part that took a second attempt --- it has a
#: box of ordinary proportions. The first choice was U+2F00 KANGXI RADICAL ONE,
#: which is a single horizontal stroke: PDFium reported it 1.6 pt tall inside an
#: 18 pt line, so it tripped the short-mark rule in the line grouper and the
#: astral page was measuring that rule rather than the surrogate pair.
ASTRAL_STANDIN = "\u3007"

#: Fonts tried, in order, for each page. macOS first, then Windows: Arial
#: Unicode covers every script here in one file, and where it is absent the
#: page-by-page fallback matters --- Windows ships CJK and Arabic coverage in
#: different files.
FONT_CANDIDATES = [
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "C:\\Windows\\Fonts\\arialuni.ttf",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]


def visual_order(text: str) -> str:
    """Logical order rearranged into the order glyphs must be placed in.

    Not simply reversed. A line with a Latin word in it has runs of both
    directions, and each run keeps its own: reversing the whole line would draw
    `PDF` as `FDP`, which is a fixture bug that looks like a bidi finding.

    So: resolve each character's direction, group into runs, emit the runs from
    last to first --- the logically first run is the rightmost --- and reverse the
    characters only inside the right-to-left ones.

    A **neutral between two runs of different direction** is the case that needs
    the two passes, and attaching it to whichever run precedes it is not good
    enough: the space between `PDF` and the Arabic after it then travelled with
    the Latin run, so the drawn line had two spaces in one place and none in the
    other, and the space came back missing from the extracted text. Resolved the
    way the real algorithm does it: a neutral takes the direction its neighbours
    agree on, and the paragraph's where they do not.
    """
    import unicodedata

    #: True for right-to-left, False for left-to-right, None for a neutral.
    def strength(char: str) -> "bool | None":
        kind = unicodedata.bidirectional(char)
        if kind in ("R", "AL", "AN"):
            return True
        if kind in ("L", "EN"):
            return False
        return None

    kinds: "list[bool | None]" = [strength(char) for char in text]
    # Paragraph direction: this is only called for a right-to-left page.
    resolved: "list[bool]" = []
    for at, kind in enumerate(kinds):
        if kind is not None:
            resolved.append(kind)
            continue
        before = next((k for k in reversed(kinds[:at]) if k is not None), None)
        after = next((k for k in kinds[at + 1 :] if k is not None), None)
        resolved.append(before if before is not None and before == after else True)

    runs: "list[tuple[bool, list[str]]]" = []
    for char, strong in zip(text, resolved):
        if runs and runs[-1][0] == strong:
            runs[-1][1].append(char)
            continue
        runs.append((strong, [char]))

    out: "list[str]" = []
    for strong, chars in reversed(runs):
        out.extend(reversed(chars) if strong else chars)
    return "".join(out)


def presentation_forms(text: str) -> str:
    """The same words spelled with Arabic Presentation Forms.

    Derived rather than typed. Hand-writing presentation forms is how the first
    version of this fixture came to spell different words on its two Arabic lines
    --- they look plausible and only a reader of Arabic would notice --- so each
    letter is mapped to a code point in the FB50--FEFF block whose NFKC
    decomposition is that letter. Compatibility decomposition is the standard's
    own statement that the two are the same character differently presented, so
    the map cannot spell a different word by construction.

    Which of the isolated, initial, medial and final forms is picked does not
    matter here: shaping is a rendering concern, and what this fixture is for is
    whether a *search* crosses the encoding difference.
    """
    import unicodedata

    forms: "dict[str, str]" = {}
    for code in range(0xFB50, 0xFF00):
        char = chr(code)
        base = unicodedata.normalize("NFKC", char)
        if len(base) == 1 and base not in forms:
            forms[base] = char
    return "".join(forms.get(char, char) for char in text)


class Line:
    """One line of text, and what it is there to discriminate."""

    def __init__(
        self, name: str, text: str, note: str = "", extracts_as: "str | None" = None
    ) -> None:
        """Records a line without laying it out yet.

        `extracts_as` is what the line comes back as when read, where that is not
        what was laid out. Two lines here need it and for different reasons: the
        presentation-form line, because PDFium maps Arabic presentation forms to
        their base letters, and the astral line, because its glyph is a stand-in.
        Both are properties of the reader rather than of the file, so the manifest
        carries the written form beside the extracted one instead of picking one
        and leaving the difference to be rediscovered.
        """
        self.name = name
        self.text = text
        self.note = note
        self._extracts_as = extracts_as

    @property
    def extracted(self) -> str:
        """What a reader gets back for this line."""
        return self._extracts_as if self._extracts_as is not None else self.text

    def drawn(self, rtl: bool) -> str:
        """The characters in the order the content stream must place them.

        **A PDF content stream has no bidi.** `Tj` advances left to right in the
        order the glyphs are given, so writing a right-to-left script in logical
        order draws it backwards on the page --- the first character read ends up
        leftmost. A real Arabic producer emits the glyphs in *visual* order, which
        for one unbroken run is the logical order reversed.

        Getting this wrong is not a cosmetic error, and it does not fail loudly.
        PDFium recognises an RTL run and reverses it to recover logical order, so
        a page laid out logically extracts *reversed* --- and the fixture's own
        expectations then look like an extraction defect. Measured before the fix:
        every Arabic line came back exactly reversed, and a query for a word in it
        found nothing.
        """
        return visual_order(self.text) if rtl else self.text


class Page:
    """One page: a script or a property, its lines, and the font that covers it."""

    def __init__(
        self, name: str, purpose: str, lines: "list[Line]", rtl: bool = False
    ) -> None:
        """Records a page. The font is chosen later, once coverage is known."""
        self.name = name
        self.purpose = purpose
        self.lines = lines
        #: Whether the script runs right to left, which decides the order the
        #: glyphs are placed in. See `Line.drawn`.
        self.rtl = rtl
        self.font_path = ""

    @property
    def text(self) -> str:
        """Every character the page needs a glyph for."""
        return "".join(line.text for line in self.lines)


def japanese_page() -> Page:
    """Kanji and Hiragana with no spaces, and a Latin token inside the run."""
    return Page(
        "japanese",
        "a script with no word separators",
        [
            # One continuous run. `検索` (search) sits in the middle of it, so a
            # query for it is a substring with a word character on both sides ---
            # which is the whole point: there is no space to anchor a boundary on.
            Line("run", "日本語の文章を検索する機能を試験する", "no spaces at all"),
            # A Latin token surrounded by Kanji. `PDF` is bounded by characters
            # that `char::is_alphanumeric` calls word characters, so there is no
            # word boundary at either end of it either.
            Line("mixed", "この文書はPDF形式で保存される", "Latin token inside CJK"),
            # An ideographic comma and full stop. They are punctuation, so they
            # *do* create boundaries, which makes them the control for the two
            # lines above: if nothing on this page has a boundary, a whole-word
            # check cannot tell "no boundaries here" from "boundaries are broken".
            Line("punctuated", "第一部、第二部。終わり", "ideographic punctuation"),
        ],
    )


def arabic_page() -> Page:
    """The same sentence in base letters and in presentation forms."""
    sentence = "البحر الأزرق واسع"
    return Page(
        "arabic",
        "right-to-left, and two encodings of the same words",
        [
            # Logical order, base letters. This is what a reader types and what a
            # producer that keeps its text and shapes at render time stores.
            Line("base", sentence, "base letters, logical order"),
            # The same words in Arabic Presentation Forms. A producer that shapes
            # at layout time writes these, and they are different code points:
            # U+FEDF is not U+0644, though both are a lam. Derived from the line
            # above rather than typed --- the first version of this fixture was
            # hand-written and spelled different words, which looks plausible to
            # anyone who does not read Arabic.
            Line(
                "forms",
                presentation_forms(sentence),
                "presentation forms, same words",
                # Measured, not intended: PDFium maps every presentation form back
                # to its base letter, so this line and the one above come back
                # character for character identical. That is the finding this page
                # exists for and it is the opposite of what was assumed.
                extracts_as=sentence,
            ),
            # A Latin word in an otherwise Arabic line, so the line carries runs of
            # both directions and the layout has to place each run where it belongs
            # rather than reversing the lot.
            Line("bidi", "الملف PDF جاهز", "direction changes mid-line"),
        ],
        rtl=True,
    )


def folding_page() -> Page:
    """Pairs a reader calls equal and Unicode does not."""
    return Page(
        "folding",
        "what the fold does and does not do",
        [
            # Precomposed, then decomposed. Identical on screen, two code points
            # against three, and `search.rs` says outright that it does not
            # normalise --- so a query for one is not expected to find the other.
            # The value of the line is that the *decision* is now observable.
            Line("nfc", "caf\u00e9 latte", "precomposed e-acute"),
            Line("nfd", "cafe\u0301 latte", "decomposed e-acute"),
            # The same decomposition with **no ascender in the word**, and it is
            # not a duplicate of the line above. A combining acute sits above the
            # x-height: measured here, U+0301 came back at 718.64--721.30 while
            # `e` was 707.80--717.68, so the two boxes do not overlap at all. The
            # line above survives only because the `f` of `caf` reaches 721.30 and
            # drags the band up to meet the accent. Remove the ascender and the
            # accent has nothing to overlap, which is a line break in the middle
            # of a word --- so the discriminating property here is the absence of
            # a tall letter, and a fixture needs both to tell them apart.
            Line("nfd-no-ascender", "resume\u0301 souvenu\u0301", "decomposed, no ascender"),
            # `İ` (U+0130) lowercases to two code points, `i` + U+0307. The fold
            # already carries a source index per folded character because `ß`
            # does the same thing; this is a second, differently shaped instance
            # of the length change, and it is the one that appears in real names.
            Line("turkish", "\u0130stanbul istanbul", "length-changing lowercase"),
            # Greek final sigma. `Σ` lowercases to `σ`, never to `ς`, so the
            # uppercase and lowercase spellings of the same word do not fold
            # together --- a genuine limit, and one no amount of care about case
            # alone fixes.
            Line("greek", "\u039f\u0394\u039f\u03a3 \u03bf\u03b4\u03cc\u03c2", "final sigma"),
            # The case the module docs use as their example, kept so the fixture
            # covers the length-changing fold that is already known to work.
            Line("sharp-s", "Stra\u00dfe STRASSE", "sharp s folds to two"),
            # A typographic ligature, here because a *decision* was reversed on
            # 2026-08-01: the fold used to refuse to normalise these, and case
            # folding does it as part of the same operation, so `final` now finds a
            # word typeset with the ligature. A fixture line rather than a sentence
            # in a doc comment for exactly that reason --- a reversed decision that
            # nothing exercises is one that can drift back.
            Line(
                "ligature",
                "\ufb01nal \ufb02our",
                "ligatures fold to their letters",
                # Measured, and it makes the decision cheaper than it looked:
                # **PDFium decomposes these before we ever see them.** U+FB01 is in
                # the Alphabetic Presentation Forms block, the same range as the
                # Arabic on page 1, and PDFium normalises the whole range on
                # extraction --- so this line comes back as two plain letters and the
                # fold's ligature rule is never reached from the *page* side. It is
                # reached from the *query* side, which is a reader who pastes a
                # ligature into the find bar, and that is the half this page can
                # still prove.
                extracts_as="final flour",
            ),
        ],
    )


def astral_page() -> Page:
    """One code point above the BMP, drawn with a stand-in glyph."""
    return Page(
        "astral",
        "a surrogate pair through every index space",
        [
            # Latin on both sides, so the astral character has neighbours whose
            # indices a check can pin: if a hit's start or end is off by one, it
            # is off against these.
            Line(
                "surrounded",
                f"before {ASTRAL_STANDIN} after",
                "astral between two words",
                extracts_as=f"before {chr(ASTRAL)} after",
            ),
            # Twice in a row, so a length error compounds instead of cancelling.
            Line(
                "doubled",
                f"pair {ASTRAL_STANDIN}{ASTRAL_STANDIN} end",
                "two astral code points adjacent",
                extracts_as=f"pair {chr(ASTRAL)}{chr(ASTRAL)} end",
            ),
        ],
    )


def pages() -> "list[Page]":
    """Every page, in the order they appear in the document."""
    return [japanese_page(), arabic_page(), folding_page(), astral_page()]


def coverage(font_path: str, text: str) -> "set[str]":
    """The characters of `text` that `font_path` has no glyph for."""
    from fontTools.ttLib import TTFont

    try:
        cmap = TTFont(font_path, fontNumber=0, lazy=True).getBestCmap()
    except Exception:
        return set(text)
    if not cmap:
        return set(text)
    return {char for char in text if ord(char) not in cmap}


def choose_font(page: Page) -> str:
    """The first candidate font covering every character `page` needs.

    A hard error rather than a fallback. A page laid out in a font that lacks
    its script draws `.notdef` boxes and extracts nothing, and a search check
    against it fails in exactly the way a real search defect would --- so the
    failure has to happen here, where it can name the font and the character.
    """
    # The stand-in is needed on the astral page and nowhere else, and it must be
    # covered by the same font as that page's Latin.
    wanted = page.text
    misses: "list[str]" = []
    for candidate in FONT_CANDIDATES:
        if not os.path.exists(candidate):
            continue
        missing = coverage(candidate, wanted)
        if not missing:
            return candidate
        shown = " ".join(f"U+{ord(c):04X}" for c in sorted(missing)[:6])
        misses.append(f"  {candidate}: missing {len(missing)} ({shown})")
    detail = "\n".join(misses) if misses else "  no candidate font exists on this machine"
    raise SystemExit(
        f"[FAIL] no font covers page '{page.name}'\n{detail}\n"
        "        add a candidate to FONT_CANDIDATES, or pass --font"
    )


def utf16be(code: int) -> bytes:
    """A code point as the hex UTF-16BE a ToUnicode bfchar target wants.

    Two hex digits' worth of subtlety: a BMP code point is one unit, and
    anything above it is a **surrogate pair**, which is the astral page's whole
    reason for existing. Writing `%04X` of the scalar value there --- the obvious
    thing --- produces a CMap entry that is silently wrong rather than rejected.
    """
    if code <= 0xFFFF:
        return b"%04X" % code
    offset = code - 0x10000
    return b"%04X%04X" % (0xD800 + (offset >> 10), 0xDC00 + (offset & 0x3FF))


class Embedded:
    """A subsetted Identity-H font, and the mapping needed to write with it."""

    def __init__(self, pdf: Pdf, tag: str, font_path: str, text: str, unicode_of: "dict[str, int]"):
        """Subsets `font_path` for `text` and adds every object it needs.

        `unicode_of` overrides what a character's CID claims to *mean* in the
        ToUnicode CMap, which is how the astral page maps a stand-in glyph to a
        code point no font has.
        """
        font, font_bytes = subset_font(font_path, text, retain_gids=True)
        metrics = font_metrics(font)
        scale = metrics["scale"]
        cmap = font.getBestCmap()
        order = font.getGlyphOrder()
        gid_of = {name: index for index, name in enumerate(order)}
        hmtx = font["hmtx"]

        self._cmap = cmap
        self._gid_of = gid_of

        used = sorted({self.gid(char) for char in text if self.gid(char)})
        widths = b" ".join(
            b"%d [%d]" % (gid, round(hmtx[order[gid]][0] * scale)) for gid in used
        )

        entries = []
        for char in sorted(set(text)):
            gid = self.gid(char)
            if not gid:
                continue
            entries.append(b"<%04X> <%s>" % (gid, utf16be(unicode_of.get(char, ord(char)))))
        to_unicode = (
            b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
            b"/CMapName /TPDF-Multilingual def /CMapType 2 def\n"
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
            b"%d beginbfchar\n%s\nendbfchar\n"
            b"endcmap CMapName currentdict /CMap defineresource pop end end"
            % (len(entries), b"\n".join(entries))
        )

        file_ref = pdf.stream(b"<< /Length1 %d >>" % len(font_bytes), font_bytes)
        desc = descriptor(pdf, f"TPDF{tag}+Multi", metrics, file_ref, b"FontFile2")
        cid_font = pdf.add(
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /TPDF%s+Multi "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor %d 0 R /DW 1000 /W [%s] /CIDToGIDMap /Identity >>"
            % (tag.encode("ascii"), desc, widths)
        )
        to_unicode_ref = pdf.stream(b"<< >>", to_unicode)
        self.ref = pdf.add(
            b"<< /Type /Font /Subtype /Type0 /BaseFont /TPDF%s+Multi "
            b"/Encoding /Identity-H /DescendantFonts [%d 0 R] /ToUnicode %d 0 R >>"
            % (tag.encode("ascii"), cid_font, to_unicode_ref)
        )

    def gid(self, char: str) -> int:
        """The glyph id for a character, or 0 if the subset has none."""
        glyph = self._cmap.get(ord(char))
        return self._gid_of.get(glyph, 0) if glyph else 0

    def encode(self, text: str) -> bytes:
        """A string as the hex 2-byte CIDs Identity-H addresses glyphs by."""
        return b"".join(b"%04X" % self.gid(char) for char in text)


def expected_text(page: Page) -> "list[str]":
    """What each line of `page` says once extracted, in reading order."""
    return [line.extracted for line in page.lines]


def queries(all_pages: "list[Page]") -> "list[dict]":
    """The searches to run, and how many hits each should find.

    Every count is stated from what this program wrote. Where the correct
    behaviour is a decision rather than a fact, `decided` records the answer we
    chose and why, and the count is that answer --- so a change in behaviour
    turns a check red and has to be argued for rather than absorbed.
    """
    index = {page.name: number for number, page in enumerate(all_pages)}
    return [
        # --- a script with no word separators -------------------------------
        {
            "name": "cjk-substring",
            "query": "検索",
            "options": {},
            "page": index["japanese"],
            "hits": 1,
            "why": "written once, in the middle of a run with no spaces",
        },
        {
            "name": "cjk-substring-whole-word",
            "query": "検索",
            "options": {"wholeWord": True},
            "page": index["japanese"],
            "hits": 0,
            "decided": (
                "0, not 1. `\\b` needs a non-word character on each side and CJK "
                "has none, so whole-word cannot match inside a Japanese run. The "
                "alternative is a script-aware boundary rule, which is a feature."
            ),
        },
        {
            "name": "latin-token-in-cjk",
            "query": "PDF",
            "options": {},
            "page": index["japanese"],
            "hits": 1,
            "why": "one Latin token, written once, surrounded by Kanji",
        },
        {
            "name": "latin-token-in-cjk-whole-word",
            "query": "PDF",
            "options": {"wholeWord": True},
            "page": index["japanese"],
            "hits": 0,
            "decided": (
                "0. Kanji are word characters, so a Latin token embedded in them "
                "has no boundary either. Same cause as above, and the one a "
                "reader is most likely to hit."
            ),
        },
        {
            "name": "cjk-before-punctuation",
            "query": "第一部",
            "options": {"wholeWord": True},
            "page": index["japanese"],
            "hits": 1,
            "why": (
                "the control for the two above: an ideographic comma IS a "
                "boundary, so whole-word works here and the rule is not simply "
                "broken for CJK"
            ),
        },
        # --- right-to-left, and two encodings of the same words -------------
        {
            "name": "arabic-base-word",
            "query": "البحر",
            "options": {},
            "page": index["arabic"],
            "hits": 2,
            "measured": (
                "2, not 1. PDFium maps Arabic presentation forms back to their "
                "base letters when it extracts, so both lines carry the same code "
                "points and a reader who types base letters finds shaped text too. "
                "Established by this corpus; it was assumed to be 1."
            ),
        },
        {
            "name": "arabic-form-word",
            "query": presentation_forms("البحر"),
            "options": {},
            "page": index["arabic"],
            "hits": 0,
            "measured": (
                "0. The consequence of the above: after extraction the page holds "
                "no presentation-form code point at all, so a query written in "
                "them matches nothing --- including the line that was written in "
                "them."
            ),
        },
        {
            "name": "arabic-mixed-direction",
            "query": "الملف PDF جاهز",
            "options": {},
            "page": index["arabic"],
            "hits": 1,
            "why": (
                "the whole mixed-direction line, in logical order: PDFium puts a "
                "right-to-left run back into reading order, and the Latin run "
                "keeps its own"
            ),
        },
        # --- what the fold does and does not do ------------------------------
        {
            "name": "precomposed-finds-itself",
            "query": "café",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "why": "one precomposed spelling on the page",
        },
        {
            "name": "precomposed-does-not-find-decomposed",
            "query": "café",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "decided": (
                "1, not 2. The fold does not normalise, which `search.rs` states "
                "outright: a hit must be the characters the query asked for, and "
                "NFC/NFD folding would highlight three code points for a "
                "two-code-point query."
            ),
        },
        {
            "name": "turkish-dotted-capital",
            "query": "istanbul",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "decided": (
                "1, and **still** 1 after case folding replaced lowercasing --- which was "
                "predicted to fix it and did not. The dotted capital folds to `i` plus a "
                "combining dot exactly as it lowercased, because the difference is a mark "
                "and not a case. Removing the dot is accent stripping, a separate decision; "
                "Unicode's Turkic mapping does fold it away and also folds `I` to a dotless "
                "one, which is right only for Turkish, and nothing here knows a document's "
                "language."
            ),
        },
        {
            "name": "across-the-expansion",
            "query": "stanbul",
            "options": {},
            "page": index["folding"],
            "hits": 2,
            "why": (
                "both spellings, and the point is the first one: it starts after a "
                "character that folded to two, so its source index cannot be "
                "computed from its folded position by arithmetic"
            ),
        },
        {
            "name": "greek-medial-sigma",
            "query": "οδοσ",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "why": "`ΟΔΟΣ` lowercases to a medial sigma, which is what this query spells",
        },
        {
            "name": "greek-final-sigma",
            "query": "οδος",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "decided": (
                "1 since 2026-08-01, and 0 before it. The spelling a reader of Greek would "
                "type, with a final sigma: case folding maps both sigmas to the medial one, "
                "so the uppercase word is found. The accented lowercase spelling is still "
                "not, because the query carries no accent --- half fixed, and the other half "
                "is accent stripping, which is a separate decision."
            ),
        },
        {
            "name": "sharp-s-is-case-folded",
            "query": "strasse",
            "options": {},
            "page": index["folding"],
            "hits": 2,
            "decided": (
                "2 since 2026-08-01, and 1 before it: the fold case-folds rather than "
                "lowercasing, so the sharp s becomes `ss` and both spellings are found. It "
                "was 1 because the sharp s is already lowercase --- and `search.rs` claimed "
                "the opposite in its own module docs until this query was written."
            ),
        },
        {
            "name": "a-ligature-is-found-by-its-letters",
            "query": "final",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "measured": (
                "1, and **not** because of the fold. PDFium decomposes U+FB01 on extraction, "
                "so this page holds the two plain letters and the query matches them "
                "directly. Worth stating because it was expected to be the fold's doing: the "
                "ligature cost of case folding is therefore near-theoretical for page text, "
                "and real only for a query."
            ),
        },
        {
            "name": "a-ligature-is-found-by-itself",
            "query": "\ufb01nal",
            "options": {},
            "page": index["folding"],
            "hits": 1,
            "decided": (
                "1, and this one *is* the fold: the query carries the ligature, the page "
                "carries two letters, and only case folding bridges them. Before 2026-08-01 "
                "the fold refused to normalise ligatures and a reader who pasted one into the "
                "find bar got nothing. This is the half of the trade that has an effect."
            ),
        },
        {
            "name": "sharp-s-finds-itself",
            "query": "straße",
            "options": {},
            "page": index["folding"],
            "hits": 2,
            "decided": (
                "2, and it was 1 until case folding landed --- which is the property "
                "worth asserting rather than the count. Folding is **symmetric**: "
                "the German spelling and the shouted one fold to the same eight "
                "characters, so typing either finds both. It was written as a "
                "control for `sharp-s-is-case-folded` and is now the same fact "
                "approached from the other side."
            ),
        },
        # --- a surrogate pair through every index space -----------------------
        {
            "name": "astral-alone",
            "query": chr(ASTRAL),
            "options": {},
            "page": index["astral"],
            "hits": 3,
            "why": "written once on the first line and twice on the second",
        },
        {
            "name": "astral-in-context",
            "query": f"before {chr(ASTRAL)} after",
            "options": {},
            "page": index["astral"],
            "hits": 1,
            "why": "the whole first line, so a hit's end index must survive the pair",
        },
        {
            "name": "after-the-pair",
            "query": "after",
            "options": {},
            "page": index["astral"],
            "hits": 1,
            "why": (
                "the word following the astral character: its index is where an "
                "off-by-one in the UTF-16 conversion lands"
            ),
        },
    ]


def build(path: str) -> "dict":
    """Writes the document and returns its manifest."""
    pdf = Pdf()
    all_pages = pages()
    pages_ref = pdf.reserve()
    page_refs = [pdf.reserve() for _ in all_pages]

    manifest: "dict" = {"pages": [], "queries": [], "astral": f"U+{ASTRAL:04X}"}

    for number, (page_ref, page) in enumerate(zip(page_refs, all_pages)):
        page.font_path = choose_font(page)
        # The astral page's stand-in is the one character whose CID must claim to
        # mean something other than itself.
        overrides = {ASTRAL_STANDIN: ASTRAL} if page.name == "astral" else {}
        font = Embedded(pdf, chr(ord("A") + number) * 2, page.font_path, page.text, overrides)

        content = ""
        y = TOP_Y
        spread = (TOP_Y - BOTTOM_Y) / max(1, len(page.lines) - 1)
        for line in page.lines:
            content += "BT /F1 %d Tf %d %.1f Td <%s> Tj ET\n" % (
                SIZE,
                TEXT_X,
                y,
                font.encode(line.drawn(page.rtl)).decode("ascii"),
            )
            y -= spread

        stream = pdf.stream(b"<< >>", content.encode("latin-1"))
        pdf.put(
            page_ref,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
            % (pages_ref, WIDTH, HEIGHT, font.ref, stream),
        )

        lines = expected_text(page)
        manifest["pages"].append(
            {
                # The three fields every corpus carries for the viewer harness's
                # reading-order check, in the shape `make_columns_pdf.py` set.
                "page": number,
                "name": page.name,
                "lines": lines,
                # What was laid out, where that differs from what comes back.
                # Present only on the lines it differs for, so its presence is
                # itself the statement that something is not read as it is written.
                "written": {
                    line.name: line.text
                    for line in page.lines
                    if line.text != line.extracted
                },
                "purpose": page.purpose,
                "font": os.path.basename(page.font_path),
                "notes": {line.name: line.note for line in page.lines},
                # True where the glyphs drawn are not the characters extracted,
                # so no consumer asserts a render of this page.
                "standin": page.name == "astral",
            }
        )

    pdf.put(
        pages_ref,
        b"<< /Type /Pages /Count %d /Kids [%s] >>"
        % (len(page_refs), b" ".join(b"%d 0 R" % ref for ref in page_refs)),
    )
    # `/Lang` is not decoration on this corpus: it is the document saying which
    # language its text is in, and a page of Arabic with no `/Lang` is what makes
    # a screen reader read it in the wrong voice. Nothing reads it yet.
    catalog = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /Lang (mul) >>" % pages_ref
    )

    with open(path, "wb") as handle:
        handle.write(pdf.serialize(catalog))

    manifest["queries"] = queries(all_pages)
    return manifest


def check(manifest: "dict") -> "list[str]":
    """Complaints about the manifest, as a list. Empty means it discriminates.

    A fixture that cannot fail is the failure this whole family of files exists
    to avoid, so the properties that make it discriminate are asserted here
    rather than left to be noticed later.
    """
    problems = []
    by_name = {page["name"]: page for page in manifest["pages"]}

    japanese = by_name["japanese"]
    if any(" " in line for line in japanese["lines"][:2]):
        problems.append("the Japanese run must have no spaces, or it tests nothing")

    # The Arabic page is the one place where written and extracted diverge on
    # purpose, so its properties are asserted about the *written* forms. Reading
    # `lines` here would compare the two extracted lines, which are identical --- and
    # that is the finding rather than a defect, so the check would forbid it.
    arabic = by_name["arabic"]
    base, forms = arabic["lines"][0], arabic["written"].get("forms", "")
    if not forms:
        problems.append("the presentation-form line records nothing it was written as")
    if base == forms:
        problems.append("the Arabic lines must be written differently, or nothing is tested")
    if not any(0xFB50 <= ord(char) <= 0xFEFF for char in forms):
        problems.append("the presentation-form line was not written in presentation forms")
    if any(0xFB50 <= ord(char) <= 0xFEFF for char in base):
        problems.append("the base-letter line carries presentation forms")
    if arabic["lines"][0] != arabic["lines"][1]:
        problems.append(
            "the two Arabic lines are expected to read identically -- if that has "
            "stopped being true, it is a change in PDFium and not in this file"
        )

    folding = by_name["folding"]
    nfc, nfd = folding["lines"][0], folding["lines"][1]
    if nfc == nfd:
        problems.append("the NFC and NFD lines are identical, so nothing is decomposed")
    if len(nfc) >= len(nfd):
        problems.append("the decomposed line should be longer than the precomposed one")

    astral = by_name["astral"]
    if not any(ord(char) > 0xFFFF for line in astral["lines"] for char in line):
        problems.append("the astral page carries no code point above the BMP")
    if not astral["standin"]:
        problems.append("the astral page must be marked standin, or a render may be asserted")

    # Every query must name a page that exists, and at least one must expect
    # zero hits: a query list where everything matches cannot tell a working
    # search from one that matches everything.
    count = len(manifest["pages"])
    for query in manifest["queries"]:
        if not 0 <= query["page"] < count:
            problems.append(f"query {query['name']} names page {query['page']}")
        kinds = sum(1 for kind in ("why", "measured", "decided") if kind in query)
        if kinds != 1:
            problems.append(
                f"query {query['name']} needs exactly one of why/measured/decided"
            )
    if not any(query["hits"] == 0 for query in manifest["queries"]):
        problems.append("no query expects zero hits, so a search that always matches passes")
    return problems


def main() -> int:
    """Writes `multilingual.pdf` and its manifest into the given directory."""
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    path = os.path.join(args.outdir, "multilingual.pdf")
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
        print(f"[OK]  page {page['page']} {page['name']:<10} {page['font']}")
    kinds = {
        kind: sum(1 for query in manifest["queries"] if kind in query)
        for kind in ("why", "measured", "decided")
    }
    print(
        f"[OK]  {len(manifest['queries'])} queries: {kinds['why']} stated from what"
        f" was written, {kinds['measured']} measured of PDFium,"
        f" {kinds['decided']} recording a decision"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
