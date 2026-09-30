#!/usr/bin/env python3
"""Generate the original MIT-licensed subset used by worker/editor tests.

uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --named-mapping output.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check before.pdf after.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls before.pdf after.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --layout-controls before.pdf after.pdf [--check options]
Writes a deterministic test font and an ignored PDF with the native UI fixture's
geometry. No installed or third-party font is read.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


# Text state a layout save may restate around its show (ISO 32000-1 Table 105),
# and the line operators it may replay afterwards to put the line matrix back.
STATE = (b"Tf", b"Tc", b"Tw", b"Tz", b"TL", b"Ts", b"Tm")
LINE = (b"Td", b"TD", b"T*")
SHOWS = (b"Tj", b"TJ")


def multiply(a, b):
    return [a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3],
            a[4] * b[0] + a[5] * b[2] + b[4], a[4] * b[1] + a[5] * b[3] + b[5]]


def text_states(operations):
    """The text state in force before each operation, following q/Q and BT."""
    state = {b"Tf": None, b"Tc": 0.0, b"Tw": 0.0, b"Tz": 100.0, b"TL": 0.0, b"Ts": 0.0,
             "tm": [1, 0, 0, 1, 0, 0], "lm": [1, 0, 0, 1, 0, 0]}
    saved, states = [], []
    for operands, operator in operations:
        states.append(dict(state))
        state = step(state, operands, operator, saved)
    states.append(dict(state))
    return states


def step(state, operands, operator, saved=None):
    state = dict(state)
    if operator == b"q" and saved is not None:
        saved.append(dict(state))
    elif operator == b"Q" and saved:
        restored = saved.pop()
        state.update({key: restored[key] for key in STATE[:-1]})
    elif operator == b"BT":
        state["tm"] = state["lm"] = [1, 0, 0, 1, 0, 0]
    elif operator == b"Tf":
        state[b"Tf"] = (operands[0], float(operands[1]))
    elif operator in STATE[1:-1]:
        state[operator] = float(operands[0])
    elif operator == b"Tm":
        state["tm"] = state["lm"] = [float(v) for v in operands]
    elif operator in LINE:
        x, y = (0.0, -state[b"TL"]) if operator == b"T*" else (float(operands[0]), float(operands[1]))
        if operator == b"TD":
            state[b"TL"] = -y
        state["tm"] = state["lm"] = multiply([1, 0, 0, 1, x, y], state["lm"])
    return state


def show_advance(operands, operator, state, fonts):
    """Horizontal text-space advance of one show, from the font's own widths."""
    from pypdf._font import Font

    name, size = state[b"Tf"]
    font = fonts[name].get_object()
    scale = state[b"Tz"] / 100
    parts = operands[0] if operator == b"TJ" else [operands[0]]
    composite = font.get("/Subtype") == "/Type0"
    if composite:
        descendant = font["/DescendantFonts"][0].get_object()
        widths, default, table = {}, float(descendant.get("/DW", 1000)), list(descendant.get("/W", []))
        index = 0
        while index < len(table):
            first, second = int(table[index]), table[index + 1].get_object()
            if isinstance(second, list):
                widths.update({first + offset: float(w) for offset, w in enumerate(second)})
                index += 2
            else:
                widths.update({code: float(table[index + 2]) for code in range(first, int(second) + 1)})
                index += 3
        width = lambda code: widths.get(code, default)
    elif "/Widths" in font:
        first = int(font.get("/FirstChar", 0))
        table = [float(w) for w in font["/Widths"]]
        missing = float(font.get("/FontDescriptor", {}).get("/MissingWidth", 0)) if "/FontDescriptor" in font else 0.0
        width = lambda code: table[code - first] if 0 <= code - first < len(table) else missing
    else:
        # A standard 14 font without Widths: Adobe's metrics, as pypdf carries them.
        metrics = Font.from_font_resource(font)
        assert isinstance(metrics.encoding, dict), "cannot measure the source show"
        width = lambda code: float(metrics.character_widths[metrics.encoding[code]])
    advance = 0.0
    for part in parts:
        if not isinstance(part, (str, bytes)):
            advance -= float(part) / 1000 * size * scale
            continue
        raw = part.original_bytes if isinstance(part, str) else bytes(part)
        stride = 2 if composite else 1
        for offset in range(0, len(raw), stride):
            code = int.from_bytes(raw[offset:offset + stride], "big")
            space = state[b"Tw"] if stride == 1 and code == 32 else 0.0
            advance += (width(code) / 1000 * size + state[b"Tc"] + space) * scale
    return advance


