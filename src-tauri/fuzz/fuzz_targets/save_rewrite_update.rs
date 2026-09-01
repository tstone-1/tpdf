//! `save::rewrite_update` --- a whole document rewritten under an arbitrary plan.
//!
//! The only target here whose subject takes something other than bytes. A plan
//! names pages, turns, crops, marks, note edits, discards and redactions, and
//! every one of those is an index or an object reference **resolved against a
//! document the plan did not come from**. That mismatch is the point: in the
//! application a plan is built from the model of the file it is about, so the
//! two agree by construction and no test ever puts a plan to a document that
//! contradicts it. A file replaced under a reader, a session restored against a
//! different revision, or a worker handed a request it did not compute are all
//! that mismatch arriving for real, and `rewrite_update` is where it has to be
//! refused rather than acted on.
//!
//! # Input layout
//!
//! ```text
//! [4 bytes, little endian: document length n] [n bytes: the document] [the rest: the plan]
//! ```
//!
//! Parsed by hand rather than by deriving `Arbitrary` over a pair, and the
//! reason is seeding. `arbitrary` consumes a struct from the front of the
//! buffer and takes collections' lengths from the back, so a seed file built by
//! concatenating a real PDF with anything is a plan of unpredictable shape over
//! a document truncated at an unpredictable offset. With an explicit length
//! prefix, `seed.py` can put a `testdata/` document in at a known place and the
//! fuzzer still owns every byte on both sides of it.
//!
//! # What is asserted
//!
//! Nothing about the output bytes, deliberately. There is no oracle for "what
//! should this rewrite produce" that is not a second copy of the writer, and
//! this repository has an entry about a writer and its own reader agreeing
//! about a document that is wrong. What is asserted is the property that does
//! not need one: whatever `rewrite_update` returns, it must be a document
//! `lopdf` can load again --- because that is what the coordinator will
//! immediately do with it, and a rewrite that produces bytes only the writer
//! can read is a data-loss defect that returns `Ok`.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target save_rewrite_update --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use lopdf::{Document, LoadOptions};
use tpdf_lib::docmodel::{MarkKind, PageSource, Point, Quad, Size, StampName, Stroke};
use tpdf_lib::edits::{Plan, PlannedDiscard, PlannedMark, PlannedNoteEdit, PlannedRedaction};
use tpdf_lib::save::{self, Job};

/// `encoding::MAX_DECODE`, repeated for the reason `lopdf_load` states.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// The plan as the fuzzer supplies it, before it is turned into a [`Plan`].
///
/// A mirror rather than a `derive(Arbitrary)` on the real type, because the
/// real type is in the application crate and a fuzz target may not add a derive
/// to it. Every field maps to exactly one field of [`Plan`]; the widths are
/// narrowed (`u8` for a page index, a bounded number of quads) so that a random
/// byte string has a real chance of naming a page the document has, rather than
/// spending the whole run being refused for naming page 3,000,000,000.
#[derive(Arbitrary, Debug)]
struct RawPlan {
    baseline: u8,
    view: u8,
    pages: Vec<RawPage>,
    marks: Vec<RawMark>,
    notes: Vec<RawNote>,
    discards: Vec<RawDiscard>,
    redactions: Vec<RawRedaction>,
}

#[derive(Arbitrary, Debug)]
struct RawPage {
    id: u8,
    /// `None` for a page of the file, `Some` for one tpdf made.
    blank: Option<(f64, f64)>,
    baseline: u8,
    turns: u8,
    crop: Option<[f64; 4]>,
}

/// Quarter turns, reduced -- and the reduction is a **found defect being worked
/// around**, not a modelling choice.
///
/// Unreduced, this generator's very first hour produced
/// `attempt to add with overflow` at `save.rs:2628`, in
/// `(page.turns + view % 4) % 4`: both operands are `u8`, `page.turns` is not
/// reduced before the addition, and any value from 253 up overflows. In a
/// release build there are no overflow checks, so the same input turns the page
/// the wrong way instead of panicking.
///
/// It is reduced here because libFuzzer stops on a crash: leaving it would spend
/// every future run re-finding this one defect and reaching nothing behind it.
/// **Delete the `% 4` to reproduce**, which is the whole of the repro.
fn turns_of(raw: u8) -> u8 {
    raw % 4
}

#[derive(Arbitrary, Debug)]
struct RawMark {
    kind: u8,
    at: u8,
    quads: Vec<[f32; 4]>,
    strokes: Vec<Vec<(f32, f32)>>,
    stamp: Option<u8>,
    reply_to: Option<(u32, u16)>,
    color: [f32; 3],
    width: f64,
    author: String,
    note: String,
    made: String,
}

#[derive(Arbitrary, Debug)]
struct RawNote {
    object: (u32, u16),
    body: String,
    made: String,
}

#[derive(Arbitrary, Debug)]
struct RawDiscard {
    object: (u32, u16),
}

#[derive(Arbitrary, Debug)]
struct RawRedaction {
    source: u8,
    shows: Vec<u16>,
    text_objects: u16,
    areas: Vec<[f32; 4]>,
    taking: Vec<String>,
    images: Vec<u16>,
    image_objects: u16,
    form_shows: Vec<(u16, u16)>,
    form_text_objects: Vec<(u16, u16)>,
}

