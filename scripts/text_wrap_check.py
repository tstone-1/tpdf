#!/usr/bin/env python3
"""A paragraph wrap on a tagged page, generated and read back with pypdf.

uv run --with pypdf scripts/text_wrap_check.py --generate <new directory>
<text-edit-probe> --growth-request <dir>/source.pdf 0 8 grow25 app grow > <dir>/requests.json
<text-edit-probe> --roundtrip <dir>/source.pdf <dir>/requests.json <new result directory>
uv run --with pypdf scripts/text_wrap_check.py --check <dir>/source.pdf <result>/edited.pdf
uv run --with pypdf --with pdfplumber scripts/text_wrap_check.py --compare <source.pdf> <edited.pdf> <page> [<requests.json>]

`--compare` is for a document the script did not write, where it cannot know which
lines should move. It pairs every glyph on the page before and after the save, as
pdfplumber reads them: a glyph must be where it was, or moved straight down the
page by one distance that every moved glyph shares. What is left over from the
source must lie on one line, the edited one, and what is left over on the saved
page is the replacement. Nothing is printed of the text itself.

Pairing alone passes a wrap that moved nothing -- every glyph is then "unchanged" --
with the new line printed over the old one. So it also counts overlapping glyphs:
the saved page may not have more pairs of them than the source had.

Text after the edit on its own line flows after it, sideways and down, so its
glyphs are left over on the edited line and turn up again among the new ones;
pairing cannot tell them from the replacement. Given the request `--growth-request`
wrote, which carries the run's original text as well as the replacement, it counts
instead: what the saved page gained over what the source lost is exactly what the
replacement adds to the original, so a flowed glyph that went missing or was drawn
twice fails, and so does one that the replacement's own count would hide.
"""
import argparse
import json
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject, NameObject,
                           NumberObject)

PITCH = 14
# Helvetica at 12 pt. The widest line is the measure the wrap breaks at, and the
# edited one is long enough that a quarter more runs past the 300 pt page.
LINES = ["FIRST LINE OF THE PARAGRAPH", "THE SECOND LINE IS THE WIDEST ONE HERE",
         "THIRD LINE", "LAST"]
NEXT = "THE NEXT PARAGRAPH"
EDITED = 1


