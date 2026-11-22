"""Client contract and real executable tests; run by scripts/gates.py.

No PDF library or live certificate is required. The input is synthetic, and the
Rust CLI tests independently inspect the output object graph with lopdf.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from tpdf_client import CommandError, CommandTimeout, ProtocolError, Result, Tpdf

ROOT = Path(__file__).resolve().parents[2]
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'src-tauri' / 'target'))
BINARY = Path(os.environ.get('TPDF_TEST_CLI', TARGET / 'debug' / ('tpdf-cli.exe' if os.name == 'nt' else 'tpdf-cli')))


def fixture(path: Path, *, form: bool = False, content: bytes | None = None,
            retained_secret: bool = False, private_form: bool = False) -> None:
    """A one-page PDF with visible, editable text; explicit xref offsets."""
    if content is None:
        content = b'BT /F1 12 Tf 30 100 Td (SYNTHETIC ORIGINAL) Tj ET'
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
        b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'\nendstream',
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    ]
    if form:
        objects[0] = b'<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>'
        objects[2] = objects[2][:-2] + b' /Annots [7 0 R] >>'
        objects += [
            b'<< /Fields [7 0 R] /DA (/F1 12 Tf 0 g) /DR << /Font << /F1 5 0 R >> >> >>',
            b'<< /Type /Annot /Subtype /Widget /FT /Tx /T (Synthetic.answer) /V (OLD) /Rect [30 200 200 225] /P 3 0 R /F 4 >>',
        ]
    if private_form:
        assert form
        objects[2] = objects[2].replace(b'/Annots [7 0 R]', b'/Annots [7 0 R 9 0 R 10 0 R]')
        objects[5] = objects[5].replace(b'/Fields [7 0 R]', b'/Fields [7 0 R 8 0 R]')
        objects += [
            b'<< /FT /Tx /T (Synthetic.private) /V (OLD) /Kids [9 0 R 10 0 R] >>',
            b'<< /Type /Annot /Subtype /Widget /Parent 8 0 R /Rect [30 300 200 325] /P 3 0 R /F 4 >>',
            b'<< /Type /Annot /Subtype /Widget /Parent 8 0 R /Rect [30 250 200 275] /P 3 0 R /F 4 >>',
        ]
    if retained_secret:
        # A comment outside the redaction is kept, and still holds the secret.
        reference = f'{len(objects) + 1} 0 R'.encode()
        if form:
            objects[2] = objects[2].replace(b'] >>', b' ' + reference + b'] >>')
        else:
            objects[2] = objects[2][:-2] + b' /Annots [' + reference + b'] >>'
        objects.append(b'<< /Type /Annot /Subtype /Text /Rect [250 340 270 360] /Contents (PRIVATE-731) /F 2 >>')
    write_pdf(path, objects)


def write_pdf(path: Path, objects: list[bytes]) -> None:
    """Number `objects` from 1, the first being the catalog, with an exact xref."""
    data = bytearray(b'%PDF-1.7\n')
    offsets = [0]
    for number, body in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f'{number} 0 obj\n'.encode() + body + b'\nendobj\n')
    xref = len(data)
    data.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        data.extend(f'{offset:010d} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    path.write_bytes(data)


def powerpoint_fixture(path: Path) -> None:
    """A page tagged the way PowerPoint for Microsoft 365 exports a slide.

    A Textbox that the RoleMap makes a Sect holds a P, whose Span carries
    ActualText equal to the words it paints. The editor rewrites that text with
    the words (BUILD.md, *PowerPoint factsheet*); before 26.9.24 the Span was
    read-only and this page offered nothing.
    """
    content = b'/Span << /MCID 0 >> BDC BT /F1 12 Tf 30 100 Td (SYNTHETIC ORIGINAL) Tj ET EMC'
    write_pdf(path, [
        b'<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R /MarkInfo << /Marked true >> >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents 4 0 R'
        b' /Resources << /Font << /F1 5 0 R >> >> /StructParents 0 >>',
        b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'\nendstream',
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
        b'<< /Type /StructTreeRoot /K 7 0 R /ParentTree 10 0 R /ParentTreeNextKey 1'
        b' /RoleMap << /Textbox /Sect >> >>',
        b'<< /Type /StructElem /S /Document /P 6 0 R /K [8 0 R] >>',
        b'<< /Type /StructElem /S /Textbox /P 7 0 R /K [9 0 R] >>',
        b'<< /Type /StructElem /S /P /P 8 0 R /Pg 3 0 R /K [11 0 R] >>',
        b'<< /Nums [0 [11 0 R]] >>',
        b'<< /Type /StructElem /S /Span /P 9 0 R /Pg 3 0 R /K 0 /ActualText (SYNTHETIC ORIGINAL) >>',
    ])


def check_document_workflow(pdf: Tpdf, directory: Path, *, identity: str | None = None,
                            retained_secret: bool = False, dry_run: bool = False) -> list[str]:
    """Real fill/redact/readback pipeline, also reused by the opt-in OS-key check.

    Select only the first of two widgets sharing a confidential value. Signing
    must be downstream of a written AND verified redaction. A dry run or an
    unverified written copy is not an approved document.
    """
    checks = []

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise RuntimeError(message)
        checks.append(message)

    source, filled, clean, signed = [directory / name for name in
                                    ('form.pdf', 'filled.pdf', 'redacted.pdf', 'signed.pdf')]
    fixture(source, form=True, private_form=True, retained_secret=retained_secret,
            content=(b'BT /F1 14 Tf 30 100 Td (PUBLIC CONTROL) Tj ET\n'
                     b'BT /F1 20 Tf 30 310 Td (PRIVATE-731) Tj ET'))
    original = source.read_bytes()
    pdf.fill(source, filled, {'Synthetic.answer': 'PUBLIC ANSWER', 'Synthetic.private': 'PRIVATE-731'})
    filled_bytes = filled.read_bytes()
    fields = pdf.fields(filled)['fields']
    require(next(field for field in fields if field['name'] == 'Synthetic.private')['widgets'] == 2,
            'workflow: confidential field has two widgets before redaction')
    values = {field['name']: field['value'] for field in fields}
    require(values == {'Synthetic.answer': 'PUBLIC ANSWER', 'Synthetic.private': 'PRIVATE-731'},
            'workflow: both form values were filled')
    before_png = directory / 'filled.png'
    pdf.render(filled, before_png, dpi=72)
    report = pdf.redact(filled, clean, regions=[{'page': 1, 'rect': [25, 65, 180, 40]}],
                        dry_run=dry_run, check=False)
    (directory / 'redaction-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    if report['written'] is not True or report['verified'] is not True:
        raise RuntimeError('workflow: redaction must be written and verified before signing')
    require(not report['reasons'], 'workflow: redaction has no verification failures')

    def readback(path: Path) -> None:
        values = {field['name']: field['value'] for field in pdf.fields(path)['fields']}
        require(values.get('Synthetic.answer') == 'PUBLIC ANSWER'
                and 'PRIVATE-731' not in values.values(), f'workflow: {path.name} preserves only public form values')
        text = pdf.text(path)['pages'][0]['text']
        require('PUBLIC CONTROL' in text and 'PRIVATE-731' not in text,
                f'workflow: {path.name} preserves public text and removes confidential text')
        require(b'PRIVATE-731' not in path.read_bytes(), f'workflow: {path.name} has no literal secret bytes')
        comments = pdf.comments(path)
        require(comments['complete'] and all('PRIVATE-731' not in json.dumps(item)
                                             for item in comments['comments']),
                f'workflow: {path.name} has no confidential comment content')

    readback(clean)
    clean_png = directory / 'redacted.png'
    rendered = pdf.render(clean, clean_png, dpi=72)
    require((rendered['width_px'], rendered['height_px']) == (300, 400)
            and clean_png.read_bytes() != before_png.read_bytes(), 'workflow: redaction changes the rendered page')
    if identity is not None:
        result = pdf.sign(clean, signed, identity=identity)
        require(result['identity']['id'] == identity, 'workflow: signing uses the selected certificate')
        signatures = pdf.verify(signed)['files'][0]['signatures']
        require(len(signatures) == 1 and signatures[0]['integrity']['verdict'] == 'intact'
                and signatures[0]['covers_whole_file'], 'workflow: signature is intact and covers the file')
        require(signed.read_bytes().startswith(clean.read_bytes()), 'workflow: signing preserves the redacted revision')
        readback(signed)
        signed_png = directory / 'signed.png'
        pdf.render(signed, signed_png, dpi=72)
        require(signed_png.read_bytes() == clean_png.read_bytes(), 'workflow: invisible signing preserves redacted rendering')
    require(source.read_bytes() == original and filled.read_bytes() == filled_bytes,
            'workflow: source and filled input remain unchanged')
    return checks


class ClientTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='tpdf-api-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'synthetic input.pdf'
        fixture(self.source)
        self.pdf = Tpdf(BINARY, timeout=30)

    def test_mixed_inline_content_keeps_declared_order(self):
        words = ['ALPHA', 'BRAVO', 'CHARLIE']
        content = ''.join(f'/Span <</MCID {i}>> BDC BT /F1 14 Tf {25 + 85*i} 60 Td '
                          f'({word}) Tj ET EMC\n' for i, word in enumerate(words)).encode()
        write_pdf(self.source, [
            b'<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 6 0 R >>',
            b'<< /Type /Pages /Count 1 /Kids [3 0 R] >>',
            b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 360 120] /Resources << /Font << /F1 5 0 R >> >> '
            b'/Contents 4 0 R /StructParents 0 >>',
            f'<< /Length {len(content)} >>\nstream\n'.encode() + content + b'endstream',
            b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
            b'<< /Type /StructTreeRoot /K [7 0 R] /ParentTree 9 0 R /ParentTreeNextKey 1 >>',
            b'<< /Type /StructElem /S /P /P 6 0 R /Pg 3 0 R /K [0 8 0 R 2] >>',
            b'<< /Type /StructElem /S /Span /P 7 0 R /Pg 3 0 R /K 1 >>',
            b'<< /Nums [0 [7 0 R 8 0 R 7 0 R]] >>',
        ])
        page, = self.pdf.text(self.source)['pages']
        self.assertEqual(page['order'], 'tagged')
        self.assertEqual(page['text'].split(), words)

    def test_fill_redact_workflow(self):
        checks = check_document_workflow(self.pdf, self.root)
        self.assertGreaterEqual(len(checks), 8)
        self.assertFalse((self.root / 'signed.pdf').exists())

    def test_workflow_stops_before_signing_on_unverified_redaction_or_dry_run(self):
        for name, options in [('retained', {'retained_secret': True}), ('dry-run', {'dry_run': True})]:
            directory = self.root / name
            directory.mkdir()
            with self.subTest(case=name), patch.object(self.pdf, 'sign') as sign:
                with self.assertRaisesRegex(RuntimeError, 'redaction must be written and verified'):
                    check_document_workflow(self.pdf, directory, identity='SYNTHETIC NEVER USED', **options)
                sign.assert_not_called()
                self.assertFalse((directory / 'signed.pdf').exists())
            report = json.loads((directory / 'redaction-report.json').read_text(encoding='utf-8'))
            if name == 'retained':
                self.assertIs(report['verified'], False)
                self.assertIs(report['written'], True)
                self.assertTrue(any('is still in the file' in reason for reason in report['reasons']))
                self.assertTrue((directory / 'redacted.pdf').exists())
                self.assertIn('PRIVATE-731', [item['body'] for item in self.pdf.comments(directory / 'redacted.pdf')['comments']])
            else:
                self.assertIs(report['written'], False)
                self.assertIsNone(report['verified'])
                self.assertFalse((directory / 'redacted.pdf').exists())

    def test_external_workflow_and_discovery(self):
        commands = {c['name'] for c in self.pdf.help()['commands']}
        self.assertTrue({'edit', 'comments', 'text-runs', 'redact', 'fill', 'sign', 'search', 'ocr', 'protect', 'unprotect', 'images', 'compress', 'form', 'hidden', 'long-term'} <= commands)
        self.assertEqual([c['name'] for c in self.pdf.help('search')['commands']], ['search'])
        output = self.root / 'changed.pdf'
        self.pdf.edit(self.source, output, [
            {'op': 'insert_blank', 'after': 1, 'width': 200, 'height': 250},
            {'op': 'annotate', 'page': 2, 'kind': 'note', 'rect': [10, 20, 20, 20], 'text': 'Synthetic API note'},
            {'op': 'move_page', 'page': 2, 'to': 1},
        ])
        self.assertEqual(self.pdf.info(output)['files'][0]['document']['pages'], 2)
        comments = self.pdf.comments(output)['comments']
        self.assertEqual([(c['page'], c['body']) for c in comments], [(1, 'Synthetic API note')])
        self.assertIn('SYNTHETIC ORIGINAL', self.pdf.text(output)['pages'][1]['text'])
        extracted = self.root / 'extracted.pdf'
        self.pdf.extract(output, extracted, pages='2')
        self.assertEqual(self.pdf.info(extracted)['files'][0]['document']['pages'], 1)
        self.assertEqual(self.pdf.comments(extracted)['comments'], [])

    def test_search_finds_what_redact_would_remove_and_says_when_nothing_matches(self):
        merged = self.root / 'merged.pdf'
        self.pdf.edit(self.source, merged, [{'op': 'insert_blank', 'after': 1, 'width': 200, 'height': 250}])
        found = self.pdf.search(merged, self.source, texts=['synthetic'], patterns=['ORIG[A-Z]+'])
        self.assertEqual([q['kind'] for q in found['queries']], ['text', 'pattern'])
        first, second = found['files']
        self.assertEqual([(m['page'], m['query'], m['hit']) for m in first['matches']],
                         [(1, 0, 'SYNTHETIC'), (1, 1, 'ORIGINAL')])
        # The blank page has nothing to search, and the report says so.
        self.assertEqual((first['pages_searched'], first['pages_without_text']), (2, [2]))
        self.assertEqual(len(second['matches']), 2)
        # Folded unless asked otherwise, and a whole word is a whole word.
        self.assertEqual(self.pdf.search(self.source, texts=['synthetic'], case_sensitive=True)['files'][0]['matches'], [])
        self.assertEqual(self.pdf.search(self.source, texts=['SYNTH'], whole_word=True)['files'][0]['matches'], [])
        self.assertEqual(len(self.pdf.search(self.source, texts=['SYNTH'])['files'][0]['matches']), 1)
        # The same line removes what it found.
        dry = self.pdf.redact(self.source, texts=['synthetic'], dry_run=True)
        self.assertEqual(dry['searches'][0]['matches'], 1)
        with self.assertRaises(CommandError) as refused:
            self.pdf.search(self.root / 'missing.pdf', self.source, texts=['synthetic'])
        self.assertEqual(refused.exception.exit_code, 3)
        self.assertEqual(len(refused.exception.report['files'][1]['matches']), 1)
        for bad in [{}, {'texts': 'one string'}]:
            with self.assertRaises((ValueError, TypeError)):
                self.pdf.search(self.source, **bad)

    def test_mark_matches_puts_a_mark_on_every_match_and_writes_nothing_for_none(self):
        hit = self.pdf.search(self.source, texts=['ORIGINAL'])['files'][0]['matches'][0]
        self.assertEqual([area['page'] for area in hit['rects']], [1])
        x, y, width, height = hit['rects'][0]['rect']
        self.assertTrue(width > 20 and 5 < height < 20, hit['rects'])
        marked = self.root / 'marked.pdf'
        report = self.pdf.mark_matches(self.source, marked, texts=['synthetic', 'original'],
                                       kind='underline', color=[0, 0.5, 1])
        self.assertEqual((report['written'], report['operations']), (True, 2))
        comments = self.pdf.comments(marked)['comments']
        self.assertEqual([(c['page'], c['kind']) for c in comments], [(1, 'underline')] * 2)
        # The mark is where the match is: [left, top, right, bottom] against [x, y, w, h].
        placed = sorted(comments, key=lambda c: c['rect'][0])[-1]['rect']
        for got, want in zip(placed, [x, y, x + width, y + height]):
            self.assertAlmostEqual(got, want, delta=1.5)
        self.assertEqual(self.pdf.comments(marked)['comments'][0]['color'], [0, 0.5, 1])
        # Nothing matched: nothing is written, and the answer says so.
        nothing = self.root / 'nothing.pdf'
        self.assertIsNone(self.pdf.mark_matches(self.source, nothing, texts=['zebra']))
        self.assertFalse(nothing.exists())
        for bad in [{'kind': 'box'}, {'color': [2, 0, 0]}, {'color': [1, 0]}]:
            with self.assertRaises(ValueError):
                self.pdf.mark_matches(self.source, nothing, texts=['synthetic'], **bad)
        self.assertFalse(nothing.exists())

    def test_mark_matches_explains_an_older_cli_without_retrying(self):
        error = CommandError(Result({}, 2, "invalid edit plan: unknown field `source_sha256`"))
        output = self.root / 'marked.pdf'
        output.write_bytes(b'previous')
        with patch.object(self.pdf, 'edit', side_effect=error) as edit:
            with self.assertRaisesRegex(CommandError, 'same checkout') as refused:
                self.pdf.mark_matches(self.source, output, texts=['SYNTHETIC'])
        edit.assert_called_once()
        self.assertIs(refused.exception.result, error.result)
        self.assertEqual(output.read_bytes(), b'previous')

    def test_mark_matches_refuses_partial_geometry(self):
        output = self.root / 'marked.pdf'
        output.write_bytes(b'previous output')
        report = self.pdf.search(self.source, texts=['ORIGINAL'])
        report['files'][0]['matches'][0]['partial_geometry'] = True
        with patch.object(self.pdf, 'search', return_value=report):
            with self.assertRaisesRegex(ValueError, 'only in part'):
                self.pdf.mark_matches(self.source, output, texts=['ORIGINAL'], force=True)
        self.assertEqual(output.read_bytes(), b'previous output')

    def test_cli_highlight_plan_binds_the_bytes_searched(self):
        import hashlib
        data = self.source.read_bytes()
        plan = subprocess.check_output([BINARY, 'search', '-', '--text', 'ORIGINAL',
                                        '--annotate', 'highlight'], input=data)
        named = subprocess.check_output([BINARY, 'search', self.source, '--text', 'ORIGINAL',
                                         '--annotate', 'highlight'])
        self.assertEqual(json.loads(plan), json.loads(named))
        self.assertEqual(json.loads(plan)['source_sha256'], hashlib.sha256(data).hexdigest())
        output = self.root / 'highlighted.pdf'
        edit = [BINARY, 'edit', self.source, '--plan', '-', '-o', output, '--force']
        subprocess.run(edit, input=plan, check=True, capture_output=True)
        self.assertEqual(self.pdf.comments(output)['comments'][0]['kind'], 'highlight')
        previous = output.read_bytes()
        fixture(self.source, content=b'BT /F1 12 Tf 90 260 Td (SYNTHETIC ORIGINAL) Tj ET')
        refused = subprocess.run(edit, input=plan, capture_output=True)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn(b'searched revision', refused.stderr)
        self.assertEqual(output.read_bytes(), previous)

    def test_mark_matches_refuses_a_different_revision_at_the_same_path(self):
        client = Tpdf(BINARY, cwd=self.root, timeout=30)
        search = client.search
        output = self.root / 'marked.pdf'
        output.write_bytes(b'prior output')

        def replace_after_search(*args, **kwargs):
            result = search(*args, **kwargs)
            before = self.source.stat()
            fixture(self.source, content=b'BT /F1 12 Tf 90 260 Td (SYNTHETIC ORIGINAL) Tj ET')
            os.utime(self.source, ns=(before.st_atime_ns, before.st_mtime_ns))
            self.assertEqual(self.source.stat().st_size, before.st_size)
            return result

        with patch.object(client, 'search', side_effect=replace_after_search):
            with self.assertRaisesRegex(CommandError, 'searched revision'):
                client.mark_matches(self.source.name, output.name, texts=['ORIGINAL'], force=True)
        self.assertEqual(output.read_bytes(), b'prior output')

    def test_render_and_crop_use_physical_points(self):
        import struct
        for unit in (1, 2):
            width, height = 400 // unit, 600 // unit
            write_pdf(self.source, [
                b'<< /Type /Catalog /Pages 2 0 R >>',
                b'<< /Type /Pages /Count 1 /Kids [3 0 R] >>',
                f'<< /Type /Page /Parent 2 0 R /UserUnit {unit} /Resources << >> '
                f'/MediaBox [0 0 {width} {height}] >>'.encode(),
            ])
            png = self.root / 'page.png'
            self.pdf.render(self.source, png, dpi=72, force=True)
            self.assertEqual(struct.unpack('>II', png.read_bytes()[16:24]), (400, 600))
            cropped = self.root / 'crop.pdf'
            self.pdf.crop(self.source, cropped, rect=[20, 30, 80, 90], force=True)
            self.pdf.render(cropped, png, dpi=72, force=True)
            self.assertEqual(struct.unpack('>II', png.read_bytes()[16:24]), (80, 90))

    def test_info_groups_page_sizes_in_physical_points(self):
        write_pdf(self.source, [
            b'<< /Type /Catalog /Pages 2 0 R >>',
            b'<< /Type /Pages /Count 3 /Kids [3 0 R 4 0 R 5 0 R] >>',
            b'<< /Type /Page /Parent 2 0 R /Resources << >> /MediaBox [0 0 400 600] >>',
            b'<< /Type /Page /Parent 2 0 R /Resources << >> /MediaBox [0 0 200 300] /UserUnit 2 >>',
            b'<< /Type /Page /Parent 2 0 R /Resources << >> /MediaBox [0 0 200 300] >>',
        ])
        sizes = self.pdf.info(self.source)['files'][0]['document']['page_sizes']
        self.assertEqual(sizes, [
            {'width_pt': 400, 'height_pt': 600, 'count': 2},
            {'width_pt': 200, 'height_pt': 300, 'count': 1},
        ])

    def test_page_helpers_merge_extract_and_split_read_back(self):
        second = self.root / 'second.pdf'
        self.pdf.edit(self.source, second, [
            {'op': 'insert_blank', 'after': 1, 'width': 200, 'height': 250},
            {'op': 'annotate', 'page': 2, 'kind': 'note', 'rect': [10, 20, 20, 20], 'text': 'SECOND'},
        ])
        merged = self.root / 'merged.pdf'
        self.assertTrue(self.pdf.merge([second, self.source], merged)['complete'])
        self.assertEqual(self.pdf.info(merged)['files'][0]['document']['pages'], 3)
        self.assertEqual([p['text'].strip() for p in self.pdf.text(merged)['pages']],
                         ['SYNTHETIC ORIGINAL', '', 'SYNTHETIC ORIGINAL'])
        extracted = self.root / 'extracted.pdf'
        self.pdf.extract(merged, extracted, pages='3,2,2')
        self.assertEqual([p['text'].strip() for p in self.pdf.text(extracted)['pages']],
                         ['', 'SYNTHETIC ORIGINAL'])
        self.assertEqual([(c['page'], c['body']) for c in self.pdf.comments(extracted)['comments']], [(1, 'SECOND')])
        split = self.pdf.split(merged, self.root / 'part.pdf', every=2)
        self.assertTrue(split['complete'])
        self.assertEqual([p['pages'] for p in split['outputs']], [2, 1])
        parts = [Path(p['path']) for p in split['outputs']]
        self.assertEqual([p.name for p in parts], ['part-1.pdf', 'part-2.pdf'])
        self.assertEqual([self.pdf.info(p)['files'][0]['document']['pages'] for p in parts], [2, 1])
        self.assertEqual(self.pdf.comments(parts[0])['comments'][0]['body'], 'SECOND')
        self.assertEqual(self.pdf.comments(parts[1])['comments'], [])
        self.assertEqual(self.pdf.text(parts[1])['pages'][0]['text'].strip(), 'SYNTHETIC ORIGINAL')

    def test_page_helpers_apply_only_selected_pages(self):
        merged = self.root / 'merged.pdf'
        self.pdf.merge([self.source, self.source], merged)
        rotated = self.root / 'rotated.pdf'
        self.pdf.rotate(merged, rotated, degrees=90, pages='2')
        sizes = self.pdf.info(rotated)['files'][0]['document']['page_sizes']
        self.assertEqual([(p['width_pt'], p['height_pt'], p['count']) for p in sizes],
                         [(300, 400, 1), (400, 300, 1)])
        reversed_ = self.root / 'reversed.pdf'
        report = self.pdf.reverse(rotated, reversed_)
        self.assertEqual((report['command'], report['outputs'][0]['pages']), ('reverse', 2))
        sizes = self.pdf.info(reversed_)['files'][0]['document']['page_sizes']
        self.assertEqual([(p['width_pt'], p['height_pt'], p['count']) for p in sizes],
                         [(400, 300, 1), (300, 400, 1)])
        cropped = self.root / 'cropped.pdf'
        self.pdf.crop(rotated, cropped, rect=[10, 20, 100, 150], pages='1')
        sizes = self.pdf.info(cropped)['files'][0]['document']['page_sizes']
        self.assertEqual([(p['width_pt'], p['height_pt'], p['count']) for p in sizes],
                         [(100, 150, 1), (400, 300, 1)])
        for page, expected in [(1, (100, 150)), (2, (400, 300))]:
            image = self.pdf.render(cropped, self.root / f'page-{page}.png', page=page, dpi=72)
            self.assertEqual((image['width_px'], image['height_px']), expected)

    def test_page_helper_refusals_preserve_existing_outputs(self):
        output = self.root / 'protected.pdf'
        output.write_bytes(b'EXISTING OUTPUT')
        with self.assertRaises(CommandError) as caught:
            self.pdf.extract(self.source, output, pages='2', force=True)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        with self.assertRaises(CommandError):
            self.pdf.rotate(self.source, output, degrees=90)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        self.pdf.rotate(self.source, output, degrees=90, force=True)
        self.assertEqual(self.pdf.info(output)['files'][0]['document']['page_sizes'][0]['width_pt'], 400)
        for sources in [str(self.source), self.source]:
            with self.assertRaises(TypeError):
                self.pdf.merge(sources, output)
        with self.assertRaises(ValueError):
            self.pdf.crop(self.source, output, rect=[0, 10, 20])

    def test_option_like_filenames_are_literal_for_every_helper(self):
        client = Tpdf(BINARY, cwd=self.root)
        # An exact option name proves that '--' disables even recognized options.
        source = self.root / '--json'
        fixture(source, form=True)
        self.assertEqual(client.info('--json')['files'][0]['path'], '--json')
        self.assertIn('SYNTHETIC ORIGINAL', client.text('--json')['pages'][0]['text'])
        field = client.fields('--json')['fields'][0]['name']
        client.fill('--json', '-filled.pdf', {field: 'LITERAL'})
        self.assertEqual(client.fields('-filled.pdf')['fields'][0]['value'], 'LITERAL')
        self.assertEqual(client.comments('--json')['comments'], [])
        self.assertEqual(client.text_runs('--json')['runs'][0]['text'], 'SYNTHETIC ORIGINAL')
        client.render('--json', '-page.png', dpi=72)
        client.edit('-filled.pdf', '-edited.pdf', [{'op': 'rotate', 'page': 1, 'degrees': 90}])
        client.merge(['--json', '-edited.pdf'], '-merged.pdf')
        client.extract('-merged.pdf', '-extracted.pdf', pages='2')
        client.rotate('-extracted.pdf', '-rotated.pdf', degrees=180)
        client.crop('-rotated.pdf', '-cropped.pdf', rect=[0, 0, 100, 100])
        self.assertTrue(client.split('-merged.pdf', '-part.pdf')['complete'])
        # Every page has text, so there is nothing to recognise: the refusal
        # shows the arguments arrived, and that nothing was written.
        with self.assertRaises(CommandError) as caught:
            client.ocr('--json', '-searchable.pdf', pages='1', languages=['en-US'])
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertIn('already has text', str(caught.exception))
        self.assertFalse((self.root / '-searchable.pdf').exists())
        self.assertEqual(client.verify('--json')['files'][0]['path'], '--json')
        with self.assertRaises(CommandError) as caught:
            client.verify('--json', strict=True)
        self.assertEqual(caught.exception.exit_code, 1)
        self.assertEqual(caught.exception.report['files'][0]['path'], '--json')

    def test_unchecked_redaction_still_raises_on_missing_input(self):
        with self.assertRaises(CommandError) as raised:
            self.pdf.redact(self.root / 'missing.pdf', self.root / 'out.pdf',
                            texts=['synthetic'], check=False)
        self.assertEqual(raised.exception.exit_code, 3)
        self.assertEqual(raised.exception.report['error']['kind'], 'refused')

    def redaction_source(self, *, retained_secret=False):
        source = self.root / '-private.pdf'
        fixture(source, retained_secret=retained_secret, content=(
            b'BT /F1 20 Tf 30 100 Td (PRIVATE-731) Tj ET\n'
            b'BT /F1 14 Tf 30 200 Td (PUBLIC CONTROL) Tj ET'
        ))
        return source

    def test_redaction_text_and_regions_remove_only_target_content(self):
        source = self.redaction_source()
        original = source.read_bytes()
        client = Tpdf(BINARY, cwd=self.root, timeout=120)
        for name, selectors in [
            ('text', {'texts': ['private-731']}),
            ('regions', {'regions': [{'page': 1, 'rect': [25, 275, 170, 35]}]}),
        ]:
            with self.subTest(selector=name):
                output = self.root / f'{name}.pdf'
                report = client.redact(source.name, output, **selectors)
                self.assertTrue(report['written'])
                self.assertIs(report['verified'], True)
                self.assertEqual(report['reasons'], [])
                self.assertEqual(report['regions'], 1)
                text = client.text(output)['pages'][0]['text']
                self.assertNotIn('PRIVATE-731', text)
                self.assertIn('PUBLIC CONTROL', text)
                self.assertNotIn(b'PRIVATE-731', output.read_bytes())
        self.assertEqual(source.read_bytes(), original)

    def test_redaction_fill_is_black_unless_another_colour_is_named(self):
        source = self.redaction_source()
        client = Tpdf(BINARY, cwd=self.root, timeout=120)
        # The colour is in the annotation's /IC and in its appearance; the
        # copy is not compressed there, so both are readable as bytes.
        for fill, colour in [(None, b'/IC[0 0 0]'), ('red', b'/IC[0.83 0.16 0.16]'),
                             ('white', b'/IC[1 1 1]')]:
            with self.subTest(fill=fill):
                output = self.root / f'fill-{fill}.pdf'
                extra = {} if fill is None else {'fill': fill}
                report = client.redact(source.name, output, texts=['private-731'], **extra)
                self.assertIs(report['verified'], True)
                self.assertIn(colour, output.read_bytes())
        with self.assertRaises(ValueError):
            client.redact(source.name, self.root / 'blue.pdf', texts=['private-731'], fill='blue')
        self.assertFalse((self.root / 'blue.pdf').exists())

    def test_redaction_dry_run_and_no_match_never_claim_verified(self):
        source = self.redaction_source()
        output = self.root / 'protected.pdf'
        output.write_bytes(b'EXISTING OUTPUT')
        report = self.pdf.redact(source, output, patterns=['PRIVATE-[0-9]+'], dry_run=True, force=True)
        self.assertTrue(report['dry_run'])
        self.assertFalse(report['written'])
        self.assertIsNone(report['verified'])
        self.assertEqual(report['searches'][0]['matches'], 1)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        report = self.pdf.redact(source, patterns=['PRIVATE-[0-9]+'], dry_run=True)
        self.assertIsNone(report['output'])
        self.assertIsNone(report['verified'])
        # Nothing matched and nothing is under the name: not an error.
        fresh = self.root / 'fresh.pdf'
        report = self.pdf.redact(source, fresh, texts=['private-731'], case_sensitive=True)
        self.assertEqual(report['searches'][0]['matches'], 0)
        self.assertFalse(report['written'])
        self.assertIsNone(report['verified'])
        self.assertFalse(fresh.exists())
        # Nothing matched over an earlier output with force: refused, so a
        # caller cannot take the earlier file for this run's copy.
        with self.assertRaises(CommandError) as caught:
            self.pdf.redact(source, output, texts=['private-731'], case_sensitive=True, force=True)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertIn("is not this run's copy", str(caught.exception))
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')

    def test_redaction_verification_failure_retains_report_and_written_copy(self):
        source = self.redaction_source(retained_secret=True)
        output = self.root / 'unverified.pdf'
        with self.assertRaises(CommandError) as caught:
            self.pdf.redact(source, output, texts=['PRIVATE-731'])
        self.assertEqual(caught.exception.exit_code, 1)
        report = caught.exception.report
        self.assertTrue(report['written'])
        self.assertIs(report['verified'], False)
        self.assertTrue(any('is still in the file' in r for r in report['reasons']), report['reasons'])
        self.assertNotIn('PRIVATE-731', self.pdf.text(output)['pages'][0]['text'])
        self.assertEqual(self.pdf.comments(output)['comments'][0]['body'], 'PRIVATE-731')
        report = self.pdf.redact(source, output, texts=['PRIVATE-731'], force=True, check=False)
        self.assertTrue(report['written'])
        self.assertIs(report['verified'], False)
        self.assertTrue(report['reasons'])

    def test_redaction_invalid_regions_and_requests_preserve_output(self):
        source = self.redaction_source()
        output = self.root / 'protected.pdf'
        output.write_bytes(b'EXISTING OUTPUT')
        for selectors in [
            {'regions': [{'page': 1, 'rect': [0, 0, -1, 10]}]},
            {'regions': [{'page': 2, 'rect': [0, 0, 10, 10]}]},
            {'regions': [{'page': 1, 'rect': [0, 0, 10, 10], 'unknown': True}]},
            {'patterns': ['(']},
        ]:
            with self.subTest(selectors=selectors), self.assertRaises(CommandError):
                self.pdf.redact(source, output, force=True, **selectors)
            self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        with self.assertRaises(TypeError):
            self.pdf.redact(source, output, texts='PRIVATE-731')
        with self.assertRaises(TypeError):
            self.pdf.redact(source, output, regions={'page': 1, 'rect': [0, 0, 10, 10]})
        with self.assertRaises(CommandError) as caught:
            self.pdf.redact(source, output, texts=['PRIVATE-731'])
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')

    def test_signing_refusals_preserve_inputs_and_outputs(self):
        # No usable identity is needed, and these cases cannot sign with a real key.
        original = self.source.read_bytes()
        output = self.root / 'signed.pdf'
        identity = 'tpdf SYNTHETIC MISSING API IDENTITY 00000000'
        for options in [{'long_term': True}, {'timestamp': 'ftp://invalid.example'},
                        {'page': 1}, {'rect': [0, 0, -1, 20]},
                        {'rect': [0, 0, 80, 40], 'lines': ['invalid']}]:
            with self.subTest(options=options), self.assertRaises(CommandError) as caught:
                self.pdf.sign(self.source, output, identity=identity, **options)
            self.assertEqual(caught.exception.exit_code, 2)
            self.assertFalse(output.exists())
        output.write_bytes(b'EXISTING OUTPUT')
        with self.assertRaises(CommandError) as caught:
            self.pdf.sign(self.source, output, identity=identity)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        self.assertEqual(self.source.read_bytes(), original)
        client = Tpdf(BINARY, cwd=self.root)
        # Missing input is refused before any identity is used; '--force' is a path.
        with self.assertRaises(CommandError) as caught:
            client.sign('--force', 'unused.pdf', identity=identity)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertFalse((self.root / 'unused.pdf').exists())

    def test_long_term_names_its_authority_and_refuses_without_writing(self):
        # The line the client builds, and that it asks for no identity.
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"long-term"}', b'')
            self.pdf.long_term('--signed.pdf', '-kept.pdf', timestamp='digicert', force=True)
            self.assertEqual(popen.call_args.args[0][1:], [
                'long-term', '--json', '-o', '-kept.pdf', '--timestamp', 'digicert', '--force',
                '--', '--signed.pdf'])
            popen.reset_mock()
            for timestamp in ['', '   ', None]:
                with self.subTest(timestamp=timestamp), self.assertRaises(ValueError):
                    self.pdf.long_term('signed.pdf', 'kept.pdf', timestamp=timestamp)
            popen.assert_not_called()
        # Through the tool: an address it does not ask is a malformed line, and
        # a document nobody signed is refused before anybody is asked. The
        # address is one nothing listens at, and nothing is sent to it.
        original = self.source.read_bytes()
        output = self.root / 'kept.pdf'
        with self.assertRaises(CommandError) as caught:
            self.pdf.long_term(self.source, output, timestamp='ftp://invalid.example')
        self.assertEqual(caught.exception.exit_code, 2)
        with self.assertRaises(CommandError) as caught:
            self.pdf.long_term(self.source, output, timestamp='http://127.0.0.1:9/')
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertIn('has no signature', str(caught.exception))
        self.assertFalse(output.exists())
        output.write_bytes(b'EXISTING OUTPUT')
        with self.assertRaises(CommandError) as caught:
            self.pdf.long_term(self.source, output, timestamp='http://127.0.0.1:9/')
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')
        self.assertEqual(self.source.read_bytes(), original)

    def test_signing_options_and_discovery_use_the_json_contract(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"identities","usable":[],"not_usable":[]}', b'')
            self.assertEqual(self.pdf.identities()['usable'], [])
            self.assertEqual(popen.call_args.args[0][1:], ['identities', '--json'])
            process.communicate.return_value = (b'{"schema":1,"command":"sign"}', b'')
            self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID')
            self.assertEqual(popen.call_args.args[0][1:],
                ['sign', '--json', '-o', 'output.pdf', '--identity', 'SYNTHETIC ID', '--', 'input.pdf'])
            self.pdf.sign('--input.pdf', '-output.pdf', identity='SYNTHETIC ID',
                rect=[10, 20, 200, 80], page=2, no_image=True, lines=[], reason='Synthetic approval',
                location='Synthetic location', timestamp='digicert', long_term=True, lock=True,
                force=True)
            self.assertEqual(popen.call_args.args[0][1:], [
                'sign', '--json', '-o', '-output.pdf', '--identity', 'SYNTHETIC ID',
                '--visible', '--rect', '10,20,200,80', '--page', '2', '--no-image', '--lines', '',
                '--reason', 'Synthetic approval', '--location', 'Synthetic location',
                '--timestamp', 'digicert', '--long-term', '--lock', '--force', '--', '--input.pdf'])
            self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID', rect=[10, 20, 200, 80],
                image='stamp.png', lines=[], reason='Synthetic approval', hide=['reason'])
            self.assertEqual(popen.call_args.args[0][1:], [
                'sign', '--json', '-o', 'output.pdf', '--identity', 'SYNTHETIC ID',
                '--visible', '--rect', '10,20,200,80', '--image', 'stamp.png', '--lines', '',
                '--hide', 'reason', '--reason', 'Synthetic approval', '--', 'input.pdf'])
            self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID', rect=[10, 20, 200, 80],
                text=['Digitally signed', '{date}'], date_format='DD.MM.YYYY')
            self.assertEqual(popen.call_args.args[0][1:], [
                'sign', '--json', '-o', 'output.pdf', '--identity', 'SYNTHETIC ID',
                '--visible', '--rect', '10,20,200,80', '--text', 'Digitally signed',
                '--text', '{date}', '--date-format', 'DD.MM.YYYY', '--', 'input.pdf'])
            self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID', anchor='Signature:',
                size=[120, 40], offset=[-4, 12.5], anchor_match=2, page=3)
            self.assertEqual(popen.call_args.args[0][1:], [
                'sign', '--json', '-o', 'output.pdf', '--identity', 'SYNTHETIC ID',
                '--visible', '--anchor', 'Signature:', '--size', '120,40', '--offset', '-4,12.5',
                '--anchor-match', '2', '--page', '3', '--', 'input.pdf'])
            for bad in [dict(anchor='x'), dict(anchor='x', size=[1]), dict(size=[1, 2]),
                        dict(offset=[1, 2]), dict(anchor_match=1),
                        dict(anchor='x', size=[1, 2], rect=[0, 0, 9, 9]),
                        dict(anchor='x', size=[1, 2], offset=[1])]:
                with self.assertRaises(ValueError, msg=bad):
                    self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID', **bad)
            self.pdf.sign('input.pdf', 'output.pdf', identity='SYNTHETIC ID',
                reason='Synthetic approval', contact='signer@example.com')
            self.assertEqual(popen.call_args.args[0][1:], [
                'sign', '--json', '-o', 'output.pdf', '--identity', 'SYNTHETIC ID',
                '--reason', 'Synthetic approval', '--contact', 'signer@example.com',
                '--', 'input.pdf'])
            popen.reset_mock()
            with self.assertRaises(TypeError):
                self.pdf.sign(self.source, 'unused.pdf', identity='SYNTHETIC ID', text='Digitally signed')
            with self.assertRaises(ValueError):
                self.pdf.sign(self.source, 'unused.pdf', identity='SYNTHETIC ID', rect=[10, 20, 200, 80],
                    image='stamp.png', no_image=True)
            with self.assertRaises(TypeError):
                self.pdf.sign(self.source, 'unused.pdf', identity='SYNTHETIC ID', hide='reason')
            popen.assert_not_called()
            for identity in ['', '   ', None]:
                with self.assertRaises(ValueError):
                    self.pdf.sign(self.source, 'unused.pdf', identity=identity)
            with self.assertRaises(TypeError):
                self.pdf.sign(self.source, 'unused.pdf', identity='SYNTHETIC ID', lines='name')
            popen.assert_not_called()

    def test_hidden_lists_words_under_a_drawn_rectangle_and_nothing_in_a_plain_page(self):
        plain = self.pdf.hidden(self.source)
        self.assertEqual((plain['found'], plain['without_text'], plain['pages']), ([], [], 1))
        self.assertGreater(plain['compared'], 10)
        covered = self.root / 'covered.pdf'
        fixture(covered, content=(
            b'BT /F1 12 Tf 30 300 Td (VISIBLE HEADING) Tj ET '
            b'BT /F1 12 Tf 30 100 Td (SYNTHETIC ORIGINAL) Tj ET '
            b'0 g 24 92 220 24 re f'))
        report = self.pdf.hidden(covered)
        self.assertEqual([(f['page'], f['text'], f['off_page']) for f in report['found']],
                         [(1, 'SYNTHETIC ORIGINAL', False)])
        # The words are still what `text` extracts, which is the point.
        self.assertIn('SYNTHETIC ORIGINAL', self.pdf.text(covered)['pages'][0]['text'])
        with self.assertRaises(CommandError) as caught:
            self.pdf.hidden(self.source, pages='2')
        self.assertEqual(caught.exception.exit_code, 3)

    def test_thumbnail_sizes_only_the_requested_page(self):
        import struct
        good = b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >>'
        for wanted, kids in [(1, [good, b'null']), (2, [b'null', good])]:
            with self.subTest(page=wanted):
                write_pdf(self.source, [
                    b'<< /Type /Catalog /Pages 2 0 R >>',
                    b'<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>',
                    *kids,
                ])
                def thumbnail(page):
                    return subprocess.run(
                        [BINARY, 'render', '-', '-o', '-', '--fit', '32x32', '--page', str(page)],
                        input=self.source.read_bytes(), capture_output=True,
                    )
                visible = thumbnail(wanted)
                self.assertEqual(visible.returncode, 0, visible.stderr)
                self.assertEqual(struct.unpack('>II', visible.stdout[16:24]), (24, 32))
                self.assertNotEqual(thumbnail(3 - wanted).returncode, 0)

    def test_render_supports_repeatable_visual_assertions(self):
        first = self.root / 'before.png'
        second = self.root / 'after.png'
        report = self.pdf.render(self.source, first, dpi=72)
        self.assertEqual((report['page'], report['dpi'], report['width_px'], report['height_px']), (1,72,300,400))
        baseline = first.read_bytes()
        self.assertEqual(baseline[:8], b'\x89PNG\r\n\x1a\n')
        self.pdf.render(self.source, second, dpi=72)
        self.assertEqual(baseline, second.read_bytes())
        edited = self.root / 'rotated.pdf'
        self.pdf.edit(self.source, edited, [{'op': 'rotate', 'page': 1, 'degrees': 90}])
        report = self.pdf.render(edited, second, dpi=72, force=True)
        self.assertEqual((report['width_px'],report['height_px']), (400,300))
        self.assertNotEqual(baseline, second.read_bytes())
        with self.assertRaises(CommandError) as caught:
            self.pdf.render(self.source, first, page=2, force=True)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertIn("past the end of the document", str(caught.exception))
        self.assertEqual(first.read_bytes(), baseline)

    def test_form_helpers_fill_through_stdin_and_read_back(self):
        source = self.root / 'form.pdf'
        fixture(source, form=True)
        fields = self.pdf.fields(source)['fields']
        field = next(f for f in fields if f['kind'] == 'text' and not f['not_editable'])
        output = self.root / 'filled.pdf'
        self.pdf.fill(source, output, {field['name']: 'API'})
        after = self.pdf.fields(output)['fields']
        self.assertEqual(next(f for f in after if f['name'] == field['name'])['value'], 'API')

    def test_merging_forms_preserves_base_fields_and_the_appendix_appearance(self):
        base, appendix, filled, merged, saved = [
            self.root / name for name in ('base.pdf', 'appendix.pdf', 'filled.pdf', 'merged.pdf', 'saved.pdf')]
        fixture(base, form=True)
        fixture(appendix, form=True)
        merged.write_bytes(b'previous output')
        with self.assertRaisesRegex(CommandError, 'appearance'):
            self.pdf.merge([base, appendix], merged, force=True)
        self.assertEqual(merged.read_bytes(), b'previous output')
        name = 'Synthetic.answer'
        self.pdf.fill(appendix, filled, {name: 'APPENDIX'})
        self.pdf.merge([base, filled], merged, force=True)
        self.assertEqual([(f['name'], f['value']) for f in self.pdf.fields(merged)['fields']], [(name, 'OLD')])
        self.pdf.fill(merged, saved, {name: 'BASE'})
        self.assertEqual(self.pdf.fields(saved)['fields'][0]['value'], 'BASE')
        source_png, merged_png = self.root / 'original.png', self.root / 'merged.png'
        self.pdf.render(filled, source_png)
        self.pdf.render(saved, merged_png, page=2)
        self.assertEqual(merged_png.read_bytes(), source_png.read_bytes())
        self.pdf.render(self.source, merged_png, force=True)
        self.assertNotEqual(merged_png.read_bytes(), source_png.read_bytes())

    def test_plain_cover_preserves_rendered_appendix_and_remains_form_editable(self):
        form, filled, merged, made, saved = [self.root / f'{name}.pdf'
                                            for name in ('form', 'filled', 'merged', 'made', 'saved')]
        fixture(form, form=True)
        merged.write_bytes(b'previous output')
        with self.assertRaisesRegex(CommandError, 'appearance'):
            self.pdf.merge([self.source, form], merged, force=True)
        self.assertEqual(merged.read_bytes(), b'previous output')
        self.pdf.fill(form, filled, {'Synthetic.answer': 'VISIBLE'})
        self.pdf.merge([self.source, filled], merged, force=True)
        self.pdf.add_fields(merged, made, [{'name': 'New', 'kind': 'text', 'page': 1,
                                            'rect': [20, 20, 100, 20]}])
        self.pdf.fill(made, saved, {'New': 'COVER'})
        self.assertEqual(self.pdf.fields(saved)['fields'][0]['value'], 'COVER')
        reference = self.root / 'filled.png'
        actual = self.root / 'appendix.png'
        self.pdf.render(filled, reference)
        for doc in (merged, saved):
            self.pdf.render(doc, actual, page=2, force=True)
            self.assertEqual(actual.read_bytes(), reference.read_bytes())

    def test_text_edit_output_stays_editable_at_the_decoded_byte_limit(self):
        content = b'BT /F1 12 Tf 30 100 Td (Ten) Tj ET'
        import hashlib
        content = content.ljust(1024 * 1024, b' ')
        fixture(self.source, content=content)
        self.assertEqual(self.pdf.text_runs(self.source)['revision'],
                         list(hashlib.sha256(content + b'\n').digest()))
        doc = self.source
        for original, replacement in [('Ten', 'Tan'), ('Tan', 'Ton')]:
            scan = self.pdf.text_runs(doc)
            self.assertEqual(scan['runs'][0]['text'], original)
            saved = self.root / f'{replacement}.pdf'
            self.pdf.edit(doc, saved, [{'op': 'replace_text', 'page': 1, 'operator': scan['runs'][0]['operator'],
                                        'revision': scan['revision'], 'original': original, 'replacement': replacement}])
            doc = saved
        self.assertEqual(self.pdf.text_runs(doc)['runs'][0]['text'], 'Ton')

    def test_off_page_header_refuses_the_entire_mixed_size_copy(self):
        small, mixed, out = [self.root / f'{name}.pdf' for name in ('small', 'mixed', 'out')]
        self.pdf.crop(self.source, small, rect=[0, 0, 100, 100])
        self.pdf.merge([self.source, small], mixed)
        out.write_bytes(b'previous output')
        args = [mixed, '-o', out, '--force', '--text', 'ORIGINAL', '--position', 'top-left']
        with self.assertRaisesRegex(CommandError, 'page 2'):
            self.pdf.run('page-numbers', *args, '--margin', '200')
        self.assertEqual(out.read_bytes(), b'previous output')
        self.assertEqual(self.pdf.run('page-numbers', *args, '--margin', '5').report['written'], [1, 2])
        for page in (1, 2):
            before, after = self.root / 'before.png', self.root / 'after.png'
            self.pdf.render(mixed, before, page=page, dpi=72, force=True)
            self.pdf.render(out, after, page=page, dpi=72, force=True)
            self.assertNotEqual(before.read_bytes(), after.read_bytes(), f'header on page {page}')

    def test_info_distinguishes_an_incomplete_signature_scan(self):
        for depth, expected in [(1, 'Signatures: none'), (12, 'Signatures: unknown (incomplete scan)')]:
            objects = [b'<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>',
                       b'<< /Type /Pages /Count 1 /Kids [3 0 R] >>',
                       b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>']
            for index in range(depth):
                objects.append(f'<< /T (group{index}) /Kids [{index + 5} 0 R] >>'.encode())
            objects.append(b'<< /FT /Tx /T (Notes) >>')
            write_pdf(self.source, objects)
            result = subprocess.check_output([BINARY, 'info', self.source], text=True)
            self.assertIn(expected, result)

    def test_search_reports_only_the_selected_blank_page(self):
        blank, mixed = self.root / 'blank.pdf', self.root / 'mixed.pdf'
        fixture(blank, content=b'')
        self.pdf.merge([self.source, blank], mixed)
        result = self.pdf.run('search', '--text', 'SYNTHETIC', '--pages', '2', mixed, check=False)
        self.assertEqual(result.exit_code, 1)
        self.assertIn('page 2 has no text to search', result.stderr)
        self.assertEqual(len(self.pdf.search(mixed, texts=['SYNTHETIC'])['files'][0]['matches']), 1)

    def test_mapping_edit_and_command_help(self):
        from types import MappingProxyType
        operation = {'op': 'rotate', 'page': 1, 'degrees': 90}
        for op in (operation, MappingProxyType(operation)):
            self.assertFalse(self.pdf.edit(self.source, self.root / 'rotated.pdf', [op], dry_run=True)['written'])
        self.assertEqual(self.pdf.run('info', '--help').report['command'], 'help')
        fixture(self.root / '--help')
        local = Tpdf(BINARY, cwd=self.root)
        self.assertEqual(local.run('info', '--', '--help').report['command'], 'info')

    def test_ocr_passes_complete_language_tags_in_preference_order(self):
        with patch.object(self.pdf, 'run') as run:
            for tags, expected in [('de-DE', ['de-DE']), (['de-DE', 'en-US'], ['de-DE', 'en-US'])]:
                self.pdf.ocr(self.source, 'out.pdf', languages=tags)
                args = run.call_args.args
                self.assertEqual([args[i+1] for i, arg in enumerate(args) if arg == '--language'], expected)

    def test_edit_text_without_a_request_file(self):
        scanned = self.pdf.text_runs(self.source)
        run = scanned['runs'][0]
        self.assertEqual(run['text'], 'SYNTHETIC ORIGINAL')
        output = self.root / 'text.pdf'
        self.pdf.edit(self.source, output, [{
            'op': 'replace_text', 'page': 1, 'operator': run['operator'],
            'revision': scanned['revision'], 'original': run['text'], 'replacement': 'SYNTHETIC',
        }])
        self.assertEqual(self.pdf.text(output)['pages'][0]['text'].strip(), 'SYNTHETIC')

    def test_edit_rewrites_powerpoint_accessible_text_with_the_words(self):
        source = self.root / 'powerpoint.pdf'
        powerpoint_fixture(source)
        original = source.read_bytes()
        scanned = self.pdf.text_runs(source)
        run = next(run for run in scanned['runs'] if run['text'] == 'SYNTHETIC ORIGINAL')
        output = self.root / 'powerpoint-edited.pdf'
        self.pdf.edit(source, output, [{
            'op': 'replace_text', 'page': 1, 'operator': run['operator'],
            'revision': scanned['revision'], 'original': run['text'], 'replacement': 'SYNTHETIC EDITED',
        }])
        self.assertEqual(self.pdf.text(output)['pages'][0]['text'].strip(), 'SYNTHETIC EDITED')
        # Independent of tpdf's reader: the Span's newest definition in the file
        # (a save appends it) must carry the new words as its ActualText.
        data = output.read_bytes()
        newest = data[data.rindex(b'\n11 0 obj'):]
        newest = newest[:newest.index(b'endobj')]
        # A text string is PDFDocEncoding or, with a byte-order mark, UTF-16BE
        # (ISO 32000-1 7.9.2.2); the writer may use either. No escapes occur
        # in these words, so the literal ends at the first parenthesis.
        start = newest.index(b'/ActualText') + len(b'/ActualText')
        literal = newest[newest.index(b'(', start) + 1:]
        literal = literal[:literal.index(b')')]
        spoken = literal[2:].decode('utf-16-be') if literal.startswith(b'\xfe\xff') else literal.decode('latin-1')
        self.assertEqual(spoken, 'SYNTHETIC EDITED')
        self.assertEqual(source.read_bytes(), original)

    def test_dry_run_and_refused_request_preserve_output(self):
        output = self.root / 'protected.pdf'
        operations = [{'op': 'rotate', 'page': 1, 'degrees': 90}]
        self.assertFalse(self.pdf.edit(self.source, output, operations, dry_run=True)['written'])
        self.assertFalse(output.exists())
        output.write_bytes(b'EXISTING OUTPUT')
        with self.assertRaises(CommandError) as caught:
            self.pdf.edit(self.source, output, operations + [{'op': 'delete_page', 'page': 8}], force=True)
        self.assertEqual(caught.exception.exit_code, 3)
        self.assertIn('operation 2', str(caught.exception))
        self.assertEqual(output.read_bytes(), b'EXISTING OUTPUT')

    def test_usage_errors_and_negative_results_are_inspectable(self):
        result = self.pdf.run('unknown-command', check=False)
        self.assertEqual(result.exit_code, 2)
        self.assertEqual(result.report['error']['kind'], 'usage')
        # A real negative verification verdict is not a generic error report.
        result = self.pdf.run('verify', self.source, '--strict', check=False)
        self.assertEqual(result.exit_code, 1)
        with self.assertRaises(CommandError) as caught:
            self.pdf.run('verify', self.source, '--strict')
        self.assertEqual(caught.exception.report, result.report)

    def test_unicode_paths_and_json_text_round_trip(self):
        output = self.root / 'Prüfung.pdf'
        self.pdf.edit(self.source, output, [{'op': 'annotate', 'page': 1, 'kind': 'note',
            'rect': [20, 30, 20, 20], 'author': 'Prüfer', 'text': 'Größe geprüft'}])
        comment = self.pdf.comments(output)['comments'][0]
        self.assertEqual((comment['author'], comment['body']), ('Prüfer', 'Größe geprüft'))

    def test_arguments_are_literal_and_cwd_is_per_client(self):
        # If the client ever uses a shell, this path would become two commands.
        path = self.root / 'literal; touch SHOULD_NOT_EXIST.pdf'
        fixture(path)
        original_cwd = Path.cwd()
        client = Tpdf(BINARY, cwd=self.root)
        self.assertEqual(client.info(path.name)['files'][0]['document']['pages'], 1)
        self.assertFalse((self.root / 'SHOULD_NOT_EXIST.pdf').exists())
        self.assertEqual(Path.cwd(), original_cwd)

    def test_input_validation_precedes_process_creation(self):
        for timeout in [0, -1, float('nan'), float('inf')]:
            with self.assertRaises(ValueError):
                Tpdf(BINARY, timeout=timeout)
        with patch('tpdf_client.subprocess.Popen') as popen:
            with self.assertRaises(ValueError):
                self.pdf.run('edit', input_json={'bad': float('nan')})
            with self.assertRaises(ValueError):
                self.pdf.run('text', self.source, '-o', 'report.json')
            popen.assert_not_called()

    def test_password_is_child_only_and_not_in_argv(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"info","files":[]}', b'')
            before = dict(os.environ)
            self.pdf.info(self.source, password='synthetic-password')
            args, kwargs = popen.call_args
            self.assertNotIn('synthetic-password', args[0])
            self.assertEqual(kwargs['env']['TPDF_API_DOCUMENT_PASSWORD'], 'synthetic-password')
            self.assertEqual(dict(os.environ), before)

    def test_a_new_password_is_child_only_and_not_in_argv(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"protect"}', b'')
            before = dict(os.environ)
            self.pdf.protect(self.source, 'out.pdf', 'synthetic-new', password='synthetic-old')
            args, kwargs = popen.call_args
            self.assertNotIn('synthetic-new', args[0])
            self.assertNotIn('synthetic-old', args[0])
            self.assertIn('--new-password-env', args[0])
            self.assertEqual(kwargs['env']['TPDF_API_NEW_PASSWORD'], 'synthetic-new')
            self.assertEqual(kwargs['env']['TPDF_API_DOCUMENT_PASSWORD'], 'synthetic-old')
            self.assertEqual(dict(os.environ), before)
            process.communicate.return_value = (b'{"schema":1,"command":"unprotect"}', b'')
            self.pdf.unprotect(self.source, 'out.pdf', 'synthetic-old')
            args, kwargs = popen.call_args
            self.assertEqual(args[0][1], 'unprotect')
            self.assertNotIn('--new-password-env', args[0])
            self.assertNotIn('TPDF_API_NEW_PASSWORD', kwargs['env'])

    def test_images_makes_one_page_for_each_picture(self):
        with tempfile.TemporaryDirectory() as directory:
            picture = os.path.join(directory, 'page.png')
            album = os.path.join(directory, 'album.pdf')
            self.pdf.render(self.source, picture, page=1, dpi=72)
            report = self.pdf.images([picture, picture], album, paper='a4')
            self.assertEqual([page['page'] for page in report['pages']], [1, 2])
            self.assertEqual(report['pages'][0]['source'], picture)
            self.assertEqual(self.pdf.info(album)['files'][0]['document']['pages'], 2)
            with self.assertRaises(TypeError):
                self.pdf.images(picture, album)
            with self.assertRaises(CommandError):
                self.pdf.images([self.source], album, force=True)

    def test_add_fields_makes_fields_that_fill_then_answers(self):
        fields = [
            {'name': 'Name', 'kind': 'text', 'page': 1, 'rect': [72, 100, 200, 20], 'max_length': 40},
            {'name': 'Agree', 'kind': 'checkbox', 'page': 1, 'rect': [72, 140, 12, 12]},
        ]
        with tempfile.TemporaryDirectory() as directory:
            made = os.path.join(directory, 'form.pdf')
            report = self.pdf.add_fields(self.source, made, fields)
            self.assertEqual(report['command'], 'form')
            self.assertEqual([field['name'] for field in report['added']], ['Name', 'Agree'])
            self.assertEqual(report['added'][0]['rect'], [72.0, 100.0, 200.0, 20.0])
            self.assertEqual((report['fields_before'], report['fields_after']), (0, 2))
            listed = {field['name']: field for field in self.pdf.fields(made)['fields']}
            self.assertEqual(listed['Name']['max_length'], 40)
            self.assertEqual(listed['Agree']['value'], False)
            filled = os.path.join(directory, 'filled.pdf')
            self.pdf.fill(made, filled, {'Name': 'Ada', 'Agree': True})
            answers = {field['name']: field['value'] for field in self.pdf.fields(filled)['fields']}
            self.assertEqual(answers, {'Name': 'Ada', 'Agree': True})
            # One problem, and nothing is written.
            again = os.path.join(directory, 'again.pdf')
            with self.assertRaises(CommandError) as refused:
                self.pdf.add_fields(made, again, [dict(fields[0]), {**fields[1], 'name': 'Other'}])
            self.assertIn('already has a field of this name', str(refused.exception))
            self.assertFalse(os.path.exists(again))
            with self.assertRaises(CommandError):
                self.pdf.add_fields(self.source, made, fields)
            self.pdf.add_fields(self.source, made, fields[:1], force=True)
            self.assertEqual(len(self.pdf.fields(made)['fields']), 1)

    def test_add_fields_builds_the_line_its_options_ask_for(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"form"}', b'')
            self.pdf.add_fields(self.source, 'out.pdf', [{'name': 'N'}])
            self.assertEqual(
                popen.call_args[0][0][1:],
                ['form', '--json', '-o', 'out.pdf', '--fields', '-', '--', os.fspath(self.source)],
            )
            self.assertEqual(process.communicate.call_args[0][0], b'[{"name":"N"}]')
            self.pdf.add_fields(self.source, 'out.pdf', [], force=True, invalidate_signatures=True)
            for flag in ('--force', '--invalidate-signatures'):
                self.assertIn(flag, popen.call_args[0][0])

    def test_compress_builds_the_line_its_options_ask_for(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"compress"}', b'')
            self.pdf.compress(self.source, 'out.pdf')
            self.assertEqual(popen.call_args[0][0][1:], ['compress', '--json', '-o', 'out.pdf', '--', os.fspath(self.source)])
            self.pdf.compress(self.source)
            self.assertIn('--dry-run', popen.call_args[0][0])
            self.assertNotIn('-o', popen.call_args[0][0])
            self.pdf.compress(
                self.source, 'out.pdf', pictures='screen', dpi=96, quality=40, jpeg=False,
                preview='p.png', force=True, invalidate_signatures=True,
            )
            line = popen.call_args[0][0]
            for part in (['--pictures', 'screen'], ['--dpi', '96'], ['--quality', '40'], ['--preview', 'p.png']):
                at = line.index(part[0])
                self.assertEqual(line[at:at + 2], part)
            for flag in ('--no-jpeg', '--force', '--invalidate-signatures'):
                self.assertIn(flag, line)
            for wrong in (96.0, '96', True):
                with self.assertRaises(TypeError):
                    self.pdf.compress(self.source, 'out.pdf', dpi=wrong)
                with self.assertRaises(TypeError):
                    self.pdf.compress(self.source, 'out.pdf', quality=wrong)

    def test_compress_estimates_and_refuses_what_would_not_be_smaller(self):
        estimate = self.pdf.compress(self.source, pictures='screen')
        self.assertFalse(estimate['written'])
        self.assertIsNone(estimate['output'])
        self.assertEqual((estimate['preset'], estimate['dpi'], estimate['quality']), ('screen', 110, 60))
        self.assertEqual(estimate['bytes_before'], os.path.getsize(self.source))
        with tempfile.TemporaryDirectory() as directory:
            out = os.path.join(directory, 'smaller.pdf')
            if estimate['bytes_after'] < estimate['bytes_before']:
                report = self.pdf.compress(self.source, out, pictures='screen')
                self.assertTrue(report['written'])
                self.assertEqual(report['bytes_after'], os.path.getsize(out))
                self.assertEqual(self.pdf.text(out)['pages'], self.pdf.text(self.source)['pages'])
                # A second pass has nothing left to save.
                with self.assertRaises(CommandError):
                    self.pdf.compress(out, os.path.join(directory, 'again.pdf'), pictures='screen')
            else:
                with self.assertRaises(CommandError):
                    self.pdf.compress(self.source, out, pictures='screen')
                self.assertFalse(os.path.exists(out))

    def test_protect_and_unprotect_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            locked = os.path.join(directory, 'locked.pdf')
            back = os.path.join(directory, 'back.pdf')
            report = self.pdf.protect(self.source, locked, 'synthetic-new')
            self.assertTrue(report['protected'])
            self.assertFalse(report['was_protected'])
            with self.assertRaises(CommandError):
                self.pdf.text(locked)
            self.assertEqual(
                self.pdf.text(locked, password='synthetic-new')['pages'],
                self.pdf.text(self.source)['pages'],
            )
            report = self.pdf.unprotect(locked, back, 'synthetic-new')
            self.assertFalse(report['protected'])
            self.assertTrue(report['was_protected'])
            self.assertEqual(self.pdf.text(back)['pages'], self.pdf.text(self.source)['pages'])
            with self.assertRaises(CommandError):
                self.pdf.unprotect(back, locked, 'synthetic-new', force=True)

    def test_malformed_and_incompatible_reports_are_not_success(self):
        for response in [b'', b'not JSON', b'{}{}', b'[]', b'{"schema":2,"command":"help"}', b'{"schema":true,"command":"help"}', b'{"schema":1,"command":"info"}']:
            with self.subTest(response=response), patch('tpdf_client.subprocess.Popen') as popen:
                process = popen.return_value
                process.returncode = 0
                process.communicate.return_value = (response, b'')
                with self.assertRaises(ProtocolError):
                    self.pdf.help()

    def test_a_protocol_error_keeps_what_the_tool_said(self):
        with patch('tpdf_client.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 4
            process.communicate.return_value = (
                b'', b'tpdf: could not work out where tpdf is installed\n')
            with self.assertRaises(ProtocolError) as raised:
                self.pdf.help()
        self.assertEqual(raised.exception.exit_code, 4)
        self.assertIn('could not work out where tpdf is installed', raised.exception.stderr)
        self.assertIn('could not work out where tpdf is installed', str(raised.exception))

    def test_timeout_closes_parent_and_child_output_pipes(self):
        # Real descendants inherit stdout/stderr. Killing only the coordinator
        # leaves communicate() blocked on the child's pipes for 60 seconds.
        original_popen = subprocess.Popen
        processes = []
        def fake_tool(argv, **kwargs):
            script = 'import subprocess,sys,time; subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); print("ready",flush=True); time.sleep(60)'
            process = original_popen([sys.executable, '-c', script], **kwargs)
            processes.append(process)
            return process
        start = time.monotonic()
        # Patch only the creation of the fake CLI, not taskkill on Windows.
        def launch(argv, **kwargs):
            if argv[0] == str(BINARY.resolve()):
                return fake_tool(argv, **kwargs)
            return original_popen(argv, **kwargs)
        with patch('tpdf_client.subprocess.Popen', side_effect=launch) as launches:
            with self.assertRaises(CommandTimeout):
                Tpdf(BINARY, timeout=0.5).help()
        if os.name == 'nt':
            cleanup = launches.call_args_list[1].args[0]
            self.assertEqual(Path(cleanup[0]), Path(os.environ['SystemRoot']) / 'System32' / 'taskkill.exe')
        self.assertLess(time.monotonic() - start, 10)
        self.assertIsNotNone(processes[0].poll())


    def test_timeout_after_parent_exits_with_its_output_still_open(self):
        original = subprocess.Popen
        processes = []
        release = self.root / 'release-child'
        child = (
            'import os,sys,time\n'
            'deadline = time.monotonic() + 15\n'
            'while time.monotonic() < deadline and not os.path.exists(sys.argv[1]):\n'
            '    time.sleep(0.05)\n'
        )

        def launch(argv, **kwargs):
            if argv[0] == str(BINARY.resolve()):
                parent = 'import subprocess,sys; subprocess.Popen([sys.executable,"-c",sys.argv[1],sys.argv[2]])'
                process = original([sys.executable, '-c', parent, child, str(release)], **kwargs)
                processes.append(process)
                return process
            return original(argv, **kwargs)

        try:
            start = time.monotonic()
            with patch('tpdf_client.subprocess.Popen', side_effect=launch), self.assertRaises(CommandTimeout):
                Tpdf(BINARY, timeout=0.5).help()
            self.assertLess(time.monotonic() - start, 12)
        finally:
            # Windows cannot enumerate this child through its exited parent.
            # Release its inherited pipes before the packaged test removes Python.
            release.touch()
            for process in processes:
                process.communicate(timeout=10)


class ExecutableLookupTests(unittest.TestCase):
    """The client finds the tool on PATH and never in the current directory.

    A script run in a folder of received documents must not start a
    `tpdf-cli.bat` somebody put beside them, which `shutil.which` would on
    Windows before Python 3.12.
    """

    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='tpdf-lookup-')
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        self.received = root / 'received'
        self.installed = root / 'installed'
        self.empty = root / 'empty'
        for folder in (self.received, self.installed, self.empty):
            folder.mkdir()
        name = 'tpdf-cli.exe' if os.name == 'nt' else 'tpdf-cli'
        for folder in (self.received, self.installed):
            planted = folder / name
            planted.write_text('@echo off\n' if os.name == 'nt' else '#!/bin/sh\n', encoding='ascii')
            planted.chmod(0o755)
        self.here = os.getcwd()
        os.chdir(self.received)
        self.addCleanup(os.chdir, self.here)

    def found(self, path: str) -> str:
        with patch.dict(os.environ, {'PATH': path}):
            return Tpdf().executable

    @unittest.skipUnless(os.name == 'nt', 'Windows native CLI discovery')
    def test_batch_launchers_are_skipped_and_refused(self):
        for ext in ('.bat', '.cmd'):
            shim = self.empty / ('tpdf-cli' + ext)
            shim.touch()
            with patch.dict(os.environ, {'PATH': str(self.empty)}):
                with self.assertRaises(FileNotFoundError):
                    Tpdf()
                with self.assertRaisesRegex(ValueError, 'native'):
                    Tpdf(shim)
                with self.assertRaisesRegex(ValueError, 'native'):
                    Tpdf(shim.name)

    def test_a_tool_in_the_current_directory_is_not_found(self):
        with patch.dict(os.environ, {'PATH': str(self.empty)}):
            with self.assertRaises(FileNotFoundError):
                Tpdf()
            with self.assertRaises(FileNotFoundError):
                Tpdf('tpdf-cli')
        # A relative PATH entry names the current directory too.
        with patch.dict(os.environ, {'PATH': os.pathsep.join(['.', str(self.empty)])}):
            with self.assertRaises(FileNotFoundError):
                Tpdf()

    def test_a_tool_on_path_is_found_and_the_current_directory_is_passed_over(self):
        # The control: the same tool in a PATH folder is found, and it is that
        # one, not the copy in the folder the script was started in.
        found = Path(self.found(os.pathsep.join([str(self.empty), str(self.installed)])))
        self.assertEqual(found.parent, self.installed.resolve())
        # An explicit path to the current directory's copy is the caller's choice.
        name = 'tpdf-cli.exe' if os.name == 'nt' else 'tpdf-cli'
        explicit = Path(Tpdf('.' + os.sep + name).executable)
        self.assertEqual(explicit.parent, self.received.resolve())


if __name__ == '__main__':
    unittest.main()
