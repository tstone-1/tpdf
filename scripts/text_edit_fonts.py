#!/usr/bin/env python3
"""Phase 5 font-preview spike, using only fonts and PDFs generated here.

uv run --with fonttools --with pypdf scripts/text_edit_fonts.py scratch/text-edit
uv run --with fonttools --with pypdf scripts/text_edit_fonts.py scratch/text-edit \
    --roundtrip-binary src-tauri/target/debug/examples/text-roundtrip
swift scripts/text_edit_webkit.swift scratch/text-edit/index.html

No input PDF is accepted: this is a feasibility instrument, not a document
loader. Production font extraction must run in the document worker. The tiny
original geometric glyphs use no system font and carry the repository's MIT
licence. Each font is embedded in a PDF and extracted again before inspection.
The browser separately measures loading, advances, and actual painted pixels.
"""

from __future__ import annotations

import argparse
import base64
import io
import json
import subprocess
from pathlib import Path


def make_font(*, cff: bool = False, fs_type: int = 0) -> bytes:
    """Build A as a solid rectangle and B as two separated vertical bars."""
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.t2CharStringPen import T2CharStringPen
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    builder = FontBuilder(1000, isTTF=not cff)
    names = [".notdef", "space", "A", "B"]
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({32: "space", 65: "A", 66: "B"})
    glyphs = {}
    for name in names:
        pen = T2CharStringPen(600, None) if cff else TTGlyphPen(None)
        rectangles = {".notdef": [(0, 0, 100, 100)], "space": [],
                      "A": [(0, 0, 400, 700)],
                      "B": [(0, 0, 100, 700), (300, 0, 400, 700)]}[name]
        for left, bottom, right, top in rectangles:
            pen.moveTo((left, bottom))
            pen.lineTo((left, top))
            pen.lineTo((right, top))
            pen.lineTo((right, bottom))
            pen.closePath()
        glyphs[name] = pen.getCharString() if cff else pen.glyph()
    if cff:
        builder.setupCFF("TPDFSynthetic", {}, glyphs, {})
    else:
        builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name: (600, 0) for name in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({"familyName": "TPDF Synthetic", "styleName": "Regular",
                            "uniqueFontIdentifier": "TPDF-Synthetic-1",
                            "fullName": "TPDF Synthetic", "psName": "TPDFSynthetic"})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800,
                     usWinDescent=200, fsType=fs_type)
    builder.setupPost()
    # Deterministic bytes, including when the script is run on another day.
    builder.font["head"].created = builder.font["head"].modified = 3800000000
    output = io.BytesIO()
    builder.save(output)
    return output.getvalue()


def variant(data: bytes, remove: str) -> bytes:
    """Remove one table without changing the outlines or their glyph order."""
    from fontTools.ttLib import TTFont

    font = TTFont(io.BytesIO(data), recalcTimestamp=False)
    del font[remove]
    output = io.BytesIO()
    font.save(output)
    return output.getvalue()


def pdf_round_trip(data: bytes, kind: str, path: Path) -> bytes:
    """Embed the font on a real page, then recover its exact stream bytes."""
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject,
                               FloatObject, NameObject, NumberObject)

    def dictionary(**entries):
        return DictionaryObject({NameObject('/' + key): value for key, value in entries.items()})

    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=160)
    stream = DecodedStreamObject()
    stream.set_data(data)
    stream[NameObject('/Length1')] = NumberObject(len(data))
    if kind != 'ttf':
        stream[NameObject('/Subtype')] = NameObject('/Type1C' if kind == 'raw-cff' else '/OpenType')
    file_key = 'FontFile2' if kind == 'ttf' else 'FontFile3'
    descriptor = dictionary(Type=NameObject('/FontDescriptor'), FontName=NameObject('/TPDFSynthetic'),
                            Flags=NumberObject(32), FontBBox=ArrayObject([NumberObject(v) for v in (0, 0, 400, 700)]),
                            ItalicAngle=NumberObject(0), Ascent=NumberObject(800), Descent=NumberObject(-200),
                            CapHeight=NumberObject(700), StemV=NumberObject(100))
    descriptor[NameObject('/' + file_key)] = writer._add_object(stream)
    font = dictionary(Type=NameObject('/Font'), Subtype=NameObject('/TrueType' if kind == 'ttf' else '/Type1'),
                      BaseFont=NameObject('/TPDFSynthetic'), Encoding=NameObject('/WinAnsiEncoding'),
                      FirstChar=NumberObject(32), LastChar=NumberObject(66),
                      Widths=ArrayObject([FloatObject(600)] * 35),
                      FontDescriptor=writer._add_object(descriptor))
    page[NameObject('/Resources')] = dictionary(Font=dictionary(F1=writer._add_object(font)))
    content = DecodedStreamObject()
    content.set_data(b'q 0 0 1 RG 2 w 10 10 m 290 10 l S Q\n'
                     b'q 0 0.5 0 rg 240 100 30 30 re f Q\n'
                     b'BT /F1 100 Tf 20 40 Td (AB) Tj ET\n')
    page[NameObject('/Contents')] = writer._add_object(content)
    writer.write(path)
    reader = PdfReader(path, strict=True)
    recovered = reader.pages[0]['/Resources']['/Font']['/F1']['/FontDescriptor']['/' + file_key].get_data()
    assert recovered == data, 'PDF font stream changed during round trip'
    assert reader.pages[0].extract_text().strip() == 'AB', 'synthetic page text'
    return recovered


