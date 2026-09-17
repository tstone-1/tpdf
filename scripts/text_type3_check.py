#!/usr/bin/env python3
"""Independent Type3 edit/readback with original, synthetic geometric glyphs.

uv run --with pypdf scripts/text_type3_check.py <text-edit-probe> <new-directory>
Exercises both signs of FontMatrix's y scale, code-32 word spacing, Unicode,
continued shows, deletion and explicit layout. Outputs belong in an ignored dir.
"""
import argparse
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject,
                          FloatObject, NameObject, NumberObject)
from text_continuation_check import value


def dictionary(**entries):
    return DictionaryObject({NameObject('/' + k): v for k, v in entries.items()})


def fixture(negative):
    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=240)

    def stream(data):
        obj = DecodedStreamObject()
        obj.set_data(data.encode('ascii'))
        return writer._add_object(obj)

    y = -700 if negative else 700
    box = f'0 {min(0, y)} 500 {max(0, y)}'
    procs = dictionary(**{name: stream(f'600 0 {box} d1 {path} f') for name, path in {
        'A': f'0 0 m 500 0 l 500 {y} l 0 {y} l h',
        'B': f'0 0 m 500 0 l 250 {y} l h',
        'C': f'0 0 m 0 {y} 500 {y} 500 0 c h',
    }.items()})
    cmap = stream('/CIDInit /ProcSet findresource begin 12 dict begin begincmap '
                  '/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def '
                  '/CMapName /Adobe-Identity-UCS def /CMapType 2 def '
                  '1 begincodespacerange <00> <FF> endcodespacerange '
                  '3 beginbfchar <01> <0041> <20> <0042> <7F> <4E2D> endbfchar '
                  'endcmap CMapName currentdict /CMap defineresource pop end end')
    font = dictionary(Type=NameObject('/Font'), Subtype=NameObject('/Type3'),
                      FirstChar=NumberObject(1), LastChar=NumberObject(127),
                      FontMatrix=ArrayObject([FloatObject(n) for n in
                          [1/1024, 0, 0, -1/1024 if negative else 1/1024, 0, 0]]),
                      FontBBox=ArrayObject([NumberObject(n) for n in [0, min(0, y), 500, max(0, y)]]),
                      Widths=ArrayObject([NumberObject(600 if n in (1, 32, 127) else 0) for n in range(1, 128)]),
                      Encoding=dictionary(Differences=ArrayObject([NumberObject(1), NameObject('/A'),
                          NumberObject(32), NameObject('/B'), NumberObject(127), NameObject('/C')])),
                      CharProcs=procs, ToUnicode=cmap)
    page[NameObject('/Resources')] = dictionary(Font=dictionary(T3=writer._add_object(font)))
    page[NameObject('/Contents')] = stream('BT /T3 12 Tf 3 Tw 40 180 Td <01207F> Tj <01> Tj ET')
    return writer


def run(probe, directory):
    directory.mkdir(parents=True, exist_ok=False)
    for negative in (False, True):
        for kind, replacement in [('replace', 'B\u4e2d'), ('delete', ''), ('layout', '\u4e2d')]:
            name = f'{"negative" if negative else "positive"}-{kind}'
            source = directory / f'{name}.pdf'
            fixture(negative).write(source)
            request = dict(page=0, contains='AB\u4e2d', replacement=replacement)
            if kind == 'layout':
                request['layout'] = dict(width=25, height=18, size=12, wrap=False, font='original')
            requests = directory / f'{name}.json'
            requests.write_text(json.dumps([request]), encoding='utf-8')
            output = directory / name
            subprocess.run([str(probe), '--roundtrip', str(source), str(requests), str(output)],
                           check=True, timeout=90)
            before, after = PdfReader(source), PdfReader(output / 'edited.pdf')
            assert ''.join(before.pages[0].extract_text().split()) == 'AB\u4e2dA'
            assert ''.join(after.pages[0].extract_text().split()) == replacement + 'A', name
            assert value(before.pages[0]['/Resources']) == value(after.pages[0]['/Resources']), name
            print('[PASS]', name, 'independent Unicode readback and unchanged font programs')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    run(args.probe.resolve(), args.directory)
