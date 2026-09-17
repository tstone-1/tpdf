#!/usr/bin/env python3
"""Generate CJK replacements and independently read their text AND outlines.

uv run --with pypdf --with fonttools==4.65.0 python scripts/text_cjk_check.py <probe> <new-dir>
The source and requests are synthetic. Native checks compare preview/save pixels
and unchanged neighbours. FontTools checks subset glyph outlines and advances
against the bundled source; extraction alone cannot detect a .notdef glyph.
"""
import argparse
import io
import json
import re
import subprocess
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.ttLib.sfnt import calcChecksum
from pypdf import PdfReader
from pypdf.generic import DecodedStreamObject, NameObject
from text_partial_check import fixture

ROOT = Path(__file__).resolve().parents[1]


def run(probe, directory):
    directory.mkdir(parents=True, exist_ok=False)
    replacement = 'ACME \u65b0\u589e\u6c49\u5b57 \u65e5\u672c\u8a9e \ud55c\uae00'
    for mode in ['auto', 'noto_sans_cjk_sc_bold']:
        writer = fixture()
        stream = DecodedStreamObject()
        stream.set_data(b'BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 -80 Td (SECOND) Tj ET')
        writer.pages[0][NameObject('/Contents')] = writer._add_object(stream)
        source = directory / f'{mode}.pdf'
        writer.write(source)
        requests = directory / f'{mode}.json'
        requests.write_text(json.dumps([dict(page=0, contains='FIRST', replacement=replacement,
            layout=dict(width=210, height=22, size=12, wrap=False, font=mode))]), encoding='utf-8')
        output = directory / mode
        subprocess.run([str(probe), '--roundtrip', str(source), str(requests), str(output)], check=True, timeout=90)
        pdf = PdfReader(output / 'edited.pdf')
        assert ''.join(pdf.pages[0].extract_text().split()) == ''.join((replacement + 'SECOND').split())
        fonts = [f.get_object() for f in pdf.pages[0]['/Resources']['/Font'].values()]
        fallback, = [f for f in fonts if 'NotoSansCJKSC' in f.get('/BaseFont', '')]
        child = fallback['/DescendantFonts'][0].get_object()
        program = child['/FontDescriptor']['/FontFile2'].get_data()
        assert len(program) < 64 * 1024
        assert calcChecksum(program) == 0xB1B0AFBA, 'invalid sfnt checksum adjustment'
        subset = TTFont(io.BytesIO(program), checkChecksums=2)
        original = TTFont(ROOT / 'vendor/fonts' / ('NotoSansCJKsc-Bold.ttf' if mode.endswith('bold') else 'NotoSansCJKsc-Regular.ttf'))
        assert subset['OS/2'].fsType == original['OS/2'].fsType == 0
        cmap = fallback['/ToUnicode'].get_data()
        blocks = b' '.join(re.findall(rb'beginbfchar(.*?)endbfchar', cmap, re.S))
        pairs = re.findall(rb'<([0-9A-F]+)>\s*<([0-9A-F]+)>', blocks)
        assert len(pairs) == len(set(replacement))
        mapping = child['/CIDToGIDMap'].get_data()
        for cid, text in pairs:
            code = int(cid, 16)
            char = bytes.fromhex(text.decode('ascii')).decode('utf-16-be')
            gid = int.from_bytes(mapping[code*2:code*2+2], 'big')
            assert gid != 0
            saved_name = subset.getGlyphName(gid)
            source_name = original.getBestCmap()[ord(char)]
            a = subset['glyf'][saved_name].getCoordinates(subset['glyf'])
            b = original['glyf'][source_name].getCoordinates(original['glyf'])
            assert a == b, 'saved glyph outline differs from bundled source'
            assert subset['hmtx'][saved_name] == original['hmtx'][source_name]
        print('[PASS]', mode, len(program), 'font bytes; Unicode, glyph outlines, widths and embedding rights verified')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    run(args.probe.resolve(), args.directory)