def edited_shows(operations, fonts):
    """Pair each edited source show with the show that replaced it.

    A byte patch replaces shows one for one. A layout save (every edit made in
    the application) replaces one show with: text state restated to the values
    the source had there, the new show, the same state and the source's line
    matrix restated, and at most one `TJ` of an empty string and a number that
    puts the cursor where the source show left it. Anything else refuses.
    """
    import difflib

    old_ops, new_ops = operations
    states = text_states(old_ops)
    matcher = difflib.SequenceMatcher(a=[repr(op) for op in old_ops], b=[repr(op) for op in new_ops], autojunk=False)
    changes, first = [], None
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        old, new = old_ops[i1:i2], new_ops[j1:j2]
        first = i1 if first is None else first
        if len(old) == len(new) and all(a[1] in SHOWS and b[1] == a[1] for a, b in zip(old, new)):
            changes += list(zip(old, new))
            continue
        assert len(old) == 1 and old[0][1] in SHOWS, "operator count changed"
        source = states[i1]
        shows = [index for index, op in enumerate(new) if op[1] in SHOWS]
        assert shows, "operator count changed"
        show = shows[0]
        state = dict(source)
        for operands, operator in new[:show]:
            assert operator in STATE, "operator count changed"
            state = step(state, operands, operator)
        assert all(state[key] == source[key] for key in STATE[:-1]) and close(state["tm"], source["tm"]), "restated text state differs from the source"
        after = dict(state)
        rest = new[show + 1:]
        cursor = None
        if rest and rest[-1][1] == b"TJ":
            cursor, rest = rest[-1], rest[:-1]
            parts = cursor[0][0]
            assert len(parts) == 2 and isinstance(parts[0], (str, bytes)) and len(parts[0]) == 0 and not isinstance(parts[1], (str, bytes)), "cursor restoration shows text"
        for operands, operator in rest:
            assert operator in STATE + LINE, "operator count changed"
            after = step(after, operands, operator)
        assert all(after[key] == source[key] for key in STATE[:-1]), "restored text state differs from the source"
        assert close(after["lm"], source["lm"]), "restored line matrix differs from the source"
        if cursor is not None:
            moved = -float(cursor[0][0][1]) / 1000 * source[b"Tf"][1] * source[b"Tz"] / 100
            # Where the source's cursor already was on its line, in text space.
            lm, tm = source["lm"], source["tm"]
            det = lm[0] * lm[3] - lm[1] * lm[2]
            ex, ey = tm[4] - lm[4], tm[5] - lm[5]
            along = (ex * lm[3] - ey * lm[2]) / det
            across = (ey * lm[0] - ex * lm[1]) / det
            assert abs(across) <= 1e-4, "source cursor left its line"
            wanted = along + show_advance(old[0][0], old[0][1], source, fonts)
            assert abs(moved - wanted) <= 1e-4, "cursor restoration does not reach the end of the source show"
        changes.append((old[0], new[show]))
    assert changes, "no text operand changed"
    return changes, first


def close(a, b):
    return all(abs(x - y) <= 1e-4 for x, y in zip(a, b))


