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

from tpdf import CommandError, CommandTimeout, ProtocolError, Tpdf

ROOT = Path(__file__).resolve().parents[2]
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'src-tauri' / 'target'))
BINARY = Path(os.environ.get('TPDF_TEST_CLI', TARGET / 'debug' / ('tpdf-cli.exe' if os.name == 'nt' else 'tpdf-cli')))


def fixture(path: Path, *, form: bool = False) -> None:
    """A one-page PDF with visible, editable text; explicit xref offsets."""
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


class ClientTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='tpdf-api-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'synthetic input.pdf'
        fixture(self.source)
        self.pdf = Tpdf(BINARY, timeout=30)

    def test_external_workflow_and_discovery(self):
        commands = {c['name'] for c in self.pdf.help()['commands']}
        self.assertTrue({'edit', 'comments', 'text-runs', 'redact', 'fill', 'sign'} <= commands)
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
        self.pdf.run('extract', output, '--pages', '2', '-o', extracted)
        self.assertEqual(self.pdf.info(extracted)['files'][0]['document']['pages'], 1)
        self.assertEqual(self.pdf.comments(extracted)['comments'], [])

    def test_form_helpers_fill_through_stdin_and_read_back(self):
        source = self.root / 'form.pdf'
        fixture(source, form=True)
        fields = self.pdf.fields(source)['fields']
        field = next(f for f in fields if f['kind'] == 'text' and not f['not_editable'])
        output = self.root / 'filled.pdf'
        self.pdf.fill(source, output, {field['name']: 'API'})
        after = self.pdf.fields(output)['fields']
        self.assertEqual(next(f for f in after if f['name'] == field['name'])['value'], 'API')

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
        with patch('tpdf.subprocess.Popen') as popen:
            with self.assertRaises(ValueError):
                self.pdf.run('edit', input_json={'bad': float('nan')})
            with self.assertRaises(ValueError):
                self.pdf.run('text', self.source, '-o', 'report.json')
            popen.assert_not_called()

    def test_password_is_child_only_and_not_in_argv(self):
        with patch('tpdf.subprocess.Popen') as popen:
            process = popen.return_value
            process.returncode = 0
            process.communicate.return_value = (b'{"schema":1,"command":"info","files":[]}', b'')
            before = dict(os.environ)
            self.pdf.info(self.source, password='synthetic-password')
            args, kwargs = popen.call_args
            self.assertNotIn('synthetic-password', args[0])
            self.assertEqual(kwargs['env']['TPDF_API_DOCUMENT_PASSWORD'], 'synthetic-password')
            self.assertEqual(dict(os.environ), before)

    def test_malformed_and_incompatible_reports_are_not_success(self):
        for response in [b'', b'not JSON', b'{}{}', b'[]', b'{"schema":2,"command":"help"}', b'{"schema":true,"command":"help"}', b'{"schema":1,"command":"info"}']:
            with self.subTest(response=response), patch('tpdf.subprocess.Popen') as popen:
                process = popen.return_value
                process.returncode = 0
                process.communicate.return_value = (response, b'')
                with self.assertRaises(ProtocolError):
                    self.pdf.help()

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
        with patch('tpdf.subprocess.Popen', side_effect=launch):
            with self.assertRaises(CommandTimeout):
                Tpdf(BINARY, timeout=0.5).help()
        self.assertLess(time.monotonic() - start, 10)
        self.assertIsNotNone(processes[0].poll())


if __name__ == '__main__':
    unittest.main()
