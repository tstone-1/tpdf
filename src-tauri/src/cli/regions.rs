//! A search hit, turned into the regions the window would mark for removal.
//!
//! **A restatement of the viewer's route, not a second opinion.** In the
//! window, *Mark all matches for redaction* takes each hit through three steps
//! in TypeScript: `matchHalves` (`src/lib/search.ts`) splits a hit that runs
//! over a page break into one half per page; `runsFor` (`src/lib/text.ts`)
//! merges a half's characters into one rectangle per run of text on a line;
//! and `areasFrom` (`src/lib/selection.ts`) orders each rectangle's sides and
//! drops one too thin to cover a glyph's centre. Each rectangle is then one
//! `Edits.redact`. The command-line tool has no webview, so the three are said
//! again here, function for function, and every choice worth arguing about is
//! argued there and not here.
//!
//! A restatement is a second copy, and `docs/TRAPS.md` records what second
//! copies do. So it is held to the original the way `reading.rs` is:
//! `cli::tests` writes every case in [`sample`] with the regions this file
//! computes to `testdata/cli/regions.json` (`TPDF_CLI_SAMPLES=write`), and
//! `src/lib/cliregions.test.ts` asks the TypeScript the same questions and
//! compares number for number. A rule changed on either side is a red test
//! there.
//!
//! ## Numbers as the webview sees them, both ways
//!
//! A character box reaches `runsFor` as the shortest decimal of its `f32`,
//! parsed as an `f64` --- `reading.rs`'s module note, and
//! [`crate::reading::webview_number`] is that route. A region travels back the
//! other way: `areasFrom` produces `f64`s, `Edits.redact` sends them as JSON,
//! and Rust parses each as the `f32` nearest the decimal. [`ipc_f32`] is that
//! route. Neither cast is the same number at every tie, and a region a
//! hundredth of a point off is a region that can miss a glyph's centre.
//!
//! The window never crops or turns anything here: `matchQuadsByPage` measures
//! from the unturned text and `outOfCrop` undoes the reader's crop, and a
//! document the tool has just opened has neither.

use crate::reading::webview_number;
use crate::search::Match;
use crate::text::PageText;

/// `selection.ts`'s `MIN_REDACTION_SIDE`, in points.
pub const MIN_REDACTION_SIDE: f64 = 0.5;

/// `search.ts`'s `MAX_MATCHES_TO_MARK`: more matches than this in one run are
/// refused rather than marked, for the reason written there --- a truncated
/// list reports a document clean with words still in it, and a list this long
/// is not one anybody reviews.
pub const MAX_MATCHES_TO_MARK: usize = 500;

/// One page's share of a hit, as `matchQuadsByPage` builds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Half {
    /// The page, counted from 0.
    pub page: u32,
    /// First character index.
    pub from: u32,
    /// Exclusive end, or `None` for "to the end of the page" --- the
    /// TypeScript's `Infinity`, which `runsFor` clamps.
    pub to: Option<u32>,
}

/// `matchHalves`: each hit as one half per page it covers.
#[must_use]
pub fn halves(matches: &[Match]) -> Vec<Half> {
    let mut out = Vec::new();
    for hit in matches {
        match hit.end_page {
            None => out.push(Half {
                page: hit.page,
                from: hit.start,
                to: Some(hit.end),
            }),
            Some(end_page) => {
                out.push(Half {
                    page: hit.page,
                    from: hit.start,
                    to: None,
                });
                out.push(Half {
                    page: end_page,
                    from: 0,
                    to: Some(hit.end),
                });
            }
        }
    }
    out
}

/// `text.ts`'s `Quad`, in the viewer's numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Quad {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// `charQuad`.
fn char_quad(boxes: &[f64], index: usize) -> Quad {
    let at = |k: usize| boxes.get(index * 4 + k).copied().unwrap_or(0.0);
    Quad {
        left: at(0),
        top: at(1),
        right: at(2),
        bottom: at(3),
    }
}