def check(before, after, page_index=0, wrapped=False, float32=False, cid_latin1=False, overhang=False, default_encoding=False, w3c=False, dash=False, agenda=False, cff_unicode=False, cff_ligatures=False, passport=False, cid_ligatures=False, numbered_list=False, nested_list=False, list_child=False):
    """Independent parser: one changed operand, identical fonts and colour data."""
    from pypdf import PdfReader
    from pypdf.generic import ContentStream, DictionaryObject, StreamObject, FloatObject

    def value(obj):
        if isinstance(obj, tuple):
            return [value(v) for v in obj]
        obj = obj.get_object() if hasattr(obj, "get_object") else obj
        if isinstance(obj, list):
            return [value(v) for v in obj]
        if isinstance(obj, StreamObject):
            return ({str(k): value(v) for k, v in obj.items() if k not in ("/Length", "/Filter")}, obj.get_data())
        if isinstance(obj, DictionaryObject):
            return {str(k): value(v) for k, v in obj.items()}
        if float32 and isinstance(obj, FloatObject):
            # Explicit opt-in for lopdf's Real storage. Default checks retain
            # exact numeric comparison. Streams and nonnumeric values stay exact.
            import math
            import struct
            number = struct.unpack(">f", struct.pack(">f", float(obj)))[0]
            assert math.isfinite(number), "nonfinite float32 value"
            return number
        return obj

    if passport:
        import hashlib
        assert hashlib.sha256(Path(before).read_bytes()).hexdigest() == "0c70c5f7df62185cbd779ab506a4e61ae77abb8ffba1f1a134fdf3c7b56c4f21", "expected unchanged passport guide"
        assert page_index == 15, "expected passport page 16"
    if agenda:
        import hashlib
        assert hashlib.sha256(Path(before).read_bytes()).hexdigest() == "5aa6129722dd20b351cf99575142666ab2c26b84555a9cabe920dcb07a17dbcd", "expected unchanged public agenda"
    readers = [PdfReader(path) for path in (before, after)]
    count = len(readers[0].pages)
    assert 0 <= page_index < count <= 128 and len(readers[1].pages) == count, "wrong page count"
    for index, (old_page, new_page) in enumerate(zip(readers[0].pages, readers[1].pages)):
        assert value(old_page["/Resources"]) == value(new_page["/Resources"]), "page resources changed"
        if index != page_index:
            assert old_page.get_contents().get_data() == new_page.get_contents().get_data(), "untouched page content changed"
            assert old_page.extract_text() == new_page.extract_text(), "untouched page text changed"
    pages = [reader.pages[page_index] for reader in readers]
    if "/StructTreeRoot" in readers[0].trailer["/Root"]:
        # Structure is cyclic through /P and references the page. Compare its
        # entire graph, independent of object renumbering, with the page as an
        # explicit leaf (its one changed content operand is checked below).
        from pypdf.generic import IndirectObject

        def structure(reader):
            identities, nodes = {}, []
            page_refs = {(p.indirect_reference.idnum, p.indirect_reference.generation): index for index, p in enumerate(reader.pages)}

            def visit(obj):
                if isinstance(obj, IndirectObject):
                    identity = (obj.idnum, obj.generation)
                    if identity in page_refs:
                        return ("page", page_refs[identity])
                    if identity in identities:
                        return ("ref", identities[identity])
                    index = identities[identity] = len(nodes)
                    nodes.append(None)
                    assert len(nodes) <= 256, "structure graph exceeds fixture limit"
                    nodes[index] = visit(obj.get_object())
                    return ("ref", index)
                if isinstance(obj, DictionaryObject):
                    return {str(k): visit(obj.raw_get(k)) for k in sorted(obj)}
                if isinstance(obj, list):
                    return [visit(v) for v in obj]
                return obj

            root = reader.trailer["/Root"]
            assert "/StructTreeRoot" in root, "tagged structure disappeared"
            top = visit(root.raw_get("/StructTreeRoot"))
            mark_info = value(root["/MarkInfo"]) if "/MarkInfo" in root else None
            return (top, nodes, [p.get("/StructParents") for p in reader.pages], mark_info)

        assert structure(readers[0]) == structure(readers[1]), "tagged structure or parent references changed"
        print("[PASS] independent parser: complete tagged structure graph and page parent key preserved")
    operations = [ContentStream(page["/Contents"], reader).operations
                  for page, reader in zip(pages, readers)]
    changes, changed_index = edited_shows(operations, pages[0]["/Resources"].get("/Font", {}))
    assert len(changes) == (6 if w3c else 1), "wrong number of changed text operands"
    if w3c:
        assert all(old[1] == b"Tj" and len(old[0]) == 1 for old, new in changes), "group edit changed non-text operators"
        assert all(new[1] == b"Tj" for old, new in changes[1:]), "group edit changed non-text operators"
    old, new = changes[0]
    assert old[1] in (b"Tj", b"TJ") and new[1] in (b"Tj", b"TJ"), "text-show operator changed"
    assert len(old[0]) == len(new[0]) == 1, "wrong text-show operand count"
    if old[1] == b"TJ":
        assert isinstance(old[0][0], list), "wrong source array"
    if new[1] == b"TJ":
        # kerning.rs keeps the source's own items around the changed middle:
        # an adjustment may survive, but only one the source array already had,
        # in its order. A new number would move text the parser cannot see.
        assert isinstance(new[0][0], list) and any(isinstance(part, (str, bytes)) for part in new[0][0]), "wrong replacement array"
        kept = [part for part in new[0][0] if not isinstance(part, (str, bytes))]
        source = iter(part for part in old[0][0] if not isinstance(part, (str, bytes))) if old[1] == b"TJ" else iter(())
        assert all(any(number == other for other in source) for number in kept), "replacement array contains an adjustment the source did not have"
    else:
        assert isinstance(new[0][0], (str, bytes)), "wrong replacement operand"
    fonts = list(pages[0]["/Resources"]["/Font"].values())
    cff_fonts = [font.get_object() for font in fonts
                 if font.get_object().get("/Subtype") == "/Type1" and
                 "/FontDescriptor" in font.get_object() and
                 "/FontFile3" in font.get_object()["/FontDescriptor"]]
    if cff_fonts:
        # pypdf otherwise warns and continues with incomplete CFF decoding.
        try:
            import fontTools.cffLib  # noqa: F401
        except ImportError as error:
            raise AssertionError("CFF readback requires fonttools; add --with fonttools") from error
    symbolic = [font.get_object() for font in fonts
                if ("/Encoding" not in font.get_object() and "/ToUnicode" in font.get_object())
                or font.get_object() in cff_fonts
                or (cid_ligatures and font.get_object().get("/Encoding") == "/Identity-H")]
    if passport:
        old_text, new_text = "ILB 53 (09.22)", "ILB 53"
    if agenda:
        old_text, new_text = ("REGULAR", "ANNUAL") if page_index == 0 else ("Community Hub", "Community")
    if symbolic:
        if agenda or passport:
            font_name = [args[0] for args, op in operations[0][:changed_index] if op == b"Tf"][-1]
            mapped_font = pages[0]["/Resources"]["/Font"][font_name]
            expected_operands = [(old, old_text), (new, new_text)]
        else:
            assert len(fonts) == len(symbolic) == 1, "symbolic readback requires a single fixture font"
            mapped_font = symbolic[0]
            expected_operands = [(old, "le" if w3c else "SYNTHETIC\u2013FIRST" if dash else "SYNTHETIC FIRST" + (" " if wrapped else "")), (new, "ll" if w3c else "EDITED\u2013FIRST" if dash else "EDITED FIRST")]
            if w3c:
                expected_operands = [(pair[0], text) for pair, text in zip(changes, ["Dumm", "y", " ", "PDF", " fi", "le"])]
                expected_operands += [(pair[1], text) for pair, text in zip(changes, ["Dummy PDF fill", "", "", "", "", ""])]
        if list_child:
            expected_operands = [(old, "SYNTHETIC SECOND"), (new, "EDITED SECOND")]
        # extract_text() deliberately falls back to identity for unmapped codes.
        # Read pypdf's parsed map explicitly so fallback cannot pass this check.
        from pypdf._cmap import get_encoding
        if cff_unicode:
            expected_operands = [(old, "SYNTHETIC \u2212\u00a0\u2018\u2019\u2013£"),
                                 (new, "EDITED £\u2013\u2019\u2018\u00a0\u2212")]
        if cff_ligatures or cid_ligatures:
            expected_operands = [(old, "SYNTHETIC ffi ffi fi fl ff"), (new, "EDITED ffi fi fl ff")]
        encoding, mapping = get_encoding(mapped_font)
        if mapped_font in cff_fonts and "/ToUnicode" not in mapped_font:
            assert isinstance(encoding, dict), "expected explicit CFF glyph encoding"
            mapping = {chr(code): text for code, text in encoding.items()}
        assert mapping and all(isinstance(k, str) and len(k) == 1 and (len(v) == 1 or ((cff_ligatures or cid_ligatures) and v in ("ffi", "ff", "fi", "fl"))) for k, v in mapping.items()), "unexpected fixture map"
        for operation, expected in expected_operands:
            parts = operation[0][0] if operation[1] == b"TJ" else operation[0]
            raw = b"".join(part.original_bytes if isinstance(part, str) else bytes(part)
                           for part in parts if isinstance(part, (str, bytes)))
            if cff_ligatures or cid_ligatures:
                expected_codes = b"SYNTHETIC ffi \x1f \x1e \x1c \x1d" if operation is old else b"EDITED \x1f \x1e \x1c \x1d"
                if cid_ligatures:
                    inverse = {text: code for code, text in mapping.items()}
                    assert len(inverse) == len(mapping), "ambiguous CID targets"
                    parts = [*"SYNTHETIC ffi ", "ffi", " ", "fi", " ", "fl", " ", "ff"] if operation is old else [*"EDITED ", "ffi", " ", "fi", " ", "fl", " ", "ff"]
                    expected_codes = b"".join(ord(inverse[text]).to_bytes(2, "big") for text in parts)
                assert raw == expected_codes, "ligature glyph codes changed"
            stride = 2 if cid_ligatures else 1
            assert len(raw) % stride == 0, "partial mapped code"
            codes = [int.from_bytes(raw[i:i+stride], "big") for i in range(0, len(raw), stride)]
            assert all(chr(code) in mapping for code in codes), "unmapped symbolic code"
            assert "".join(mapping[chr(code)] for code in codes) == expected, "wrong mapped operand"
    # Let the independent parser apply the font's encoding and ToUnicode map.
    # Comparing raw operand bytes to ASCII cannot verify symbolic font codes.
    expected_text = ("SYNTHETIC ÄÖÜ äöü ß", "ÖÄÜ äöü ß" if overhang else "ÄÖÜ äöü ß") if cid_latin1 or overhang else ("SYNTHETIC FIRST", "EDITED FIRST")
    if list_child:
        expected_text = ("SYNTHETIC SECOND", "EDITED SECOND")
    if cff_unicode:
        expected_text = ("SYNTHETIC \u2212\u00a0\u2018\u2019\u2013£", "EDITED £\u2013\u2019\u2018\u00a0\u2212")
    if cff_ligatures or cid_ligatures:
        expected_text = ("SYNTHETIC ffi ffi fi fl ff", "EDITED ffi fi fl ff")
    if dash:
        expected_text = ("SYNTHETIC\u2013FIRST", "EDITED\u2013FIRST")
    if default_encoding:
        expected_text = ("SYNTHETIC ' ` £ ß", "£ ' ` ß")
    if w3c:
        assert count == 1 and page_index == 0
        expected_text = ("Dummy PDF file", "Dummy PDF fill")
    if agenda or passport:
        assert (count == 16 and page_index == 15) if passport else (count == 2 and page_index in (0, 1)), "wrong public fixture page count"
        original = pages[0].extract_text()
        assert original.count(old_text) == 1, "wrong public fixture text"
        expected = original.replace(old_text, new_text)
        actual = pages[1].extract_text()
        # pypdf infers an extra space before the separately positioned trailing
        # space on page 2 after shortening the heading. The mapped operands above
        # are exact, and every other content operand/resource was already compared.
        assert (actual.split() == expected.split() if page_index == 1 else actual == expected), "wrong public fixture replacement or adjacent text"
    else:
        for page, first in zip(pages, expected_text):
            if list_child:
                assert " ".join(page.extract_text().split()) == "1. SYNTHETIC FIRST 1. " + first, "nested child text or labels changed"
                continue
            if numbered_list or nested_list:
                label = "1." if nested_list else "2."
                assert " ".join(page.extract_text().split()) == "1. " + first + " " + label + " SYNTHETIC SECOND", "list text or labels changed"
                continue
            assert " ".join(page.extract_text().split()) == " ".join((first + ("" if w3c else " SYNTHETIC SECOND")).split()), "wrong decoded text"
        if not symbolic:
            # The page text above folds whitespace, so an added or lost space
            # passes it. Decode both edited operands exactly through the font's
            # own encoding (pypdf's table, not the editor's).
            from pypdf._cmap import get_encoding
            font_name = [args[0] for args, op in operations[0][:changed_index] if op == b"Tf"][-1]
            encoding, _ = get_encoding(pages[0]["/Resources"]["/Font"][font_name].get_object())
            assert isinstance(encoding, dict), "expected a simple font's encoding table"
            for operation, text in ((old, expected_text[0] + (" " if wrapped else "")), (new, expected_text[1])):
                parts = operation[0][0] if operation[1] == b"TJ" else operation[0]
                raw = b"".join(part.original_bytes if isinstance(part, str) else bytes(part)
                               for part in parts if isinstance(part, (str, bytes)))
                assert all(code in encoding for code in raw) and "".join(encoding[code] for code in raw) == text, "wrong decoded operand"
    if float32:
        print("[PASS] independent parser: only target text operands changed; resources agree at float32 precision with exact stream bytes")
    else:
        print("[PASS] independent parser: only target text operands changed; font and colour resources preserved")