def generate(directory):
    directory.mkdir(parents=True, exist_ok=False)
    writer = PdfWriter()
    font = writer._add_object(DictionaryObject({NameObject(k): NameObject(v) for k, v in
        [("/Type", "/Font"), ("/Subtype", "/Type1"), ("/BaseFont", "/Helvetica"),
         ("/Encoding", "/WinAnsiEncoding")]}))
    body = [b"BT /F1 12 Tf 20 200 Td"]
    for mcid, line in enumerate(LINES):
        move = b"" if mcid == 0 else f"0 -{PITCH} Td ".encode()
        body.append(move + f"/P <</MCID {mcid}>> BDC ({line}) Tj EMC".encode())
    body.append(f"0 -60 Td /P <</MCID {len(LINES)}>> BDC ({NEXT}) Tj EMC ET".encode())
    stream = DecodedStreamObject()
    stream.set_data(b"\n".join(body))
    page = writer.add_blank_page(300, 240)
    page[NameObject("/Resources")] = DictionaryObject({
        NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
    page[NameObject("/Contents")] = writer._add_object(stream)
    page[NameObject("/StructParents")] = NumberObject(0)
    root = writer._add_object(DictionaryObject())
    document = writer._add_object(DictionaryObject())
    first, second = writer._add_object(DictionaryObject()), writer._add_object(DictionaryObject())
    page_ref = page.indirect_reference
    for element, mcids in [(first, range(len(LINES))), (second, [len(LINES)])]:
        element.get_object().update({
            NameObject("/Type"): NameObject("/StructElem"), NameObject("/S"): NameObject("/P"),
            NameObject("/P"): document, NameObject("/Pg"): page_ref,
            NameObject("/K"): ArrayObject(NumberObject(m) for m in mcids)})
    document.get_object().update({
        NameObject("/Type"): NameObject("/StructElem"), NameObject("/S"): NameObject("/Document"),
        NameObject("/P"): root, NameObject("/Pg"): page_ref,
        NameObject("/K"): ArrayObject([first, second])})
    slots = ArrayObject([first] * len(LINES) + [second])
    parents = writer._add_object(DictionaryObject({
        NameObject("/Nums"): ArrayObject([NumberObject(0), slots])}))
    root.get_object().update({
        NameObject("/Type"): NameObject("/StructTreeRoot"), NameObject("/K"): ArrayObject([document]),
        NameObject("/ParentTree"): parents})
    writer._root_object[NameObject("/StructTreeRoot")] = root
    writer.write(directory / "source.pdf")
    # BT, Tf, Td, then per line (Td,) BDC, Tj, EMC: the edited line's Tj is 8.
    print(json.dumps({"page": 0, "operator": 8, "trial": "grow25", "width": "app"}))


def shows(path, page=0):
    """Every text show on a page as (text, x, y) in page space."""
    out = []

    def visit(text, cm, tm, _font, _size):
        # pypdf hands a show's text with the line break it inferred after it.
        if text.strip():
            x = tm[4] * cm[0] + tm[5] * cm[2] + cm[4]
            y = tm[4] * cm[1] + tm[5] * cm[3] + cm[5]
            out.append((text.strip(), round(x, 3), round(y, 3)))
    PdfReader(path).pages[page].extract_text(visitor_text=visit)
    return out


def check(source, saved):
    before, after = shows(source), shows(saved)
    edited_y = 200 - EDITED * PITCH
    moved = {line for line in LINES[EDITED + 1:]}
    text = " ".join(t for t, _, _ in after)
    assert LINES[EDITED] not in [t for t, _, _ in after], "the edited line is still there"
    for line, x, y in before:
        if line == LINES[EDITED]:
            continue
        match = [(ax, ay) for t, ax, ay in after if t == line]
        assert len(match) == 1, f"{line!r} is {len(match)} times on the saved page"
        ax, ay = match[0]
        expected = (x, round(y - PITCH, 3)) if line in moved else (x, y)
        assert (ax, ay) == expected, f"{line!r} at {(ax, ay)}, expected {expected}"
    new = [(t, x, y) for t, x, y in after if t not in LINES and t != NEXT]
    lines = sorted({y for _, _, y in new}, reverse=True)
    assert lines == [edited_y, edited_y - PITCH], f"replacement lines at {lines}"
    assert all(x == 20 for _, x, y in new if y == edited_y - PITCH), new
    assert "THE SECOND LINE" in text
    print(f"[PASS] {len(new)} replacement shows on 2 lines; {len(moved)} lines one pitch lower; "
          f"{len(before) - 1 - len(moved)} shows unmoved to 0.001 pt")


def glyphs(path, page):
    """Every visible character on a page as (text, x0, top, x1, bottom), as
    pdfplumber reads it: top grows down the page."""
    import pdfplumber
    with pdfplumber.open(path) as pdf:
        return [(c["text"], c["x0"], c["top"], c["x1"], c["bottom"])
                for c in pdf.pages[page].chars if c["text"].strip()]


def overlaps(chars):
    """How many pairs of glyphs overlap by more than a third of the smaller one
    each way. Characters, not words: pdfplumber merges characters printed over
    each other into one word, so a word count cannot see text set on top of
    text. A kerned pair overlaps by a sliver and never by a third."""
    chars = sorted(chars, key=lambda c: c[1])
    count = 0
    for i, a in enumerate(chars):
        for b in chars[i + 1:]:
            if b[1] >= a[3]:
                break
            across = min(a[3], b[3]) - max(a[1], b[1])
            down = min(a[4], b[4]) - max(a[2], b[2])
            if (across > min(a[3] - a[1], b[3] - b[1]) / 3
                    and down > min(a[4] - a[2], b[4] - b[2]) / 3):
                count += 1
    return count


# How far apart two readings of one glyph may be and still be one glyph. A reader
# that places a show from the text cursor and one that places it from an explicit
# Tm need not agree to the last hundredth: on page 5 of the Hugo minutes
# pdfminer puts a cursor-continued show 0.024 pt right of where poppler puts it,
# in the untouched source, while both agree on the saved file, where the same
# show has a Tm of its own. Twice that, and far below any real move.
TOLERANCE = 0.05


class Pool:
    """Glyphs of one page, taken out one at a time by the nearest match."""

    def __init__(self, chars):
        self.by_text = {}
        for c in chars:
            self.by_text.setdefault(c[0], []).append(c)

    def find(self, text, x, top):
        for c in self.by_text.get(text, ()):
            if abs(c[1] - x) <= TOLERANCE and abs(c[2] - top) <= TOLERANCE:
                return c
        return None

    def take(self, text, x, top):
        c = self.find(text, x, top)
        if c is not None:
            self.by_text[text].remove(c)
        return c

    def rest(self):
        return [c for found in self.by_text.values() for c in found]


def visible(text):
    return sum(1 for ch in text if not ch.isspace())


def compare(source, saved, page, request=None):
    """The glyphs of `saved` against `source`: unchanged, moved straight down
    by one shared distance, or the edited line's own. Counts only; no text is
    printed.

    Glyphs rather than shows, because a moved show is drawn from a `Tm` of its
    own and a reader groups text into chunks at every one of them, so the same
    glyphs come back cut into different pieces. And decided a line at a time,
    because a line moved down lands exactly where the next one was: glyph by
    glyph, an "e" at one position is as happy to pair with the line that moved
    into it as with the line that was there, and the second line is then short
    of the glyph it needs."""
    before, after = glyphs(source, page), glyphs(saved, page)
    # The shared distance: from the glyphs a plain pairing leaves over.
    left = Pool(after)
    rest = [c for c in before if left.take(c[0], c[1], c[2]) is None]
    distances = {}
    for text, x, top, *_ in rest:
        for c in left.by_text.get(text, ()):
            if abs(c[1] - x) <= TOLERANCE and c[2] > top + 1:
                distance = round(c[2] - top, 1)
                distances[distance] = distances.get(distance, 0) + 1
    drop = max(distances, key=distances.get) if distances else None
    # Each source line is unchanged or moved as a whole, whichever accounts for
    # more of its glyphs against the untouched saved page; moved lines claim
    # their places first, since those are the places another line vacated.
    lines = {}
    for c in before:
        lines.setdefault(c[2], []).append(c)
    fresh = Pool(after)
    moving = set()
    if drop is not None:
        for top, line in lines.items():
            down = sum(1 for c in line if fresh.find(c[0], c[1], top + drop))
            here = sum(1 for c in line if fresh.find(c[0], c[1], top))
            if down > here:
                moving.add(top)
    left = Pool(after)
    unchanged, moved, gone = 0, 0, []
    for top in sorted(lines, key=lambda top: top not in moving):
        for c in lines[top]:
            if top in moving and left.take(c[0], c[1], top + drop) is not None:
                moved += 1
            elif left.take(c[0], c[1], top) is not None:
                unchanged += 1
            else:
                gone.append(c)
    tops = sorted({c[2] for c in gone})
    assert not gone or tops[-1] - tops[0] < 1, (
        f"{len(gone)} source glyphs neither stayed nor moved by the shared {drop} pt, "
        f"on {len(tops)} lines rather than the edited one")
    new = left.rest()
    assert new, "the replacement is not on the saved page"
    counted = ""
    if request is not None:
        edit, = json.loads(Path(request).read_text())
        added = visible(edit["replacement"]) - visible(edit["original"])
        assert len(new) - len(gone) == added, (
            f"the saved page gained {len(new) - len(gone)} glyphs over the source's "
            f"leftovers, and the replacement adds {added} to the original")
        counted = f", {added} added as the request says"
    crowded = overlaps(before), overlaps(after)
    assert crowded[1] <= crowded[0], (
        f"{crowded[1]} pairs of glyphs overlap on the saved page, {crowded[0]} on the source")
    print(f"[PASS] {unchanged} glyphs unchanged, {moved} moved down {drop} pt, "
          f"{len(gone)} replaced on one line, {len(new)} new on "
          f"{len({round(c[2], 1) for c in new})} lines{counted}; "
          f"overlapping pairs {crowded[0]} -> {crowded[1]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--generate', type=Path)
    parser.add_argument('--check', nargs=2, type=Path)
    parser.add_argument('--compare', nargs='+', metavar='ARG',
                        help='<source.pdf> <edited.pdf> <page> [<requests.json>]')
    args = parser.parse_args()
    if args.generate:
        generate(args.generate)
    elif args.check:
        check(*args.check)
    elif args.compare:
        if len(args.compare) not in (3, 4):
            parser.error('--compare takes <source.pdf> <edited.pdf> <page> [<requests.json>]')
        source, saved, page, *request = args.compare
        compare(Path(source), Path(saved), int(page), *request)
    else:
        parser.error('--generate, --check or --compare')


if __name__ == '__main__':
    main()
