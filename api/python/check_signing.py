"""Opt-in OS-key integration check, separate from unattended test discovery.

Run from a checkout with a disposable document-signing identity already present:
  python api/python/check_signing.py --executable /path/to/tpdf-cli \
    --identity '<certificate SHA-256 or exact subject>' --output-dir scratch/sign-api \
    --expect-untrusted

The OS may prompt to allow key use. No key is imported, exported, or trusted here.
--timestamp explicitly enables a request to that authority. --long-term also
contacts certificate authorities for revocation evidence and requires a trusted
CA-issued signer; failure never falls back to ordinary signing. Outputs and a JSON
receipt are kept for independent inspection; the output directory must be new.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

from test_api import check_document_workflow, fixture
from tpdf_client import CommandError, Tpdf



def long_term_checks(report: dict, field: str) -> list[str]:
    """Require the expected B-LTA readback, beyond verify --strict's policy.

    Strict verification also accepts signatures with no revocation evidence;
    this check requires complete good evidence for the signer and its timestamp.
    """
    checks = []

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise RuntimeError('long-term: ' + message)
        checks.append('long-term: ' + message)

    files = report.get('files', [])
    require(report.get('strict_passed') is True and len(files) == 1
            and files[0].get('error') is None, 'strict verification passes for one readable file')
    signatures = files[0]['signatures']
    require(len(signatures) == 2, 'one document signature and one archive timestamp are present')
    signature, archive = signatures
    require(signature['field'] == field and signature.get('document_timestamp') is False
            and archive.get('document_timestamp') is True,
            'the selected signature is followed by a document timestamp')
    require(all(item['integrity']['verdict'] == 'intact' for item in signatures),
            'both signed revisions are intact')
    require(signature['covers_whole_file'] is False and signature['appended_bytes'] > 0
            and archive['covers_whole_file'] is True and archive['appended_bytes'] == 0,
            'the archive timestamp covers the final revision')
    for name, item in [('signer', signature), ('archive authority', archive)]:
        require((item.get('trust') or {}).get('standing') in ('trusted', 'trusted_at_timestamp'),
                name + ' is trusted')
    for name, item in [('signature timestamp', signature.get('timestamp')),
                       ('archive timestamp', archive.get('timestamp'))]:
        require(item is not None and item['attested'] is True
                and item['integrity']['verdict'] == 'intact'
                and (item.get('trust') or {}).get('standing') in ('trusted', 'trusted_at_timestamp'),
                name + ' is intact, attested and trusted')
    for name, item in [('signer', signature), ('timestamp authority', signature['timestamp'])]:
        require((item.get('revocation') or {}).get('standing') == 'good',
                name + ' has good embedded revocation evidence')
        chain = item.get('revocation_chain') or {}
        certificates = chain.get('certificates', [])
        require(chain.get('standing') == 'good' and chain.get('end') == 'root'
                and chain.get('dropped') == 0 and bool(certificates)
                and all(c['revocation']['standing'] == 'good' for c in certificates),
                name + ' has complete good evidence through its chain')
    return checks


def check_long_term(pdf: Tpdf, source: Path, directory: Path, identity: str,
                    timestamp: str) -> list[str]:
    """Opt-in network signing; any refusal ends the check without a fallback."""
    output = directory / 'long-term.pdf'
    original = source.read_bytes()
    signed = pdf.sign(source, output, identity=identity, timestamp=timestamp, long_term=True)
    if signed['identity']['id'] != identity or not output.read_bytes().startswith(original):
        raise RuntimeError('long-term: selected identity and original revision must be preserved')
    result = pdf.run('verify', '--strict', '--', output, check=False)
    (directory / 'long-term-verify.json').write_text(
        json.dumps(result.report, indent=2) + '\n', encoding='utf-8')
    if result.exit_code != 0:
        raise RuntimeError('long-term: strict verification failed; inspect long-term-verify.json')
    checks = long_term_checks(result.report, signed['field'])
    # Both the document signature and the archive must notice covered-byte changes.
    altered = directory / 'long-term-altered.pdf'
    content = output.read_bytes()
    if b'SYNTHETIC ORIGINAL' not in original:
        raise RuntimeError('long-term: tamper control source text is missing')
    altered.write_bytes(content.replace(b'SYNTHETIC ORIGINAL', b'SYNTHETIC XRIGINAL', 1))
    tampered = pdf.run('verify', '--strict', '--', altered, check=False)
    signatures = tampered.report['files'][0]['signatures']
    if tampered.exit_code != 1 or len(signatures) != 2 or any(
            item['integrity']['verdict'] != 'altered' for item in signatures):
        raise RuntimeError('long-term: tampering must invalidate both signed revisions')
    checks.append('long-term: covered-byte tampering invalidates both signed revisions')
    if source.read_bytes() != original:
        raise RuntimeError('long-term: the source changed')
    return checks


def check_signing(pdf: Tpdf, directory: Path, identity: str, *,
                  expect_untrusted: bool = False, timestamp: str | None = None,
                  long_term: bool = False) -> dict:
    if long_term and (not timestamp or expect_untrusted):
        raise ValueError('long-term checks require --timestamp and a trusted signer')
    checks = []

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise RuntimeError(message)
        checks.append(message)
        print('[OK] ' + message, flush=True)

    offered = pdf.identities()['usable']
    matches = [item for item in offered if identity in (item['id'], item['subject'])]
    require(len(matches) == 1, 'exactly one explicitly selected identity is usable')
    selected = matches[0]['id']
    source = directory / '-input.pdf'
    fixture(source)
    original = source.read_bytes()
    invisible = directory / 'invisible.pdf'
    visible = directory / 'visible.pdf'
    for path, options in [
        (invisible, {}),
        (visible, {'rect': [40, 40, 220, 80], 'page': 1, 'no_image': True,
                   'lines': ['label', 'name'], 'reason': 'Synthetic API approval',
                   'location': 'Synthetic API test'}),
    ]:
        report = pdf.sign(source, path, identity=selected, **options)
        require(report['identity']['id'] == selected, f'{path.name}: selected certificate was used')
        require(report['visible'] == bool(options), f'{path.name}: requested visibility is reported')
        require(path.read_bytes().startswith(original), f'{path.name}: original revision is preserved')
        verified = pdf.verify(path)
        signatures = verified['files'][0]['signatures']
        require(len(signatures) == 1 and signatures[0]['integrity']['verdict'] == 'intact'
                and signatures[0]['covers_whole_file'], f'{path.name}: readback is intact and covers the file')
        strict = pdf.run('verify', '--strict', '--', path, check=False)
        if expect_untrusted:
            require(strict.exit_code == 1 and not strict.report['strict_passed']
                    and signatures[0]['trust']['standing'] == 'untrusted',
                    f'{path.name}: self-signed identity remains untrusted')
        else:
            require(strict.exit_code == 0 and strict.report['strict_passed'],
                    f'{path.name}: strict verification passes')

    before = invisible.read_bytes()
    try:
        pdf.sign(source, invisible, identity=selected)
    except CommandError as error:
        require(error.exit_code == 3 and invisible.read_bytes() == before,
                'existing output is refused and preserved')
    else:
        raise RuntimeError('existing output was overwritten without force')

    require(b'SYNTHETIC ORIGINAL' in before, 'tamper control locates covered source text')
    altered = directory / 'altered.pdf'
    altered.write_bytes(before.replace(b'SYNTHETIC ORIGINAL', b'SYNTHETIC XRIGINAL', 1))
    tampered = pdf.run('verify', '--strict', '--', altered, check=False)
    require(tampered.exit_code == 1
            and tampered.report['files'][0]['signatures'][0]['integrity']['verdict'] == 'altered',
            'covered-byte tampering fails verification')

    images = []
    for path in (source, invisible, visible):
        png = directory / (path.stem + '.png')
        pdf.render(path, png, dpi=72)
        images.append(png.read_bytes())
    require(images[0] == images[1], 'invisible signing preserves rendered page')
    require(images[0] != images[2], 'visible signing changes rendered page')
    if timestamp is not None:
        stamped = directory / 'timestamped.pdf'
        pdf.sign(source, stamped, identity=selected, timestamp=timestamp)
        stamp = pdf.verify(stamped)['files'][0]['signatures'][0]['timestamp']
        require(stamp is not None and stamp['attested'] and stamp['integrity']['verdict'] == 'intact',
                'requested timestamp is present and intact')
    if long_term:
        for message in check_long_term(pdf, source, directory, selected, timestamp):
            checks.append(message)
            print('[OK] ' + message, flush=True)
    require(source.read_bytes() == original, 'source remains unchanged')
    workflow = directory / 'workflow'
    workflow.mkdir()
    for message in check_document_workflow(pdf, workflow, identity=selected):
        checks.append(message)
        print('[OK] ' + message, flush=True)
    return {'identity': selected, 'checks': checks, 'timestamp': timestamp, 'long_term': long_term,
            'files': {path.relative_to(directory).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in sorted(directory.rglob('*')) if path.is_file()}}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', required=True)
    parser.add_argument('--identity', required=True)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--expect-untrusted', action='store_true')
    parser.add_argument('--timestamp')
    parser.add_argument('--long-term', action='store_true',
                        help='also require successful CA-backed signing with revocation data and an archive timestamp')
    parser.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args()
    if args.long_term and (not args.timestamp or args.expect_untrusted):
        parser.error('--long-term requires --timestamp and cannot use --expect-untrusted')
    directory = args.output_dir.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    client = Tpdf(args.executable, timeout=args.timeout)
    report = check_signing(client, directory, args.identity,
                           expect_untrusted=args.expect_untrusted, timestamp=args.timestamp, long_term=args.long_term)
    (directory / 'receipt.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'[OK] {len(report["checks"])} OS-key signing checks passed', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