/// `isPlaced`: extent on **both** axes, unlike `reading.ts`'s `placed`.
fn is_placed(q: Quad) -> bool {
    q.right > q.left && q.bottom > q.top
}

/// `characterTurns`.
fn character_turns(text: &PageText, index: usize) -> u32 {
    (u32::from(text.quarter_turns) + u32::from(text.char_turns.get(index).copied().unwrap_or(0)))
        % 4
}

/// `onSameLine`.
fn on_same_line(a: Quad, b: Quad, sideways: bool) -> bool {
    let (a0, a1, b0, b1) = if sideways {
        (a.left, a.right, b.left, b.right)
    } else {
        (a.top, a.bottom, b.top, b.bottom)
    };
    let overlap = a1.min(b1) - a0.max(b0);
    let shorter = (a1 - a0).min(b1 - b0);
    shorter > 0.0 && overlap / shorter > 0.5
}

/// `runsFor`: one rectangle per run of text on a line, flat, four numbers each.
#[must_use]
pub fn runs_for(text: &PageText, from: u32, to: Option<u32>) -> Vec<f64> {
    let boxes: Vec<f64> = text.boxes.iter().copied().map(webview_number).collect();
    let end = to.map_or(text.codes.len(), |to| (to as usize).min(text.codes.len()));
    let mut runs: Vec<Quad> = Vec::new();
    let mut current_turns: Option<u32> = None;
    for index in (from as usize)..end {
        let quad = char_quad(&boxes, index);
        if !is_placed(quad) {
            continue;
        }
        let turns = character_turns(text, index);
        if let (Some(current), Some(was)) = (runs.last_mut(), current_turns) {
            if was == turns && on_same_line(*current, quad, turns % 2 == 1) {
                current.left = current.left.min(quad.left);
                current.right = current.right.max(quad.right);
                current.top = current.top.min(quad.top);
                current.bottom = current.bottom.max(quad.bottom);
                continue;
            }
        }
        runs.push(quad);
        current_turns = Some(turns);
    }
    runs.iter()
        .flat_map(|q| [q.left, q.top, q.right, q.bottom])
        .collect()
}

/// An `f64` as `Edits.redact` delivers it to Rust: its JSON decimal, parsed as
/// the nearest `f32`.
#[must_use]
pub fn ipc_f32(value: f64) -> f32 {
    // Rust's `Display` for `f64` is the shortest decimal that round-trips,
    // which is the digits `JSON.stringify` writes; `serde_json` parses the
    // decimal, not the `f64`, into the `f32`.
    format!("{value}").parse().unwrap_or(f32::NAN)
}

/// `areasFrom`: each run as `[left, top, right, bottom]`, sides ordered, and a
/// run thinner than [`MIN_REDACTION_SIDE`] either way dropped.
#[must_use]
pub fn areas_from(quads: &[f64]) -> Vec<[f32; 4]> {
    let mut areas = Vec::new();
    for run in quads.chunks(4) {
        let [a, b, c, d] = run else {
            continue;
        };
        let (left, right) = (a.min(*c), a.max(*c));
        let (top, bottom) = (b.min(*d), b.max(*d));
        if right - left < MIN_REDACTION_SIDE {
            continue;
        }
        if bottom - top < MIN_REDACTION_SIDE {
            continue;
        }
        areas.push([ipc_f32(left), ipc_f32(top), ipc_f32(right), ipc_f32(bottom)]);
    }
    areas
}

/// One page's regions for the halves on it, in the order the halves come.
///
/// `matchQuadsByPage`'s inner loop and `redactMatches`' `areasFrom`, joined:
/// the page's quads are gathered half by half and then turned into areas.
#[must_use]
pub fn regions_on(text: &PageText, page: u32, halves: &[Half]) -> Vec<[f32; 4]> {
    let mut quads: Vec<f64> = Vec::new();
    for half in halves.iter().filter(|half| half.page == page) {
        quads.extend(runs_for(text, half.from, half.to));
    }
    areas_from(&quads)
}

