#!/usr/bin/env python3
"""Rebuild pinned static CJK fallback fonts from the upstream variable TTF.

uv run --with fonttools==4.65.0 python scripts/build_cjk_fonts.py <source.ttf>
Download source using the URL in vendor/fonts/manifest.json's cjk entry.
This is a maintainer task, never part of application startup or PDF editing.
"""
import hashlib
import argparse
import io
import json
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools import subset

ROOT = Path(__file__).resolve().parents[1] / 'vendor/fonts'
SHA = '990c807e79c25662a5a9ecf7f971baeb2bf2eab9a559e5ecf15cdfdb8561d21f'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--check', action='store_true', help='rebuild in memory and verify pinned outputs')
args = parser.parse_args()
source = args.source
assert hashlib.sha256(source.read_bytes()).hexdigest() == SHA, 'source digest mismatch'
manifest = json.loads((ROOT / 'manifest.json').read_text(encoding='utf-8'))
for weight, style in [(400, 'Regular'), (700, 'Bold')]:
    font = TTFont(source, recalcTimestamp=False)
    assert font['OS/2'].fsType == 0, 'bundled source must permit editable embedding and subsetting'
    instantiateVariableFont(font, {'wght': weight}, inplace=True, updateFontNames=True)
    # Keep every Unicode character, but not shaping/vertical/locale alternates.
    # The editor uses horizontal scalar glyphs. This also avoids ttf-parser's
    # u16 loca-count overflow at the upstream font's 65,535 glyphs.
    coverage = set(font.getBestCmap())
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ['*']
    options.name_legacy = True
    options.name_languages = ['*']
    options.notdef_outline = True
    options.glyph_names = True
    options.drop_tables += ['BASE', 'GDEF', 'GPOS', 'GSUB', 'vhea', 'vmtx', 'VORG', 'STAT']
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=coverage)
    subsetter.subset(font)
    assert set(font.getBestCmap()) == coverage, 'Unicode coverage changed'
    assert len(font.getGlyphOrder()) < 65535, 'font exceeds parser glyph limit'
    font.recalcTimestamp = False
    target = ROOT / f'NotoSansCJKsc-{style}.ttf'
    output = io.BytesIO()
    font.save(output)
    data = output.getvalue()
    if args.check:
        assert target.read_bytes() == data, f'{target.name} differs from reproducible build'
    else:
        target.write_bytes(data)
    entry = dict(file=target.name, sha256=hashlib.sha256(data).hexdigest())
    manifest['files'] = [f for f in manifest['files'] if f['file'] != target.name] + [entry]
    print('[OK]', target.name, len(data), 'bytes', '(verified)' if args.check else '(written)')
manifest['cjk'] = dict(version='NotoSansCJK-2.004', source_sha256=SHA,
    source='https://raw.githubusercontent.com/notofonts/noto-cjk/523d033d6cb47f4a80c58a35753646f5c3608a78/Sans/Variable/TTF/NotoSansCJKsc-VF.ttf',
    build='scripts/build_cjk_fonts.py; fonttools 4.65.0; wght=400/700; all Unicode characters; no shaping/vertical alternates; original timestamps',
    license_file='OFL-CJK.txt')
if args.check:
    assert json.loads((ROOT / 'manifest.json').read_text(encoding='utf-8')) == manifest
else:
    with (ROOT / 'manifest.json').open('w', encoding='utf-8', newline='\n') as out:
        out.write(json.dumps(manifest, indent=2) + '\n')
