#!/usr/bin/env python3
"""Independent soft-mask/sample-mapping readback through the contained worker.

uv run --with fonttools --with pypdf scripts/text_image_check.py <text-edit-probe> <new-directory>
Generates the original fixtures with testdata/make_textedit_alpha.py, edits text
beside each image, and checks the saved copy with a second parser: every image
and soft-mask stream byte-identical, every entry still there, and the text
changed. Then it damages one entry per fixture and requires a refusal, so a
fixture that stopped carrying the entry could not pass by doing nothing.
All inputs are synthetic; outputs belong in an ignored directory.
"""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, DictionaryObject, IndirectObject,
                           NameObject, NumberObject)

ROOT = Path(__file__).resolve().parents[1]
# Each fixture, the entries it exists to exercise, and the entries of its mask.
FIXTURES = {
    'alpha': (['/DecodeParms', '/Metadata', '/SMask'], ['/DecodeParms']),
    'palette': (['/Decode'], None),
    'masked': (['/Decode', '/SMask'], ['/DecodeParms']),
}


def resolve(value):
    return value.get_object() if isinstance(value, IndirectObject) else value


def image(reader):
    xobjects = resolve(resolve(reader.pages[0]['/Resources'])['/XObject'])
    (found,) = [resolve(v) for v in xobjects.values()
                if resolve(v).get('/Subtype') == '/Image']
    return found


def fingerprint(reader):
    found = image(reader)
    entry = {'keys': sorted(found.keys()), 'sha': hashlib.sha256(found._data).hexdigest()}
    if '/SMask' in found:
        mask = resolve(found.raw_get('/SMask'))
        entry['mask_keys'] = sorted(mask.keys())
        entry['mask_sha'] = hashlib.sha256(mask._data).hexdigest()
    return entry


def damage(source, target, entry):
    """Write a copy whose one interesting entry is no longer the accepted one."""
    writer = PdfWriter(clone_from=PdfReader(source))
    found = image(writer)
    if entry == '/SMask':
        # Alpha read as colour is a different image, not a stricter one.
        resolve(found.raw_get('/SMask'))[NameObject('/ColorSpace')] = NameObject('/DeviceRGB')
    elif entry == '/DecodeParms':
        target_dict = found if '/DecodeParms' in found else resolve(found.raw_get('/SMask'))
        resolve(target_dict.raw_get('/DecodeParms'))[NameObject('/Predictor')] = NumberObject(12)
    elif entry == '/Decode':
        found[NameObject('/Decode')] = ArrayObject([NumberObject(0), NumberObject(254)])
    elif entry == '/Metadata':
        resolve(found.raw_get('/Metadata'))[NameObject('/Type')] = NameObject('/XObject')
    writer.write(target)


def status(probe, source):
    result = subprocess.run([str(probe), '--inspect', str(source), '--all-pages'],
                            capture_output=True, text=True, encoding='utf-8', timeout=90)
    if result.returncode:
        raise SystemExit(f'[FAIL] inspection failed for {source.name}: {result.stderr.strip()}')
    report = json.loads(result.stdout)
    (page,) = report['pages']
    return page['status']


def run(probe, directory):
    directory.mkdir(parents=True, exist_ok=False)
    subprocess.run([sys.executable, str(ROOT / 'testdata/make_textedit_alpha.py'),
                    str(directory)], check=True, timeout=180)
    requests = directory / 'edit.json'
    requests.write_text(json.dumps(
        [dict(page=0, contains='SYNTHETIC FIRST', replacement='SYNTHETIC EDIT')]), encoding='utf-8')
    for name, (entries, mask_entries) in FIXTURES.items():
        source = directory / f'{name}.pdf'
        before = fingerprint(PdfReader(source))
        # The fixture control: a generator that stopped writing these entries
        # would leave every check below passing on an ordinary opaque image.
        missing = [entry for entry in entries if entry not in before['keys']]
        if missing or (mask_entries or []) != [e for e in (mask_entries or [])
                                               if e in before.get('mask_keys', [])]:
            raise SystemExit(f'[FAIL] {name}.pdf does not carry {missing or mask_entries}')
        output = directory / name
        subprocess.run([str(probe), '--roundtrip', str(source), str(requests), str(output)],
                       check=True, timeout=180)
        saved = PdfReader(output / 'edited.pdf')
        if fingerprint(saved) != before:
            raise SystemExit(f'[FAIL] {name}.pdf image or soft mask changed on save')
        text = saved.pages[0].extract_text()
        if 'SYNTHETIC EDIT' not in text or 'SYNTHETIC FIRST' in text:
            raise SystemExit(f'[FAIL] {name}.pdf text was not replaced')
        print(f'[OK] {name}.pdf: {", ".join(entries)} preserved; text edited beside the image')
        for entry in entries:
            broken = directory / f'{name}-{entry[1:].lower()}.pdf'
            damage(source, broken, entry)
            if status(probe, broken) != 'refused':
                raise SystemExit(f'[FAIL] {name}.pdf with a damaged {entry} was still editable')
        print(f'[OK] {name}.pdf: each of {", ".join(entries)} refused when it is not the accepted form')
    print(f'[PASS] {len(FIXTURES)} image fixtures preserved and independently read back')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    run(args.probe, args.directory)


if __name__ == '__main__':
    main()
