//! The order a page is meant to be read in, for callers with no webview.
//!
//! **A restatement of `src/lib/reading.ts`, not a second opinion.** The viewer
//! decides reading order in TypeScript --- the copy path, the screen reader and
//! the highlight text all go through `readingLines` there --- and the
//! command-line `tpdf text` has no webview to ask. So this file says the same
//! thing in Rust, function for function and in the same order, and every
//! choice worth arguing about is argued in `reading.ts` and not here.
//!
//! A restatement is a second copy, and `docs/TRAPS.md` records what second
//! copies do. So it is held to the original the way `words.rs` is:
//! `cli::tests` writes every case in [`tests::cases`] with the order this file
//! computes to `testdata/cli/reading.json` (`TPDF_CLI_SAMPLES=write`), and
//! `src/lib/clireading.test.ts` asks `reading.ts` the same questions and
//! compares index for index. A rule changed on either side is a red test there.
//!
//! ## Numbers as the webview sees them
//!
//! `PageText::boxes` is `f32`, and it reaches `reading.ts` as JSON, where each
//! value is the **shortest decimal** that round-trips the `f32`, parsed as an
//! `f64`. That is not the `f64` a widening cast gives: `595.2756f32 as f64` is
//! `595.2755737304688`, and JavaScript is holding `595.2756`. Every comparison
//! below is between such numbers --- a gap against a threshold, an overlap
//! against half a height --- so computing on the widened value would be a
//! different page at exactly the ties. [`webview_number`] takes the route the
//! value takes to the viewer; a non-finite value becomes `0`, which is what
//! `null ?? 0` makes of the `null` JSON writes for one.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::structure::TaggedRun;
use crate::text::PageText;

/// `reading.ts`'s `CUT_CHARS`.
const CUT_CHARS: f64 = 3.0;
/// `MAX_DEPTH`.
const MAX_DEPTH: usize = 12;
/// `SHORT_MARK`.
const SHORT_MARK: f64 = 0.5;
/// `SLIVER_PT`.
const SLIVER_PT: f64 = 0.1;
/// `SLIVER_OF_LINE`.
const SLIVER_OF_LINE: f64 = 0.05;

/// Which of the two routes ordered a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// The document's own tags cover every visible character, and were used.
    Tagged,
    /// Recovered from where the characters sit: `reading.ts`'s XY-cut.
    Geometric,
}

/// A half-open range of character indices, `reading.ts`'s `IndexRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Range {
    /// First index.
    pub from: usize,
    /// One past the last.
    pub to: usize,
}

/// A page's reading order: which route decided it, and its lines in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// Tags or geometry.
    pub route: Route,
    /// Each line's index ranges, in reading order. `readingLines`.
    pub lines: Vec<Vec<Range>>,
}

impl Reading {
    /// Every index, in reading order: `readingOrder`.
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        self.lines
            .iter()
            .flatten()
            .flat_map(|range| range.from..range.to)
            .collect()
    }
}

/// The value an `f32` becomes in the viewer. See the module note.
#[must_use]
pub fn webview_number(value: f32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    // Rust's `Display` for `f32` is the shortest string that round-trips, the
    // same digits `serde_json` writes; parsing it as `f64` is what JSON.parse
    // does.
    format!("{value}").parse().unwrap_or(0.0)
}

/// The page as `reading.ts` receives it.
#[derive(Clone, Debug)]
struct View {
    codes: Vec<u32>,
    boxes: Vec<f64>,
    quarter_turns: u32,
    char_turns: Vec<u8>,
    runs: Vec<TaggedRun>,
}

impl View {
    fn of(text: &PageText) -> View {
        View {
            codes: text.codes.clone(),
            boxes: text.boxes.iter().copied().map(webview_number).collect(),
            quarter_turns: u32::from(text.quarter_turns),
            char_turns: text.char_turns.clone(),
            runs: text.runs.clone(),
        }
    }

