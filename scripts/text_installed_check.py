#!/usr/bin/env python3
"""Edit a document subset through an installed copy of its font, and read the
saved result back with parsers the editor did not write.

uv run --with pypdf --with fonttools python scripts/text_installed_check.py <text-edit-probe> <new-dir>
swift scripts/text_edit_pdfkit.swift <new-dir> --installed
swift scripts/text_edit_pdfkit.swift <new-dir>/cff --installed

The document embeds a subset of `testdata/make_installed_fonts.py`'s original
TPDFInstalledSans holding only the characters of its own two lines, the way a
producer embeds the font it had installed. The replacement needs letters that
subset lacks, and the request supplies the whole fixture font the way the app
process supplies an installed file (`textedit::Installed`), so the worker's
preview/save pixel agreement and unchanged neighbours are checked by the probe.

FontTools then checks what was embedded against the "installed" font itself:
a subset rather than the file, every saved glyph's outline and advance, the
/W widths, the embedding rights and the PostScript name. Two copies the editor
must not use -- one whose width for a character of the document's subset is
three font units off, and one whose rights forbid subsetting -- are sent the
same way and must fall to Noto Sans. `--installed` on the PDFKit script reads
the first output's text and pixels independently.

The same edit is then made with the CFF-outline build of the font (cff.otf,
OpenType with a `CFF ` table) as the installed copy, into `<new-dir>/cff`.
What must come out is a CIDFontType0 descendant whose FontFile3 is a bare
CID-keyed CFF (/CIDFontType0C) with no CIDToGIDMap, each code the CID of its
glyph; fontTools' own CFF parser then compares every embedded charstring's
outline and width with the installed font's, the /W widths, and the `fsType`
the program carries as Distiller does, `/FSType N def /OrigFontType /OpenType
def`. A copy with `fsType` 8 must carry 8. Finally the saved file is edited
again through the probe with a character in neither subset, which reads the
embedded CID-keyed CFF back as the document's own font and asks the installed
copy for the rest.
"""
import argparse
import base64
import io
import json
import re
import shutil
import subprocess
from pathlib import Path

from fontTools import subset as subsetting
from fontTools.cffLib import CFFFontSet
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.sfnt import calcChecksum
from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject, FloatObject,
                           NameObject, NumberObject)

ROOT = Path(__file__).resolve().parents[1]
FONTS = ROOT / 'src-tauri/src/textedit/fonts/installed'
NAME = 'TPDFInstalledSans'
REPLACEMENT = 'Edited Ω FIRST'


def document_subset(full: bytes) -> bytes:
    """The document's own subset: its two lines' characters, glyph names kept."""
    font = TTFont(io.BytesIO(full), recalcTimestamp=False)
    options = subsetting.Options()
    options.name_IDs = ['*']
    options.notdef_outline = True
    options.recalc_timestamp = False
    subsetter = subsetting.Subsetter(options)
    subsetter.populate(text='SYNTHETIC FIRST SECOND')
    subsetter.subset(font)
    output = io.BytesIO()
    font.save(output)
    return output.getvalue()


def fixture(path: Path, program: bytes) -> None:
    font = TTFont(io.BytesIO(program))
    cmap, unit = font.getBestCmap(), 1000 / font['head'].unitsPerEm
    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=240)
    stream = DecodedStreamObject()
    stream.set_data(program)
    stream[NameObject('/Length1')] = NumberObject(len(program))
    descriptor = DictionaryObject({NameObject('/' + key): NumberObject(number) for key, number in
        dict(Flags=32, ItalicAngle=0, Ascent=879, Descent=-195, CapHeight=700, StemV=80).items()})
    descriptor.update({NameObject('/Type'): NameObject('/FontDescriptor'),
                       NameObject('/FontName'): NameObject(f'/ABCDEF+{NAME}'),
                       NameObject('/FontBBox'): ArrayObject([NumberObject(n) for n in [0, 0, 1000, 700]]),
                       NameObject('/FontFile2'): writer._add_object(stream)})
    widths = [FloatObject(font['hmtx'][cmap[code]][0] * unit) if code in cmap else NumberObject(0)
              for code in range(32, 90)]
    dictionary = DictionaryObject({NameObject('/' + key): NameObject('/' + name) for key, name in
        dict(Type='Font', Subtype='TrueType', BaseFont=f'ABCDEF+{NAME}', Encoding='WinAnsiEncoding').items()})
    dictionary.update({NameObject('/FirstChar'): NumberObject(32), NameObject('/LastChar'): NumberObject(89),
                       NameObject('/Widths'): ArrayObject(widths),
                       NameObject('/FontDescriptor'): writer._add_object(descriptor)})
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):
        DictionaryObject({NameObject('/F1'): writer._add_object(dictionary)})})
    content = DecodedStreamObject()
    content.set_data(b'BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\nBT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET')
    page[NameObject('/Contents')] = writer._add_object(content)
    writer.write(path)