/// Every case the parity test asks about, with what this file answers.
///
/// Reading's eighteen pages --- every rotation, turned characters, unplaced
/// boxes, an empty page --- each asked for the whole page, a middle range, a
/// range running off the end and an empty one; then `areasFrom`'s own edges
/// on raw quads; then hits, some over a page break, through [`halves`].
#[cfg(test)]
#[must_use]
pub fn sample() -> serde_json::Value {
    let through_ipc = |value: &dyn erased::Json| -> serde_json::Value {
        serde_json::from_str(&value.text()).expect("parses")
    };
    let mut runs = Vec::new();
    // Reading's `split-line` pages are left out: they ask in which order the
    // halves of one line are read, and a region is found by index, so they
    // would be three more pages of the kind already here.
    let pages = crate::reading::tests::cases()
        .into_iter()
        .filter(|case| !case.name.starts_with("split-line"));
    for case in pages {
        let mut text = case.text;
        text.extract_ms = 0.0;
        let len = u32::try_from(text.codes.len()).expect("a short page");
        let ranges: [(u32, Option<u32>); 4] = [
            (0, None),
            (len / 4, Some(len - len / 4)),
            (len / 2, Some(len + 10)),
            (len / 2, Some(len / 2)),
        ];
        for (from, to) in ranges {
            let quads = runs_for(&text, from, to);
            runs.push(serde_json::json!({
                "name": case.name,
                "text": through_ipc(&text),
                "from": from,
                "to": to,
                "quads": quads,
                "areas": through_ipc(&areas_from(&quads)),
            }));
        }
    }
    let raw: [&[f64]; 6] = [
        &[10.0, 20.0, 50.0, 30.0],
        &[50.0, 30.0, 10.0, 20.0],
        &[10.0, 20.0, 10.4, 30.0],
        &[10.0, 20.0, 50.0, 20.3],
        &[10.0, 20.0, 50.0, 30.0, 60.0, 70.0],
        &[0.1, 0.2, 0.700_000_000_000_000_1, 595.275_573_730_468_8],
    ];
    let areas: Vec<serde_json::Value> = raw
        .iter()
        .map(|quads| {
            serde_json::json!({
                "quads": quads,
                "areas": through_ipc(&areas_from(quads)),
            })
        })
        .collect();
    let hit = |page: u32, start: u32, end: u32, end_page: Option<u32>| Match {
        page,
        start,
        end,
        end_page,
        before: String::new(),
        hit: String::new(),
        after: String::new(),
    };
    let matches = vec![
        hit(0, 3, 9, None),
        hit(0, 40, 2, Some(1)),
        hit(1, 5, 7, None),
        hit(4, 0, 12, Some(5)),
    ];
    serde_json::json!({
        "min_redaction_side": MIN_REDACTION_SIDE,
        "max_matches_to_mark": MAX_MATCHES_TO_MARK,
        "runs": runs,
        "areas": areas,
        "matches": through_ipc(&matches),
        "halves": halves(&matches),
    })
}