def tagged_controls(before, after, page_index=0, wrapped=False):
    """Prove nonvisual structure corruption fails the independent readback."""
    import contextlib
    import io
    import tempfile
    from pypdf import PdfWriter
    from pypdf.generic import DecodedStreamObject, NameObject, NumberObject, TextStringObject

    check(before, after, page_index, wrapped)
    with tempfile.TemporaryDirectory(prefix="tpdf-tagged-controls-") as room:
        sample = PdfWriter(clone_from=after)
        paragraphs = sample.root_object["/StructTreeRoot"]["/K"][0].get_object()["/K"]
        modes = ["root", "parent", "mcid", "alternate", "page_key"]
        if any("/EndIndent" in p.get_object().get("/A", {}) for p in paragraphs):
            modes.append("end_indent")
        if len(sample.pages) > 1:
            modes += ["page_owner", "other_page"]
        if any(len(p.get_object()["/K"]) > 1 for p in paragraphs):
            modes += ["item_order", "item_missing"]
        if any(isinstance(item, dict) for p in paragraphs for item in p.get_object()["/K"]):
            modes += ["mcr_page", "mcr_id"]
        for mode in modes:
            writer = PdfWriter(clone_from=after)
            root = writer.root_object["/StructTreeRoot"]
            paragraph = root["/K"][0].get_object()["/K"][0].get_object()
            if mode == "root":
                del writer.root_object["/StructTreeRoot"]
            elif mode == "parent":
                paragraph[NameObject("/P")] = root.indirect_reference
            elif mode == "mcid":
                paragraph["/K"][0] = NumberObject(1)
            elif mode == "alternate":
                paragraph[NameObject("/ActualText")] = TextStringObject("STALE SYNTHETIC TEXT")
            elif mode == "end_indent":
                attrs = next(p.get_object()["/A"] for p in root["/K"][0].get_object()["/K"] if "/EndIndent" in p.get_object().get("/A", {}))
                del attrs["/EndIndent"]
            elif mode == "page_key":
                writer.pages[0][NameObject("/StructParents")] = NumberObject(1)
            elif mode == "page_owner":
                old = paragraph.raw_get("/Pg")
                paragraph[NameObject("/Pg")] = next(p.indirect_reference for p in writer.pages if p.indirect_reference != old)
            elif mode in ("item_order", "item_missing"):
                items = next(p.get_object()["/K"] for p in root["/K"][0].get_object()["/K"] if len(p.get_object()["/K"]) > 1)
                if mode == "item_order":
                    items.reverse()
                else:
                    items.pop()
            elif mode in ("mcr_page", "mcr_id"):
                mcr = next(item for p in root["/K"][0].get_object()["/K"] for item in p.get_object()["/K"] if isinstance(item, dict))
                if mode == "mcr_page":
                    old = mcr.raw_get("/Pg")
                    mcr[NameObject("/Pg")] = next(p.indirect_reference for p in writer.pages if p.indirect_reference != old)
                else:
                    mcr[NameObject("/MCID")] = NumberObject(int(mcr["/MCID"]) + 1)
            else:
                other = next(p for index, p in enumerate(writer.pages) if index != page_index)
                stream = DecodedStreamObject()
                stream.set_data(other.get_contents().get_data() + b"\n% changed untouched page\n")
                other[NameObject("/Contents")] = writer._add_object(stream)
            target = Path(room) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, page_index, wrapped)
            except AssertionError as error:
                expected = "untouched page content" if mode == "other_page" else "tagged structure"
                assert expected in str(error), str(error)
            else:
                raise AssertionError("structure control survived: " + mode)
            print("[PASS] independent structure control:", mode)