def inspect_font(data: bytes, text: str = 'ABZ') -> dict:
    """Report preview prerequisites; never infer editability from successful loading.

    fsType is a conservative technical screen, not a licence grant. Reserved or
    contradictory bits and absent rights metadata stay unknown. No-subsetting
    prevents extending/rebuilding a subset; it does not prohibit loading intact
    bytes. Source: https://learn.microsoft.com/en-us/typography/opentype/spec/os2
    """
    from fontTools.ttLib import TTFont, TTLibError

    result = {'container': 'unsupported', 'rights': 'unknown', 'hasCmap': False,
              'missing': sorted(set(text)), 'canSubset': False, 'candidate': False}
    try:
        font = TTFont(io.BytesIO(data), recalcTimestamp=False)
    except TTLibError:
        return result
    result['container'] = 'opentype-cff' if 'CFF ' in font else 'truetype'
    cmap = (font.getBestCmap() or {}) if 'cmap' in font else {}
    result['hasCmap'] = bool(cmap)
    result['missing'] = sorted({char for char in text if cmap.get(ord(char), '.notdef') == '.notdef'})
    if 'OS/2' in font:
        bits = font['OS/2'].fsType
        level = bits & 0xE
        known = not (bits & ~0x30E) and level in (0, 2, 4, 8)
        if known:
            result['rights'] = ('bitmap-only' if bits & 0x200 else
                                {0: 'installable', 2: 'restricted', 4: 'preview-print', 8: 'editable'}[level])
            result['canSubset'] = not bool(bits & 0x100)
    result['candidate'] = result['hasCmap'] and result['rights'] in ('installable', 'editable')
    return result


