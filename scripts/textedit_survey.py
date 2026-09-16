#!/usr/bin/env python3
"""Read-only, contained text-edit discovery across explicitly chosen PDFs.

python3 scripts/textedit_survey.py <text-edit-probe> <input.pdf> ... --output <report.json>
uv run --with pypdf scripts/textedit_survey.py <text-edit-probe> --self-test
Pass original producer exports, not a directory glob that mixes them with edited
outputs and deliberate negative controls. This measures discovery, not whether
arbitrary replacement text fits or whether a saved PDF renders correctly.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def validate(report):
    count, pages = report['page_count'], report['pages']
    if type(count) is not int or not 1 <= count <= 128 or len(pages) != count:
        raise ValueError('incomplete or oversized page report')
    for index, page in enumerate(pages):
        if type(page['page']) is not int or page['page'] != index:
            raise ValueError('missing, duplicated or reordered page')
        state = page['status']
        if state == 'refused':
            if not isinstance(page.get('reason'), str) or not page['reason']:
                raise ValueError('refused page lacks a reason')
        elif state in ('editable', 'no_runs'):
            runs = page['runs']
            if type(runs) is not int or runs < 0 or (runs > 0) != (state == 'editable'):
                raise ValueError('page status and run count disagree')
        else:
            raise ValueError('unknown page status')
    return report


def inspect(probe, source):
    before = digest(source)
    result = subprocess.run([str(probe), '--inspect', str(source), '--all-pages'],
                            capture_output=True, text=True, encoding='utf-8', timeout=60)
    if digest(source) != before:
        raise RuntimeError('inspection changed its source')
    if result.returncode:
        raise RuntimeError(f'inspection failed for {source}: {result.stderr.strip()}')
    report = validate(json.loads(result.stdout))
    return {'source': str(source), 'sha256': before, **report}


def self_test(probe):
    from pypdf import PdfWriter
    from pypdf.generic import (ArrayObject, DictionaryObject, NameObject,
                               NumberObject, DecodedStreamObject)
    with tempfile.TemporaryDirectory(prefix='tpdf-inspection-') as directory:
        root = Path(directory)

        def fixture(name, count):
            writer = PdfWriter()
            for index in range(count):
                page = writer.add_blank_page(width=300, height=240)
                font = DictionaryObject({NameObject('/Type'): NameObject('/Font'),
                    NameObject('/Subtype'): NameObject('/Type1'),
                    NameObject('/BaseFont'): NameObject('/Courier' if index == 1 else '/Helvetica')})
                page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):
                    DictionaryObject({NameObject('/F1'): writer._add_object(font)})})
                stream = DecodedStreamObject()
                stream.set_data(b'' if index == 2 else
                    b'BT /Span << /ActualText (SYNTHETIC SECRET) >> BDC ET' if index == 3 else
                    b'BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET')
                page[NameObject('/Contents')] = writer._add_object(stream)
            path = root / name
            writer.write(path)
            return path

        source = fixture('mixed.pdf', 5)
        original = digest(source)
        first = subprocess.run([str(probe), '--inspect', str(source)],
                               capture_output=True, text=True, encoding='utf-8', check=True, timeout=60)
        assert json.loads(first.stdout) == {'page': 0, 'status': 'editable', 'runs': 1}
        report = inspect(probe, source)
        assert [p['status'] for p in report['pages']] == ['editable', 'refused', 'no_runs', 'refused', 'editable']
        assert report['pages'][3]['reason'] == 'inline BDC marked content is not editable yet'
        assert 'SECRET' not in json.dumps(report)
        totals = refusal_totals([report, report])
        assert sum(totals.values()) == 4 and totals[report['pages'][3]['reason']] == 2
        assert refusal_totals([]) == {}
        assert digest(source) == original
        assert len(list(root.iterdir())) == 1, 'inspection wrote extra files'
        # Exercise the real worker reply, including the privacy boundary on
        # unknown metadata names. Unknown keys and all values must stay private.
        for key, feature in [('IDTree', 'IDTree'), ('ClassMap', 'ClassMap'),
                             ('SYNTHETIC_SECRET', 'unrecognized')]:
            writer = PdfWriter()
            writer.add_blank_page(width=300, height=240)
            tree = DictionaryObject({NameObject('/Type'): NameObject('/StructTreeRoot'),
                                     NameObject('/' + key): NameObject('/SYNTHETIC_SECRET')})
            writer.root_object[NameObject('/StructTreeRoot')] = writer._add_object(tree)
            tagged = root / f'tagged-{key}.pdf'
            writer.write(tagged)
            tagged_report = inspect(probe, tagged)
            assert tagged_report['pages'] == [{'page': 0, 'status': 'refused',
                'reason': f'unsupported {feature} metadata in tagged structure root'}]
            assert 'SECRET' not in json.dumps(tagged_report['pages'])
        # Author a complete tagged page with an independent PDF writer. The
        # custom-role control must be editable; standard types may not use the
        # same alias to disguise their content as a supported paragraph.
        def dictionary(**items):
            return DictionaryObject({NameObject('/' + key): value for key, value in items.items()})

        base = fixture('role-base.pdf', 1)
        for role in ['SyntheticParagraph', 'Figure', 'Table', 'Span', 'Link']:
            writer = PdfWriter(clone_from=base)
            page = writer.pages[0]
            tree = writer._add_object(dictionary(Type=NameObject('/StructTreeRoot')))
            document = writer._add_object(dictionary(Type=NameObject('/StructElem'),
                S=NameObject('/Document'), P=tree))
            paragraph = writer._add_object(dictionary(Type=NameObject('/StructElem'),
                S=NameObject('/' + role), P=document, Pg=page.indirect_reference, K=NumberObject(0)))
            document.get_object()[NameObject('/K')] = paragraph
            tree.get_object().update(dictionary(K=document,
                RoleMap=dictionary(**{role: NameObject('/P')}),
                ParentTree=writer._add_object(dictionary(Nums=ArrayObject([
                    NumberObject(0), ArrayObject([paragraph])])))))
            writer.root_object[NameObject('/StructTreeRoot')] = tree
            page[NameObject('/StructParents')] = NumberObject(0)
            stream = DecodedStreamObject()
            stream.set_data(b'/' + role.encode('ascii') + b' << /MCID 0 >> BDC\n' +
                            page.get_contents().get_data() + b'\nEMC')
            page[NameObject('/Contents')] = writer._add_object(stream)
            path = root / f'role-{role}.pdf'
            writer.write(path)
            result = inspect(probe, path)['pages']
            if role == 'SyntheticParagraph':
                assert result == [{'page': 0, 'status': 'editable', 'runs': 1}]
            else:
                assert result == [{'page': 0, 'status': 'refused',
                    'reason': 'tagged RoleMap contains unsupported or conflicting roles'}]
        assert len(inspect(probe, fixture('boundary.pdf', 128))['pages']) == 128
        too_many = fixture('oversized.pdf', 129)
        for args, reason in [([str(too_many), '--all-pages'], '1 to 128 pages'),
                             ([str(source), '--unknown'], 'usage:'),
                             ([str(root / 'absent.pdf'), '--all-pages'], 'could not open')]:
            failed = subprocess.run([str(probe), '--inspect', *args],
                                    capture_output=True, text=True, encoding='utf-8', timeout=60)
            assert failed.returncode != 0 and not failed.stdout.strip() and reason in failed.stderr
        # Prove the collector cannot report success over missing or duplicated pages.
        for mutate in (lambda r: r['pages'].pop(),
                       lambda r: r['pages'][3].update(page=0),
                       lambda r: r['pages'][0].update(runs=0)):
            bad = json.loads(json.dumps(report))
            mutate(bad)
            try:
                validate(bad)
            except ValueError:
                pass
            else:
                raise AssertionError('invalid report passed validation')
    print('[PASS] later-page refusal, tagged metadata privacy, role-map semantics, empty page, continued discovery, page bound and incomplete-report controls')


def refusal_totals(records):
    # These count the first refusal on each page, not every unsupported construct.
    totals = {}
    for record in records:
        for page in record['pages']:
            if page['status'] == 'refused':
                reason = page['reason']
                totals[reason] = totals.get(reason, 0) + 1
    return dict(sorted(totals.items(), key=lambda item: (-item[1], item[0])))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('pdfs', nargs='*', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    probe = args.probe.resolve(strict=True)
    if args.self_test:
        if args.pdfs or args.output:
            parser.error('--self-test does not accept inputs or an output path')
        self_test(probe)
        return
    if not args.pdfs or args.output is None:
        parser.error('provide explicit PDFs and --output')
    if args.output.exists():
        parser.error('output already exists; choose a new report path')
    if len({p.resolve() for p in args.pdfs}) != len(args.pdfs):
        parser.error('duplicate input path')
    records = [inspect(probe, source) for source in args.pdfs]
    states = ['editable', 'refused', 'no_runs']
    counts = {state: sum(page['status'] == state for r in records for page in r['pages'])
              for state in states}
    output = {'probe_sha256': digest(probe), 'documents': records, 'page_totals': counts,
              'refusal_totals': refusal_totals(records)}
    # No partial report on a missing file, worker failure or malformed reply.
    args.output.write_text(json.dumps(output, indent=2) + '\n', encoding='utf-8')
    print(f'[PASS] {len(records)} documents, {sum(counts.values())} pages: {counts}')


if __name__ == '__main__':
    main()