    fn len(&self) -> usize {
        self.codes.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct Quad {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Clone, Copy, Debug)]
struct Axes {
    sideways: bool,
    along_sign: i8,
    cross_sign: i8,
}

#[derive(Clone, Copy, Debug)]
struct Extents {
    along_start: f64,
    along_end: f64,
    cross_start: f64,
    cross_end: f64,
}

#[derive(Clone, Debug)]
struct Fragment {
    ranges: Vec<Range>,
    bounds: Quad,
    /// `turns`, which `reading.ts` leaves `undefined` on a page whose
    /// characters all run one way.
    turns: Option<u32>,
}

#[derive(Clone, Debug)]
struct Placed {
    index: usize,
    bounds: Quad,
    extents: Extents,
}

/// `quarterTurns`.
fn quarter(turns: u32) -> u32 {
    turns % 4
}

/// `axesFor`.
fn axes_for(turns: u32) -> Axes {
    let at = quarter(turns);
    Axes {
        sideways: at % 2 == 1,
        along_sign: if at == 2 || at == 3 { -1 } else { 1 },
        cross_sign: if at == 1 || at == 2 { -1 } else { 1 },
    }
}

/// `extentsOf`.
fn extents_of(q: Quad, axes: Axes) -> Extents {
    let along0 = if axes.sideways { q.top } else { q.left };
    let along1 = if axes.sideways { q.bottom } else { q.right };
    let cross0 = if axes.sideways { q.left } else { q.top };
    let cross1 = if axes.sideways { q.right } else { q.bottom };
    Extents {
        along_start: if axes.along_sign == 1 {
            along0
        } else {
            -along1
        },
        along_end: if axes.along_sign == 1 {
            along1
        } else {
            -along0
        },
        cross_start: if axes.cross_sign == 1 {
            cross0
        } else {
            -cross1
        },
        cross_end: if axes.cross_sign == 1 {
            cross1
        } else {
            -cross0
        },
    }
}

/// `charQuad`.
fn char_quad(view: &View, index: usize) -> Quad {
    let at = |k: usize| view.boxes.get(index * 4 + k).copied().unwrap_or(0.0);
    Quad {
        left: at(0),
        top: at(1),
        right: at(2),
        bottom: at(3),
    }
}

/// `characterTurns`.
fn character_turns(view: &View, index: usize) -> u32 {
    quarter(view.quarter_turns + u32::from(view.char_turns.get(index).copied().unwrap_or(0)))
}

/// `placed`.
fn placed(q: Quad) -> bool {
    q.right > q.left || q.bottom > q.top
}

/// `absorb`.
fn absorb(into: &mut Quad, add: Quad) {
    into.left = into.left.min(add.left);
    into.right = into.right.max(add.right);
    into.top = into.top.min(add.top);
    into.bottom = into.bottom.max(add.bottom);
}

fn empty_box() -> Quad {
    Quad {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    }
}

/// A numeric comparison as JavaScript's `a - b` sort key makes it.
fn by(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// The median of `values` as `reading.ts` takes it: sorted, the element at
/// `floor(n / 2)`, `0` when there is none.
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| by(*a, *b));
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// `typicalCross`.
fn typical_cross(view: &View, axes: Axes) -> f64 {
    let heights: Vec<f64> = (0..view.len())
        .map(|index| char_quad(view, index))
        .filter(|q| placed(*q))
        .map(|q| {
            let e = extents_of(q, axes);
            e.cross_end - e.cross_start
        })
        .filter(|h| *h > 0.0)
        .collect();
    median(heights)
}

/// `sliver`.
fn sliver(e: Extents, typical: f64) -> bool {
    let height = e.cross_end - e.cross_start;
    height < SLIVER_PT && height < SLIVER_OF_LINE * typical
}

/// `sameBand`.
fn same_band(a: Extents, b: Extents) -> bool {
    let overlap = a.cross_end.min(b.cross_end) - a.cross_start.max(b.cross_start);
    if overlap <= 0.0 {
        return false;
    }
    let (ha, hb) = (a.cross_end - a.cross_start, b.cross_end - b.cross_start);
    let shorter = ha.min(hb);
    if shorter <= 0.0 {
        return false;
    }
    if shorter < ha.max(hb) * SHORT_MARK {
        return true;
    }
    overlap / shorter > 0.5
}

/// `COMBINING`: a nonspacing or enclosing mark.
static COMBINING: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[\p{Mn}\p{Me}]$").expect("a fixed pattern compiles"));

/// `combining`.
fn combining(code: u32) -> bool {
    char::from_u32(code).is_some_and(|c| COMBINING.is_match(c.encode_utf8(&mut [0; 4])))
}

/// `isVisible`: whether JavaScript's `trim` leaves anything of the character.
///
/// `trim` removes ECMAScript's WhiteSpace and LineTerminator, which is not
/// Rust's `char::is_whitespace`: JavaScript counts U+FEFF and not U+0085, and
/// Rust the other way round. Spelled out rather than borrowed for that reason.
fn is_visible(code: u32) -> bool {
    let Some(c) = char::from_u32(code) else {
        // A lone surrogate: JavaScript keeps it through `trim`, so it counts.
        return true;
    };
    let js_space = matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    );
    !js_space
}

