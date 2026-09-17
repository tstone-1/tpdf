#!/usr/bin/env python3
"""Independent partial-page/ActualText readback through the contained worker.

uv run --with pypdf scripts/text_partial_check.py <text-edit-probe> <new-directory>
Uses the committed original synthetic embedded font.
All inputs are synthetic; outputs belong in an ignored directory.
"""
import argparse
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, ContentStream, DecodedStreamObject,
                          DictionaryObject, NameObject, NumberObject)
from text_continuation_check import value

ROOT = Path(__file__).resolve().parents[1]


def fixture():
    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=240)
    program = DecodedStreamObject()
    program.set_data((ROOT / 'src-tauri/src/textedit/synthetic.ttf').read_bytes())
    descriptor = DictionaryObject({NameObject('/' + key): NumberObject(number) for key, number in
        dict(Flags=32, ItalicAngle=0, Ascent=800, Descent=-200, CapHeight=700, StemV=100).items()})
    descriptor.update({NameObject('/Type'): NameObject('/FontDescriptor'),
                       NameObject('/FontName'): NameObject('/TPDFSynthetic'),
                       NameObject('/FontBBox'): ArrayObject([NumberObject(n) for n in [0, 0, 400, 700]]),
                       NameObject('/FontFile2'): writer._add_object(program)})
    font = DictionaryObject({NameObject('/' + key): NameObject('/' + name) for key, name in
        dict(Type='Font', Subtype='TrueType', BaseFont='TPDFSynthetic', Encoding='WinAnsiEncoding').items()})
    font.update({NameObject('/FirstChar'): NumberObject(32), NameObject('/LastChar'): NumberObject(89),
                 NameObject('/Widths'): ArrayObject([NumberObject(600) for _ in range(58)]),
                 NameObject('/FontDescriptor'): writer._add_object(descriptor)})
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):
        DictionaryObject({NameObject('/F1'): writer._add_object(font)})})
    return writer


def tagged_table(writer):
    def dictionary(**entries):
        return DictionaryObject({NameObject('/' + key): entry for key, entry in entries.items()})

    def node(role, parent):
        return writer._add_object(dictionary(S=NameObject('/' + role), P=parent))

    page = writer.pages[0]
    root = writer._add_object(dictionary(Type=NameObject('/StructTreeRoot')))
    document = node('Document', root)
    table = node('Table', document)
    row = node('TR', table)
    cell = node('TD', row)
    paragraph = node('P', cell)
    for parent, child in [(root, document), (document, table), (table, row), (row, cell), (cell, paragraph)]:
        parent.get_object()[NameObject('/K')] = child
    paragraph.get_object().update(dictionary(Pg=page.indirect_reference, K=NumberObject(0)))
    cell.get_object()[NameObject('/A')] = dictionary(O=NameObject('/Table'), ColSpan=NumberObject(2))
    parents = writer._add_object(dictionary(Nums=ArrayObject([NumberObject(0), ArrayObject([paragraph])])))
    root.get_object()[NameObject('/ParentTree')] = parents
    page[NameObject('/StructParents')] = NumberObject(0)
    writer._root_object[NameObject('/StructTreeRoot')] = root


def run(probe, directory):
    directory.mkdir(parents=True, exist_ok=False)
    cases = {
        'logical': ('BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC (SECOND) Tj ET', None, 'IN'),
        'logical-layout': ('BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC (SECOND) Tj ET',
                           dict(width=36, height=20, size=12, wrap=False, font='original'), 'IN'),
        'logical-delete': ('BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC (SECOND) Tj ET', None, ''),
        'alternate': ('BT /F1 12 Tf 40 180 Td (FIRST) Tj /Span << /ActualText <FEFF00A0> >> BDC ( ) Tj EMC (SECOND) Tj ET', None, 'IN'),
        'skew': ('BT /F1 12 Tf 1 .2 .1 1 40 80 Tm (SECOND) Tj ET BT /F1 12 Tf 40 180 Td (FIRST) Tj ET', None, 'IN'),
        'mirror': ('BT /F1 12 Tf -1 0 0 1 160 80 Tm (SECOND) Tj ET BT /F1 12 Tf 40 180 Td (FIRST) Tj ET', None, 'IN'),
        'cell-paragraph': ('/P << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC', None, 'IN'),
    }
    for name, (body, layout, replacement) in cases.items():
        writer = fixture()
        stream = DecodedStreamObject()
        stream.set_data(body.encode('ascii'))
        writer.pages[0][NameObject('/Contents')] = writer._add_object(stream)
        if name == 'cell-paragraph':
            tagged_table(writer)
        source = directory / (name + '.pdf')
        writer.write(source)
        request = dict(page=0, contains='FIRST', replacement=replacement)
        if layout:
            request['layout'] = layout
        requests = directory / (name + '.json')
        requests.write_text(json.dumps([request]), encoding='utf-8')
        output = directory / name
        subprocess.run([str(probe), '--roundtrip', str(source), str(requests), str(output)], check=True, timeout=90)
        before, after = PdfReader(source), PdfReader(output / 'edited.pdf')
        assert value(before.pages[0]['/Resources']) == value(after.pages[0]['/Resources']), name
        old = ContentStream(before.pages[0].get_contents(), before).operations
        new = ContentStream(after.pages[0].get_contents(), after).operations
        actual = [args[1]['/ActualText'] for args, op in new if op == b'BDC' and '/ActualText' in args[1]]
        if name.startswith('logical'):
            assert actual == [replacement], (name, actual)
        elif name == 'alternate':
            assert actual == ['\u00a0']
        if not layout:
            assert len(old) == len(new), name
            for (a, op), (b, new_op) in zip(old, new):
                if op == b'Tj' and str(a[0]) == 'FIRST':
                    assert new_op in (b'Tj', b'TJ')
                    text = b[0][0] if new_op == b'TJ' else b[0]
                    assert str(text) == replacement
                elif op == b'BDC' and name.startswith('logical'):
                    assert new_op == op
                else:
                    assert value(ArrayObject(a)) == value(ArrayObject(b)) and new_op == op, name
        if name == 'cell-paragraph':
            # Compare the structure graph with page references as leaves, avoiding
            # its parent cycles and allowing object numbers to change on save.
            def graph(reader):
                seen, result = {}, []
                def visit(obj):
                    if hasattr(obj, 'idnum'):
                        if obj == reader.pages[0].indirect_reference:
                            return 'PAGE'
                        key = obj.idnum, obj.generation
                        if key not in seen:
                            seen[key] = len(result)
                            result.append(None)
                            result[seen[key]] = visit(obj.get_object())
                        return ('ref', seen[key])
                    if isinstance(obj, dict):
                        return {str(k): visit(v) for k, v in sorted(obj.items())}
                    if isinstance(obj, list):
                        return [visit(v) for v in obj]
                    return obj
                visit(reader.trailer['/Root'].raw_get('/StructTreeRoot'))
                return result
            assert graph(before) == graph(after)
        print('[PASS]', name, 'independent logical text, resources and preserved content')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    run(args.probe.resolve(), args.directory)
