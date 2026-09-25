#!/usr/bin/env python3
"""What a paragraph model would have to work with, aggregated over chosen PDFs.

python3 scripts/textedit_blocks.py <text-edit-probe> <input.pdf> ... --output <report.json>
    [--manifest testdata/textedit-public-corpus.json] [--records <new dir>]
    [--agree-every N] [--jobs N]
python3 scripts/textedit_blocks.py <probe> --report <records dir> --output <report.json>
    [--manifest ...] [--rule signals|all|none]
uv run --with pypdf scripts/textedit_blocks.py <text-edit-probe> --self-test

Drives `text-edit-probe --blocks` once per file (see `src/probes/text_edit_blocks.rs`
for what it emits) and answers four questions about wrapping an edit onto a new line:

1. Of the edits a longer replacement is still refused for, how many are refused because
   the text would leave the **page** -- which is what wrapping is for -- against refused
   by a **neighbour** the line push cannot move far enough, against refused for something
   wrapping cannot help with at all.
2. Whether the page **states** the block: a structure tree, a parent-tree slot per MCID,
   and an element that owns the run. Reported as shares, tagged against untagged.
3. What a **geometric** block rule would have to work with on the untagged pages: the
   distributions of the signals (line pitch, left-edge agreement, font and size
   agreement, whether anything is painted between two lines), and then one candidate
   rule measured **against the tagged pages, where the answer is known** -- every pair of
   consecutive lines the rule joins that the tags put in different blocks is a false
   positive, and its shape is reported, not only its count.
4. What is **below** a block's last line, from the probe's render of the page, and how
   often a block could take one more line without landing on it.

The rule lives here rather than in the probe, so that the instrument cannot be the
evidence for its own rule, and `--rule all` / `--rule none` replace it with "everything
is one paragraph" and "nothing is" -- two mutations `--self-test` requires to move the
numbers, because a classifier whose output does not depend on its rule is measuring
nothing. The report carries no document text.

What these numbers are not: a population rate (the sample is chosen to cover producers),
a claim that a joined pair would reflow correctly, or a model of columns -- a line here is
split at a horizontal gap wider than `GUTTER_EM`, which is a guess about gutters and is
reported as one.
"""
import argparse
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

TRIALS = ['grow10', 'grow25', 'grow50']

# First match wins. The five `line_full` reasons are the editor's own `Room`, and they
# are the whole of question 1: `page_edge` is the population wrapping exists for, the
# three `neighbour_*` are what the line push met and could not move, and `clip` is
# neither -- the document itself has cut the space off.
STOPS = [
    ('page_edge', 'it reaches the edge of the page'),
    ('clip', 'the document clips the space after it'),
    ('neighbour_text', 'other text follows it'),
    ('neighbour_unmovable', 'the text after it cannot be moved'),
    ('neighbour_drawing', 'a picture or a drawing follows it'),
]
NEIGHBOUR = ('neighbour_text', 'neighbour_unmovable', 'neighbour_drawing')
OTHER = [
    ('box_width', ('exceeds the box width', 'ink exceeds the box',
                   'exceed the original text advance', 'exceed the original text bounds')),
    ('box_height', ('exceeds the box height',)),
    ('overlap', ('would overlap another line',)),
    ('precision', ('annot preserve',)),
    ('glyph', ('no validated glyph', 'unmapped font code', 'several glyphs',
               'shows spaces as gaps', 'does not contain a required character')),
]

# An element that describes a phrase rather than a block: the block that owns the *line*
# is one of its ancestors. ISO 32000-1 Table 333's inline-level types, plus the two
# neutral containers and the grouping types a leaf can sit directly inside.
INLINE_ROLES = {'Span', 'Quote', 'Note', 'Reference', 'BibEntry', 'Code', 'Link',
                'Annot', 'Ruby', 'Warichu', 'RB', 'RT', 'RP', 'WT', 'WP',
                'Em', 'Strong', 'NonStruct', 'Private', 'Artifact'}

# The candidate rule's constants, all in the larger of the two lines' font sizes except
# where a point is stated. Each is a guess, and the distributions below are what say
# whether it is a good one.
BASELINE_TOL = 0.5      # points; two runs are on one line when their baselines agree
GUTTER_EM = 2.0         # a horizontal gap this wide inside a line splits it
SHIFT_EM = 0.5          # a run this far off a fuller line's baseline is on that line
LEFT_TOL = 1.0          # points; left edges of two lines of one block
INDENT_MAX_EM = 4.0     # a first line may start this far right of the rest
PITCH_MAX_EM = 3.0      # a plausible line pitch, as a multiple of the font size
SKIP_PITCH_EM = 2.0     # the widest pitch of a pair with another line of the page between
LEAD_TOL = 0.5          # points; how far a pitch may sit from the block's first