/// `fragmentsOf`.
fn fragments_of(view: &View, axes: Axes, gap: f64) -> Vec<Fragment> {
    if view.char_turns.is_empty() {
        return aligned_fragments(view, axes, gap);
    }
    // A `Map` iterates in insertion order, so a list searched linearly is the
    // same thing; there are at most four keys.
    let mut groups: Vec<(u32, Vec<usize>)> = Vec::new();
    let mut direction = view.quarter_turns;
    for index in 0..view.len() {
        if placed(char_quad(view, index)) {
            direction = character_turns(view, index);
        }
        match groups.iter_mut().find(|(turns, _)| *turns == direction) {
            Some((_, indices)) => indices.push(index),
            None => groups.push((direction, vec![index])),
        }
    }
    let mut fragments = Vec::new();
    for (turns, indices) in groups {
        let group = View {
            codes: indices
                .iter()
                .map(|i| view.codes.get(*i).copied().unwrap_or(0))
                .collect(),
            boxes: indices
                .iter()
                .flat_map(|i| {
                    (0..4).map(move |k| view.boxes.get(i * 4 + k).copied().unwrap_or(0.0))
                })
                .collect(),
            quarter_turns: turns,
            char_turns: Vec::new(),
            runs: Vec::new(),
        };
        let own = axes_for(turns);
        for fragment in aligned_fragments(&group, own, cut_width(&group, own)) {
            let original: Vec<usize> = fragment
                .ranges
                .iter()
                .flat_map(|r| r.from..r.to)
                .map(|i| indices[i])
                .collect();
            fragments.push(Fragment {
                ranges: ranges_of(original),
                bounds: fragment.bounds,
                turns: Some(turns),
            });
        }
    }
    fragments
}

