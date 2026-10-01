//! Centred lines: which of them an edit can keep centred.
//!
//! The writer keeps a centred line centred by starting its replacement half the
//! change in width earlier (`layout::prepare`). That is sound only while the run
//! is the whole line, so a centred run with other text on its line stays
//! read-only here, before any edit is offered.
use super::{Inspection, Run};

/// The refusal for an edit that carries no layout: written at the run's own
/// origin, it would leave the line off centre.
pub(super) const NEEDS_LAYOUT: &str =
    "a centred line needs a text layout to stay centred; in an edit plan, name a \"font\"";

// The same test `layout::Axis::beside` applies: more than half of the shorter
// span across the line is shared.
fn beside(run: &Run, other: &[f32; 4], cross: usize) -> bool {
    let (low, high) = (run.display_rect[cross], run.display_rect[cross + 2]);
    let shared = other[cross + 2].min(high) - other[cross].max(low);
    let shorter = (other[cross + 2] - other[cross]).min(high - low);
    shared > 0.1 && shared > shorter / 2.
}

/// Moves every centred run that shares its line to the read-only text.
/// `turned` says the page is displayed a quarter turn from its own space.
pub(super) fn settle(page: &mut Inspection, turned: bool) {
    let shared: Vec<u32> = page
        .runs
        .runs
        .iter()
        .filter(|run| page.centred.contains(&run.operator))
        .filter(|run| {
            let along_x = (run.matrix[0].abs() > run.matrix[1].abs()) != turned;
            let cross = usize::from(along_x);
            page.runs
                .runs
                .iter()
                .chain(&page.preserved)
                .filter(|other| other.operator != run.operator && !other.text.trim().is_empty())
                .map(|other| &other.display_rect)
                .chain(&page.form_text_bounds)
                .any(|other| beside(run, other, cross))
        })
        .map(|run| run.operator)
        .collect();
    page.centred.retain(|operator| !shared.contains(operator));
    page.runs.runs.retain(|run| {
        if shared.contains(&run.operator) {
            page.preserved.push(run.clone());
            false
        } else {
            true
        }
    });
}