def edit(probe: Path, source: Path, directory: Path, installed: bytes,
         contains: str = 'SYNTHETIC FIRST', replacement: str = REPLACEMENT) -> PdfReader:
    requests = directory.with_suffix('.json')
    requests.write_text(json.dumps([dict(page=0, contains=contains, replacement=replacement,
        layout=dict(width=250, height=20, size=12, wrap=False, font='auto', grow=False,
                    installed=dict(name=NAME, program=base64.b64encode(installed).decode('ascii'))))]),
        encoding='utf-8')
    subprocess.run([str(probe), '--roundtrip', str(source), str(requests), str(directory)], check=True, timeout=120)
    return PdfReader(directory / 'edited.pdf')


def added_font(pdf: PdfReader):
    fonts = {str(key): value.get_object() for key, value in pdf.pages[0]['/Resources']['/Font'].items()}
    added = [font for key, font in fonts.items() if key.startswith('/TPDFEdit')]
    assert len(added) == 1, 'expected exactly one added font'
    return added[0]


def patched(full: bytes, tag: bytes, offset: int, value: int) -> bytes:
    data = bytearray(full)
    count = int.from_bytes(data[4:6], 'big')
    for at in range(12, 12 + 16 * count, 16):
        if data[at:at + 4] == tag:
            start = int.from_bytes(data[at + 8:at + 12], 'big') + offset
            data[start:start + 2] = value.to_bytes(2, 'big')
            return bytes(data)
    raise AssertionError('table missing')