/// `alignedFragments`.
fn aligned_fragments(view: &View, axes: Axes, gap: f64) -> Vec<Fragment> {
    let mut items: Vec<Placed> = Vec::new();
    let typical = typical_cross(view, axes);
    // Keyed by the index before, which is -1 before the first placed one.
    let mut trailing: HashMap<i64, Vec<usize>> = HashMap::new();
    let mut last: i64 = -1;
    for index in 0..view.len() {
        let q = char_quad(view, index);
        let extents = extents_of(q, axes);
        let mark = last >= 0 && combining(view.codes[index]);
        if !placed(q) || sliver(extents, typical) || mark {
            trailing.entry(last).or_default().push(index);
            if mark && placed(q) {
                if let Some(base) = items.last_mut() {
                    absorb(&mut base.bounds, q);
                    base.extents = extents_of(base.bounds, axes);
                }
            }
            continue;
        }
        items.push(Placed {
            index,
            bounds: q,
            extents,
        });
        last = index as i64;
    }
    if items.is_empty() {
        return if view.len() > 0 {
            vec![Fragment {
                ranges: vec![Range {
                    from: 0,
                    to: view.len(),
                }],
                bounds: empty_box(),
                turns: None,
            }]
        } else {
            Vec::new()
        };
    }
    // `reading.ts`'s fix, 2026-09-27: what came before the first placed
    // character was filed under -1, which no fragment reads, and dropped out of
    // the order. It joins the first placed character's fragment.
    if let Some(leading) = trailing.remove(&-1) {
        let first = items[0].index as i64;
        let mut joined = leading;
        joined.extend(trailing.remove(&first).unwrap_or_default());
        trailing.insert(first, joined);
    }

    let mut by_cross = items.clone();
    by_cross.sort_by(|a, b| by(a.extents.cross_start, b.extents.cross_start));
    let mut bands: Vec<Vec<Placed>> = Vec::new();
    let mut band: Vec<Placed> = Vec::new();
    let mut band_extents: Option<Extents> = None;
    for item in by_cross {
        if let Some(extents) = band_extents.as_mut() {
            if same_band(*extents, item.extents) {
                extents.cross_end = extents.cross_end.max(item.extents.cross_end);
                band.push(item);
                continue;
            }
        }
        if !band.is_empty() {
            bands.push(std::mem::take(&mut band));
        }
        band_extents = Some(item.extents);
        band = vec![item];
    }
    if !band.is_empty() {
        bands.push(band);
    }

    let mut fragments = Vec::new();
    for mut members in bands {
        members.sort_by(|a, b| by(a.extents.along_start, b.extents.along_start));
        let mut current: Vec<Placed> = Vec::new();
        let mut reach = f64::NEG_INFINITY;
        for item in members {
            if !current.is_empty() && item.extents.along_start - reach > gap {
                fragments.push(fragment_of(&current, &trailing));
                current.clear();
            }
            reach = reach.max(item.extents.along_end);
            current.push(item);
        }
        if !current.is_empty() {
            fragments.push(fragment_of(&current, &trailing));
        }
    }
    fragments
}

/// `fragmentOf`.
fn fragment_of(members: &[Placed], trailing: &HashMap<i64, Vec<usize>>) -> Fragment {
    let mut indices = Vec::new();
    let mut bounds = members[0].bounds;
    for item in members {
        indices.push(item.index);
        if let Some(after) = trailing.get(&(item.index as i64)) {
            indices.extend_from_slice(after);
        }
        absorb(&mut bounds, item.bounds);
    }
    Fragment {
        ranges: ranges_of(indices),
        bounds,
        turns: None,
    }
}

/// `cutWidth`.
fn cut_width(view: &View, axes: Axes) -> f64 {
    let widths: Vec<f64> = (0..view.len())
        .map(|index| char_quad(view, index))
        .filter(|q| placed(*q))
        .map(|q| {
            let e = extents_of(q, axes);
            e.along_end - e.along_start
        })
        .filter(|w| *w > 0.0)
        .collect();
    median(widths) * CUT_CHARS
}

/// `rangesOf`.
fn ranges_of(mut indices: Vec<usize>) -> Vec<Range> {
    indices.sort_unstable();
    let mut ranges: Vec<Range> = Vec::new();
    for index in indices {
        match ranges.last_mut() {
            Some(last) if last.to == index => last.to = index + 1,
            _ => ranges.push(Range {
                from: index,
                to: index + 1,
            }),
        }
    }
    ranges
}

type Span = (Fragment, Extents);