/// `serde_json::to_string` behind one call, for [`sample`].
#[cfg(test)]
mod erased {
    pub trait Json {
        fn text(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn text(&self) -> String {
            serde_json::to_string(self).expect("serialises")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(boxes: &[[f32; 4]], quarter_turns: u8, char_turns: Vec<u8>) -> PageText {
        PageText {
            codes: vec![u32::from('x'); boxes.len()],
            boxes: boxes.iter().flatten().copied().collect(),
            width_pt: 600.0,
            height_pt: 800.0,
            quarter_turns,
            char_turns,
            ..PageText::default()
        }
    }

    #[test]
    fn a_hit_over_a_page_break_is_one_half_on_each_page() {
        let hit = Match {
            page: 2,
            start: 30,
            end: 4,
            end_page: Some(3),
            before: String::new(),
            hit: String::new(),
            after: String::new(),
        };
        assert_eq!(
            halves(&[hit]),
            vec![
                Half {
                    page: 2,
                    from: 30,
                    to: None
                },
                Half {
                    page: 3,
                    from: 0,
                    to: Some(4)
                },
            ]
        );
    }

    #[test]
    fn characters_on_one_line_join_one_run_and_a_new_line_starts_another() {
        let page = text(
            &[
                [10.0, 10.0, 20.0, 20.0],
                [20.0, 11.0, 30.0, 21.0],
                [10.0, 40.0, 20.0, 50.0],
            ],
            0,
            Vec::new(),
        );
        assert_eq!(
            runs_for(&page, 0, None),
            vec![10.0, 10.0, 30.0, 21.0, 10.0, 40.0, 20.0, 50.0]
        );
    }

    #[test]
    fn a_character_with_no_extent_on_either_axis_is_not_placed() {
        // Width without height: `reading.rs`'s `placed` would keep it, and
        // `isPlaced` does not.
        let page = text(&[[10.0, 10.0, 20.0, 10.0]], 0, Vec::new());
        assert!(runs_for(&page, 0, None).is_empty());
    }

    #[test]
    fn a_turned_character_does_not_join_an_upright_run() {
        // The second box lies inside the first on both axes, so only the turn
        // can keep the two apart: read sideways or upright, they overlap.
        let boxes = [[10.0, 10.0, 20.0, 20.0], [12.0, 12.0, 18.0, 18.0]];
        assert_eq!(runs_for(&text(&boxes, 0, vec![0, 1]), 0, None).len(), 8);
        // The control: the same two boxes, both upright, are one run.
        assert_eq!(runs_for(&text(&boxes, 0, Vec::new()), 0, None).len(), 4);
    }

    #[test]
    fn on_a_sideways_page_lines_are_told_apart_across() {
        // Two characters stacked down the page at one x: one line when the
        // lines run sideways, two when they do not.
        let stacked = [[10.0, 10.0, 20.0, 20.0], [10.0, 20.0, 20.0, 30.0]];
        assert_eq!(runs_for(&text(&stacked, 1, Vec::new()), 0, None).len(), 4);
        assert_eq!(runs_for(&text(&stacked, 0, Vec::new()), 0, None).len(), 8);
    }

    #[test]
    fn a_range_is_clamped_to_the_page_and_an_open_end_runs_to_it() {
        let page = text(
            &[[10.0, 10.0, 20.0, 20.0], [10.0, 40.0, 20.0, 50.0]],
            0,
            Vec::new(),
        );
        assert_eq!(runs_for(&page, 1, Some(99)), runs_for(&page, 1, None));
        assert_eq!(runs_for(&page, 1, None).len(), 4);
    }

    #[test]
    fn a_run_thinner_than_half_a_point_either_way_is_not_a_region() {
        assert!(areas_from(&[10.0, 20.0, 10.4, 30.0]).is_empty());
        assert!(areas_from(&[10.0, 20.0, 50.0, 20.4]).is_empty());
        assert_eq!(areas_from(&[10.0, 20.0, 10.5, 20.5]).len(), 1);
    }

    #[test]
    fn sides_are_ordered_and_a_partial_run_is_dropped() {
        assert_eq!(
            areas_from(&[50.0, 30.0, 10.0, 20.0, 1.0, 2.0]),
            vec![[10.0, 20.0, 50.0, 30.0]]
        );
    }

    #[test]
    fn a_region_is_narrowed_through_its_decimal_not_by_a_cast() {
        // Halfway between 1.0 and the next `f32`: a cast rounds to even and
        // gives 1.0, while the decimal `JSON.stringify` writes lies just above
        // the midpoint and parses up. The window's IPC does the second.
        let midpoint = 1.0 + 2f64.powi(-24);
        assert_eq!(ipc_f32(midpoint), 1.0_f32 + f32::EPSILON);
        assert_eq!(midpoint as f32, 1.0_f32);
    }

    #[test]
    fn a_region_arrives_as_the_f32_nearest_its_decimal() {
        // `595.2755737304688` is `595.2756f32` widened; its decimal parses back
        // to the same `f32`, which is what the window's IPC delivers.
        assert_eq!(ipc_f32(595.275_573_730_468_8), 595.2756_f32);
    }
}