def run(probe: Path, directory: Path) -> None:
    directory.mkdir(parents=True, exist_ok=False)
    full = (FONTS / 'full.ttf').read_bytes()
    installed = TTFont(io.BytesIO(full))
    source = directory / 'synthetic-before.pdf'
    fixture(source, document_subset(full))

    pdf = edit(probe, source, directory / 'worker', full)
    shutil.copyfile(directory / 'worker' / 'edited.pdf', directory / 'synthetic-after.pdf')
    text = ' '.join(pdf.pages[0].extract_text().split())
    assert text == f'{REPLACEMENT} SYNTHETIC SECOND', text
    font = added_font(pdf)
    assert font['/Subtype'] == '/Type0' and font['/Encoding'] == '/Identity-H'
    assert re.fullmatch(rf'/[A-Z]{{6}}\+{NAME}', font['/BaseFont']), font['/BaseFont']
    child = font['/DescendantFonts'][0].get_object()
    program = child['/FontDescriptor']['/FontFile2'].get_data()
    assert calcChecksum(program) == 0xB1B0AFBA, 'invalid sfnt checksum adjustment'
    saved = TTFont(io.BytesIO(program), checkChecksums=2)
    assert len(program) < len(full) and len(saved.getGlyphOrder()) < len(installed.getGlyphOrder()), \
        'the whole installed font was embedded'
    assert saved['name'].getDebugName(6) == NAME
    assert saved['OS/2'].fsType == installed['OS/2'].fsType == 0
    pairs = re.findall(rb'<([0-9A-F]+)>\s*<([0-9A-F]+)>',
                       b' '.join(re.findall(rb'beginbfchar(.*?)endbfchar', font['/ToUnicode'].get_data(), re.S)))
    assert len(pairs) == len(set(REPLACEMENT)), 'one code per distinct character'
    declared = {}
    items = list(child['/W'])
    for at in range(0, len(items), 2):
        declared[int(items[at])] = float(items[at + 1][0])
    mapping = child['/CIDToGIDMap'].get_data()
    unit = 1000 / installed['head'].unitsPerEm
    for cid, target in pairs:
        code, char = int(cid, 16), bytes.fromhex(target.decode('ascii')).decode('utf-16-be')
        gid = int.from_bytes(mapping[code * 2:code * 2 + 2], 'big')
        assert gid != 0, f'.notdef for {char!r}'
        name, source_name = saved.getGlyphName(gid), installed.getBestCmap()[ord(char)]
        if name != '.notdef' and saved['glyf'][name].numberOfContours:
            assert saved['glyf'][name].getCoordinates(saved['glyf']) == \
                installed['glyf'][source_name].getCoordinates(installed['glyf']), f'outline of {char!r}'
        assert saved['hmtx'][name][0] == installed['hmtx'][source_name][0], f'advance of {char!r}'
        assert abs(declared[code] - installed['hmtx'][source_name][0] * unit) < 1e-3, f'/W of {char!r}'
    original = PdfReader(source).pages[0]['/Resources']['/Font']['/F1'].get_object()
    kept = pdf.pages[0]['/Resources']['/Font']['/F1'].get_object()
    assert kept['/FontDescriptor']['/FontFile2'].get_data() == original['/FontDescriptor']['/FontFile2'].get_data()
    print(f'[PASS] installed copy: {len(program)} of {len(full)} font bytes embedded; '
          f'Unicode, outlines, advances, /W, rights and name verified; document font unchanged')

    # 'S' is in the document's subset and not in the replacement: only the
    # width comparison can see it.
    gid = installed.getGlyphID(installed.getBestCmap()[ord('S')])
    wider = patched(full, b'hmtx', 4 * gid, installed['hmtx'][installed.getBestCmap()[ord('S')]][0] + 3)
    for label, copy in [('width three units off', wider), ('fsType no-subsetting', patched(full, b'OS/2', 8, 0x100))]:
        pdf = edit(probe, source, directory / label.split()[0], copy)
        base = added_font(pdf)['/BaseFont']
        assert base.endswith('+NotoSans') or base == '/NotoSans', f'{label}: {base}'
        assert ' '.join(pdf.pages[0].extract_text().split()) == f'{REPLACEMENT} SYNTHETIC SECOND'
        print(f'[PASS] {label}: refused, set in {base[1:]}')



def outline(glyph_set, name: str):
    pen = DecomposingRecordingPen(glyph_set)
    glyph_set[name].draw(pen)
    return pen.value


def check_cff(font, installed: TTFont, fs_type: int, replacement: str) -> int:
    """An added font against the installed CFF font; the embedded size."""
    assert font['/Subtype'] == '/Type0' and font['/Encoding'] == '/Identity-H'
    assert re.fullmatch(rf'/[A-Z]{{6}}\+{NAME}', font['/BaseFont']), font['/BaseFont']
    child = font['/DescendantFonts'][0].get_object()
    assert child['/Subtype'] == '/CIDFontType0' and '/CIDToGIDMap' not in child
    info = child['/CIDSystemInfo']
    assert (info['/Registry'], info['/Ordering'], info['/Supplement']) == ('Adobe', 'Identity', 0)
    stream = child['/FontDescriptor']['/FontFile3'].get_object()
    assert stream['/Subtype'] == '/CIDFontType0C', stream['/Subtype']
    assert '/FontFile2' not in child['/FontDescriptor']
    data = stream.get_data()
    embedded = CFFFontSet()
    embedded.decompile(io.BytesIO(data), None)
    top = embedded[embedded.fontNames[0]]
    assert top.ROS == ('Adobe', 'Identity', 0), top.ROS
    assert top.PostScript == f'/FSType {fs_type} def /OrigFontType /OpenType def', top.PostScript
    source_cff = installed['CFF '].cff
    assert len(top.CharStrings) < len(source_cff[source_cff.fontNames[0]].CharStrings), \
        'the whole installed font was embedded'
    pairs = re.findall(rb'<([0-9A-F]+)>\s*<([0-9A-F]+)>',
                       b' '.join(re.findall(rb'beginbfchar(.*?)endbfchar', font['/ToUnicode'].get_data(), re.S)))
    assert len(pairs) == len(set(replacement)), 'one code per distinct character'
    declared = {}
    items = list(child['/W'])
    for at in range(0, len(items), 2):
        declared[int(items[at])] = float(items[at + 1][0])
    assert set(declared) == {int(cid, 16) for cid, _ in pairs}
    charset = top.charset
    embedded_glyphs, installed_glyphs = top.CharStrings, installed.getGlyphSet()
    unit = 1000 / installed['head'].unitsPerEm
    for cid, target in pairs:
        code, char = int(cid, 16), bytes.fromhex(target.decode('ascii')).decode('utf-16-be')
        # Identity-H: the code is the CID, and the charset names that CID's glyph.
        name = f'cid{code:05d}'
        assert code != 0 and name in charset, f'no glyph for CID {code} ({char!r})'
        source_name = installed.getBestCmap()[ord(char)]
        charstring = embedded_glyphs[name]
        charstring.decompile()
        assert outline(embedded_glyphs, name) == outline(installed_glyphs, source_name), f'outline of {char!r}'
        charstring.draw(DecomposingRecordingPen(embedded_glyphs))
        assert charstring.width == installed['hmtx'][source_name][0], f'charstring width of {char!r}'
        assert abs(declared[code] - installed['hmtx'][source_name][0] * unit) < 1e-3, f'/W of {char!r}'
    return len(data)