/// `blocksOf`.
fn blocks_of(fragments: Vec<Fragment>, axes: Axes, gap: f64, depth: usize) -> Vec<Vec<Fragment>> {
    if fragments.len() < 2 || depth >= MAX_DEPTH {
        return vec![fragments];
    }
    let spans: Vec<Span> = fragments
        .iter()
        .map(|f| (f.clone(), extents_of(f.bounds, axes)))
        .collect();

    let columns = split(&spans, |e| (e.along_start, e.along_end), gap);
    if columns.len() > 1 {
        return columns
            .into_iter()
            .flat_map(|group| {
                blocks_of(
                    group.into_iter().map(|s| s.0).collect(),
                    axes,
                    gap,
                    depth + 1,
                )
            })
            .collect();
    }
    let rows = split_once(&spans, |e| (e.cross_start, e.cross_end));
    if rows.len() > 1 {
        return rows
            .into_iter()
            .flat_map(|group| {
                blocks_of(
                    group.into_iter().map(|s| s.0).collect(),
                    axes,
                    gap,
                    depth + 1,
                )
            })
            .collect();
    }
    vec![fragments]
}

fn sorted_by_start(spans: &[Span], of: impl Fn(&Extents) -> (f64, f64)) -> Vec<Span> {
    let mut sorted = spans.to_vec();
    sorted.sort_by(|a, b| by(of(&a.1).0, of(&b.1).0));
    sorted
}