def generate(directory: Path) -> None:
    """Write the fixture matrix and a browser page whose report is machine-readable."""
    from fontTools.ttLib import TTFont

    directory.mkdir(parents=True, exist_ok=True)
    ttf, otf = make_font(), make_font(cff=True)
    cff_font = TTFont(io.BytesIO(otf))
    raw = cff_font['CFF '].compile(cff_font)
    cases = [
        ('truetype', ttf, 'ttf', True),
        ('opentype-cff', otf, 'otf', True),
        ('missing-cmap', variant(ttf, 'cmap'), 'ttf', False),
        ('missing-rights', variant(ttf, 'OS/2'), 'ttf', False),
        ('restricted', make_font(fs_type=2), 'ttf', False),
        ('preview-print', make_font(fs_type=4), 'ttf', False),
        ('editable', make_font(fs_type=8), 'ttf', True),
        ('no-subsetting', make_font(fs_type=0x100), 'ttf', True),
        ('bitmap-only', make_font(fs_type=0x200), 'ttf', False),
        ('conflicting-rights', make_font(fs_type=0xC), 'ttf', False),
        ('reserved-rights', make_font(fs_type=1), 'ttf', False),
        ('raw-cff', raw, 'raw-cff', False),
    ]
    reports = []
    for name, data, kind, expected in cases:
        recovered = pdf_round_trip(data, kind, directory / f'{name}.pdf')
        report = inspect_font(recovered)
        assert report['candidate'] == expected, (name, report)
        if name == 'no-subsetting':
            assert not report['canSubset'], 'no-subsetting flag was ignored'
        if name == 'editable':
            assert report['canSubset'], 'editable control should allow subsetting'
        if report['hasCmap']:
            assert report['missing'] == ['Z'], (name, 'subset coverage', report)
        assert inspect_font(recovered, 'AB')['missing'] == ([] if report['hasCmap'] else ['A', 'B'])
        reports.append({'name': name, **report, 'data': base64.b64encode(recovered).decode('ascii')})
        print(f'[PASS] {name}: rights={report["rights"]}, cmap={report["hasCmap"]}, candidate={report["candidate"]}', flush=True)
    # Every glyph is original to this script, so testing even deliberately
    # restricted *synthetic* cases does not load anyone else's restricted font.
    reports.append({'name': 'invalid-font', 'data': base64.b64encode(b'not a font').decode('ascii'),
                    'candidate': False, 'hasCmap': False, 'rights': 'unknown', 'missing': ['A', 'B', 'Z']})
    payload = json.dumps(reports).replace('<', '\\u003c')
    script = Path(__file__).with_name('text_edit_fonts.js').read_text(encoding='utf-8')
    html = ('<!doctype html><meta charset="utf-8"><title>Text editing font probe</title>'
            '<h1>Text editing font probe</h1><p>Original synthetic glyphs; A is a rectangle, B is two bars.</p>'
            '<pre id="result">RUNNING</pre><div id="samples"></div>'
            '<script id="cases" type="application/json">' + payload + '</script><script>' + script + '</script>')
    (directory / 'index.html').write_text(html, encoding='utf-8', newline='')
    (directory / 'fonts.json').write_text(json.dumps([{k: v for k, v in r.items() if k != 'data'} for r in reports], indent=2) + '\n', encoding='utf-8', newline='')
    print(f'[PASS] {len(cases)} PDF/font round trips; browser entry: {directory / "index.html"}', flush=True)


def check_round_trip(directory: Path, binary: Path) -> None:
    """Exercise the real surgical writer and prove its strict verdict can fail."""
    from pypdf import PdfReader

    for name in ('truetype', 'opentype-cff'):
        source = directory / f'{name}.pdf'
        before = PdfReader(source, strict=True).pages[0]
        for label, replacement, expected_success in (
            ('replace', 'BA', True),
            ('missing-glyph', 'ABZ', False),
            ('no-op', 'AB', False),
            ('overflow', 'BBBB', False),
        ):
            output = directory / f'{name}-{label}'
            command = [str(binary.resolve()), str(source.resolve()), '--needle', 'AB',
                       '--replacement', replacement, '--outdir', str(output.resolve()), '--strict-surgical']
            result = subprocess.run(command, capture_output=True, timeout=60)
            # Keep the whole transcript so a native crash cannot be mistaken
            # for the intended negative verdict merely because both exit != 0.
            (directory / f'{name}-{label}.log').write_bytes(result.stdout + result.stderr)
            if expected_success:
                assert result.returncode == 0, (name, label, result.stderr.decode('utf-8', 'replace'))
                for action, expected in (('set-text', 'BA'), ('remove', '')):
                    after = PdfReader(output / f'B-surgical-{action}.pdf', strict=True).pages[0]
                    assert after.extract_text().strip() == expected, (name, action, 'saved text')
                    original_ops = before.get_contents().operations
                    actual_ops = after.get_contents().operations
                    assert [op for op in original_ops if op[1] != b'Tj'] == [op for op in actual_ops if op[1] != b'Tj'], 'unrelated PDF operators changed'
                    font_before = before['/Resources']['/Font']['/F1']
                    font_after = after['/Resources']['/Font']['/F1']
                    key = '/FontFile2' if name == 'truetype' else '/FontFile3'
                    assert font_before['/FontDescriptor'][key].get_data() == font_after['/FontDescriptor'][key].get_data(), 'embedded font changed'
            else:
                assert result.returncode == 1 and b'[FAIL] strict surgical check:' in result.stderr, (name, label, 'did not fail through the strict verdict')
            print(f'[PASS] {name} {label}: expected {"success" if expected_success else "strict refusal"}', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--roundtrip-binary', type=Path, help='also check surgical replacement and its failing controls')
    args = parser.parse_args()
    generate(args.directory)
    if args.roundtrip_binary:
        check_round_trip(args.directory, args.roundtrip_binary)