def run_cff(probe: Path, directory: Path) -> None:
    directory.mkdir()
    full = (FONTS / 'full.ttf').read_bytes()
    otf = (FONTS / 'cff.otf').read_bytes()
    installed = TTFont(io.BytesIO(otf))
    assert 'CFF ' in installed and 'glyf' not in installed
    source = directory / 'synthetic-before.pdf'
    fixture(source, document_subset(full))
    pdf = edit(probe, source, directory / 'worker', otf)
    shutil.copyfile(directory / 'worker' / 'edited.pdf', directory / 'synthetic-after.pdf')
    assert ' '.join(pdf.pages[0].extract_text().split()) == f'{REPLACEMENT} SYNTHETIC SECOND'
    size = check_cff(added_font(pdf), installed, installed['OS/2'].fsType, REPLACEMENT)
    assert installed['OS/2'].fsType == 0
    original = PdfReader(source).pages[0]['/Resources']['/Font']['/F1'].get_object()
    kept = pdf.pages[0]['/Resources']['/Font']['/F1'].get_object()
    assert kept['/FontDescriptor']['/FontFile2'].get_data() == original['/FontDescriptor']['/FontFile2'].get_data()
    print(f'[PASS] installed CFF copy: {size} of {len(otf)} font bytes embedded as CIDFontType0C; '
          f'Unicode, outlines, charstring widths, /W, fsType and name verified; document font unchanged')

    editable = patched(otf, b'OS/2', 8, 8)
    pdf = edit(probe, source, directory / 'editable', editable)
    check_cff(added_font(pdf), TTFont(io.BytesIO(editable)), 8, REPLACEMENT)
    print('[PASS] installed CFF copy with fsType 8: the program carries /FSType 8')

    # The saved file edited again: the run is now in the embedded CID-keyed
    # CFF, and `z` is in neither subset.
    again = f'{REPLACEMENT}z'
    pdf = edit(probe, directory / 'synthetic-after.pdf', directory / 'again', otf,
               contains=REPLACEMENT, replacement=again)
    assert ' '.join(pdf.pages[0].extract_text().split()) == f'{again} SYNTHETIC SECOND'
    fonts = [value.get_object() for key, value in pdf.pages[0]['/Resources']['/Font'].items()
             if str(key).startswith('/TPDFEdit')]
    newest = [font for font in fonts if b'<007A>' in font['/ToUnicode'].get_data()]
    assert len(newest) == 1, f'{len(fonts)} added fonts, {len(newest)} with z'
    check_cff(newest[0], installed, 0, again)
    print(f'[PASS] the saved file edited again ({len(fonts)} added fonts on the page): its CID-keyed '
          f'CFF read back, the new character set from the installed copy')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    run(args.probe.resolve(), args.directory)
    run_cff(args.probe.resolve(), args.directory / 'cff')