/// `split`.
fn split(spans: &[Span], of: impl Fn(&Extents) -> (f64, f64), gap: f64) -> Vec<Vec<Span>> {
    let sorted = sorted_by_start(spans, &of);
    let mut groups: Vec<Vec<Span>> = Vec::new();
    let mut group: Vec<Span> = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for span in sorted {
        let (start, end) = of(&span.1);
        if !group.is_empty() && start - reach > gap {
            groups.push(std::mem::take(&mut group));
        }
        group.push(span);
        reach = reach.max(end);
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
}

/// `splitOnce`.
fn split_once(spans: &[Span], of: impl Fn(&Extents) -> (f64, f64)) -> Vec<Vec<Span>> {
    let mut sorted = sorted_by_start(spans, &of);
    let (mut best_at, mut best_size): (Option<usize>, f64) = (None, 0.0);
    let mut reach = f64::NEG_INFINITY;
    for (index, span) in sorted.iter().enumerate() {
        let (start, end) = of(&span.1);
        if index > 0 && start - reach > best_size {
            best_at = Some(index);
            best_size = start - reach;
        }
        reach = reach.max(end);
    }
    match best_at {
        None => vec![sorted],
        Some(at) => {
            let rest = sorted.split_off(at);
            vec![sorted, rest]
        }
    }
}

/// `readingBlocks`, flattened to lines as `readingLines` flattens it, with the
/// route that ran.
#[must_use]
pub fn read(text: &PageText) -> Reading {
    let view = View::of(text);
    let axes = axes_for(view.quarter_turns);
    let gap = cut_width(&view, axes);
    let fragments = fragments_of(&view, axes, gap);
    if let Some(tagged) = usable_runs(&view) {
        let lines = ownership(&view, tagged)
            .into_iter()
            .flat_map(|owned| lines_of(&view, within(&view, &fragments, &owned), axes))
            .collect();
        return Reading {
            route: Route::Tagged,
            lines,
        };
    }
    let lines = if view.char_turns.is_empty() {
        blocks_of(fragments, axes, gap, 0)
            .into_iter()
            .flat_map(|block| lines_of(&view, block, axes))
            .collect()
    } else {
        direction_groups(&fragments, axes)
            .into_iter()
            .flat_map(|(turns, group)| {
                let own = axes_for(turns);
                let view = &view;
                blocks_of(group, own, gap, 0)
                    .into_iter()
                    .flat_map(move |block| lines_of(view, block, own))
            })
            .collect()
    };
    Reading {
        route: Route::Geometric,
        lines,
    }
}

/// `directionGroups`.
fn direction_groups(block: &[Fragment], axes: Axes) -> Vec<(u32, Vec<Fragment>)> {
    let mut groups: Vec<(u32, Vec<Fragment>)> = Vec::new();
    for fragment in block {
        let turns = fragment.turns.unwrap_or(0);
        let copy = Fragment {
            ranges: fragment.ranges.clone(),
            bounds: fragment.bounds,
            turns: None,
        };
        match groups.iter_mut().find(|(t, _)| *t == turns) {
            Some((_, group)) => group.push(copy),
            None => groups.push((turns, vec![copy])),
        }
    }
    let mut boxed: Vec<(u32, Vec<Fragment>, Quad)> = groups
        .into_iter()
        .map(|(turns, fragments)| {
            let mut bounds = fragments[0].bounds;
            for fragment in &fragments {
                absorb(&mut bounds, fragment.bounds);
            }
            (turns, fragments, bounds)
        })
        .collect();
    boxed.sort_by(|a, b| {
        let (ea, eb) = (extents_of(a.2, axes), extents_of(b.2, axes));
        by(ea.cross_start, eb.cross_start).then(by(ea.along_start, eb.along_start))
    });
    boxed.into_iter().map(|(t, f, _)| (t, f)).collect()
}

/// `linesOf`.
fn lines_of(view: &View, block: Vec<Fragment>, axes: Axes) -> Vec<Vec<Range>> {
    if block.iter().any(|f| f.turns.is_some()) {
        return direction_groups(&block, axes)
            .into_iter()
            .flat_map(|(turns, group)| lines_of(view, group, axes_for(turns)))
            .collect();
    }
    let mut ordered = block;
    ordered.sort_by(|a, b| {
        let (ea, eb) = (extents_of(a.bounds, axes), extents_of(b.bounds, axes));
        by(ea.cross_start, eb.cross_start).then(by(ea.along_start, eb.along_start))
    });
    let mut lines: Vec<(Vec<Fragment>, Quad)> = Vec::new();
    for fragment in ordered {
        if let Some((fragments, bounds)) = lines.last_mut() {
            if same_band(extents_of(*bounds, axes), extents_of(fragment.bounds, axes)) {
                absorb(bounds, fragment.bounds);
                fragments.push(fragment);
                continue;
            }
        }
        let bounds = fragment.bounds;
        lines.push((vec![fragment], bounds));
    }
    lines
        .into_iter()
        .map(|(fragments, _)| {
            along_line(view, fragments, axes)
                .into_iter()
                .flat_map(|fragment| fragment.ranges)
                .collect()
        })
        .collect()
}

/// `sameRow`: halves of one row of text overlap by more than half of the
/// taller one.
fn same_row(a: Extents, b: Extents) -> bool {
    let overlap = a.cross_end.min(b.cross_end) - a.cross_start.max(b.cross_start);
    let taller = (a.cross_end - a.cross_start).max(b.cross_end - b.cross_start);
    taller > 0.0 && overlap / taller > 0.5
}

/// `alongLine`: one line's fragments in the order the line is read. Dealt
/// into rows by [`same_row`], the rows in the order they were opened, and each
/// row from its far end when its characters are written against the along
/// axis.
fn along_line(view: &View, fragments: Vec<Fragment>, axes: Axes) -> Vec<Fragment> {
    if fragments.len() < 2 {
        return fragments;
    }
    let mut rows: Vec<(Extents, Vec<Fragment>)> = Vec::new();
    for fragment in fragments {
        let extents = extents_of(fragment.bounds, axes);
        match rows.iter_mut().find(|(first, _)| same_row(*first, extents)) {
            Some((_, row)) => row.push(fragment),
            None => rows.push((extents, vec![fragment])),
        }
    }
    rows.into_iter()
        .flat_map(|(_, mut row)| {
            if row.len() < 2 {
                return row;
            }
            let backwards = written_backwards(view, &row, axes);
            let start = |fragment: &Fragment| {
                let extents = extents_of(fragment.bounds, axes);
                if backwards {
                    -extents.along_end
                } else {
                    extents.along_start
                }
            };
            // Stable, as JavaScript's `sort` is.
            row.sort_by(|a, b| by(start(a), start(b)));
            row
        })
        .collect()
}

/// `writtenBackwards`.
fn written_backwards(view: &View, fragments: &[Fragment], axes: Axes) -> bool {
    let (mut forwards, mut backwards) = (0usize, 0usize);
    for range in fragments.iter().flat_map(|fragment| &fragment.ranges) {
        let mut before: Option<f64> = None;
        for index in range.from..range.to {
            let q = char_quad(view, index);
            if !placed(q) || combining(view.codes.get(index).copied().unwrap_or(0)) {
                continue;
            }
            let at = extents_of(q, axes).along_start;
            if let Some(before) = before {
                if at > before {
                    forwards += 1;
                } else if at < before {
                    backwards += 1;
                }
            }
            before = Some(at);
        }
    }
    backwards > forwards
}

/// `usableRuns`: the tags, when they claim every visible character.
fn usable_runs(view: &View) -> Option<&[TaggedRun]> {
    if view.runs.is_empty() {
        return None;
    }
    let mut claimed = vec![false; view.len()];
    for run in &view.runs {
        let to = (run.end as usize).min(view.len());
        for slot in claimed.iter_mut().take(to).skip(run.start as usize) {
            *slot = true;
        }
    }
    for (index, claimed) in claimed.iter().enumerate() {
        if *claimed {
            continue;
        }
        if is_visible(view.codes[index]) && placed(char_quad(view, index)) {
            return None;
        }
    }
    Some(&view.runs)
}

/// `ownership`.
fn ownership(view: &View, runs: &[TaggedRun]) -> Vec<Vec<usize>> {
    let mut owner: Vec<i64> = vec![-1; view.len()];
    for (at, run) in runs.iter().enumerate() {
        let to = (run.end as usize).min(owner.len());
        for slot in owner.iter_mut().take(to).skip(run.start as usize) {
            *slot = at as i64;
        }
    }
    let mut last = -1;
    for slot in &mut owner {
        if *slot == -1 {
            *slot = last;
        } else {
            last = *slot;
        }
    }
    let mut next = if runs.is_empty() { -1 } else { 0 };
    for slot in owner.iter_mut().rev() {
        if *slot == -1 {
            *slot = next;
        } else {
            next = *slot;
        }
    }
    let mut owned: Vec<Vec<usize>> = vec![Vec::new(); runs.len()];
    for (index, at) in owner.iter().enumerate() {
        if let Some(list) = usize::try_from(*at).ok().and_then(|at| owned.get_mut(at)) {
            list.push(index);
        }
    }
    owned
}

/// `within`.
fn within(view: &View, fragments: &[Fragment], owned: &[usize]) -> Vec<Fragment> {
    let mine: std::collections::HashSet<usize> = owned.iter().copied().collect();
    let mut out = Vec::new();
    for fragment in fragments {
        let mut indices = Vec::new();
        let mut bounds: Option<Quad> = None;
        for range in &fragment.ranges {
            for index in range.from..range.to {
                if !mine.contains(&index) {
                    continue;
                }
                indices.push(index);
                let at = char_quad(view, index);
                if !placed(at) {
                    continue;
                }
                match bounds.as_mut() {
                    Some(b) => absorb(b, at),
                    None => bounds = Some(at),
                }
            }
        }
        if indices.is_empty() {
            continue;
        }
        out.push(Fragment {
            ranges: ranges_of(indices),
            bounds: bounds.unwrap_or_else(empty_box),
            turns: fragment.turns,
        });
    }
    out.sort_by_key(|f| f.ranges.first().map_or(0, |r| r.from));
    out
}

#[cfg(test)]
pub(crate) mod tests;
