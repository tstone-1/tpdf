"""Opt-in OS-key integration check, separate from unattended test discovery.

Run from a checkout with a disposable document-signing identity already present:
  python api/python/check_signing.py --executable /path/to/tpdf-cli \
    --identity '<certificate SHA-256 or exact subject>' --output-dir scratch/sign-api \
    --expect-untrusted

The OS may prompt to allow key use. No key is imported, exported, or trusted here.
--timestamp explicitly enables a request to that authority. Outputs and a JSON
receipt are kept for independent inspection; the output directory must be new.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

from test_api import check_document_workflow, fixture
from tpdf import CommandError, Tpdf


def check_signing(pdf: Tpdf, directory: Path, identity: str, *,
                  expect_untrusted: bool = False, timestamp: str | None = None) -> dict:
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
    require(source.read_bytes() == original, 'source remains unchanged')
    workflow = directory / 'workflow'
    workflow.mkdir()
    for message in check_document_workflow(pdf, workflow, identity=selected):
        require(True, message)
    return {'identity': selected, 'checks': checks, 'timestamp': timestamp,
            'files': {path.relative_to(directory).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in sorted(directory.rglob('*')) if path.is_file()}}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', required=True)
    parser.add_argument('--identity', required=True)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--expect-untrusted', action='store_true')
    parser.add_argument('--timestamp')
    parser.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args()
    directory = args.output_dir.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    client = Tpdf(args.executable, timeout=args.timeout)
    report = check_signing(client, directory, args.identity,
                           expect_untrusted=args.expect_untrusted, timestamp=args.timestamp)
    (directory / 'receipt.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'[OK] {len(report["checks"])} OS-key signing checks passed', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