def classify(verdict):
    """One refusal's category. `ok` and the five `Room` reasons are exact strings from
    `layout.rs`; everything else is sorted by needle, and anything unmatched is `other`
    with its message kept, so no refusal is hidden by being unrecognised."""
    if verdict == 'ok':
        return 'ok'
    if 'no room for more text on this line' in verdict:
        for name, needle in STOPS:
            if needle in verdict:
                return name
        return 'line_full_unnamed'
    for name, needles in OTHER:
        if any(needle in verdict for needle in needles):
            return name
    return 'other'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def measure(probe, source, agree_every):
    """One document through the probe, with the source proved unchanged either side."""
    before = digest(source)
    started = time.monotonic()
    result = subprocess.run([str(probe), '--blocks', str(source), f'--agree-every={agree_every}'],
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


# ---------------------------------------------------------------------------
# Lines, segments and blocks.
# ---------------------------------------------------------------------------

def baseline(run):
    """The run's baseline in the displayed page, y downwards.

    `app_layout` gives the box the height `size` above the baseline and the rest below,
    and `free_width` shapes it from `run.size` down, so the box's top edge is exactly one
    font size above the baseline whatever `minimum_height` does to its depth."""
    return run['rect'][1] + run['size']


def usable(run):
    return run['axis_aligned'] and 'ink_below' in run


def block_element(run, elements):
    """The nearest ancestor of the run's owning element, itself included, that is not an
    inline-level type -- the element that owns the *line* rather than a phrase in it.

    `None` where the run is untagged or the chain runs out, which is reported as unknown
    rather than folded into either answer."""
    current = run.get('element')
    for _ in range(32):
        if current is None:
            return None
        entry = elements.get(current)
        if entry is None:
            return current
        if entry['role'] not in INLINE_ROLES:
            return current
        current = entry['parent']
    return None


class Segment:
    """One piece of one line: the runs on a shared baseline with no wide gap between."""

    def __init__(self, index, runs, elements):
        # Its position in the page's own list, so that an edge between two segments is
        # a pair of numbers rather than two object identities: `lines` is called more
        # than once per page, and identities do not survive that.
        self.index = index
        self.runs = runs
        self.baseline = baseline(runs[0])
        self.left = min(r['rect'][0] for r in runs)
        self.right = max(r['rect'][2] for r in runs)
        self.bottom = max(r['rect'][3] for r in runs)
        self.size = max(r['size'] for r in runs)
        self.font = runs[0]['font']
        self.one_font = len({r['font'] for r in runs}) == 1
        self.one_size = max(r['size'] for r in runs) - min(r['size'] for r in runs) <= 0.01
        self.ink_below = min(r['ink_below'] for r in runs)
        self.page_edge_below = all(r['below_is_page_edge'] for r in runs)
        blocks = {block_element(r, elements) for r in runs}
        self.tagged = None not in blocks
        self.blocks = blocks if self.tagged else set()

    def stops(self, trial):
        return Counter(classify(r[trial]) for r in self.runs)


def shifted(groups):
    """The baseline groups with every group set off another merged into it,
    top down, each as its runs and, for a group that kept another, the keeping group's
    baseline, which its segments take (`None` for any other, whose segments keep their
    own). A group that kept another has its runs in order along the line, which the
    gutter split reads; any other keeps the order it had.

    A group is set off another when it has fewer characters, its baseline is within
    `SHIFT_EM` of the other's, and none of its runs is a gutter or more clear of the
    other's extent: a superscript, a footnote mark, the lowered E of the TeX logo. The
    other need not be the next baseline: another mark, or a staggered column's line, can
    lie between.
    The rule in `blocks.rs` counts characters other than spaces; the records carry only
    `chars`, spaces included."""
    counts = [sum(r['chars'] for r in g) for g in groups]

    def fits(index, other):
        host = groups[other]
        size = max(r['size'] for r in host)
        left, right = min(r['rect'][0] for r in host), max(r['rect'][2] for r in host)
        gutter = GUTTER_EM * size
        return (counts[other] > counts[index]
                and abs(baseline(host[0]) - baseline(groups[index][0])) <= SHIFT_EM * size
                and all(r['rect'][0] - right <= gutter and left - r['rect'][2] <= gutter
                        for r in groups[index]))

    hosts = []
    for index in range(len(groups)):
        near = [o for o in range(len(groups)) if fits(index, o)]
        near.sort(key=lambda o: abs(baseline(groups[o][0]) - baseline(groups[index][0])))
        hosts.append(near[0] if near else None)
    merged = {}
    for index in range(len(groups)):
        root = index
        while hosts[root] is not None:
            root = hosts[root]
        merged.setdefault(root, []).append(index)
    return [(None, groups[root]) if len(members) == 1
            else (baseline(groups[root][0]),
                  sorted((r for m in members for r in groups[m]), key=lambda r: r['rect'][0]))
            for root, members in sorted(merged.items())]


def lines(page):
    """The page's axis-aligned runs as lines of segments, in reading order down the page.

    A line is a maximal set of runs whose baselines agree to `BASELINE_TOL`; it is then
    split wherever the horizontal gap to the next fragment exceeds `GUTTER_EM` of the
    larger size, which is the rule's only guess about columns and tab stops."""
    elements = page.get('elements') or {}
    runs = sorted((r for r in page['tried'] if usable(r)),
                  key=lambda r: (baseline(r), r['rect'][0]))
    out = []
    group = []
    for run in runs:
        if group and baseline(run) - baseline(group[0]) > BASELINE_TOL:
            out.append(group)
            group = []
        group.append(run)
    if group:
        out.append(group)
    result, index = [], 0
    for line_baseline, group in shifted(out):
        pieces, current = [], [group[0]]
        for run in group[1:]:
            gap = run['rect'][0] - max(r['rect'][2] for r in current)
            if gap > GUTTER_EM * max(run['size'], max(r['size'] for r in current)):
                pieces.append(current)
                current = []
            current.append(run)
        pieces.append(current)
        row = []
        for piece in pieces:
            segment = Segment(index, piece, elements)
            if line_baseline is not None:
                segment.baseline = line_baseline
            row.append(segment)
            index += 1
        result.append(row)
    return result


def signals(before, after):
    """Every number the candidate rule reads about one pair of consecutive segments."""
    size = max(before.size, after.size)
    pitch = after.baseline - before.baseline
    first_ink = before.bottom + before.ink_below
    return {
        'size': size,
        'pitch': pitch,
        'pitch_em': pitch / size if size else math.inf,
        'left_delta': after.left - before.left,
        'same_font': before.font == after.font and before.one_font and after.one_font,
        'same_size': abs(before.size - after.size) <= 0.01,
        # Reported, and deliberately not read by `joined`: `pairs` already requires it
        # of every pair it hands over, so a second test of it there could never fail.
        'overlaps': min(before.right, after.right) - max(before.left, after.left) > 0,
        # The first ink below `before`, within its own horizontal span, is `after`'s own
        # line when it is not above `after`'s box top. Anything higher was painted in
        # between -- a rule, a figure's edge, a footnote separator.
        'clear_between': first_ink >= after.baseline - after.size - 0.5,
    }


def joined(sign, indented_allowed):
    """The candidate rule over one pair's signals."""
    if not (0 < sign['pitch_em'] <= PITCH_MAX_EM):
        return False
    if not (sign['same_font'] and sign['same_size'] and sign['clear_between']):
        return False
    if abs(sign['left_delta']) <= LEFT_TOL:
        return True
    # A first line set further right than the rest of its paragraph is the one indent
    # worth admitting, and only where `before` has not already been joined from above.
    return indented_allowed and -INDENT_MAX_EM * sign['size'] < sign['left_delta'] < -LEFT_TOL


def pairs(rows):
    """Every (segment, following segment, signals) the rule is asked about.

    A segment is paired with the segment on the nearest line below it that overlaps it
    horizontally, the nearest in left edge where several do. Not the page's next line:
    two columns whose baselines are staggered alternate on the page, so the next line of
    each is always the other column's, and pairing there left every line of both columns
    unpaired, which no count below could show. A pair with another line of the page
    between is held to `SKIP_PITCH_EM`: a column's own lines are a line pitch apart, and
    over the tagged pages a wider pitch across a skipped line was a paragraph gap."""
    out = []
    for at, row in enumerate(rows):
        for before in row:
            for skipped, nxt in enumerate(rows[at + 1:]):
                candidates = [s for s in nxt
                              if min(before.right, s.right) - max(before.left, s.left) > 0]
                if candidates:
                    after = min(candidates, key=lambda s: abs(s.left - before.left))
                    sign = signals(before, after)
                    if not skipped or sign['pitch_em'] <= SKIP_PITCH_EM:
                        out.append((before, after, sign))
                    break
    return out


def rule_edges(rows, rule):
    """The pairs `rule` joins, as a map from a segment's index to the next segment.

    `all` joins every pair and `none` joins none: two mutations of the classifier, so
    that a number produced here can be shown to depend on the rule that produced it."""
    joins = {}
    taken = set()
    for before, after, sign in pairs(rows):
        if rule == 'all':
            take = True
        elif rule == 'none':
            take = False
        else:
            take = joined(sign, indented_allowed=before.index not in taken)
        if take and after.index not in taken:
            joins[before.index] = (after, sign)
            taken.add(after.index)
    return joins


def blocks(rows, rule):
    """The page's segments chained into blocks under `rule`."""
    joins = rule_edges(rows, rule)
    chains = []
    seen = set()
    # Reading order, and every edge runs from an earlier segment to a later one, so the
    # first time a segment is reached unclaimed it starts a block. Deciding that from
    # the edges instead loses a line the leading check drops: it still has an incoming
    # edge, and a segment that is neither in a chain nor a chain start is in no block
    # at all -- which is how five blocks first reported as four.
    for segment in (s for row in rows for s in row):
        if segment.index in seen:
            continue
        chain, pitches = [segment], []
        seen.add(segment.index)
        current = segment
        while current.index in joins:
            after, sign = joins[current.index]
            # Leading consistency, applied to the chain rather than the pair: a block's
            # lines are evenly spaced, and a pitch that steps out of line ends it.
            if after.index in seen:
                break
            if pitches and abs(sign['pitch'] - pitches[0]) > LEAD_TOL and rule == 'signals':
                break
            pitches.append(sign['pitch'])
            chain.append(after)
            seen.add(after.index)
            current = after
        chains.append((chain, pitches))
    return chains


def tag_blocks(rows):
    """The page's segments grouped by the block element the tags give them.

    Only where every segment on the page is tagged; a partly tagged page has no known
    answer and is counted as one rather than half-answered."""
    segments = [s for row in rows for s in row]
    if not segments or not all(s.tagged and len(s.blocks) == 1 for s in segments):
        return None
    grouped = defaultdict(list)
    for segment in segments:
        grouped[next(iter(segment.blocks))].append(segment)
    out = []
    for chain in grouped.values():
        chain.sort(key=lambda s: (s.baseline, s.left))
        pitches = [b.baseline - a.baseline for a, b in zip(chain, chain[1:])
                   if b.baseline > a.baseline]
        out.append((chain, pitches))
    return out


# ---------------------------------------------------------------------------
# Aggregation.
# ---------------------------------------------------------------------------

def bucket(value, edges):
    for edge in edges:
        if value <= edge:
            return f'<={edge}'
    return f'>{edges[-1]}'


def empty():
    return {'runs': 0, 'runs_axis_aligned': 0,
            'pages': Counter(), 'mcid_failures': Counter(),
            'stops': {t: Counter() for t in TRIALS},
            'run_tagging': Counter(), 'wrap_tagging': {t: Counter() for t in TRIALS},
            'pair_signals': Counter(), 'pitch_em': Counter(), 'left_delta': Counter(),
            'confusion': Counter(), 'false_join_shape': Counter(),
            'room': Counter(), 'room_headroom': Counter(), 'room_below_what': Counter(),
            'room_wrap': Counter()}


def tally(record, into, rule):
    for page in record['pages']:
        if page['status'] != 'editable':
            into['pages']['refused'] += 1
            continue
        into['pages']['editable'] += 1
        tagged = page.get('tagged', False)
        mcid_ok = page.get('mcid_map') == 'ok'
        into['pages']['tagged' if tagged else 'untagged'] += 1
        if tagged and not mcid_ok:
            into['pages']['tagged_without_mcid_map'] += 1
            into['mcid_failures'][page.get('mcid_map', 'missing')] += 1
        if 'failed' in (page.get('render') or {}):
            into['pages']['render_failed'] += 1
        elif (page.get('render') or {}).get('ink_fraction', 0) > 0.5:
            into['pages']['render_mostly_ink'] += 1

        for run in page['tried']:
            into['runs'] += 1
            if run['axis_aligned']:
                into['runs_axis_aligned'] += 1
            if not tagged:
                state = 'untagged'
            elif not mcid_ok:
                state = 'tagged_no_mcid_map'
            elif run.get('element') is None:
                state = 'tagged_run_unowned'
            else:
                state = 'tagged'
            # The denominator for the split below. A count of refusals bucketed by a
            # property is not evidence about that property without it: if nine runs in
            # ten are untagged, so are nine refusals in ten, whatever tagging is worth.
            into['run_tagging'][state] += 1
            for trial in TRIALS:
                category = classify(run[trial])
                into['stops'][trial][category] += 1
                if category == 'page_edge' or category in NEIGHBOUR:
                    where = 'page_edge' if category == 'page_edge' else 'neighbour'
                    into['wrap_tagging'][trial][f'{where}/{state}'] += 1

        # The geometric signals, over every pair of consecutive lines.
        rows = lines(page)
        for before, after, sign in pairs(rows):
            state = 'tagged' if (tagged and mcid_ok and before.tagged and after.tagged) else 'untagged'
            into['pitch_em'][f'{state}/{bucket(round(sign["pitch_em"], 2), [0.5, 1.0, 1.2, 1.5, 2.0, 3.0])}'] += 1
            into['left_delta'][f'{state}/{bucket(round(abs(sign["left_delta"]), 2), [0.1, 1.0, 3.0, 12.0, 36.0])}'] += 1
            for name in ('same_font', 'same_size', 'clear_between'):
                into['pair_signals'][f'{state}/{name}'] += bool(sign[name])
            into['pair_signals'][f'{state}/pairs'] += 1

        # The rule against the tags, on the pages where the tags give an answer.
        truth = tag_blocks(rows) if (tagged and mcid_ok) else None
        if truth is not None:
            same = {}
            for chain, _ in truth:
                for segment in chain:
                    same[segment.index] = next(iter(segment.blocks))
            joins = rule_edges(rows, rule)
            for before, after, sign in pairs(rows):
                if before.index not in same or after.index not in same:
                    continue
                agree = same[before.index] == same[after.index]
                joined_here = before.index in joins and joins[before.index][0].index == after.index
                into['confusion'][f'{"join" if joined_here else "split"}/{"same" if agree else "different"}'] += 1
                if joined_here and not agree:
                    into['false_join_shape'][false_join_shape(before, after, page)] += 1

        # What is below a block's last line, and whether one more line fits.
        chosen = truth if truth is not None else blocks(rows, rule)
        source = 'tags' if truth is not None else 'rule'
        page_pitches = [p for _, pitches in chosen for p in pitches]
        page_gaps = [a.ink_below for chain, _ in chosen for a in chain[:-1]]
        for chain, pitches in chosen:
            # A block whose own text is refused for the page edge is the population
            # wrapping would serve; every other block is a bystander, and mixing the two
            # answers a question nobody asked.
            wrapping = any(classify(run['grow25']) == 'page_edge'
                           for segment in chain for run in segment.runs)
            room(chain, pitches, page_pitches, page_gaps, page, source, into, wrapping)


def false_join_shape(before, after, page):
    """What a pair the rule joined and the tags separated actually looks like.

    A count alone would say the rule is wrong this often and not what it is wrong
    about, and the shapes are what decide whether the rule is fixable."""
    roles = {page['elements'][b]['role'] for b in before.blocks | after.blocks
             if b in page.get('elements', {})}
    if len(before.runs) == 1 and len(after.runs) == 1 and len(before.blocks | after.blocks) == 2:
        kind = 'two one-line blocks'
    elif roles and len(roles) > 1:
        kind = 'different roles'
    else:
        kind = 'consecutive blocks of the same role'
    return f'{kind} ({"/".join(sorted(roles)) or "unknown"})'


def room(chain, pitches, page_pitches, page_gaps, page, source, into, wrapping=False):
    """Whether this block could take one more line without landing on what is below.

    The pitch is the block's own where it has one, the page's median otherwise. The
    clearance a new line needs is that pitch **plus** the gap an existing following line
    leaves below a hit rectangle, which is measured from this block's own interior lines
    where it has them and from the page's otherwise -- so the criterion reproduces what a
    line that is already there looks like rather than assuming what one would need."""
    last = max(chain, key=lambda s: (s.baseline, s.left))
    pitch = statistics.median(pitches) if pitches else (
        statistics.median(page_pitches) if page_pitches else last.size * 1.2)
    interior = [s.ink_below for s in chain if s is not last]
    gap = statistics.median(interior) if interior else (
        statistics.median(page_gaps) if page_gaps else 0.0)
    needed = pitch + gap
    fits = last.ink_below >= needed
    into['room'][f'{source}/{"fits" if fits else "blocked"}'] += 1
    into['room'][f'{source}/blocks'] += 1
    if wrapping:
        into['room_wrap'][f'{source}/{"fits" if fits else "blocked"}'] += 1
        into['room_wrap'][f'{source}/blocks'] += 1
    if needed > 0:
        into['room_headroom'][f'{source}/{bucket(round(last.ink_below / needed, 2), [0.25, 0.5, 0.9, 1.0, 1.5, 3.0])}'] += 1
    into['room_below_what'][f'{source}/{below_what(last, page)}'] += 1


def below_what(last, page):
    """What the first ink below the block's last line belongs to."""
    if last.page_edge_below:
        return 'page edge, nothing below'
    y = last.bottom + last.ink_below
    for run in page['tried']:
        if not usable(run):
            continue
        rect = run['rect']
        if rect[1] - 0.5 <= y <= rect[3] + 0.5 and min(rect[2], last.right) - max(rect[0], last.left) > 0:
            return 'another line of text'
    return 'something the text runs do not account for'


def aggregate(records, manifest, rule):
    producers = {}
    if manifest:
        data = json.loads(Path(manifest).read_text(encoding='utf-8'))
        for group in ('files', 'followup_files', 'external_test_files', 'prototype_files',
                      'expansion_files'):
            for entry in data.get(group, []):
                producers[entry['filename']] = entry.get('producer', entry['title'])
    files, total = [], empty()
    for record in records:
        stats = empty()
        tally(record, stats, rule)
        tally(record, total, rule)
        files.append({'file': Path(record['source']).name,
                      'producer': producers.get(Path(record['source']).name, ''),
                      'pages_inspected': len(record['pages']),
                      'seconds': record['seconds'],
                      'agreement_checked': record['agreement']['checked'],
                      'agreement_disagreements': record['agreement']['disagreements'],
                      'stats': stats})
    return {'rule': rule, 'files': files, 'total': total,
            'agreement_checked': sum(f['agreement_checked'] for f in files),
            'agreement_disagreements': sum(len(f['agreement_disagreements']) for f in files)}


def pct(part, whole):
    return f'{100 * part / whole:.0f}' if whole else '-'


def refusal_table(summary):
    names = ['ok', 'page_edge'] + list(NEIGHBOUR) + ['clip', 'box_height', 'glyph', 'other']
    lines = ['| Producer | Runs | ' + ' | '.join(f'+25% {n}' for n in names) + ' |',
             '|---|---:|' + '---:|' * len(names)]
    for entry in summary['files'] + [{'producer': '**All runs**', 'file': '', 'stats': summary['total']}]:
        stops = entry['stats']['stops']['grow25']
        label = entry['producer'] or entry['file']
        lines.append(f'| {label} | {entry["stats"]["runs"]} | '
                     + ' | '.join(str(stops.get(n, 0)) for n in names) + ' |')
    return '\n'.join(lines)


def trial_table(summary):
    names = ['ok', 'page_edge'] + list(NEIGHBOUR) + ['clip', 'box_width', 'box_height',
                                                     'overlap', 'precision', 'glyph', 'other',
                                                     'line_full_unnamed']
    lines = ['| Trial | Runs | ' + ' | '.join(names) + ' |', '|---|---:|' + '---:|' * len(names)]
    for trial in TRIALS:
        stops = summary['total']['stops'][trial]
        lines.append(f'| {trial} | {sum(stops.values())} | '
                     + ' | '.join(str(stops.get(n, 0)) for n in names) + ' |')
    return '\n'.join(lines)


def show(summary):
    total = summary['total']
    out = [refusal_table(summary), '', trial_table(summary), '',
           'Pages: ' + json.dumps(dict(sorted(total['pages'].items()))),
           'MCID map failures: ' + json.dumps(dict(total['mcid_failures'])),
           'Every run, tagged against untagged (the denominator): '
           + json.dumps(dict(sorted(total['run_tagging'].items()))),
           'Where the wrapping-relevant refusals are, tagged against untagged: '
           + json.dumps(dict(sorted(total['wrap_tagging']['grow25'].items()))),
           'Pair signals: ' + json.dumps(dict(sorted(total['pair_signals'].items()))),
           'Line pitch in ems: ' + json.dumps(dict(sorted(total['pitch_em'].items()))),
           'Left-edge difference in points: ' + json.dumps(dict(sorted(total['left_delta'].items()))),
           f'The {summary["rule"]} rule against the tags: '
           + json.dumps(dict(sorted(total['confusion'].items()))),
           'False joins by shape: ' + json.dumps(dict(total['false_join_shape'].most_common(12))),
           'Room below a block: ' + json.dumps(dict(sorted(total['room'].items()))),
           'Room below a block whose own text is refused for the page edge: '
           + json.dumps(dict(sorted(total['room_wrap'].items()))),
           'Clearance as a fraction of what one more line needs: '
           + json.dumps(dict(sorted(total['room_headroom'].items()))),
           'What is below: ' + json.dumps(dict(sorted(total['room_below_what'].items())))]
    return '\n'.join(out)


# ---------------------------------------------------------------------------
# The fixture, and the controls over this file.
# ---------------------------------------------------------------------------

def fixture(path):
    """A synthetic document with a known answer for every question this file asks.

    **Page 1 is tagged**: two paragraphs of two lines each, at the same 14 pt pitch, the
    same left edge, the same font and the same size, so that the boundary between them is
    invisible to geometry and stated only by the tags. That is the false positive the
    rule has to be measured for, and a fixture without it would let a rule that joins
    everything score perfectly.

    The second paragraph's last line is owned by a `Span` inside it rather than by the
    paragraph directly, which is what producers do to a phrase with its own language or
    style. Without climbing past the Span the tags say three blocks where there are two,
    so the climb has a case here that fails when it is removed.

    **Page 2 is untagged**, and has to be a separate page: on a tagged page, text outside
    a marked-content sequence is read-only and is never offered as a run at all, so a
    mixed page cannot carry both halves. It holds three lines whose pitch steps from 14
    to 18 pt, so that leading consistency is the only thing ending that chain; then two
    lines 24 pt apart with a painted rule after them, which is the one signal stopping a
    join every other signal allows; then a line with a lot of clear space under it, and a
    last line with the rest of the page below it.

    **Page 3 is untagged too**, and exists for the two constants page 2 leaves untested:
    a line with a gutter in it, which must split into two segments and does not without
    `GUTTER_EM`; a line of three fragments a hand's width apart, which must stay one and
    does not when `GUTTER_EM` shrinks; and a pair of lines at a plausible pitch whose left
    edges are 80 pt apart, which must not be joined and is as soon as `LEFT_TOL` grows."""
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'testdata'))
    from make_text_pdf import Pdf  # noqa: E402

    pdf = Pdf()
    pages_ref = pdf.reserve()
    first_page = pdf.reserve()
    second_page = pdf.reserve()
    third_page = pdf.reserve()
    root_ref = pdf.reserve()
    font = pdf.add(b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica '
                   b'/Encoding /WinAnsiEncoding >>')
    bold = pdf.add(b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold '
                   b'/Encoding /WinAnsiEncoding >>')
    tagged_lines = [(72, 100, 'ALPHA ALPHA ALPHA'), (72, 114, 'ALPHA ALPHA'),
                    (72, 128, 'BETA BETA BETA'), (72, 142, 'BETA BETA')]
    # ZETA's second line starts 0.4 pt right of the first, which is the sub-point drift
    # `LEFT_TOL` exists to absorb and which a fixture set to whole points cannot show.
    plain_lines = [(72, 120, 'ZETA ZETA ZETA'), (72.4, 134, 'ZETA ZETA'), (72, 152, 'ZETA'),
                   (72, 200, 'GAMMA GAMMA GAMMA'), (72, 224, 'GAMMA GAMMA'),
                   (72, 248, 'DELTA DELTA'), (72, 400, 'EPSILON')]
    body = [b'/P << /MCID %d >> BDC BT /F1 12 Tf %d %d Td (%s) Tj ET EMC\n'
            % (mcid, x, 842 - y, text.encode('ascii'))
            for mcid, (x, y, text) in enumerate(tagged_lines)]
    # A gutter, three fragments that are not one, two lines set to different edges, and
    # two that agree on everything but the font.
    gutter_lines = [(72, 100, 'IOTA IOTA', b'F1'), (400, 100, 'KAPPA', b'F1'),
                    (72, 114, 'IOTA', b'F1'),
                    (200, 300, 'NU NU NU NU NU', b'F1'),
                    (120, 314, 'XI XI XI XI XI XI XI', b'F1'),
                    (72, 500, 'PI', b'F1'), (100, 500, 'RHO', b'F1'),
                    (140, 500, 'SIGMA', b'F1'),
                    (72, 560, 'TAU TAU TAU', b'F1'), (72, 574, 'UPSILON UPS', b'F2'),
                    (72, 620, 'PHI PHI', b'F1'), (72, 636, 'CHI CHI', b'F1', 14)]

    def draw(rows):
        return [b'BT /%s %d Tf %.2f %.2f Td (%s) Tj ET\n'
                % (row[3], row[4] if len(row) > 4 else 12, row[0], 842 - row[1],
                   row[2].encode('ascii')) for row in rows]

    plain = draw([(x, y, text, b'F1') for x, y, text in plain_lines])
    plain.append(b'0 0 0 rg 72 %d 200 2 re f\n' % (842 - 232))
    page = (b'<< /Type /Page /Parent %d 0 R /MediaBox [0 0 595 842] '
            b'/Resources << /Font << /F1 %d 0 R /F2 %d 0 R >> >> /Contents %d 0 R')
    pdf.put(first_page, page % (pages_ref, font, bold, pdf.stream(b'<< >>', b''.join(body)))
            + b' /StructParents 0 >>')
    pdf.put(second_page,
            page % (pages_ref, font, bold, pdf.stream(b'<< >>', b''.join(plain))) + b' >>')
    pdf.put(third_page, page % (pages_ref, font, bold,
                                pdf.stream(b'<< >>', b''.join(draw(gutter_lines)))) + b' >>')
    pdf.put(pages_ref, b'<< /Type /Pages /Count 3 /Kids [%d 0 R %d 0 R %d 0 R] >>'
            % (first_page, second_page, third_page))
    document = pdf.reserve()
    second = pdf.reserve()
    first = pdf.add(b'<< /Type /StructElem /S /P /P %d 0 R /Pg %d 0 R /K [0 1] >>'
                    % (document, first_page))
    span = pdf.add(b'<< /Type /StructElem /S /Span /P %d 0 R /Pg %d 0 R /K [3] >>'
                   % (second, first_page))
    pdf.put(second, b'<< /Type /StructElem /S /P /P %d 0 R /Pg %d 0 R /K [2 %d 0 R] >>'
            % (document, first_page, span))
    pdf.put(document, b'<< /Type /StructElem /S /Document /P %d 0 R /K [%d 0 R %d 0 R] >>'
            % (root_ref, first, second))
    tree = pdf.add(b'<< /Nums [0 [%d 0 R %d 0 R %d 0 R %d 0 R]] >>'
                   % (first, first, second, span))
    pdf.put(root_ref, b'<< /Type /StructTreeRoot /K [%d 0 R] /ParentTree %d 0 R '
            b'/ParentTreeNextKey 1 >>' % (document, tree))
    catalog = pdf.add(b'<< /Type /Catalog /Pages %d 0 R /StructTreeRoot %d 0 R '
                      b'/MarkInfo << /Marked true >> >>' % (pages_ref, root_ref))
    path.write_bytes(pdf.serialize(catalog))


