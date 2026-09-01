//! Building the PDF that gets handed to the printer.
//!
//! Printing hands a **PDF** to the operating system, never pixels. That is not a
//! convenience: measured on macOS, `cupsfilter -d <queue>` against a configured
//! AirPrint device returned output byte-identical to the input, so the document
//! reaches the printer untouched and anything we rendered first could only throw
//! information away. Where a printer is not PDF-native the conversion is CUPS's
//! own, and pre-rasterising at a resolution we guessed would be strictly worse
//! than letting it choose (docs/PLAN.md, Phase 1 → Print).
//!
//! So the only question left is *which* PDF. Three cases:
//!
//! - **Everything, unrotated.** The file itself, byte for byte. Rewriting a
//!   document in order to change nothing about it is pure risk: `lopdf` drops
//!   encryption silently (AGENTS.md), a rewrite reflows structure, and the
//!   printer was going to receive these exact bytes anyway.
//! - **A page range.** Pages are deleted **in place** rather than re-parented
//!   under a fresh `/Pages`. That matters: `/Resources`, `/MediaBox`, `/CropBox`
//!   and `/Rotate` are *inheritable*, so a page moved out from under its parent
//!   loses whatever it was inheriting --- and a page that has lost its resources
//!   still counts as a page, still opens, and prints blank.
//! - **A range in an order the file does not have.** Here the tree *is* rebuilt,
//!   because a permutation cannot be expressed any other way, and the four
//!   inheritable attributes are written onto each page first so that nothing is
//!   lost on the way --- `pagetree::reorder_pages`. Only when the orders differ:
//!   a subset in document order still takes the in-place path above.
//! - **A rotated view.** The reader asked for what they are looking at, so the
//!   view rotation is composed onto each page's own `/Rotate` --- the effective
//!   one, resolved up the `/Parent` chain, not the literal one, which is absent
//!   on exactly the documents that inherit it.
//!
//! The outline is dropped whenever pages are. Its destinations name pages that
//! are no longer in the file, and a table of contents that points at nothing is
//! worse than none --- the same reason a bounded outline walk reports what it cut
//! rather than presenting a partial tree as whole.

use std::path::Path;

use lopdf::{Document, LoadOptions};

use crate::docmodel::PageSource;
use crate::edits::Plan;
use crate::pagetree::{agreed_turns, apply_turns};
use crate::sweep;

use crate::encoding::MAX_DECODE;

/// One page of a print job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePlan {
    /// Which page of the file, one-based, as `lopdf` numbers them.
    pub number: u32,
    /// Quarter-turns clockwise an **edit** has applied to it, 0 to 3.
    ///
    /// Composed with the job's view rotation rather than replacing it: a reader
    /// who turned page 3 in the document and is looking at the whole thing
    /// sideways asked for both.
    pub turns: u8,
}

/// Which pages to print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pages {
    /// Every page, in document order, exactly as the file has them.
    All,
    /// Exactly these, one-based, printed **in the order they are listed here**.
    ///
    /// That is a change, and the old behaviour was a trap waiting for its
    /// trigger: [`build`] produced a subset by deleting the pages nobody asked
    /// for, so the survivors kept the positions the file gave them and `[3, 1]`
    /// printed page 1 then page 3. Harmless while nothing could reorder a
    /// document, and silently wrong the day `Command::Move` was wired --- a
    /// reader who rearranged a document and pressed print would have got the old
    /// order on paper, with nothing downstream able to say so: [`expect_pages`]
    /// compares how many pages came out, never which.
    ///
    /// A listed order that is already the document's costs nothing: `build`
    /// rewrites the page tree only when the two differ.
    Only(Vec<PagePlan>),
    /// The plan carries a page tpdf made, which no list of file page numbers can
    /// name.
    ///
    /// **A variant rather than a shorter list**, because the alternative is a
    /// lie: a `Pages::Only` that quietly leaves the made pages out is a complete
    /// looking selection of the wrong document, and `docs/TRAPS.md` records a
    /// list with a silently missing element as the shape that survives review.
    ///
    /// Nothing prints from it. [`route`] answers [`Route::Working`] for any plan
    /// that produces one --- the working document is written by `save.rs`, which
    /// is the only writer that can make the page --- and [`build`] refuses it.
    /// That refusal is unreachable today and is kept as the type carrying the
    /// constraint, which is what this repository does with a guard it cannot
    /// reach.
    Unlistable,
}

/// What to print, and how it should be oriented.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub pages: Pages,
    /// Quarter-turns clockwise the *view* is rotated by, 0 to 3.
    pub turns: u8,
}

impl Job {
    /// Whether this job is the document exactly as it already is on disk.
    ///
    /// [`Pages::All`] rather than "a selection naming every page": a plan that
    /// lists all of them with no turns describes the same document, and this
    /// question decides whether the file is handed over **byte for byte**. A
    /// rewrite that changes nothing is the risk `lopdf` dropping encryption is
    /// about, so the caller says which it means rather than this inferring it
    /// from a list it would have to parse the document to check.
    #[must_use]
    pub fn is_passthrough(&self) -> bool {
        self.pages == Pages::All && self.turns % 4 == 0
    }
}

/// What a print job should contain: the reader's range, or the working document.
///
/// Two routes in, and they are exclusive on purpose. `pages` is an explicit
/// range --- what a print panel's "pages 2 to 4" will be, and what the window
/// harness uses to exercise the refusals --- and it says nothing about edits.
/// `plan` is the working document as the model holds it, and then which pages,
/// in what order, and how each is turned all come from there.
///
/// **A document nobody has edited is [`Pages::All`]**, which is what hands the
/// file to the printer byte for byte. Spelling the same document out as a
/// selection would rewrite it to produce itself --- and `lopdf` drops encryption
/// on the way through, so that is not a rewrite worth risking for a job that was
/// always going to be the file.
///
/// Pure, and takes the plan rather than the model that holds it: what decides the
/// shape of a job is worth being able to test without a document open.
#[must_use]
pub fn select(plan: Option<&Plan>, pages: Option<Vec<u32>>) -> Pages {
    if let Some(wanted) = pages {
        return Pages::Only(
            wanted
                .into_iter()
                .map(|number| PagePlan { number, turns: 0 })
                .collect(),
        );
    }
    let Some(plan) = plan else {
        return Pages::All;
    };
    if plan.is_identity() {
        return Pages::All;
    }
    let mut listed = Vec::with_capacity(plan.pages.len());
    for page in &plan.pages {
        let PageSource::Baseline(number) = page.source else {
            return Pages::Unlistable;
        };
        listed.push(PagePlan {
            // One-based, as `lopdf` numbers pages; a `Baseline` is the
            // zero-based baseline page the model works in.
            number: number + 1,
            turns: page.turns,
        });
    }
    Pages::Only(listed)
}

/// Where a job's bytes come from.
///
/// **Three producers, and until 2026-08-22 the middle one did not exist** ---
/// which is the whole of the defect this enum was added to close. Everything
/// that was not handed over byte for byte went through [`build`], which grew its
/// own page walk when printing came first and needed a subset of what saving
/// does. The two then drifted exactly as `docs/TRAPS.md` describes two copies of
/// a distinction drifting: `save.rs` learned to write marks and crops, this did
/// not, and nothing compared them. A reader who highlighted a paragraph and
/// pressed Print got paper with no highlight on it.
///
/// Naming the three rather than deciding inside the command is what makes the
/// decision testable at all: a check inside a Tauri command has no failing case
/// a test can reach, which this repository has paid for twice.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// The file itself, unparsed. Only for a document nobody has changed.
    Passthrough,
    /// The working document --- pages, order, turns, crops **and marks** ---
    /// produced by the same writer a save uses. `save::print_bytes`.
    Working,
    /// The pages the reader typed into a range, built by [`build`].
    ///
    /// It carries no plan, so it carries no marks and no crops: an explicit
    /// range says nothing about edits, which is what [`select`] has always
    /// documented. That is a gap and it is a narrower one than it was --- see
    /// the *Not done* in `docs/PLAN.md` §8.
    Range(Job),
}

/// Which producer a job should use.
///
/// Pure, and takes the same two inputs [`select`] does plus the view rotation,
/// so the whole decision can be exercised without a document open.
///
/// The order of the arms is the order of the questions. A document nobody has
/// changed is the file, whatever else is true --- that is the passthrough, and
/// it is what keeps an encrypted document printable, since parsing one would
/// refuse. A reader's explicit range wins next, because it is an instruction
/// about pages and the working document is not. Everything else is the working
/// document.
#[must_use]
pub fn route(plan: Option<&Plan>, pages: Option<Vec<u32>>, view: u8) -> Route {
    let explicit = pages.is_some();
    let job = Job {
        pages: select(plan, pages),
        turns: view,
    };
    if job.is_passthrough() {
        return Route::Passthrough;
    }
    if explicit || plan.is_none() {
        return Route::Range(job);
    }
    Route::Working
}