def layout_controls(before, after, page_index=0, wrapped=False, **options):
    """Prove the readback of an edited page refuses what it exists to catch.

    Each control damages one thing in a copy of a passing output and requires
    `check` to refuse it for that reason: the input left unedited, an added
    space, a changed font resource, a restated text state that differs from the
    source, a cursor restoration a unit short, a kerning number the source never
    had, and a painting operator inside the edited region. A control that does
    not apply to this output (no restoration, no array) is skipped by name.
    """
    import contextlib
    import io
    import tempfile
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, ContentStream, FloatObject, NameObject, NumberObject

    check(before, after, page_index, wrapped, **options)
    source = ContentStream(PdfReader(before).pages[page_index]["/Contents"], PdfReader(before)).operations

    def damaged(mode):
        writer = PdfWriter(clone_from=after)
        page = writer.pages[page_index]
        if mode == "font_resource":
            font = next(iter(page["/Resources"]["/Font"].values())).get_object()
            font[NameObject("/Spoiled")] = NumberObject(1)
            return writer
        stream = ContentStream(page["/Contents"], writer)
        ops = stream.operations
        index = next(i for i, (old, new) in enumerate(zip(source, ops)) if old != new) if mode != "unedited" else None
        if mode == "unedited":
            return PdfWriter(clone_from=before)
        # The edited text object: from the first changed operation to its ET.
        end = next(i for i in range(index, len(ops)) if ops[i][1] == b"ET")
        shows = [i for i in range(index, end) if ops[i][1] in SHOWS]
        if mode == "added_space":
            operands, operator = ops[shows[0]]
            from pypdf.generic import ByteStringObject
            # A Tj's operand list is itself the list of parts to change.
            parts = operands[0] if operator == b"TJ" else operands
            last = max(i for i, part in enumerate(parts) if isinstance(part, (str, bytes)))
            raw = parts[last].original_bytes if isinstance(parts[last], str) else bytes(parts[last])
            parts[last] = ByteStringObject(raw + (b"\x00 " if options.get("cid_ligatures") else b" "))
        elif mode == "restated_state":
            restated = [i for i in range(index, end) if ops[i][1] == b"Tc"]
            if not restated:
                return None
            ops[restated[-1]] = ([FloatObject(0.5)], b"Tc")
        elif mode == "cursor_short":
            cursor = [i for i in range(index, end) if ops[i][1] == b"TJ" and len(ops[i][0][0]) == 2 and len(ops[i][0][0][0]) == 0]
            if not cursor:
                return None
            number = ops[cursor[0]][0][0][1]
            ops[cursor[0]][0][0][1] = FloatObject(float(number) + 1)
        elif mode == "new_kern":
            operands, operator = ops[shows[0]]
            if operator != b"TJ":
                return None
            operands[0].insert(1, NumberObject(-7))
        elif mode == "painting":
            ops.insert(shows[0], ([], b"n"))
        page.replace_contents(stream)
        return writer

    reasons = {
        "unedited": "no text operand changed",
        "added_space": "wrong",
        "font_resource": "page resources changed",
        "restated_state": "text state differs",
        "cursor_short": "cursor restoration",
        "new_kern": "adjustment the source did not have",
        "painting": "operator count changed",
    }
    with tempfile.TemporaryDirectory(prefix="tpdf-layout-controls-") as room:
        for mode, reason in reasons.items():
            writer = damaged(mode)
            if writer is None:
                print("[SKIP] readback control does not apply to this output:", mode)
                continue
            target = Path(room) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, page_index, wrapped, **options)
            except AssertionError as error:
                # A ligature fixture pins its glyph codes before it decodes them,
                # and a subset without a space cannot decode the added one.
                other = ("ligature glyph codes", "unmapped symbolic code") if mode == "added_space" else ()
                assert reason in str(error) or any(text in str(error) for text in other), f"{mode} refused for another reason: {error}"
            else:
                raise AssertionError("readback control survived: " + mode)
            print("[PASS] readback control refused:", mode)