def self_test(probe):
    with tempfile.TemporaryDirectory(prefix='tpdf-blocks-') as directory:
        path = Path(directory) / 'blocks.pdf'
        fixture(path)
        record = measure(probe, path, 1)
        tagged_page, plain_page, gutter_page = record['pages']
        assert [p['status'] for p in record['pages']] == ['editable'] * 3, record['pages']
        assert tagged_page['tagged'] and tagged_page['mcid_map'] == 'ok', tagged_page['mcid_map']
        assert not plain_page['tagged'], plain_page
        assert record['agreement']['checked'] > 0 and not record['agreement']['disagreements']

        # The probe read the structure tree: the first two lines belong to one element
        # and the next two to another, through the parent tree and past the Document.
        owners = [r['element'] for r in sorted(tagged_page['tried'], key=baseline)]
        assert len(owners) == 4 and owners[0] == owners[1], owners
        assert len({tuple(owners[:2]), (owners[2],), (owners[3],)}) == 3, owners
        assert [r['role'] for r in sorted(tagged_page['tried'], key=baseline)] \
            == ['P', 'P', 'P', 'Span'], tagged_page['tried']
        assert [r['element'] for r in plain_page['tried']] == [None] * 7, plain_page['tried']

        tagged_rows, plain_rows = lines(tagged_page), lines(plain_page)
        assert [len(row) for row in tagged_rows] == [1] * 4, tagged_rows
        assert [len(row) for row in plain_rows] == [1] * 7, plain_rows
        assert sorted(round(sign['pitch'], 1) for _, _, sign in pairs(plain_rows)) \
            == [14.0, 18.0, 24.0, 24.0, 48.0, 152.0]
        # Leading consistency, and nothing else, ends the three-line chain: the 18 pt
        # step passes every pairwise signal.
        stepped = next(s for b, _, s in pairs(plain_rows) if round(b.baseline) == 134)
        assert round(stepped['pitch'], 1) == 18.0 and joined(stepped, indented_allowed=True)

        # The gutter page: one line split in two, one line of three fragments kept whole,
        # and a pair the left edges refuse.
        gutter_rows = lines(gutter_page)
        assert [len(row) for row in gutter_rows] == [2] + [1] * 8, gutter_rows
        assert [len(s.runs) for row in gutter_rows for s in row] == [1] * 5 + [3] + [1] * 4, \
            gutter_rows
        offset = next(s for b, _, s in pairs(gutter_rows) if round(b.baseline) == 300)
        assert round(offset['left_delta']) == -80 and offset['same_font'] and offset['overlaps']
        assert round(offset['pitch'], 1) == 14.0 and offset['clear_between']
        assert not joined(offset, indented_allowed=True), offset
        # Two lines that agree on everything the rule reads except the font.
        styled = next(s for b, _, s in pairs(gutter_rows) if round(b.baseline) == 560)
        assert round(styled['pitch'], 1) == 14.0 and abs(styled['left_delta']) <= LEFT_TOL
        assert styled['same_size'] and styled['clear_between'] and not styled['same_font']
        assert not joined(styled, indented_allowed=True), styled
        # And two that agree on everything except the size.
        resized = next(s for b, _, s in pairs(gutter_rows) if round(b.baseline) == 620)
        assert styled['same_font'] is False and resized['same_font'] is True
        assert not resized['same_size'] and resized['clear_between']
        assert 0 < resized['pitch_em'] <= PITCH_MAX_EM and abs(resized['left_delta']) <= LEFT_TOL
        assert not joined(resized, indented_allowed=True), resized
        # The sub-point drift `LEFT_TOL` absorbs, which is a joined pair and not a flush one.
        drifted = next(s for b, _, s in pairs(plain_rows) if round(b.baseline) == 120)
        assert 0 < abs(drifted['left_delta']) <= LEFT_TOL, drifted
        assert joined(drifted, indented_allowed=True)
        # The painted rule between GAMMA's second line and DELTA is what `clear_between`
        # is for: every other signal of that pair says join, and it is the one that does not.
        interrupted = next(s for b, _, s in pairs(plain_rows) if round(b.baseline) == 224)
        assert interrupted['same_font'] and interrupted['same_size'] and interrupted['overlaps']
        assert round(interrupted['pitch'], 1) == 24.0 and not interrupted['clear_between']
        assert not joined(interrupted, indented_allowed=True)
        first_pair = next(s for b, _, s in pairs(plain_rows) if round(b.baseline) == 200)
        assert first_pair['clear_between'] and joined(first_pair, indented_allowed=True)

        # The tags' answer on page 1 is two blocks; geometry's is one chain of four.
        truth = tag_blocks(tagged_rows)
        assert truth is not None and sorted(len(c) for c, _ in truth) == [2, 2], truth
        assert tag_blocks(plain_rows) is None, 'an untagged page has no known answer'
        assert sorted(len(c) for c, _ in blocks(tagged_rows, 'signals')) == [4], \
            blocks(tagged_rows, 'signals')

        # Two columns whose baselines are staggered alternate on the page, so each line's
        # next line of the page is the other column's. Pairing is by the nearest
        # overlapping line below, and across another line only at a line pitch: at 12 pt,
        # 12 pt apart pairs and 25 pt apart does not. Plain data, no probe: the question
        # is the pairing, and a fixture drawn for it would test the drawing too.
        def run(x, y):
            return {'rect': [x, y - 12, x + 100, y + 3], 'size': 12, 'font': 'F',
                    'axis_aligned': True, 'ink_below': 1, 'below_is_page_edge': False,
                    'chars': 10}
        staggered = {'tried': [run(20, y) for y in (100, 112, 124)]
                     + [run(200, y + 5) for y in (100, 112, 124)]}
        paired = {(round(b.baseline), round(a.baseline)) for b, a, _ in pairs(lines(staggered))}
        assert paired == {(100, 112), (112, 124), (105, 117), (117, 129)}, paired
        gap = {'tried': [run(20, 100), run(200, 110), run(20, 125)]}
        assert not pairs(lines(gap)), [(b.baseline, a.baseline) for b, a, _ in pairs(lines(gap))]
        gap['tried'][2] = run(20, 112)
        assert len(pairs(lines(gap))) == 1
        # A run 2 pt below its line and next to it along it is on that line, and the
        # line keeps its own baseline; 7 pt below, past half an em, it is not.
        def lowered(drop):
            mark = {**run(122, 100 + drop), 'chars': 1}
            mark['rect'][2] = 128
            return {'tried': [run(20, 100), mark, run(20, 114)]}
        rows = lines(lowered(2))
        assert [[round(s.baseline) for s in row] for row in rows] == [[100], [114]], rows
        assert {(round(b.baseline), round(a.baseline)) for b, a, _ in pairs(rows)} == {(100, 114)}
        assert len(lines(lowered(7))) == 3
        # A second mark between the first and its line leaves both on the line.
        marks = lowered(2)
        marks['tried'].insert(1, {**run(128, 101), 'chars': 1})
        marks['tried'][1]['rect'][2] = 134
        assert [[round(s.baseline) for s in row] for row in lines(marks)] == [[100], [114]]

        counts = {}
        for rule in ('signals', 'all', 'none'):        counts = {}
        for rule in ('signals', 'all', 'none'):
            stats = empty()
            tally(record, stats, rule)
            counts[rule] = (stats['room']['rule/blocks'], dict(stats['confusion']))
        # A classifier whose output does not move with its rule is measuring nothing.
        # `none` is one block per segment, 7 on page 2 and 6 on page 3. `all` is not one
        # block per page: it joins every pair there is, and a pair only exists between
        # lines that overlap horizontally, so page 3's second line reaches nothing below
        # it and the page keeps three blocks.
        assert counts['none'][0] == 7 + 10 and counts['all'][0] == 1 + 3, counts
        assert counts['signals'][0] == 5 + 9, counts
        assert counts['signals'][1] == {'join/same': 2, 'join/different': 1}, counts['signals']
        assert counts['none'][1] == {'split/same': 2, 'split/different': 1}, counts['none']

        stats = empty()
        tally(record, stats, 'signals')
        # ZETA loses its third line to the pitch step; GAMMA stops at the painted rule.
        assert sorted(len(c) for c, _ in blocks(plain_rows, 'signals')) == [1, 1, 1, 2, 2]
        assert sorted(len(c) for c, _ in blocks(gutter_rows, 'signals')) == [1] * 8 + [2]
        # EPSILON has the rest of the page under it, DELTA has EPSILON 150 pt below and
        # ZETA's orphaned third line has GAMMA 36 pt below, all of which take a line.
        # ZETA's own last line has the third right under it and GAMMA's has the rule.
        # On page 3, NU has XI right under it and everything else has room.
        assert stats['room']['rule/blocks'] == 14 and stats['room']['rule/fits'] == 9, stats['room']
        # On the tagged page the tags decide the blocks: ALPHA is blocked by BETA under
        # it, BETA has the rest of the page.
        assert stats['room']['tags/blocks'] == 2 and stats['room']['tags/fits'] == 1, stats['room']
        below = dict(stats['room_below_what'])
        assert below.get('rule/page edge, nothing below') == 3, below
        assert below.get('rule/another line of text') == 10, below
        assert below.get('rule/something the text runs do not account for') == 1, below
        assert below.get('tags/another line of text') == 1, below

        # The refusal categories, on a page built so that one line has the page edge
        # ahead of it and one has a neighbour.
        assert classify('ok') == 'ok'
        assert classify('There is no room for more text on this line: it reaches the '
                        'edge of the page. Shorten the text.') == 'page_edge'
        assert classify('There is no room for more text on this line: the text after it '
                        'cannot be moved.') == 'neighbour_unmovable'
        assert classify('There is no room for more text on this line: something new.') \
            == 'line_full_unnamed'
        assert classify('replacement exceeds the box width') == 'box_width'
        assert classify('something new') == 'other'

        # The collector must not accept a report with a trial missing.
        broken = json.loads(json.dumps(record))
        del broken['pages'][0]['tried'][0]['grow25']
        try:
            tally(broken, empty(), 'signals')
        except KeyError:
            pass
        else:
            raise AssertionError('a missing trial was counted')
    print('[PASS] the probe read the structure tree and the parent tree, two tagged blocks '
          'that geometry cannot tell apart were reported separately, a painted rule between '
          'two lines is seen, the rule chains four lines that the tags split, both classifier '
          'mutations move the block count, the room below each block is what the page has, '
          'and a missing trial is refused')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('pdfs', nargs='*', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--records', type=Path)
    parser.add_argument('--report', type=Path, help='aggregate saved records again')
    parser.add_argument('--rule', default='signals', choices=('signals', 'all', 'none'))
    parser.add_argument('--agree-every', type=int, default=97)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    started = time.monotonic()
    if args.self_test:
        self_test(args.probe.resolve(strict=True))
        return
    if args.report is not None:
        records = [json.loads(path.read_text(encoding='utf-8'))
                   for path in sorted(args.report.glob('*.json'))]
    else:
        probe = args.probe.resolve(strict=True)
        if not args.pdfs or args.output is None:
            parser.error('provide explicit PDFs and --output')
        if args.records is not None:
            args.records.mkdir()  # new, so a stale record cannot enter a comparison
        with ThreadPoolExecutor(args.jobs) as pool:
            records = list(pool.map(lambda p: measure(probe, p, args.agree_every), args.pdfs))
        if args.records is not None:
            for record in records:
                path = args.records / (Path(record['source']).stem + '.json')
                path.write_text(json.dumps(record) + '\n', encoding='utf-8')
    if args.output is None or args.output.exists():
        parser.error('provide a new --output path')
    summary = aggregate(records, args.manifest, args.rule)
    summary['wall_seconds'] = round(time.monotonic() - started, 1)
    summary['sources'] = [{'file': Path(r['source']).name, 'sha256': r['sha256']} for r in records]
    args.output.write_text(json.dumps(summary, indent=2, default=dict) + '\n', encoding='utf-8')
    print(show(summary))
    print(f"[DONE] {len(records)} documents, {summary['total']['runs']} runs, "
          f"{summary['agreement_checked']} worker agreement checks, "
          f"{summary['agreement_disagreements']} disagreements, {summary['wall_seconds']} s")


if __name__ == '__main__':
    main()
