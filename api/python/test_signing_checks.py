"""Offline controls for the opt-in long-term signing instrument; no OS keys used."""
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

from check_signing import check_long_term, check_signing, long_term_checks
from tpdf_client import CommandError, Result


def good_report():
    chain = {'standing': 'good', 'end': 'root', 'dropped': 0,
             'certificates': [{'revocation': {'standing': 'good'}},
                              {'revocation': {'standing': 'good'}}]}
    stamp = {'attested': True, 'integrity': {'verdict': 'intact'},
             'trust': {'standing': 'trusted'}, 'revocation': {'standing': 'good'},
             'revocation_chain': chain}
    signature = {'field': 'Signature1', 'document_timestamp': False,
                 'covers_whole_file': False, 'appended_bytes': 123,
                 'integrity': {'verdict': 'intact'}, 'trust': {'standing': 'trusted'},
                 'timestamp': stamp, 'revocation': {'standing': 'good'},
                 'revocation_chain': deepcopy(chain)}
    archive = deepcopy(signature)
    archive.update(field='Timestamp1', document_timestamp=True,
                   covers_whole_file=True, appended_bytes=0)
    return {'strict_passed': True,
            'files': [{'error': None, 'signatures': [signature, archive]}]}


class SigningCheckTests(unittest.TestCase):
    def test_complete_report_passes(self):
        self.assertTrue(long_term_checks(good_report(), 'Signature1'))

    def test_incomplete_or_untrusted_evidence_is_refused_even_when_strict_passed(self):
        # Each control breaks one property while strict_passed remains true.
        cases = [
            (['document_timestamp'], True),
            (['integrity', 'verdict'], 'altered'),
            (['trust', 'standing'], 'untrusted'),
            (['timestamp'], None),
            (['timestamp', 'attested'], False),
            (['timestamp', 'integrity', 'verdict'], 'broken'),
            (['timestamp', 'trust', 'standing'], 'untrusted'),
            (['revocation', 'standing'], 'none'),
            (['revocation_chain', 'standing'], 'unchecked'),
            (['revocation_chain', 'dropped'], 1),
            (['revocation_chain', 'end'], 'no_issuer'),
            (['revocation_chain', 'certificates'], []),
            (['revocation_chain', 'certificates', 1, 'revocation', 'standing'], 'revoked'),
            (['timestamp', 'revocation', 'standing'], 'unknown'),
            (['timestamp', 'revocation_chain', 'dropped'], 1),
        ]
        for path, value in cases:
            report = good_report()
            current = report['files'][0]['signatures'][0]
            for part in path[:-1]:
                current = current[part]
            current[path[-1]] = value
            with self.subTest(path=path), self.assertRaises(RuntimeError):
                long_term_checks(report, 'Signature1')

    def test_missing_or_wrong_archive_is_refused(self):
        for name in ['absent', 'ordinary-signature', 'partial-coverage', 'appended-bytes', 'altered', 'untrusted']:
            report = good_report()
            signatures = report['files'][0]['signatures']
            archive = signatures[1]
            if name == 'absent':
                signatures.pop()
            elif name == 'ordinary-signature':
                archive['document_timestamp'] = False
            elif name == 'partial-coverage':
                archive['covers_whole_file'] = False
            elif name == 'appended-bytes':
                archive['appended_bytes'] = 1
            elif name == 'altered':
                archive['integrity']['verdict'] = 'altered'
            else:
                archive['trust']['standing'] = 'untrusted'
            with self.subTest(case=name), self.assertRaises(RuntimeError):
                long_term_checks(report, 'Signature1')

    def test_invalid_options_stop_before_key_discovery(self):
        pdf = Mock()
        for options in [dict(long_term=True),
                        dict(long_term=True, timestamp='digicert', expect_untrusted=True)]:
            with self.assertRaises(ValueError):
                check_signing(pdf, Path('unused'), 'SYNTHETIC', **options)
        pdf.identities.assert_not_called()
        pdf.sign.assert_not_called()

    def test_successful_run_requires_long_term_and_tests_both_tampered_signatures(self):
        for tamper_verdicts in [('altered', 'altered'), ('altered', 'intact')]:
            with self.subTest(tamper=tamper_verdicts), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source = root / 'source.pdf'
                source.write_bytes(b'SYNTHETIC ORIGINAL')
                pdf = Mock()

                def sign(source, output, **options):
                    output.write_bytes(source.read_bytes() + b' SYNTHETIC SIGNED REVISION')
                    return {'identity': {'id': 'SYNTHETIC'}, 'field': 'Signature1'}

                pdf.sign.side_effect = sign
                tampered = good_report()
                tampered['strict_passed'] = False
                for item, verdict in zip(tampered['files'][0]['signatures'], tamper_verdicts):
                    item['integrity']['verdict'] = verdict
                pdf.run.side_effect = [Result(good_report(), 0, ''), Result(tampered, 1, '')]
                if tamper_verdicts[1] == 'altered':
                    self.assertTrue(check_long_term(pdf, source, root, 'SYNTHETIC', 'digicert'))
                else:
                    with self.assertRaisesRegex(RuntimeError, 'tampering must invalidate both'):
                        check_long_term(pdf, source, root, 'SYNTHETIC', 'digicert')
                pdf.sign.assert_called_once_with(source, root / 'long-term.pdf', identity='SYNTHETIC',
                                                timestamp='digicert', long_term=True)
                self.assertEqual(pdf.run.call_count, 2)
                self.assertIn(b'SYNTHETIC XRIGINAL', (root / 'long-term-altered.pdf').read_bytes())
                self.assertTrue((root / 'long-term-verify.json').exists())

    def test_long_term_refusal_never_falls_back(self):
        pdf = Mock()
        pdf.sign.side_effect = CommandError(Result(
            {'schema': 1, 'command': 'sign', 'message': 'synthetic refusal'}, 3, ''))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source.pdf'
            source.write_bytes(b'SYNTHETIC ORIGINAL')
            with self.assertRaises(CommandError):
                check_long_term(pdf, source, root, 'SYNTHETIC', 'digicert')
            pdf.sign.assert_called_once_with(source, root / 'long-term.pdf',
                                            identity='SYNTHETIC', timestamp='digicert', long_term=True)
            pdf.run.assert_not_called()
            self.assertFalse((root / 'long-term.pdf').exists())
            self.assertEqual(source.read_bytes(), b'SYNTHETIC ORIGINAL')


if __name__ == '__main__':
    unittest.main()