def named_mapping(target):
    """Synthetic WinAnsi font with agreeing Mac/Windows maps and ToUnicode."""
    import io
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import DecodedStreamObject, NameObject

    source = ROOT / "testdata/textedit-embedded.pdf"
    writer = PdfWriter(clone_from=source)
    font = writer.pages[0]["/Resources"]["/Font"]["/F1"]
    program = TTFont(io.BytesIO(font["/FontDescriptor"]["/FontFile2"].get_data()), recalcTimestamp=False)
    legacy = CmapSubtable.newSubtable(0)
    legacy.platformID, legacy.platEncID, legacy.language = 1, 0, 0
    legacy.cmap = {code: name for code, name in program.getBestCmap().items() if 32 <= code <= 126}
    program["cmap"].tables.append(legacy)
    output = io.BytesIO()
    program.save(output)
    stream = DecodedStreamObject()
    stream.set_data(output.getvalue())
    font["/FontDescriptor"][NameObject("/FontFile2")] = writer._add_object(stream)
    mapping = DecodedStreamObject()
    mapping.set_data(b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <20> <59> <0020> endbfrange endcmap CMapName currentdict /CMap defineresource pop end end")
    font[NameObject("/ToUnicode")] = writer._add_object(mapping)
    writer.write(target)
    assert "SYNTHETIC FIRST" in PdfReader(target).pages[0].extract_text()
    print("[PASS] generated independently decoded matching named-font maps")


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--named-mapping":
        named_mapping(Path(sys.argv[2]))
        return
    if len(sys.argv) >= 4 and sys.argv[1] in ("--check", "--tagged-controls", "--layout-controls"):
        checking = sys.argv[1] in ("--check", "--layout-controls")
        page, wrapped, float32, default_encoding = 0, False, False, False
        w3c = dash = agenda = cff_unicode = cff_ligatures = passport = cid_ligatures = numbered_list = nested_list = list_child = False
        for option in sys.argv[4:]:
            if option.startswith("--page="):
                page = int(option.split("=", 1)[1])
            elif option == "--wrapped":
                wrapped = True
            elif option == "--default-encoding" and checking:
                default_encoding = True
            elif option == "--nested-list-child" and checking:
                list_child = True
            elif option == "--nested-list" and checking:
                nested_list = True
            elif option == "--list" and checking:
                numbered_list = True
            elif option == "--passport" and checking:
                passport = True
            elif option == "--agenda" and checking:
                agenda = True
            elif option == "--cid-ligatures" and checking:
                cid_ligatures = True
            elif option == "--cff-ligatures" and checking:
                cff_ligatures = True
            elif option == "--cff-unicode" and checking:
                cff_unicode = True
            elif option == "--dash" and checking:
                dash = True
            elif option == "--w3c-dummy" and checking:
                w3c = True
            elif option == "--float32" and checking:
                float32 = True
            else:
                raise SystemExit("expected --page=N (zero based), --wrapped, --float32, --default-encoding, --cff-unicode, --cff-ligatures, --cid-ligatures, --dash, --agenda, --passport, --list, --nested-list, --nested-list-child or --w3c-dummy (--check only)")
        options = dict(float32=float32, default_encoding=default_encoding, w3c=w3c, dash=dash, agenda=agenda, cff_unicode=cff_unicode, cff_ligatures=cff_ligatures, passport=passport, cid_ligatures=cid_ligatures, numbered_list=numbered_list, nested_list=nested_list, list_child=list_child)
        if sys.argv[1] == "--layout-controls":
            layout_controls(*sys.argv[2:4], page, wrapped, **options)
            return
        action = check if sys.argv[1] == "--check" else tagged_controls
        if float32 or default_encoding or w3c or dash or agenda or cff_unicode or cff_ligatures or passport or cid_ligatures or numbered_list or nested_list or list_child:
            check(*sys.argv[2:4], page, wrapped, float32=float32, default_encoding=default_encoding, w3c=w3c, dash=dash, agenda=agenda, cff_unicode=cff_unicode, cff_ligatures=cff_ligatures, passport=passport, cid_ligatures=cid_ligatures, numbered_list=numbered_list, nested_list=nested_list, list_child=list_child)
        else:
            action(*sys.argv[2:4], page, wrapped)
        return
    if len(sys.argv) != 1:
        raise SystemExit("expected no arguments, --check, --layout-controls or --tagged-controls before.pdf after.pdf")
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, DecodedStreamObject, NameObject, NumberObject

    data = make_font(characters="AB SYNTHETIC FIRST EDITED SECOND")
    (ROOT / "src-tauri/src/textedit/synthetic.ttf").write_bytes(data)
    target = ROOT / "testdata/textedit-embedded.pdf"
    pdf_round_trip(data, "ttf", target)
    writer = PdfWriter(clone_from=PdfReader(target))
    page = writer.pages[0]
    page[NameObject("/MediaBox")] = ArrayObject([NumberObject(v) for v in (0, 0, 300, 240)])
    font = page["/Resources"]["/Font"]["/F1"]
    font[NameObject("/LastChar")] = NumberObject(89)
    font[NameObject("/Widths")] = ArrayObject([NumberObject(600)] * 58)
    content = DecodedStreamObject()
    content.set_data(b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\n"
                     b"BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    print("[PASS] generated synthetic TrueType subset and editor PDF")


if __name__ == "__main__":
    main()