/// The ten kinds, in declaration order. A `u8` chooses one by remainder, so
/// every value of the byte selects a kind and none is wasted on a refusal that
/// would never reach the writer.
fn kind_of(raw: u8) -> MarkKind {
    match raw % 10 {
        0 => MarkKind::Highlight,
        1 => MarkKind::Underline,
        2 => MarkKind::StrikeOut,
        3 => MarkKind::Note,
        4 => MarkKind::Squiggly,
        5 => MarkKind::Square,
        6 => MarkKind::Ellipse,
        7 => MarkKind::TextBox,
        8 => MarkKind::Ink,
        _ => MarkKind::Stamp,
    }
}

fn stamp_of(raw: u8) -> StampName {
    match raw % 4 {
        0 => StampName::Approved,
        1 => StampName::Confidential,
        2 => StampName::Draft,
        _ => StampName::Final,
    }
}

/// The plan, and the job it is for beside it.
///
/// Both come out of one value because a request carries both. `Job` is a save or
/// a print, and a print carries the reader's own rotation --- `% 4` because it is
/// quarter turns, and a raw byte would spend 63 of every 64 values on a number no
/// caller can produce. The odd/even split reaches both variants, which matters:
/// the encryption refusal is a property of `Job::Print` alone, so a target that
/// only ever built a save could not reach it.
fn plan_of(raw: RawPlan) -> (Plan, Job) {
    let job = if raw.view % 2 == 0 {
        Job::Save
    } else {
        Job::Print {
            view: (raw.view / 2) % 4,
        }
    };
    let plan = Plan {
        baseline: u32::from(raw.baseline),
        // Never anything else, and that is a property rather than a
        // simplification: `Plan::opened_as` is `#[serde(skip)]` precisely so a
        // plan that crossed the worker boundary always carries `None`, and this
        // target stands in for that caller.
        opened_as: None,
        pages: raw
            .pages
            .into_iter()
            .map(|page| tpdf_lib::edits::PageView {
                id: u64::from(page.id),
                source: match page.blank {
                    Some((width, height)) => PageSource::Blank(Size { width, height }),
                    None => PageSource::Baseline(u32::from(page.baseline)),
                },
                turns: turns_of(page.turns),
                crop: page.crop,
            })
            .collect(),
        marks: raw
            .marks
            .into_iter()
            .map(|mark| PlannedMark {
                kind: kind_of(mark.kind),
                at: u32::from(mark.at),
                quads: mark
                    .quads
                    .into_iter()
                    .map(|q| Quad {
                        left: q[0],
                        top: q[1],
                        right: q[2],
                        bottom: q[3],
                    })
                    .collect(),
                strokes: mark
                    .strokes
                    .into_iter()
                    .map(|points| Stroke {
                        points: points.into_iter().map(|(x, y)| Point { x, y }).collect(),
                    })
                    .collect(),
                stamp: mark.stamp.map(stamp_of),
                reply_to: mark.reply_to,
                color: mark.color,
                width: mark.width,
                author: mark.author,
                note: mark.note,
                made: mark.made,
            })
            .collect(),
        redactions: raw
            .redactions
            .into_iter()
            .map(|region| PlannedRedaction {
                source: u32::from(region.source),
                shows: region.shows.into_iter().map(usize::from).collect(),
                text_objects: usize::from(region.text_objects),
                areas: region.areas,
                taking: region.taking,
                images: region.images.into_iter().map(usize::from).collect(),
                image_objects: usize::from(region.image_objects),
                form_shows: region
                    .form_shows
                    .into_iter()
                    .map(|(form, at)| (usize::from(form), usize::from(at)))
                    .collect(),
                form_text_objects: region
                    .form_text_objects
                    .into_iter()
                    .map(|(form, count)| (usize::from(form), usize::from(count)))
                    .collect(),
            })
            .collect(),
        notes: raw
            .notes
            .into_iter()
            .map(|note| PlannedNoteEdit {
                object: note.object,
                body: note.body,
                made: note.made,
            })
            .collect(),
        discards: raw
            .discards
            .into_iter()
            .map(|discard| PlannedDiscard {
                object: discard.object,
            })
            .collect(),
    };
    (plan, job)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let length = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if length > data.len() - 4 {
        return;
    }
    let (document, tail) = data[4..].split_at(length);

    let Ok(raw) = RawPlan::arbitrary_take_rest(Unstructured::new(tail)) else {
        return;
    };
    let (plan, job) = plan_of(raw);

    let Ok(written) = save::rewrite_update(document, &plan, job, None) else {
        // Refusing is the expected answer for nearly every plan a fuzzer builds,
        // and it is the *correct* one: a plan that names pages this document
        // does not have has to be turned down rather than approximated.
        return;
    };

    // The one property that needs no second implementation of the writer: the
    // coordinator loads what came back, so bytes only this writer can read are
    // a silent data-loss defect that returned `Ok`.
    if Document::load_mem_with_options(
        &written,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: None,
            ..Default::default()
        },
    )
    .is_err()
    {
        panic!(
            "rewrite_update accepted a plan and produced {} bytes that lopdf \
             cannot load back",
            written.len()
        );
    }
});
