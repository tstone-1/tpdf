#!/usr/bin/env python3
"""How often a longer replacement is refused, and why, across chosen PDFs.

python3 scripts/textedit_growth.py <text-edit-probe> <input.pdf> ... --output <report.json>
    [--manifest testdata/textedit-public-corpus.json] [--agree-every N] [--jobs N]
uv run --with pypdf scripts/textedit_growth.py <text-edit-probe> --self-test
python3 scripts/textedit_growth.py <text-edit-probe> --compare <before-records> <after-records>

Drives `text-edit-probe --growth` once per file (see src/probes/text_edit_growth.rs for
the trials and modes) and aggregates the verdicts. Refusal messages are sorted into the
categories in CATEGORIES; anything unmatched is counted under `other` with its message,
so nothing is hidden. The report carries no document text.

What the numbers are: for every editable run with visible text, whether the editor
would accept the same text with two letters swapped, a quarter shorter, and 10/25/50%
longer (using only the run's own characters), with the box the editor opens (`app`,
which since 26.9.15 follows the typed text into the room after the line), with no
layout (`patch`), and with the box widened by hand until its own width is no longer
the objection (`widened`, which keeps `grow` cleared so the column stays comparable
across that change). What they are not: a population rate (the sample is
chosen to cover producers), a claim about saving (see `--roundtrip`), or a model of
text columns. Pages beyond the 128th are not inspected.

`--records <new directory>` keeps each file's raw verdicts (operators and verdicts, no
text), and `--compare` lists every verdict that was `ok` in the first set of records and
is not in the second, run by run, so a change that moves the totals up cannot hide one
that moved individual runs down.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time

# First match wins. `box_width` is the box's (or the original advance's) own width:
# the only refusal widening the box can answer. `line_full` is the same shortage
# reported by a box that already grew to the room it had, and widening that one
# by hand cannot help, which is why it is a category of its own rather than more
# needles under `box_width` -- the ladder in the probe would climb past the thing
# that stopped it. The message names the cause (a neighbour, the page edge, a
# clip); the category does not, so read `reasons` when that matters.
CATEGORIES = [
    ('line_full', ('no room for more text on this line',)),
    ('box_width', ('exceeds the box width', 'ink exceeds the box',
                   'exceed the original text advance', 'exceed the original text bounds')),
    ('overlap', ('would overlap another line',)),
    ('page_edge', ('extends beyond the page',)),
    ('clip', ('clips this area', 'partly clipped')),
    ('box_height', ('exceeds the box height',)),
    ('precision', ('annot preserve',)),
    ('glyph', ('no validated glyph', 'unmapped font code', 'several glyphs',
               'shows spaces as gaps')),
]
# The `widened` mode clears `grow`, so `line_full` cannot appear in a widened
# cell; it is listed here because it is the same shortage under another name.
NO_ROOM = ('line_full', 'overlap', 'page_edge', 'clip')
TRIALS = ['identity', 'control', 'shrink25', 'grow10', 'grow25', 'grow50']
GROWTHS = ['grow10', 'grow25', 'grow50']
LONG = 20  # characters; a run at least this long is counted as line-like too


def classify(verdict):
    if verdict == 'ok':
        return 'ok'
    for name, needles in CATEGORIES:
        if any(needle in verdict for needle in needles):
            return name
    return 'other'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def measure(probe, source, agree_every):
    before = digest(source)
    started = time.monotonic()
    result = subprocess.run([str(probe), '--growth', str(source), f'--agree-every={agree_every}'],
                            capture_output=True, text=True, encoding='utf-8', timeout=7200)
    if digest(source) != before:
        raise RuntimeError(f'measurement changed its source: {source}')
    if result.returncode:
        raise RuntimeError(f'measurement failed for {source}: {result.stderr.strip()}')
    report = json.loads(result.stdout)
    pages = report['pages']
    if len(pages) != report['pages_inspected'] or [p['page'] for p in pages] != list(range(len(pages))):
        raise ValueError(f'incomplete page report for {source}')
    for page in pages:
        if page['status'] == 'editable':
            for run in page['tried']:
                for trial in TRIALS:
                    if trial not in run:
                        raise ValueError(f'run without trial {trial} in {source}')
    return {'source': str(source), 'sha256': before,
            'seconds': round(time.monotonic() - started, 1), **report}


def empty():
    return {'runs': 0, **{t: {} for t in TRIALS},
            **{f'{g}_widened': {} for g in GROWTHS}, 'other_reasons': {}, 'reasons': {},
            'ceiling': {}}


def count(bucket, key):
    bucket[key] = bucket.get(key, 0) + 1


def tally(record, into, long_into):
    for page in record['pages']:
        if page['status'] != 'editable':
            continue
        for run in page['tried']:
            targets = [into] + ([long_into] if run['chars'] >= LONG else [])
            for target in targets:
                target['runs'] += 1
            for trial in TRIALS:
                result = run[trial]
                verdicts = [] if result is None else [
                    (trial, mode, result[mode]) for mode in ('app', 'patch') if mode in result]
                if result is not None and 'widened' in result:
                    verdicts.append((f'{trial}_widened', 'app', result['widened']['verdict']))
                if result is None:
                    for target in targets:
                        count(target[trial], 'not_applicable')
                for key, mode, verdict in verdicts:
                    category = classify(verdict)
                    for target in targets:
                        bucket = target[key].setdefault(mode, {})
                        count(bucket, category)
                        if category == 'other':
                            count(target['other_reasons'], verdict)
                        if category != 'ok':
                            count(target['reasons'].setdefault(f'{key}/{mode}', {}), verdict)
            # The reflow ceiling, from the geometry of the discovered runs only.
            widened = run['grow25'] and run['grow25'].get('widened')
            if widened and run['axis_aligned']:
                extension = widened['width'] - run['width']
                category = classify(widened['verdict'])
                if category == 'ok':
                    where = ('ok_inside_text_extent'
                             if run['right'] + extension <= run['extent_right'] + 0.5
                             else 'ok_past_rightmost_text')
                elif category == 'overlap':
                    where = ('overlap_same_line_neighbour'
                             if run['gap_right'] is not None and run['gap_right'] < extension
                             else 'overlap_other_content')
                else:
                    where = category
                for target in targets:
                    count(target['ceiling'], where)


def pct(part, whole):
    return f'{100 * part / whole:.0f}' if whole else '-'


def row(name, stats):
    """Rates over the runs a trial applies to; a run too short or too uniform for a
    same-length change has no control, and is not counted as a refusal."""
    n = stats['runs']
    cells = []
    for trial in ('identity', 'control', 'shrink25', 'grow25'):
        app = stats[trial].get('app', {})
        cells.append(pct(app.get('ok', 0), sum(app.values())))
    for g in GROWTHS:
        w = stats[f'{g}_widened'].get('app', {})
        total = sum(w.values())
        room = sum(w.get(c, 0) for c in NO_ROOM)
        cells.append(f"{pct(w.get('ok', 0), total)} / {pct(room, total)} / "
                     f"{pct(total - w.get('ok', 0) - room, total)}")
    return f'| {name} | {n} | ' + ' | '.join(cells) + ' |'


def breakdown(stats):
    """Counts, not rates: every trial and mode against every category."""
    names = ['ok'] + [name for name, _ in CATEGORIES] + ['other']
    lines = ['| Trial | Mode | Tried | ' + ' | '.join(names) + ' |',
             '|---|---|---:|' + '---:|' * len(names)]
    for trial in TRIALS:
        keys = [(trial, 'app')] + ([] if trial == 'identity' else [(trial, 'patch')])
        if trial in GROWTHS:
            keys.append((f'{trial}_widened', 'app'))
        for key, mode in keys:
            bucket = stats[key].get(mode, {})
            label = 'widened' if key.endswith('_widened') else mode
            lines.append(f'| {trial} | {label} | {sum(bucket.values())} | '
                         + ' | '.join(str(bucket.get(c, 0)) for c in names) + ' |')
    return '\n'.join(lines)


def aggregate(records, manifest):
    producers = {}
    if manifest:
        data = json.loads(Path(manifest).read_text(encoding='utf-8'))
        for group in ('files', 'followup_files', 'external_test_files', 'prototype_files',
                      'expansion_files'):
            for entry in data.get(group, []):
                producers[entry['filename']] = entry.get('producer', entry['title'])
    files, total, total_long = [], empty(), empty()
    for record in records:
        stats, long_stats = empty(), empty()
        tally(record, stats, long_stats)
        tally(record, total, total_long)
        pages = record['pages']
        files.append({'file': Path(record['source']).name,
                      'producer': producers.get(Path(record['source']).name, ''),
                      'pages_inspected': len(pages),
                      'pages_editable': sum(p['status'] == 'editable' for p in pages),
                      'seconds': record['seconds'],
                      'agreement_checked': record['agreement']['checked'],
                      'agreement_disagreements': record['agreement']['disagreements'],
                      'all': stats, 'long': long_stats})
    return {'files': files, 'total': total, 'total_long': total_long,
            'agreement_checked': sum(f['agreement_checked'] for f in files),
            'agreement_disagreements': sum(len(f['agreement_disagreements']) for f in files)}


def table(summary):
    lines = ['| File | Runs | Unchanged ok % | Same length ok % | 25% shorter ok % | '
             '+25% as typed ok % | +10% widened: ok / no room / other % | +25% widened | '
             '+50% widened |',
             '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for f in summary['files']:
        lines.append(row(f['file'], f['all']))
    lines.append(row('**All runs**', summary['total']))
    lines.append(row(f'**Runs of {LONG}+ characters**', summary['total_long']))
    return '\n'.join(lines)


def self_test(probe):
    from pypdf import PdfWriter
    from pypdf.generic import DictionaryObject, NameObject, DecodedStreamObject
    with tempfile.TemporaryDirectory(prefix='tpdf-growth-') as directory:
        root = Path(directory)
        writer = PdfWriter()
        page = writer.add_blank_page(width=300, height=240)
        font = DictionaryObject({NameObject('/Type'): NameObject('/Font'),
            NameObject('/Subtype'): NameObject('/Type1'),
            NameObject('/BaseFont'): NameObject('/Helvetica'),
            NameObject('/Encoding'): NameObject('/WinAnsiEncoding')})
        page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):
            DictionaryObject({NameObject('/F1'): writer._add_object(font)})})
        stream = DecodedStreamObject()
        # Free: a short word at the start of a wide line. Edge: a line ending 3pt
        # from the right edge. Neighbour: a line followed 1pt later by more text.
        stream.set_data(b'BT /F1 12 Tf 20 200 Td (FREEWORD) Tj ET\n'
                        b'BT /F1 12 Tf 225 150 Td (EDGEWORD) Tj ET\n'
                        b'BT /F1 12 Tf 20 100 Td (BOXED) Tj ET\n'
                        b'BT /F1 12 Tf 64 100 Td (NEXT) Tj ET\n'
                        b'BT /F1 12 Tf 20 40 Td [(KER) 80 (NED) 80 (RUN)] TJ ET')
        page[NameObject('/Contents')] = writer._add_object(stream)
        path = root / 'growth.pdf'
        writer.write(path)
        report = measure(probe, path, 1)
        assert report['agreement']['checked'] > 0 and not report['agreement']['disagreements']
        tried = report['pages'][0]['tried']
        assert sorted(r['chars'] for r in tried) == [4, 5, 8, 8, 9]
        kerned = next(r for r in tried if r['chars'] == 9)
        # The box the editor opens is the run's own advance, kerns included, and the
        # layout keeps the source's kerns around a change (until 26.9.14 it laid the
        # run out again from glyph widths and refused both of these as box width).
        assert classify(kerned['identity']['app']) == 'ok', kerned['identity']
        assert classify(kerned['control']['app']) == 'ok', kerned['control']
        assert classify(kerned['control']['patch']) == 'ok'
        assert classify(kerned['shrink25']['patch']) == 'ok'
        free = next(r for r in tried if r['right'] < 150 and r['chars'] == 8)
        edge = next(r for r in tried if r['right'] > 250)
        boxed = next(r for r in tried if r['chars'] == 5)
        assert boxed['gap_right'] is not None and free['gap_right'] is None
        for run in (free, edge, boxed):
            assert classify(run['identity']['app']) == 'ok' and 'patch' not in run['identity']
            assert classify(run['control']['app']) == 'ok'
            assert classify(run['shrink25']['app']) == 'ok'
            for g in GROWTHS:
                # The byte-patch writer has the source's own advance and no box
                # to grow, so it refuses every longer edit whatever the line has.
                assert classify(run[g]['patch']) == 'box_width', run[g]['patch']
        # A box the editor opens grows into the room the line has. FREEWORD has
        # 280 pt of it; EDGEWORD ends 3 pt from the page and BOXED 2 pt short of
        # NEXT, and one more character of either is 8 pt. The message has to name
        # which of the two stopped it, or the reader is told the line is full and
        # cannot see what filled it.
        for g in GROWTHS:
            assert classify(free[g]['app']) == 'ok', free[g]['app']
            assert classify(kerned[g]['app']) == 'ok', kerned[g]['app']
            assert classify(edge[g]['app']) == 'line_full', edge[g]['app']
            assert 'edge of the page' in edge[g]['app'], edge[g]['app']
            assert classify(boxed[g]['app']) == 'line_full', boxed[g]['app']
            assert 'other text follows it' in boxed[g]['app'], boxed[g]['app']
        assert [free[g]['added'] for g in GROWTHS] == [1, 2, 4]
        assert all(classify(free[g]['widened']['verdict']) == 'ok' for g in GROWTHS)
        assert all(classify(edge[g]['widened']['verdict']) == 'page_edge' for g in GROWTHS)
        assert all(classify(boxed[g]['widened']['verdict']) == 'overlap' for g in GROWTHS)
        summary = aggregate([report], None)
        assert summary['total']['ceiling'] == {'ok_inside_text_extent': 3, 'page_edge': 1,
                                               'overlap_same_line_neighbour': 1}, summary['total']['ceiling']
        # The collector must not accept a report with a trial missing.
        broken = json.loads(json.dumps(report))
        del broken['pages'][0]['tried'][0]['grow25']
        try:
            tally(broken, empty(), empty())
        except (KeyError, TypeError):
            pass
        else:
            raise AssertionError('a missing trial was counted')
        assert classify('something new') == 'other'
    print('[PASS] kerned run accepted unchanged and swapped in the default box, a box the editor opens grows into a free line and is refused at the page edge '
          'and at a neighbour by name, patch refuses every longer edit, a box widened by hand reaches the same ceiling, worker agrees, missing trials refused')


def verdicts(record):
    """Every (page, operator, trial, mode) and its verdict, from one raw record."""
    found = {}
    for page in record['pages']:
        if page['status'] != 'editable':
            continue
        for run in page['tried']:
            for trial in TRIALS:
                result = run[trial]
                if result is None:
                    continue
                for mode in ('app', 'patch'):
                    if mode in result:
                        found[(page['page'], run['operator'], trial, mode)] = result[mode]
                if 'widened' in result:
                    found[(page['page'], run['operator'], trial, 'widened')] = \
                        result['widened']['verdict']
    return found


def compare(before_dir, after_dir):
    """Verdicts accepted before and not after, and the counts moved each way."""
    before_files = sorted(p.name for p in before_dir.glob('*.json'))
    after_files = sorted(p.name for p in after_dir.glob('*.json'))
    if not before_files or before_files != after_files:
        raise SystemExit(f'record sets differ: {before_files} against {after_files}')
    regressions, gained, same = [], 0, 0
    for name in before_files:
        old = json.loads((before_dir / name).read_text(encoding='utf-8'))
        new = json.loads((after_dir / name).read_text(encoding='utf-8'))
        if old['sha256'] != new['sha256']:
            raise SystemExit(f'{name}: the records describe different source bytes')
        a, b = verdicts(old), verdicts(new)
        if a.keys() != b.keys():
            raise SystemExit(f'{name}: the records tried different runs or trials')
        for key, verdict in a.items():
            if verdict == 'ok' and b[key] != 'ok':
                regressions.append((name, *key, b[key]))
            elif verdict != 'ok' and b[key] == 'ok':
                gained += 1
            else:
                same += 1
    for regression in regressions:
        print('[REGRESSION]', json.dumps(regression))
    print(f'[COMPARE] {len(before_files)} files, {same} verdicts unchanged in kind, '
          f'{gained} refused before and accepted now, {len(regressions)} accepted before and refused now')
    return 1 if regressions else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('pdfs', nargs='*', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--agree-every', type=int, default=97)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--self-test', action='store_true')
    parser.add_argument('--records', type=Path)
    parser.add_argument('--compare', nargs=2, type=Path)
    args = parser.parse_args()
    if args.compare:
        raise SystemExit(compare(*args.compare))
    probe = args.probe.resolve(strict=True)
    if args.self_test:
        self_test(probe)
        return
    if not args.pdfs or args.output is None:
        parser.error('provide explicit PDFs and --output')
    if args.output.exists():
        parser.error('output already exists; choose a new report path')
    if args.records is not None:
        args.records.mkdir()  # new, so a stale record cannot enter a comparison
    started = time.monotonic()
    with ThreadPoolExecutor(args.jobs) as pool:
        records = list(pool.map(lambda p: measure(probe, p, args.agree_every), args.pdfs))
    if args.records is not None:
        for record in records:
            path = args.records / (Path(record['source']).stem + '.json')
            path.write_text(json.dumps(record) + '\n', encoding='utf-8')
    summary = aggregate(records, args.manifest)
    summary['probe_sha256'] = digest(probe)
    summary['wall_seconds'] = round(time.monotonic() - started, 1)
    summary['sources'] = [{'file': Path(r['source']).name, 'sha256': r['sha256']} for r in records]
    args.output.write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(table(summary))
    print()
    print(breakdown(summary['total']))
    print()
    print('Reflow ceiling at +25% (axis-aligned runs):', json.dumps(summary['total']['ceiling']))
    print('Unclassified refusals:', json.dumps(summary['total']['other_reasons']))
    print(f"[DONE] {len(records)} documents, {summary['total']['runs']} runs, "
          f"{summary['agreement_checked']} worker agreement checks, "
          f"{summary['agreement_disagreements']} disagreements, {summary['wall_seconds']} s")


if __name__ == '__main__':
    main()