/// Produces the bytes to hand to the print system.
///
/// # Errors
///
/// A page number outside the document, or an empty selection. Both are refused
/// rather than repaired: a range the reader typed is an instruction, and
/// silently printing the pages that happened to exist --- or nothing at all ---
/// is the kind of plausible wrong answer that only shows up on paper.
pub fn build(source: &Path, job: &Job) -> Result<Vec<u8>, String> {
    if job.is_passthrough() {
        return std::fs::read(source).map_err(|e| format!("could not read {source:?}: {e}"));
    }

    let mut doc = Document::load_with_options(
        source,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse {source:?}: {e}"))?;

    // **An encrypted document is printed whole or not at all**, and the reason
    // is that this branch reserialises: `lopdf`'s full writer emits every object
    // in the clear and drops the `/Encrypt` dictionary with it, so a job built
    // here is a decrypted copy of the reader's document. That reaches a printer,
    // the platform's own PDF reader, and --- through Print to PDF, which is how
    // most people make a copy --- a file on disk. Measured 2026-08-23 before the
    // guard: a selection of one page from `incr-encrypted-open.pdf` produced
    // 1,278 bytes with the encryption gone and nothing said.
    //
    // `is_passthrough` was doing this job and only for the whole document: it
    // hands the file over byte for byte, which is correct and stays correct.
    // What it never covered is a selection or a turn, and the comment on it says
    // "a rewrite that changes nothing is the risk" --- the risk is also a rewrite
    // that changes something.
    //
    // No password is needed to decide this, which is why none is taken.
    // `was_encrypted` answers for a document `lopdf` opened (an empty user
    // password, the commonest case) and `is_encrypted` for one it could not, and
    // both are refused: even with the key, this writer cannot put the encryption
    // back. An append could, and a page selection is not appendable.
    if doc.was_encrypted() || doc.is_encrypted() {
        return Err(format!(
            "{source:?} is encrypted, and printing a selection of it would produce an \
             unencrypted copy. Print the whole document instead."
        ));
    }

    let table = doc.get_pages();
    let present: Vec<u32> = table.keys().copied().collect();
    let wanted = resolve(&job.pages, &present)?;

    // Resolved to object ids **before** anything is dropped, and that ordering is
    // load-bearing: `get_pages` numbers the pages it finds from 1, so deleting
    // page 2 of four renumbers the old page 4 to 3. Reading the table afterwards
    // therefore looks up a number that now names a different page --- or, as here,
    // no page at all, so the turn was silently dropped. Object ids do not move.
    // Caught by `a_third_parser_checks_a_job_built_from_a_document_we_did_not_write`,
    // which keeps the first and last pages of `rotated.pdf`: the last came back at
    // its own rotation rather than a quarter past it.
    let view = i16::from(job.turns % 4);
    let plan: Vec<_> = wanted
        .iter()
        .filter_map(|page| {
            let id = table.get(&page.number).copied()?;
            Some((id, ((i16::from(page.turns) + view).rem_euclid(4)) as u8))
        })
        .collect();

    let kept: Vec<u32> = wanted.iter().map(|page| page.number).collect();
    let dropped: Vec<u32> = present
        .iter()
        .copied()
        .filter(|number| !kept.contains(number))
        .collect();
    // Only when the reader's order differs from the file's --- `reorder_pages`
    // flattens the page tree, and doing that to a job that is merely a *subset*
    // would rewrite the ancestry of every page for nothing. The predicate is
    // this caller's, because `print` learns it by comparing the numbers it just
    // resolved while `save` is told it by the model; the action below is shared.
    let order: Vec<_> = plan.iter().map(|(id, _)| *id).collect();
    let moved = wanted.windows(2).any(|two| two[0].number >= two[1].number);
    crate::pagetree::materialise(&mut doc, &dropped, moved.then_some(order.as_slice()))?;

    // Per page rather than per document, because an edit turns one page and the
    // view turns all of them --- and the two add. Applied per *object*, since a
    // `/Kids` array may name one twice; `pagetree::agreed_turns` refuses a plan
    // asking one object for two different angles rather than applying whichever
    // it met first.
    apply_turns(&mut doc, &agreed_turns(&plan)?)?;

    sweep::collect(&mut doc)?;

    crate::save::serialise(&mut doc, "the print job")
}

/// Checks a built job against what was asked for, before it reaches paper.
///
/// `found` comes from an independent parser, never from the writer that
/// produced the bytes --- see `print_macos::read` for why that is the whole
/// point. `expected` is `None` for "everything", where there is no count to
/// compare against and the only wrong answer that can be recognised is nothing
/// at all.
///
/// # Errors
///
/// A count that disagrees, or an empty job.
pub fn expect_pages(found: usize, expected: Option<usize>) -> Result<(), String> {
    match expected {
        Some(expected) if found != expected => Err(format!(
            "the print job has {found} pages, not the {expected} asked for"
        )),
        None if found == 0 => Err("the print job came out empty".into()),
        _ => Ok(()),
    }
}

/// Which sheets of a built job a print panel's page range names, zero-based.
///
/// **This is not [`select`], and the difference is which document the numbers
/// are about.** `select` decides what goes *into* a job --- pages of the file,
/// composed with the reader's edits --- and runs before a byte is written. This
/// runs after: the job exists, a panel has shown it to the reader, and they have
/// typed a range over the sheets in front of them. So `1` here is the job's first
/// sheet whatever page of the original it came from, which is what a reader
/// looking at a print preview means by it.
///
/// `None` is every sheet, which is what a panel returns when the reader left the
/// range alone or the field was disabled.
///
/// **Refused rather than repaired**, for the reason [`build`] gives: a range is
/// an instruction, and printing the sheets that happened to be inside it is the
/// plausible wrong answer that only shows up on paper. The panel validates
/// against the bounds it was given, so a bad pair arriving here means the bounds
/// were wrong rather than the reader.
///
/// **Nothing on macOS calls this, and that is not an oversight.** `NSPrintPanel`
/// applies its own range to the document it was handed, so the range never
/// reaches our code there; `print_win::present` has to apply it itself. The
/// function lives here rather than in `print_win.rs` so that the arithmetic --- the
/// half that decides which page comes out --- is compiled and tested on every
/// platform instead of only the one that cannot run the tests locally.
///
/// # Errors
///
/// A range running backwards, starting before the first sheet, or ending past
/// the last.
pub fn sheets(range: Option<(u32, u32)>, count: u32) -> Result<Vec<u32>, String> {
    let Some((first, last)) = range else {
        return Ok((0..count).collect());
    };
    if first > last {
        return Err(format!("pages {first} to {last} run backwards"));
    }
    if first < 1 {
        return Err("there is no page 0 to print from".into());
    }
    if last > count {
        return Err(format!("page {last} is not in this job, which has {count}"));
    }
    Ok((first - 1..last).collect())
}

/// The pages to keep, validated against what the document has.
fn resolve(pages: &Pages, present: &[u32]) -> Result<Vec<PagePlan>, String> {
    match pages {
        Pages::All => Ok(present
            .iter()
            .map(|&number| PagePlan { number, turns: 0 })
            .collect()),
        // Unreachable, and kept because the type carries the constraint rather
        // than a comment: `route` sends any plan that produces this to
        // `Route::Working`, which never reaches `build`. See `Pages::Unlistable`.
        Pages::Unlistable => {
            Err("this document has a page tpdf made, which a page range cannot name".into())
        }
        Pages::Only(wanted) => {
            if wanted.is_empty() {
                return Err("no pages selected".into());
            }
            for page in wanted {
                if !present.contains(&page.number) {
                    return Err(format!(
                        "page {} is not in this document, which has {}",
                        page.number,
                        present.len()
                    ));
                }
            }
            Ok(wanted.clone())
        }
    }
}

#[cfg(test)]
mod tests {

    /// An encrypted document is printed whole or refused, never reserialised.
    ///
    /// **A selection of one page produced 1,278 bytes with the encryption gone**
    /// before this guard, measured 2026-08-23 --- the same silent decryption
    /// `save.rs` refuses, on a path whose output reaches Print to PDF and
    /// therefore a file the reader keeps. `is_passthrough` covered the whole
    /// document and nothing else.
    ///
    /// **The whole-document arm is the control, and it is the half that must
    /// keep working.** Without it a guard that refused every encrypted document
    /// would pass every assertion here while removing the only way to print one.
    #[test]
    fn an_encrypted_document_is_printed_whole_or_refused() {
        let mut examined = 0;
        for name in ["incr-encrypted-open.pdf", "incr-encrypted-pw.pdf"] {
            let path = std::path::Path::new("../testdata").join(name);
            if !path.exists() {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            }
            examined += 1;

            let whole = build(
                &path,
                &Job {
                    pages: Pages::All,
                    turns: 0,
                },
            )
            .unwrap_or_else(|why| panic!("{name}: the whole document must print: {why}"));
            assert_eq!(
                whole,
                std::fs::read(&path).expect("read the fixture"),
                "{name}: handed over byte for byte, so the encryption is intact"
            );

            let why = build(
                &path,
                &Job {
                    pages: Pages::Only(vec![PagePlan {
                        number: 1,
                        turns: 0,
                    }]),
                    turns: 0,
                },
            )
            .expect_err("a selection must be refused");
            assert!(
                why.contains("encrypted"),
                "{name}: the message names the reason: {why}"
            );

            // A turn is the other way into the rewrite, and it is not covered by
            // the page list: `is_passthrough` asks about both.
            let why = build(
                &path,
                &Job {
                    pages: Pages::All,
                    turns: 1,
                },
            )
            .expect_err("a turn must be refused");
            assert!(why.contains("encrypted"), "{name}: {why}");
        }
        assert!(
            examined > 0,
            "no encrypted fixture, so this checked nothing"
        );
    }

    /// The control: an unencrypted document still prints a selection.
    #[test]
    fn an_unencrypted_document_still_prints_a_selection() {
        let path = std::path::Path::new("../testdata/rotated.pdf");
        if !path.exists() {
            println!("[SKIP] rotated.pdf: fixture not generated");
            return;
        }
        let built = build(
            path,
            &Job {
                pages: Pages::Only(vec![PagePlan {
                    number: 1,
                    turns: 0,
                }]),
                turns: 0,
            },
        )
        .expect("a selection of an unencrypted document must build");
        assert!(!built.is_empty());
        assert!(
            built.len() < std::fs::read(path).expect("read").len(),
            "and it is a subset rather than the whole file"
        );
    }
    use super::{build, route, select, sheets, Job, PagePlan, Pages, Route};
    use crate::pagetree::{drop_pages, effective_rotation};

    /// A selection of pages, none of them turned by an edit.
    ///
    /// Every check below that names a page range predates per-page turns and
    /// says nothing about them, so the zero is what keeps them asking their own
    /// question. The ones that *are* about a turn build their own plan.
    fn only(numbers: &[u32]) -> Pages {
        Pages::Only(
            numbers
                .iter()
                .map(|&number| PagePlan { number, turns: 0 })
                .collect(),
        )
    }
    use lopdf::{dictionary, Document, Object, ObjectId, Stream};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// A four-page document with a model, and its handle.
    fn modelled() -> crate::edits::Edits {
        let edits = crate::edits::Edits::default();
        edits.open(1, 4, None);
        edits
    }

    /// A four-page model, and one mark on the page `at` names.
    fn marked(edits: &crate::edits::Edits, at: usize) {
        let pages = edits.state(1).expect("state").pages;
        edits
            .annotate(
                1,
                crate::edits::NewMark {
                    kind: crate::docmodel::MarkKind::Highlight,
                    stamp: None,
                    reply_to: None,
                    page: pages[at].id,
                    quads: vec![72.0, 100.0, 300.0, 118.0],
                    strokes: Vec::new(),
                    color: [1.0, 0.9, 0.2],
                    width: crate::docmodel::INK_WIDTH,
                    author: String::new(),
                    note: String::new(),
                },
                "D:20260822T120000Z".to_string(),
            )
            .expect("mark");
    }

    #[test]
    fn a_cropped_or_marked_document_is_built_rather_than_handed_over() {
        // **The defect, at the point where it was decided.** Both of these
        // answered `Route::Passthrough` --- a crop because `is_identity` never
        // asked about the box, and a mark because nothing between `select` and
        // the printer knew marks existed. The file went to the printer
        // unchanged, so a reader saw their highlight on screen and not on paper.
        let cropped = modelled();
        let pages = cropped.state(1).expect("state").pages;
        cropped
            .crop(1, pages[0].id, Some([100.0, 100.0, 400.0, 500.0]))
            .expect("crop");
        assert_eq!(
            route(Some(&cropped.plan(1).expect("plan")), None, 0),
            Route::Working,
            "a cropped page has to be written, not handed over at its full size"
        );

        let marked_up = modelled();
        marked(&marked_up, 1);
        assert_eq!(
            route(Some(&marked_up.plan(1).expect("plan")), None, 0),
            Route::Working,
            "a highlight the reader made belongs on the paper"
        );
    }

    #[test]
    fn a_document_nobody_has_touched_is_still_handed_over() {
        // The control, and it is the expensive one to get wrong in the other
        // direction: building a job for a document that is already the job
        // rewrites it to produce itself, and `lopdf` drops encryption on the way
        // through --- so an encrypted document nobody edited would print as an
        // unencrypted one, or not at all.
        let edits = modelled();
        assert_eq!(
            route(Some(&edits.plan(1).expect("plan")), None, 0),
            Route::Passthrough,
        );
        // And with no model at all, which is what printing a document the reader
        // has not edited through any route looks like.
        assert_eq!(route(None, None, 0), Route::Passthrough);
    }

    #[test]
    fn a_range_the_reader_typed_wins_over_the_working_document() {
        // An explicit range is an instruction about pages and the working
        // document is not, so the range decides --- which `select` has always
        // documented and which this pins now that there is a second producer to
        // pick between.
        let edits = modelled();
        marked(&edits, 0);
        let chosen = route(Some(&edits.plan(1).expect("plan")), Some(vec![2, 3]), 0);
        assert_eq!(
            chosen,
            Route::Range(Job {
                pages: only(&[2, 3]),
                turns: 0
            }),
        );
    }

    #[test]
    fn the_view_rotation_alone_is_enough_to_need_building() {
        // A reader looking at a document sideways and pressing Print asked for it
        // sideways. `is_passthrough` has always said so --- this is the assertion
        // that `route` did not lose it while gaining a third answer.
        let edits = modelled();
        assert_eq!(
            route(Some(&edits.plan(1).expect("plan")), None, 1),
            Route::Working,
        );
    }

    #[test]
    fn a_third_parser_sees_the_view_rotation_on_a_job_built_from_the_working_document() {
        // **Written because a mutation survived.** Sending the working document
        // through the save writer moved the view rotation onto a code path that
        // nothing exercised: `the_view_rotation_alone_is_enough_to_need_building`
        // asserts which producer is chosen and says nothing about what it
        // produces, and every rotation test beside it drives `build`, which is
        // now the *other* producer. Deleting the view turn from
        // `save::print_bytes` left all of them green --- a reader looking at an
        // annotated document sideways would have printed it upright.
        //
        // PDFKit rather than `lopdf`, for this module's standing reason: the
        // rotation a page ends up at is resolved up its `/Parent` chain, and a
        // writer asked what it wrote agrees with itself.
        let path = Path::new("../testdata/rotated.pdf");
        if !path.exists() {
            println!("[SKIP] rotated.pdf: fixture not generated");
            return;
        }
        let source = std::fs::read(path).expect("read");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] rotated.pdf: the OS parser refused the source document");
            return;
        };
        let edits = crate::edits::Edits::default();
        let count = u32::try_from(before.pages.len()).expect("pages");
        edits.open(1, count, None);
        // A mark, so the plan is not the file and the working route is taken at
        // all --- the fixture's own rotations are what make the turn observable.
        marked(&edits, 0);
        // **And an edit turn on one page, without which the fixture cannot tell
        // the two rules apart.** Written without it first, and a mutation
        // replacing `page.turns + view` with `view` alone SURVIVED every test in
        // this module: with no page carrying an edit turn the two expressions are
        // the same number. That is the trap about a fixture where the right rule
        // and the wrong rule agree, and the ingredient it was missing is this
        // line rather than a stronger assertion.
        let turned = 1usize;
        let pages = edits.state(1).expect("state").pages;
        edits.rotate(1, pages[turned].id, 1).expect("rotate");

        let plan = edits.plan(1).expect("plan");
        assert_eq!(route(Some(&plan), None, 1), Route::Working, "the route");
        let _serial = crate::save::print_lock();
        let bytes = crate::save::print_bytes(path, &plan, 1, None, &crate::save::Here)
            .expect("print bytes");
        let after = os_pdf::read(&bytes).expect("the OS parser reads the job");

        assert_eq!(after.pages.len(), before.pages.len(), "every page is kept");
        let want: Vec<i64> = before
            .pages
            .iter()
            .enumerate()
            .map(|(at, page)| {
                // The view's quarter on every page, and the edit's quarter on one
                // of them as well --- which is what "composed rather than
                // replaced" means, and the only page where the two rules disagree.
                let quarters = if at == turned { 2 } else { 1 };
                (page.rotation + 90 * quarters).rem_euclid(360)
            })
            .collect();
        let got: Vec<i64> = after.pages.iter().map(|page| page.rotation).collect();
        assert_eq!(got, want, "each page a quarter past where the file had it");
        // The control, and `rotated.pdf` is the only fixture that can supply it:
        // its four pages carry 0/90/180/270, so a job that ignored the turn would
        // come back equal to `before` and a job that applied it twice would not
        // match either. Two of the four values differ from the source, which is
        // what makes the comparison above an assertion rather than a tautology.
        let unturned: Vec<i64> = before.pages.iter().map(|page| page.rotation).collect();
        assert_ne!(got, unturned, "and not simply the rotations the file had");
    }

    #[test]
    fn a_printed_job_carries_the_marks_and_the_crop() {
        // **The measurement that found the defect, as an assertion.** A job built
        // from a plan with one mark and one crop came back with no page carrying
        // `/Annots` and none carrying `/CropBox`, because `build` had its own
        // page walk and `save.rs` had the one that writes them.
        //
        // Read back with `lopdf`, which is the writer's own reader --- and that
        // is enough *here* and would not be for a geometry claim: what is being
        // asserted is the structural presence of two keys that were entirely
        // absent, which a loader cannot hallucinate. Where the mark actually
        // lands is `save.rs`'s own tests and `annot-probe`'s pixels.
        let path = Path::new("../testdata/text-heavy.pdf");
        if !path.exists() {
            println!("[SKIP] text-heavy.pdf: fixture not generated");
            return;
        }
        let edits = crate::edits::Edits::default();
        let source = std::fs::read(path).expect("read");
        let count = Document::load_mem(&source).expect("load").get_pages().len();
        edits.open(1, u32::try_from(count).expect("pages"), None);

        let pages = edits.state(1).expect("state").pages;
        edits
            .crop(1, pages[0].id, Some([100.0, 100.0, 400.0, 500.0]))
            .expect("crop");
        marked(&edits, 1);

        let plan = edits.plan(1).expect("plan");
        assert_eq!(route(Some(&plan), None, 0), Route::Working, "the route");
        let _serial = crate::save::print_lock();
        let bytes = crate::save::print_bytes(path, &plan, 0, None, &crate::save::Here)
            .expect("print bytes");

        let out = Document::load_mem(&bytes).expect("reload");
        let table = out.get_pages();
        let has = |number: u32, key: &[u8]| {
            table
                .get(&number)
                .and_then(|id| out.get_object(*id).ok())
                .and_then(|object| object.as_dict().ok())
                .is_some_and(|dict| dict.has(key))
        };
        assert!(has(1, b"CropBox"), "the page the reader cropped");
        assert!(has(2, b"Annots"), "the page the reader marked");
        // And the two are on the pages they were put on rather than on whichever
        // page the walk happened to reach --- an assertion that only one page
        // carries each is what makes the two above about placement rather than
        // about presence anywhere in the file.
        assert!(!has(2, b"CropBox"), "and the crop is on page 1 alone");
        assert!(!has(1, b"Annots"), "and the mark is on page 2 alone");
    }

    #[test]
    fn an_unedited_document_prints_as_the_file_itself() {
        let edits = modelled();
        assert_eq!(
            select(Some(&edits.plan(1).expect("plan")), None),
            Pages::All,
            "spelling it out as a selection of every page would rewrite the file to \
             produce the file --- and lopdf drops encryption on the way through"
        );
    }

    #[test]
    fn an_edited_document_prints_the_pages_the_model_kept() {
        let edits = modelled();
        let pages = edits.state(1).expect("state").pages;
        edits.delete(1, pages[1].id).expect("delete");
        edits.rotate(1, pages[3].id, 1).expect("rotate");

        assert_eq!(
            select(Some(&edits.plan(1).expect("plan")), None),
            Pages::Only(vec![
                PagePlan {
                    number: 1,
                    turns: 0
                },
                PagePlan {
                    number: 3,
                    turns: 0
                },
                PagePlan {
                    number: 4,
                    turns: 1
                },
            ]),
            "one-based page numbers of the file, the deleted one absent, and the \
             turn on the page it was applied to"
        );
    }

    /// A plan carrying a page tpdf made cannot be spelled as file page numbers,
    /// and the route says so.
    ///
    /// **Both halves, because either alone is satisfied by a defect.** A
    /// `select` that answered `Unlistable` while `route` still sent the job to
    /// `Range` would print a refusal at the panel; a `route` that answered
    /// `Working` while `select` quietly listed two of three pages would print
    /// the wrong document. The control is the same document without the made
    /// page, which still lists.
    #[test]
    fn a_plan_with_a_page_tpdf_made_cannot_be_listed_and_goes_to_the_working_writer() {
        let edits = modelled();
        let pages = edits.state(1).expect("state").pages;
        // The control is an edit that *is* listable, rather than the untouched
        // document --- which answers `Pages::All` and would agree with a `select`
        // that could no longer list anything at all.
        edits.rotate(1, pages[0].id, 1).expect("rotate");
        assert!(
            matches!(
                select(Some(&edits.plan(1).expect("plan")), None),
                Pages::Only(_)
            ),
            "the control: an edited document without a made page still lists"
        );

        edits
            .insert(1, Some(pages[0].id), [200.0, 400.0])
            .expect("insert a blank page");
        let plan = edits.plan(1).expect("plan");

        assert_eq!(select(Some(&plan), None), Pages::Unlistable);
        assert_eq!(
            route(Some(&plan), None, 0),
            Route::Working,
            "the working writer is the only one that can make the page"
        );
        // An explicit range still wins, and still says nothing about edits ---
        // which is what `route` documents and is worth pinning here, because the
        // made page is the first thing a plan can carry that the range cannot
        // express.
        assert!(matches!(
            route(Some(&plan), Some(vec![1]), 0),
            Route::Range(_)
        ));
    }

    #[test]
    fn an_explicit_range_is_what_it_says_and_owes_nothing_to_the_model() {
        let edits = modelled();
        let pages = edits.state(1).expect("state").pages;
        edits.rotate(1, pages[0].id, 2).expect("rotate");

        assert_eq!(
            select(Some(&edits.plan(1).expect("plan")), Some(vec![2, 3])),
            Pages::Only(vec![
                PagePlan {
                    number: 2,
                    turns: 0
                },
                PagePlan {
                    number: 3,
                    turns: 0
                },
            ]),
            "a range is an instruction about which pages, and carries no edits --- \
             the window harness sends one and has no model at all"
        );
    }

    #[test]
    fn a_document_with_no_model_prints_the_whole_file() {
        assert_eq!(select(None, None), Pages::All);
    }

    #[test]
    fn a_handle_that_names_no_document_is_an_error_before_this_is_reached() {
        // The half that moved out of this function when it stopped taking the
        // model: `print_document` reads the plan and propagates the failure, so a
        // stale handle is an error rather than a silent fallback to the whole
        // file --- which would print the pages the reader deleted.
        let edits = crate::edits::Edits::default();
        assert!(edits.plan(9).is_err());
    }

    /// The operating system's own PDF parser, whichever platform this is.
    ///
    /// PDFKit on macOS, `Windows.Data.Pdf` on Windows. Both are the platform's own
    /// PDF stack, so both are independent of the `lopdf` that writes these jobs and
    /// of the PDFium that draws what the reader sees --- which is the whole property
    /// these checks are built on. The two modules deliberately expose the same
    /// `Reading` shape so that one set of expectations covers both.
    ///
    /// **The four checks below were macOS-only until 2026-07-30, and three of them
    /// had no reason to be.** They were written when PDFKit was the only independent
    /// parser available, and the gate then said "no third parser here" rather than
    /// anything about the property under test. Printing is the one subsystem whose
    /// output leaves the process, so the platform without a third parser was also
    /// the platform where nothing checked that its jobs were readable at all.
    #[cfg(target_os = "macos")]
    use crate::print_macos as os_pdf;
    #[cfg(windows)]
    use crate::print_win as os_pdf;

    /// Whether the OS parser on this platform can extract text.
    ///
    /// PDFKit can; `Windows.Data.Pdf` renders and reports geometry and has no text
    /// API at all. A runtime constant rather than a `cfg` on the test, so the check
    /// that needs text still *runs* on Windows --- it asserts everything it can and
    /// prints a `[SKIP]` naming what it could not. `BUILD.md`'s rule: a check that
    /// quietly stops existing on one platform is worse than one that skips out loud,
    /// because a vanished check and a passing one look identical in a summary.
    const OS_PARSER_HAS_TEXT: bool = cfg!(target_os = "macos");

    use crate::testutil::TempDir;

    /// A document whose pages inherit `/Resources` and `/Rotate` from the tree.
    ///
    /// Inherited on purpose: a page re-parented under a fresh `/Pages` loses
    /// both, and the result still opens and still counts the right number of
    /// pages. Each page's content names its own number, so a subset can be
    /// checked for *which* pages it kept rather than only how many.
    fn fixture(path: &Path, pages: usize, tree_rotate: i64) {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });

        let mut kids = Vec::new();
        for number in 1..=pages {
            let content = format!("BT /F1 24 Tf 72 700 Td (page {number}) Tj ET");
            let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => contents_id,
            })));
        }

        let mut tree = dictionary! {
            "Type" => "Pages",
            "Count" => pages as i64,
            "Kids" => kids,
            // Inheritable, and deliberately only here.
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        if tree_rotate != 0 {
            tree.set("Rotate", tree_rotate);
        }
        doc.objects.insert(pages_id, Object::Dictionary(tree));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");

        // Trailing bytes past `%%EOF`, and they are load-bearing. Without them
        // the fixture is a file lopdf itself wrote, so loading and saving it
        // reproduces it byte for byte --- and "the file was handed over
        // untouched" becomes true of a full rewrite too. Both passthrough
        // mutations survived until this was here. Readers tolerate the tail
        // (`hostile-trailing` exists for exactly that), lopdf does not emit it.
        let mut bytes = std::fs::read(path).expect("read back");
        bytes.extend_from_slice(b"\n% a tail no serialiser would reproduce\n");
        std::fs::write(path, bytes).expect("retag");
    }

    /// A document whose page tree has an intermediate level.
    ///
    /// `fixture` above builds every page directly under the root, which is what
    /// a generator does and not what a producer does --- real documents balance
    /// the tree, so a page's `/Parent` chain is two or more nodes long. Deleting
    /// a page has to decrement `/Count` on **every** ancestor, and with a flat
    /// tree "the page's parent" and "the whole chain" are the same thing, so
    /// nothing can tell the two apart. Found by a mutation that survived
    /// (`D4`), not by reading the code.
    ///
    /// Returns the root and the intermediate node ids, so a check can name the
    /// level it is asserting about.
    fn nested_fixture(path: &Path, groups: usize, per_group: usize) -> (ObjectId, Vec<ObjectId>) {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });

        let mut number = 0;
        let mut middles = Vec::new();
        let mut root_kids = Vec::new();
        for _ in 0..groups {
            let middle_id = doc.new_object_id();
            let mut kids = Vec::new();
            for _ in 0..per_group {
                number += 1;
                let content = format!("BT /F1 24 Tf 72 700 Td (page {number}) Tj ET");
                let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
                kids.push(Object::Reference(doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => middle_id,
                    "Contents" => contents_id,
                })));
            }
            doc.objects.insert(
                middle_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Parent" => root_id,
                    "Count" => per_group as i64,
                    "Kids" => kids,
                }),
            );
            middles.push(middle_id);
            root_kids.push(Object::Reference(middle_id));
        }

        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => number as i64,
                "Kids" => root_kids,
                // Inheritable, and two levels above the pages on purpose.
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => root_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");
        (root_id, middles)
    }

    /// The `/Count` a page-tree node declares.
    fn declared_count(doc: &Document, id: ObjectId) -> i64 {
        doc.get_object(id)
            .and_then(Object::as_dict)
            .and_then(|node| node.get(b"Count"))
            .and_then(Object::as_i64)
            .expect("a page tree node must declare a count")
    }

    /// Reloads built bytes.
    fn reload(bytes: &[u8]) -> Document {
        Document::load_mem(bytes).expect("the print job must be a readable document")
    }

    /// The text drawn on each page, in order, so a subset can be identified.
    fn page_labels(doc: &Document) -> Vec<String> {
        doc.get_pages()
            .values()
            .map(|id| String::from_utf8_lossy(&doc.get_page_content(*id)).into_owned())
            .collect()
    }

    #[test]
    fn printing_everything_unrotated_hands_over_the_file_itself() {
        // Not "an equivalent document". Rewriting to change nothing is pure
        // risk --- lopdf drops encryption silently, and a rewrite reflows
        // structure --- and the printer was going to get these bytes anyway.
        let dir = TempDir::new("passthrough");
        let path = dir.join("in.pdf");
        fixture(&path, 3, 0);
        let original = std::fs::read(&path).expect("read");

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 0,
            },
        )
        .expect("build");
        assert_eq!(out, original);
    }

    #[test]
    fn a_turn_of_four_quarters_is_still_the_file_itself() {
        let dir = TempDir::new("fullturn");
        let path = dir.join("in.pdf");
        fixture(&path, 2, 0);
        let original = std::fs::read(&path).expect("read");
        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 4,
            },
        )
        .expect("build");
        assert_eq!(out, original);
    }

    #[test]
    fn a_page_range_keeps_exactly_the_pages_asked_for() {
        let dir = TempDir::new("range");
        let path = dir.join("in.pdf");
        fixture(&path, 5, 0);

        let out = build(
            &path,
            &Job {
                pages: only(&[2, 4]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let labels = page_labels(&printed);
        assert_eq!(labels.len(), 2);
        // Which pages, not merely how many: a subset that kept the wrong two
        // has the right count and is entirely wrong.
        assert!(labels[0].contains("page 2"), "{labels:?}");
        assert!(labels[1].contains("page 4"), "{labels:?}");
    }

    #[test]
    fn a_kept_page_still_inherits_its_resources() {
        // The trap this module exists to avoid. A page that lost `/Resources`
        // still opens, still counts, and prints blank --- so the assertion has
        // to reach the font through the page, which is what a renderer does.
        let dir = TempDir::new("inherit");
        let path = dir.join("in.pdf");
        fixture(&path, 4, 0);

        let out = build(
            &path,
            &Job {
                pages: only(&[3]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let page = *printed.get_pages().values().next().expect("one page");

        // Not `get_page_fonts`, which was the first version of this and could
        // not fail. lopdf collects the page's resources *and* every ancestor's
        // and merges them, so a page carrying its own empty `/Resources` still
        // reports the inherited font --- while a renderer, following PDF
        // 32000-1 §7.7.3.4, would see the page's own dictionary replace what it
        // inherits and draw nothing. The oracle was more forgiving than the
        // thing it stands in for, and the mutation modelling that failure
        // survived it.
        //
        // So: the page must still be inheriting (no `/Resources` of its own),
        // and an ancestor must still supply the font.
        let dictionary = printed.get_dictionary(page).expect("page");
        assert!(
            dictionary.get(b"Resources").is_err(),
            "the page carries its own /Resources, which replaces what it inherits"
        );

        let mut at = page;
        let mut found = None;
        for _ in 0..64 {
            let Ok(node) = printed.get_dictionary(at) else {
                break;
            };
            if let Ok(resources) = node
                .get(b"Resources")
                .and_then(|r| printed.dereference(r).map(|(_, o)| o))
                .and_then(Object::as_dict)
            {
                found = resources
                    .get(b"Font")
                    .and_then(|f| printed.dereference(f).map(|(_, o)| o))
                    .and_then(Object::as_dict)
                    .ok()
                    .and_then(|fonts| fonts.get(b"F1").ok())
                    .map(|_| ());
                break;
            }
            match node.get(b"Parent").and_then(Object::as_reference) {
                Ok(parent) => at = parent,
                Err(_) => break,
            }
        }
        assert!(
            found.is_some(),
            "no ancestor supplies the font the page draws with"
        );
    }

    #[test]
    fn a_rotation_composes_with_one_the_page_inherits() {
        // The document is already sideways and the reader has turned the view
        // another quarter. Composing against the literal `/Rotate` --- absent on
        // every page here --- would print 90 instead of 180.
        let dir = TempDir::new("compose");
        let path = dir.join("in.pdf");
        fixture(&path, 2, 90);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 1,
            },
        )
        .expect("build");
        let printed = reload(&out);
        for id in printed.get_pages().values() {
            assert_eq!(effective_rotation(&printed, *id), 180);
        }
    }

    /// Three page numbers over two objects: `/Kids` names the first page twice.
    ///
    /// `fixture` above gives every page an object of its own, which is what a
    /// generator does. A `/Kids` array can name one page twice, and `lopdf`'s page
    /// walk keeps no visited set, so `get_pages` then maps numbers 1 and 2 onto one
    /// object. Nothing else in the tree is shaped this way.
    ///
    /// **Three rather than two, and that is what makes it useful.** With only the
    /// shared page there is no selection that drops both of its numbers and keeps
    /// something --- an empty selection is refused before `drop_pages` is reached
    /// --- so the `/Count` arithmetic would have been unreachable. Page 3 is the
    /// page that lets it be asked for.
    ///
    /// Returns the root `/Pages` id, so a check can read its `/Count`.
    fn shared_fixture(path: &Path) -> ObjectId {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let page_of = |doc: &mut Document, label: &str| {
            let content = format!("BT /F1 24 Tf 72 700 Td ({label}) Tj ET");
            let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => contents_id,
            })
        };
        let shared = page_of(&mut doc, "page 1");
        let only = page_of(&mut doc, "page 3");
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => 3i64,
                "Kids" => vec![
                    Object::Reference(shared),
                    Object::Reference(shared),
                    Object::Reference(only),
                ],
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");
        pages_id
    }

    /// Asserted rather than assumed, for the reason `save.rs` gives at length: a
    /// future `lopdf` that deduplicates its page walk would leave both guards
    /// below reachable by nothing while their outcomes kept passing.
    #[test]
    fn the_shared_fixture_really_does_present_one_object_under_two_numbers() {
        let dir = TempDir::new("shared-pre");
        let path = dir.join("in.pdf");
        let _ = shared_fixture(&path);

        let doc = Document::load(&path).expect("load");
        let ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        assert_eq!(ids.len(), 3, "three page numbers");
        assert_eq!(
            ids[0], ids[1],
            "and the first two resolve to ONE object --- if this fails, lopdf now \
             deduplicates and the guards below are dead code"
        );
        assert_ne!(
            ids[1], ids[2],
            "the control: the third is a different object, so the assertion above \
             is about a shared page rather than about every id being equal"
        );
    }

    #[test]
    fn a_page_named_twice_is_turned_once() {
        let dir = TempDir::new("shared-turn");
        let path = dir.join("in.pdf");
        let _ = shared_fixture(&path);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 1,
            },
        )
        .expect("build");
        let printed = reload(&out);
        for id in printed.get_pages().values() {
            assert_eq!(
                effective_rotation(&printed, *id),
                90,
                "one quarter-turn. Composing once per page number reads back the \
                 90 it just wrote and leaves 180 --- on paper, sideways"
            );
        }
    }

    /// A print job carries the reader's *edits*, not only their view.
    ///
    /// Until deleting a page landed, a job was the file plus one rotation applied
    /// to every page --- so a reader who turned page 3 in the document and pressed
    /// print got page 3 the way it was on disk, silently. Each page now takes its
    /// own turn, and the view's on top of it.
    ///
    /// The fixture inherits `/Rotate 90` from the page tree and states none of its
    /// own, which is what makes the composition visible: a build that *set* each
    /// page's turn rather than adding it produces 90 and 180 here where the answer
    /// is 180 and 270.
    #[test]
    fn each_page_takes_its_own_edit_and_the_view_rotation_on_top() {
        let dir = TempDir::new("per-page");
        let path = dir.join("in.pdf");
        fixture(&path, 3, 90);

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![
                    PagePlan {
                        number: 1,
                        turns: 0,
                    },
                    PagePlan {
                        number: 2,
                        turns: 1,
                    },
                    PagePlan {
                        number: 3,
                        turns: 2,
                    },
                ]),
                // The reader is also looking at the whole document sideways.
                turns: 1,
            },
        )
        .expect("build");

        let printed = reload(&out);
        let rotations: Vec<i64> = printed
            .get_pages()
            .values()
            .map(|id| effective_rotation(&printed, *id).rem_euclid(360))
            .collect();
        assert_eq!(
            rotations,
            vec![180, 270, 0],
            "inherited 90, plus each page's own edit, plus the view's quarter"
        );
    }

    /// The same job with no view rotation, so the edit is the only thing moving.
    ///
    /// The control for the check above: with both in force, a build that ignored
    /// the plan and applied the view to everything would produce 180/180/180 --- a
    /// wrong answer that differs from the right one on two pages, but a build that
    /// ignored the *view* and applied only the plan produces this row instead, and
    /// the two failures are not distinguishable from one comparison.
    #[test]
    fn a_page_the_reader_turned_prints_turned_with_no_view_rotation_at_all() {
        let dir = TempDir::new("per-page-only");
        let path = dir.join("in.pdf");
        fixture(&path, 3, 90);

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![
                    PagePlan {
                        number: 1,
                        turns: 0,
                    },
                    PagePlan {
                        number: 2,
                        turns: 1,
                    },
                    PagePlan {
                        number: 3,
                        turns: 2,
                    },
                ]),
                turns: 0,
            },
        )
        .expect("build");

        let printed = reload(&out);
        let rotations: Vec<i64> = printed
            .get_pages()
            .values()
            .map(|id| effective_rotation(&printed, *id).rem_euclid(360))
            .collect();
        assert_eq!(rotations, vec![90, 180, 270]);
    }

    /// The damaging member of the family: not a page turned twice, but a page
    /// deleted that the reader asked to print.
    #[test]
    fn a_page_a_kept_number_also_names_is_not_dropped() {
        let dir = TempDir::new("shared-drop");
        let path = dir.join("in.pdf");
        let _ = shared_fixture(&path);

        // Print page 1 only. Page 2 is the same object, so building `doomed` from
        // the dropped numbers alone deletes the page that was asked for and prints
        // a blank sheet.
        let out = build(
            &path,
            &Job {
                pages: only(&[1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let labels = page_labels(&printed);
        assert!(
            labels.iter().any(|label| label.contains("page 1")),
            "the page asked for is still there and still draws its text: {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("page 3")),
            "the control: the page that only a dropped number names really was \
             dropped, so the assertion above is not satisfied by a build that \
             deletes nothing: {labels:?}"
        );
    }

    /// `/Count` counts entries in the tree, and two numbers naming one doomed
    /// object remove two of them.
    ///
    /// Reachable only because the fixture has a third page: dropping both numbers
    /// of the shared page needs something left to keep, and an empty selection is
    /// refused long before `drop_pages`.
    #[test]
    fn a_shared_page_costs_the_tree_one_count_per_number_it_answered_to() {
        let dir = TempDir::new("shared-count");
        let path = dir.join("in.pdf");
        let pages_id = shared_fixture(&path);

        let before = reload(&std::fs::read(&path).expect("read"));
        assert_eq!(
            declared_count(&before, pages_id),
            3,
            "the control: the tree starts out claiming three pages"
        );

        // Keep page 3 only, so both of the shared page's numbers go.
        let out = build(
            &path,
            &Job {
                pages: only(&[3]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        assert_eq!(
            page_labels(&printed).len(),
            1,
            "one page is left in the tree"
        );
        assert_eq!(
            declared_count(&printed, pages_id),
            1,
            "and `/Count` says so. Decrementing once per doomed OBJECT leaves 2 \
             here --- a tree that claims a page it does not have"
        );
    }

    #[test]
    fn a_rotation_wraps_rather_than_growing() {
        let dir = TempDir::new("wrap");
        let path = dir.join("in.pdf");
        fixture(&path, 1, 270);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 2,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let id = *printed.get_pages().values().next().expect("one page");
        assert_eq!(effective_rotation(&printed, id), 90);
    }

    #[test]
    fn a_page_the_document_does_not_have_is_refused() {
        let dir = TempDir::new("range-error");
        let path = dir.join("in.pdf");
        fixture(&path, 3, 0);
        let error = build(
            &path,
            &Job {
                pages: only(&[2, 9]),
                turns: 0,
            },
        )
        .expect_err("must refuse");
        assert!(error.contains('9'), "{error}");
    }

    #[test]
    fn an_empty_selection_is_refused() {
        // Printing nothing is a bug in whatever built the job, not a job.
        let dir = TempDir::new("empty");
        let path = dir.join("in.pdf");
        fixture(&path, 2, 0);
        assert!(build(
            &path,
            &Job {
                pages: only(&[]),
                turns: 0,
            },
        )
        .is_err());
    }

    #[test]
    fn a_subset_drops_the_objects_it_orphaned() {
        // "Fewer objects than before" is not this test: deleting a page removes
        // the page object itself, so the count falls whether or not anything
        // was collected, and the mutation that deletes the sweep survived that
        // assertion. What only the sweep can remove is the *content stream* a
        // deleted page pointed at, so that is what is named and looked for.
        let dir = TempDir::new("collect");
        let path = dir.join("in.pdf");
        fixture(&path, 8, 0);

        let source = Document::load(&path).expect("load");
        let orphaned: Vec<_> = source
            .get_pages()
            .iter()
            .filter(|(number, _)| **number != 1)
            .flat_map(|(_, id)| source.get_page_contents(*id))
            .collect();
        assert!(!orphaned.is_empty(), "the fixture has no streams to orphan");

        let out = build(
            &path,
            &Job {
                pages: only(&[1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        // Object numbers are deliberately not made contiguous, so an id here
        // still names what it named in the source.
        let left: Vec<_> = orphaned
            .iter()
            .filter(|id| printed.objects.contains_key(id))
            .collect();
        assert!(
            left.is_empty(),
            "{} content stream(s) of dropped pages are still in the file: {left:?}",
            left.len()
        );
    }

    #[test]
    fn an_outline_naming_pages_that_are_gone_is_dropped() {
        let dir = TempDir::new("outline");
        let path = dir.join("in.pdf");
        fixture(&path, 4, 0);

        // Give it an outline pointing at a page the subset will not keep.
        let mut doc = Document::load(&path).expect("load");
        let last = *doc.get_pages().get(&4).expect("page 4");
        let item = doc.add_object(dictionary! {
            "Title" => Object::string_literal("The end"),
            "Dest" => vec![Object::Reference(last), "Fit".into()],
        });
        let outlines = doc.add_object(dictionary! {
            "Type" => "Outlines", "First" => item, "Last" => item, "Count" => 1,
        });
        doc.catalog_mut()
            .expect("catalog")
            .set("Outlines", outlines);
        doc.save(&path).expect("save");

        let out = build(
            &path,
            &Job {
                pages: only(&[1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        assert!(printed
            .catalog()
            .expect("catalog")
            .get(b"Outlines")
            .is_err());
    }

    /// What the OS's own PDF parser makes of built bytes.
    ///
    /// A **third** parser: independent of `lopdf`, which wrote the job, and of
    /// PDFium, which drew what the reader was looking at. Every other check in this
    /// module asks `lopdf` to read back a file `lopdf` produced, which cannot
    /// distinguish "the document says this" from "our serialiser and our loader
    /// agree about this" --- and it is the second that a printer does not care
    /// about. Neither platform's choice is a neutral third party either, which is
    /// the point: each is the parser that platform's own print path uses.
    fn read_back(bytes: &[u8]) -> os_pdf::Reading {
        // macOS asks for text as well, because the checks below assert *which*
        // pages survived and PDFKit can say. The print path deliberately does not
        // pay for that (see `print_macos::read`), and Windows cannot supply it at
        // any price.
        #[cfg(target_os = "macos")]
        return crate::print_macos::read_with_text(bytes)
            .expect("the OS parser could not read the print job");
        #[cfg(windows)]
        return crate::print_win::read(bytes).expect("the OS parser could not read the print job");
    }

    #[test]
    fn a_third_parser_reads_back_exactly_the_pages_that_were_kept() {
        let dir = TempDir::new("pdfkit-range");
        let path = dir.join("in.pdf");
        fixture(&path, 5, 0);

        let out = build(
            &path,
            &Job {
                pages: only(&[2, 4]),
                turns: 0,
            },
        )
        .expect("build");

        let reading = read_back(&out);
        // The portable half: *how many* pages a parser that did not write them can
        // find. Asserted on both platforms.
        assert_eq!(reading.pages.len(), 2, "{reading:?}");

        if !OS_PARSER_HAS_TEXT {
            // Said out loud rather than gated away. The count above is a real check
            // and this is a real hole in it: a subset that kept pages 1 and 3 would
            // satisfy everything asserted here. `a_third_parser_checks_a_job_built_-`
            // `from_a_document_we_did_not_write` covers the same property on both
            // platforms by using per-page rotation instead of text, which is why
            // this gap is acceptable rather than merely admitted.
            println!(
                "[SKIP] which pages survived: this platform's OS parser extracts no \
                 text, so only the count is pinned here"
            );
            return;
        }
        // Which pages, read by something that did not write them.
        assert!(
            reading.pages[0]
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("page 2"),
            "{reading:?}"
        );
        assert!(
            reading.pages[1]
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("page 4"),
            "{reading:?}"
        );
    }

    #[test]
    fn a_third_parser_sees_the_rotation_the_page_inherited_and_the_one_we_added() {
        // The pair is the point. `effective_rotation` returning 0 instead of
        // reading the tree writes 90 here where 180 is correct, and only the
        // second case can tell --- the first is 90 either way.
        for (inherited, expected) in [(0, 90), (90, 180)] {
            let dir = TempDir::new(&format!("pdfkit-turn-{inherited}"));
            let path = dir.join("in.pdf");
            fixture(&path, 2, inherited);

            let out = build(
                &path,
                &Job {
                    pages: Pages::All,
                    turns: 1,
                },
            )
            .expect("build");

            let reading = read_back(&out);
            for page in &reading.pages {
                assert_eq!(
                    page.rotation, expected,
                    "inherited {inherited}: {reading:?}"
                );
            }
        }
    }

    #[test]
    fn a_third_parser_accepts_the_handed_over_file_tail_and_all() {
        // The passthrough fixture carries bytes past `%%EOF` so a rewrite is
        // distinguishable from a copy. That trick is only legitimate because
        // readers tolerate the tail --- asserted here rather than assumed, by a
        // reader that is not the one which wrote it.
        let dir = TempDir::new("pdfkit-tail");
        let path = dir.join("in.pdf");
        fixture(&path, 3, 0);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 0,
            },
        )
        .expect("build");

        assert_eq!(out, std::fs::read(&path).expect("source"));
        assert_eq!(read_back(&out).pages.len(), 3);
    }

    /// A job prints its pages in the order it lists them.
    ///
    /// This used to be documented as *not* happening --- the subset came out in
    /// document order --- and the day a reader could rearrange a document that
    /// stopped being a quirk and became a print that silently disagrees with the
    /// screen. Read back through PDFKit, on `rotated.pdf` because its four
    /// distinct rotations are the only thing that names a page: a job in the
    /// wrong order has the right pages and the right count.
    #[test]
    fn a_job_prints_its_pages_in_the_order_it_lists_them() {
        let path = Path::new("../testdata/rotated.pdf");
        if !path.exists() {
            println!("[SKIP] rotated.pdf not generated");
            return;
        }
        let source = std::fs::read(path).expect("read source");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] the OS parser refused rotated.pdf");
            return;
        };
        let at: Vec<i64> = before
            .pages
            .iter()
            .map(|page| page.rotation.rem_euclid(360))
            .collect();
        assert_eq!(
            at.iter().collect::<HashSet<_>>().len(),
            4,
            "the fixture discriminates: four pages, four different rotations"
        );

        let job = build(
            path,
            &Job {
                pages: only(&[4, 1, 3]),
                turns: 0,
            },
        )
        .expect("build");
        let after = os_pdf::read(&job).expect("the OS parser reads the job");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![at[3], at[0], at[2]],
            "the pages listed, in the order they were listed"
        );

        // The control: the same three in ascending order are the document's own
        // order, and nothing rewrites the tree for them. A build that reordered
        // unconditionally passes the assertion above.
        let ascending = build(
            path,
            &Job {
                pages: only(&[1, 3, 4]),
                turns: 0,
            },
        )
        .expect("build");
        assert_eq!(
            os_pdf::read(&ascending)
                .expect("read")
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![at[0], at[2], at[3]]
        );
    }

    /// `build` fed documents that no Rust code in this repository wrote.
    ///
    /// Every other check in this module builds its input with `fixture`, which
    /// is `lopdf`'s own serialiser --- so the module tests a writer against its
    /// own reader, and `read_back` makes only the *output* side independent.
    /// A defect the writer and the loader share is invisible to that, which is
    /// the trap `docs/TRAPS.md` records as "a writer and its own reader agree
    /// about a document that is wrong", and printing is the one subsystem here
    /// whose output leaves the process.
    ///
    /// So both ends are independent here. The inputs come from the hand-rolled
    /// generators under `testdata/`, which assemble PDF bytes directly and
    /// share no code with anything under test; the page list, the expected
    /// rotations and the verdict all come from PDFKit. `lopdf` appears only as
    /// the thing being tested --- deliberately, because a check that derives
    /// its expectations from the library under test agrees with itself by
    /// construction.
    ///
    /// The subset keeps the first and last page, so the dropped set is neither
    /// a prefix nor a suffix and the `/Kids` surgery lands in the middle, and
    /// it adds a quarter turn, so each surviving page's rotation has to be
    /// resolved up its own `/Parent` chain and composed rather than written.
    ///
    /// What makes *which* pages survived observable is per-page rotation:
    /// `rotated.pdf` carries 0/90/180/270 on four otherwise byte-identical
    /// pages, so keeping the wrong two is a different rotation pair. On a
    /// fixture whose pages all share one rotation only the count and the
    /// composition are pinned, and the run says which case each fixture was
    /// rather than leaving that to be assumed.
    #[test]
    fn a_third_parser_checks_a_job_built_from_a_document_we_did_not_write() {
        let mut examined = 0;
        for name in [
            "rotated.pdf",
            "text-heavy.pdf",
            "vector-multi.pdf",
            "outline-hostile.pdf",
            "incr-scan-5p.pdf",
            "hostile-filters.pdf",
        ] {
            let path = Path::new("../testdata").join(name);
            if !path.exists() {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            }
            let source = std::fs::read(&path).expect("read source");

            // The baseline, from the parser that will read the print job --- not
            // from the one that builds it.
            let Some(before) = os_pdf::read(&source) else {
                println!("[SKIP] {name}: the OS parser refused the source document");
                continue;
            };
            let count = before.pages.len();
            if count < 3 {
                println!("[SKIP] {name}: {count} pages, too few to drop a middle");
                continue;
            }

            let keep = [1u32, u32::try_from(count).expect("page count")];
            let expected: Vec<i64> = keep
                .iter()
                .map(|number| {
                    let at = usize::try_from(*number).expect("page number") - 1;
                    (before.pages[at].rotation + 90).rem_euclid(360)
                })
                .collect();

            let out = build(
                &path,
                &Job {
                    pages: only(&keep),
                    turns: 1,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: build failed: {e}"));

            let after = os_pdf::read(&out)
                .unwrap_or_else(|| panic!("{name}: the OS parser could not read the built job"));

            assert_eq!(
                after.pages.len(),
                keep.len(),
                "{name}: page count, {after:?}"
            );
            let got: Vec<i64> = after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            assert_eq!(got, expected, "{name}: rotations");

            // Says which of the two cases this fixture was, because a fixture
            // with one rotation throughout cannot report a wrong *choice* of
            // pages and a run that does not say so reads as if it had.
            let distinct: HashSet<i64> = before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            let discriminating = if distinct.len() > 1 {
                "pins which pages survived"
            } else {
                "pins the count and the composition only"
            };
            println!("[OK] {name:20} {count} pages, rotations {distinct:?} --- {discriminating}");
            examined += 1;
        }

        // A run where every fixture was absent prints six SKIP lines and
        // otherwise looks exactly like a run where every one passed.
        assert!(
            examined > 0,
            "no fixture was examined --- generate testdata/ (BUILD.md, Test fixtures)"
        );
    }

    #[test]
    fn a_job_of_the_wrong_size_is_refused_before_it_reaches_paper() {
        use super::expect_pages;
        assert!(expect_pages(2, Some(2)).is_ok());
        assert!(expect_pages(1, Some(2)).is_err());
        assert!(expect_pages(3, Some(2)).is_err());
        // "Everything" has no count to check against, so the only recognisable
        // wrong answer is nothing at all.
        assert!(expect_pages(5, None).is_ok());
        assert!(expect_pages(0, None).is_err());
        // And an empty selection is refused earlier, by `resolve` --- so a
        // zero-page job with a zero expectation is a state nothing can reach,
        // and this pins which of the two guards is doing the work.
        assert!(expect_pages(0, Some(0)).is_ok());
    }

    #[test]
    fn a_job_in_document_order_keeps_the_page_tree_the_file_had() {
        // The control for reordering, and the reason `build` asks whether the
        // orders differ instead of rebuilding every time. Both routes produce
        // the same *document* here --- the same pages in the same order --- and
        // what tells them apart is the shape of the tree underneath.
        let dir = TempDir::new("nested-order");
        let path = dir.join("in.pdf");
        nested_fixture(&path, 3, 2);

        let kind = |bytes: &[u8]| {
            let doc = Document::load_mem(bytes).expect("load");
            let root = doc
                .catalog()
                .expect("a catalog")
                .get(b"Pages")
                .and_then(Object::as_reference)
                .expect("a page tree");
            let first = doc
                .get_object(root)
                .and_then(Object::as_dict)
                .expect("the root")
                .get(b"Kids")
                .and_then(Object::as_array)
                .expect("kids")
                .first()
                .and_then(|entry| entry.as_reference().ok())
                .expect("a first kid");
            String::from_utf8_lossy(
                doc.get_object(first)
                    .and_then(Object::as_dict)
                    .expect("a kid")
                    .get(b"Type")
                    .and_then(Object::as_name)
                    .expect("a type"),
            )
            .into_owned()
        };

        let ascending = build(
            &path,
            &Job {
                pages: only(&[1, 2, 4]),
                turns: 0,
            },
        )
        .expect("build");
        assert_eq!(
            kind(&ascending),
            "Pages",
            "a subset in document order is deleted in place, so the groups above \
             it are still groups"
        );

        let shuffled = build(
            &path,
            &Job {
                pages: only(&[4, 1, 2]),
                turns: 0,
            },
        )
        .expect("build");
        assert_eq!(
            kind(&shuffled),
            "Page",
            "and the same pages in another order cannot be, so the tree is rebuilt"
        );
    }

    #[test]
    fn every_level_of_the_page_tree_learns_it_lost_a_page() {
        // Three groups of two. Dropping one page from the first group and both
        // from the last means the root must fall by three while the middles
        // fall by different amounts --- so a walk that stops at the page's own
        // parent, and one that decrements the root once per *group* rather than
        // once per page, are both wrong here and in different directions.
        let dir = TempDir::new("nested");
        let path = dir.join("in.pdf");
        let (root, middles) = nested_fixture(&path, 3, 2);

        let mut doc = Document::load(&path).expect("load");
        assert_eq!(declared_count(&doc, root), 6);
        drop_pages(&mut doc, &[1, 5, 6]).expect("drop");

        assert_eq!(declared_count(&doc, root), 3, "root");
        assert_eq!(declared_count(&doc, middles[0]), 1, "first group");
        assert_eq!(declared_count(&doc, middles[1]), 2, "untouched group");
        assert_eq!(declared_count(&doc, middles[2]), 0, "emptied group");
        // And the tree agrees with itself: what the root claims is what a
        // reader walking `/Kids` actually finds.
        assert_eq!(doc.get_pages().len(), 3);
    }

    /// The control for replacing `lopdf::delete_pages` with `drop_pages`.
    ///
    /// A refactor claiming to change nothing has to be shown to change nothing,
    /// so both routes run on the same input and their bytes are compared. Same
    /// procedure as the mark-and-sweep move, which was verified by running the
    /// pre-move code as a control rather than by reading it.
    ///
    /// The 775-page corpora are deliberately **not** in this list even though
    /// they are the interesting case: `lopdf`'s side of the comparison is the
    /// quadratic one, and in the debug profile the gate runs in it costs 20 s.
    /// They were checked once, by hand, at 775 -> 2 pages --- identical bytes,
    /// 620.5 ms against 1.2 ms and 663.1 ms against 1.0 ms (docs/PLAN.md).
    #[test]
    fn control_page_deletion_matches_lopdf_byte_for_byte() {
        use std::time::Instant;

        let save = |doc: &mut Document| {
            super::sweep::collect(doc).expect("collect");
            let mut out = Vec::new();
            doc.save_to(&mut out).expect("save");
            out
        };
        let load = |path: &Path| {
            Document::load_with_options(
                path,
                lopdf::LoadOptions {
                    max_decompressed_size: Some(super::MAX_DECODE),
                    ..Default::default()
                },
            )
            .expect("load")
        };

        let dir = TempDir::new("control");
        let synthetic = dir.join("in.pdf");
        fixture(&synthetic, 6, 90);

        let mut cases: Vec<(String, PathBuf)> = vec![("synthetic-6p".into(), synthetic)];
        for name in [
            "vector-multi.pdf",
            "rotated.pdf",
            "outline-hostile.pdf",
            "incr-scan-5p.pdf",
        ] {
            let path = Path::new("../testdata").join(name);
            if path.exists() {
                cases.push((name.into(), path));
            } else {
                println!("[SKIP] {name}: fixture not generated");
            }
        }

        for (name, path) in cases {
            let present: Vec<u32> = load(&path).get_pages().keys().copied().collect();
            // Keep the first and the last, so the dropped set is neither a
            // prefix nor a suffix and the `/Kids` surgery has to be right in
            // the middle of the array.
            let keep = [1, *present.last().expect("pages")];
            let dropped: Vec<u32> = present
                .iter()
                .copied()
                .filter(|n| !keep.contains(n))
                .collect();

            let mut theirs = load(&path);
            let t = Instant::now();
            theirs.delete_pages(&dropped);
            let their_ms = t.elapsed().as_secs_f64() * 1e3;
            let their_bytes = save(&mut theirs);

            let mut ours = load(&path);
            let t = Instant::now();
            drop_pages(&mut ours, &dropped).expect("drop");
            let our_ms = t.elapsed().as_secs_f64() * 1e3;
            let our_bytes = save(&mut ours);

            println!(
                "[{}] {name:22} {:>4} -> {:>2} pages   lopdf {:>9.1} ms   ours {:>7.1} ms   {:>6.0}x",
                if our_bytes == their_bytes { "OK" } else { "DIFF" },
                present.len(),
                keep.len(),
                their_ms,
                our_ms,
                their_ms / our_ms.max(1e-6),
            );
            assert_eq!(our_bytes, their_bytes, "{name}: bytes differ");
        }
    }

    /// A panel's range names sheets of the job, and every bound is inclusive.
    ///
    /// The interesting row is the last: `1` is the first sheet, not an index, so
    /// an off-by-one here prints the wrong page and nothing else notices --- the
    /// job is correct, the spooler is correct, and the paper is wrong.
    #[test]
    fn a_page_range_names_the_sheets_between_its_ends() {
        assert_eq!(sheets(None, 3), Ok(vec![0, 1, 2]));
        assert_eq!(sheets(Some((1, 3)), 3), Ok(vec![0, 1, 2]));
        assert_eq!(sheets(Some((2, 3)), 3), Ok(vec![1, 2]));
        assert_eq!(sheets(Some((2, 2)), 3), Ok(vec![1]));
        assert_eq!(sheets(Some((1, 1)), 3), Ok(vec![0]));
    }

    /// A job with no sheets asks for none, rather than refusing.
    ///
    /// `None` is "whatever this job has", and a job that has nothing is a
    /// question for whoever built it. Refusing here would put a second, later
    /// answer in front of a reader for a failure `build` already reports.
    #[test]
    fn no_range_over_an_empty_job_asks_for_nothing() {
        assert_eq!(sheets(None, 0), Ok(vec![]));
    }

    /// Three ways a range is wrong, and none of them is repaired.
    ///
    /// Clamping is the tempting alternative and it is the one that reaches
    /// paper: "3 to 99" on a four-sheet job would silently become "3 to 4",
    /// which is a plausible answer to a question the reader did not ask. The
    /// same argument `build` makes about a page outside the document.
    #[test]
    fn a_range_that_cannot_be_printed_is_refused_rather_than_clamped() {
        assert!(sheets(Some((3, 2)), 4).is_err());
        assert!(sheets(Some((0, 2)), 4).is_err());
        assert!(sheets(Some((1, 5)), 4).is_err());
        assert!(sheets(Some((1, 1)), 0).is_err());
    }

    #[test]
    fn a_parent_cycle_does_not_hang_the_rotation_walk() {
        // `effective_rotation` runs on input we did not write. A `/Parent` loop
        // is exactly the shape the outline walk already has to defend against.
        let dir = TempDir::new("cycle");
        let path = dir.join("in.pdf");
        fixture(&path, 1, 0);
        let mut doc = Document::load(&path).expect("load");
        let page = *doc.get_pages().values().next().expect("one page");
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("page")
            .set("Parent", Object::Reference(page));
        assert_eq!(effective_rotation(&doc, page), 0);
    }
}
