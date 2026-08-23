//! Proves that moving every document behind a process boundary changed nothing
//! the reader can see --- and that it really moved.
//!
//! `examples/worker_probe.rs` compares a worker against an in-process render at the
//! *protocol* level. This compares the two at the level callers actually use:
//! one [`RenderService`] per backend, driven through the same public methods the
//! viewer calls, on the same document. The comparison has to be on **pixels**,
//! because `AGENTS.md` records a sandboxed PDFium returning `ok` while drawing a
//! different typeface with about the same amount of ink.
//!
//! The order below is load-bearing. The worker service runs **first**, and the
//! absence of `libpdfium` from the dynamic linker's image table at that point is
//! what says the app process never maps the parser at all --- dyld's own table
//! rather than a milestone of ours, because a mark reports what our code
//! believes it did and the question is what the process *is*. The in-process
//! service then makes the same image appear, which is the control saying the
//! scan can see one and the first check was not passing on a wrong substring.
//!
//! ```text
//! cargo run --release --example backend-probe -- testdata/text-heavy.pdf
//! ```
//!
//! This is the macOS-only body of the `backend-probe` bin; the entry point that
//! gates it is `../backend_probe.rs`.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use tpdf_lib::annots::Comments;
use tpdf_lib::outline::Outline;
use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, Tile, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::search::PageMatches;
use tpdf_lib::startup;
use tpdf_lib::worker;
// Whether this platform starts workers before a document is chosen. Taken from
// the module that owns the fact rather than restated: two checks here branch on
// it --- the spare-lifetime one, which has nothing to leak, and
// `settled_descriptors`, which has nothing to wait for --- and a local
// `cfg!(target_os = "macos")` covering only the first was wrong for a day. The
// wait it left uncovered spent its whole bound and retired the pool it was about
// to measure, which was recorded as a defect in the pool. See the trap.
use tpdf_lib::worker::PRESPAWNS;
use tpdf_lib::worker_child;

/// Tiles are compared at this size: inside the useful range `AGENTS.md`
/// measured, and small enough that a fixture renders quickly.
const TILE: u16 = 512;

/// A render at least this slow can have a withdrawal delivered into it.
///
/// Derived from the *first* tile's measured time, which no defect in the
/// withdrawal path can influence --- a skip condition read off the thing under
/// test is how a broken mechanism reports `[SKIP]` instead of `[FAIL]`.
const WITHDRAWABLE_MS: f64 = 120.0;

/// An idle timeout no run reaches, for services whose pools must hold still.
///
/// A quantity rather than a flag, because `render.rs` deliberately has no
/// spelling for "off" --- a "no value" marker drawn from the value's own range is
/// how a sentinel collides with a real value the moment the timing is right.
const NO_RETIRE: Duration = Duration::from_secs(3600);

/// The idle timeout the retirement phase runs at.
///
/// Four seconds, and both bounds on it are real. It must be long enough that the
/// control below --- a pool sampled *before* the timeout --- is not racing the
/// staggered moments at which a burst's workers finish, which on the A0 sheet are
/// spread over more than a second. It must be short enough that the phase does
/// not dominate the run.
const RETIRE_IDLE: Duration = Duration::from_secs(4);

/// When the control samples the pool: after at least one sweep, well short of
/// [`RETIRE_IDLE`]. The sweep interval is a quarter of the timeout, so a second
/// of it has already passed and a reaper that ignores timestamps has had its
/// chance.
const RETIRE_CONTROL_AT: Duration = Duration::from_millis(1_200);

/// How many workers a document keeps however long it idles.
///
/// Written out rather than imported from `render.rs`, where it is private. That
/// is the point: a check that read the constant would agree with any value it
/// was given, including zero --- and retiring the *last* worker is a distinct
/// defect from not retiring at all, costing the next page turn a re-parse.
const KEPT_WARM: usize = 1;

pub fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        // No platform gate: `worker_child::main` establishes its own boundary and
        // refuses to serve a document without one, which is checked at run time on
        // the process that actually parses the PDF.
        worker_child::main(&args);
    }

    // The first argument that is not a flag, so the child mode below can take
    // the same document without the positional index shifting under it.
    let Some(document) = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    else {
        eprintln!("usage: backend-probe <file.pdf>");
        std::process::exit(2);
    };
    if args.iter().any(|a| a == LIFETIME_ARGV) {
        run_spare_lifetime(document);
    }
    if !document.exists() {
        eprintln!(
            "[FAIL] {} does not exist --- see AGENTS.md on generating fixtures",
            document.display()
        );
        std::process::exit(1);
    }

    let mut report = Report::default();
    let library_dir = library_dir();

    // ------------------------------------------------------- the worker first
    // Retirement pinned off, and this is not tidiness --- half the checks below
    // compare *which pids* the pool holds across a gap, and on a slow corpus that
    // gap can exceed the default idle timeout. A worker legitimately retired
    // between two samples would read as a pool that shrank for the wrong reason,
    // in a check about crash recovery. Retirement gets its own service, at the
    // bottom, where it is the subject rather than the weather.
    let workers = RenderService::start_tuned(
        library_dir.clone(),
        Backend::Worker,
        tpdf_lib::render::pool_size(),
        NO_RETIRE,
    );
    let worker_doc = match wait(|reply| workers.open(document.clone(), false, None, reply)) {
        Ok(info) => info,
        Err(e) => {
            println!("[FAIL] a worker-backed service opens the document      {e}");
            std::process::exit(1);
        }
    };
    // Placed from the document's own first page, which both backends report
    // identically or the geometry check below fails first.
    let at = Placement::inside(worker_doc.pages.first().unwrap_or(&PageSize {
        width_pt: 612.0,
        height_pt: 792.0,
    }));
    let worker_tile = tile_of(&workers, &worker_doc, 1, at);
    // Sampled before anything concurrent has been asked for, because that is the
    // only moment the laziness claim is observable. A service that opened every
    // document with a full pool would satisfy every check below it while costing
    // `capacity` parses of a file nobody has scrolled yet.
    let opened_pool = pool_pids(&workers);
    let opened_children = worker_pids();
    let opened_spares = workers.spare_pids();
    let opened_with = opened_pool.len();

    report.check(
        "the app process has not mapped libpdfium",
        !pdfium_is_mapped(),
        if pdfium_is_mapped() {
            "it has --- something in the worker path parses in this process".into()
        } else {
            format!(
                "{} pages opened and a tile rendered without it, {} images loaded",
                worker_doc.page_count,
                loaded_images()
            )
        },
    );
    report.check(
        "a worker was spawned to do it instead",
        marked("worker spawned"),
        marks(),
    );

    // ---------------------------------------------------- then the in-process
    let in_process = RenderService::start_with(library_dir, Backend::InProcess);
    let native_doc = match wait(|reply| in_process.open(document.clone(), false, None, reply)) {
        Ok(info) => info,
        Err(e) => {
            println!("[FAIL] an in-process service opens the document        {e}");
            std::process::exit(1);
        }
    };
    let native_tile = tile_of(&in_process, &native_doc, 1, at);

    // The control for the first check. Without it, "libpdfium is not mapped" is
    // equally satisfied by a scan that never matches anything --- a wrong
    // substring, a table read the wrong way --- and the strongest claim in this
    // file would rest on a typo.
    report.check(
        "the in-process backend does map it, so the scan can see it",
        pdfium_is_mapped(),
        format!("{} images loaded", loaded_images()),
    );
    report.check(
        "the two services really are different backends",
        workers.backend() == Backend::Worker && in_process.backend() == Backend::InProcess,
        format!("{:?} and {:?}", workers.backend(), in_process.backend()),
    );

    // ------------------------------------------------------------- the pixels
    match (&worker_tile, &native_tile) {
        (Ok(theirs), Ok(ours)) => {
            let same = theirs.bytes == ours.bytes;
            report.check(
                "a tile is identical whichever backend rendered it",
                same,
                if same {
                    format!("{} bytes", ours.bytes.len())
                } else {
                    format!(
                        "{} vs {} bytes, {} differing",
                        theirs.bytes.len(),
                        ours.bytes.len(),
                        differing(&theirs.bytes, &ours.bytes)
                    )
                },
            );
            // Without this, "identical" is satisfied by two blank buffers ---
            // which is exactly what a render that never ran produces.
            let distinct = distinct_values(&ours.bytes);
            report.check(
                "the compared tile is not a uniform buffer",
                distinct > 1,
                format!("{distinct} distinct byte values"),
            );
        }
        (worker, native) => {
            for (which, result) in [("worker", worker), ("in-process", native)] {
                if let Err(e) = result {
                    report.check(
                        &format!("the {which} backend renders a tile"),
                        false,
                        e.clone(),
                    );
                }
            }
        }
    }

    // The two request fields the comparison above leaves at their defaults. A
    // worker that dropped `turns` or `invert` on the way through the protocol
    // would render a perfectly good upright tile, and every check so far would
    // agree with it.
    let worker_view = wait(|reply| workers.tile(view_state_request(worker_doc.id, 2, at), reply));
    let native_view =
        wait(|reply| in_process.tile(view_state_request(native_doc.id, 2, at), reply));
    match (bytes_of(&worker_view), bytes_of(&native_view)) {
        (Ok(theirs), Ok(ours)) => {
            report.check(
                "a turned and inverted tile is identical too",
                theirs == ours,
                format!(
                    "{} bytes, {} differing",
                    ours.len(),
                    differing(&theirs, &ours)
                ),
            );
            // And the control that says the view state did something at all: if
            // it were dropped on *both* sides this would match the plain tile,
            // and "identical" would be two backends agreeing about nothing.
            let plain = worker_tile
                .as_ref()
                .map(|t| t.bytes.clone())
                .unwrap_or_default();
            report.check(
                "and it is not simply the plain tile again",
                ours != plain,
                format!(
                    "{} bytes against the plain tile's {}",
                    ours.len(),
                    plain.len()
                ),
            );
        }
        (theirs, ours) => report.check(
            "a turned and inverted tile is identical too",
            false,
            format!("{theirs:?} / {ours:?}"),
        ),
    }

    // ----------------------------------------------------------- the geometry
    let same_geometry = worker_doc.page_count == native_doc.page_count
        && worker_doc.pages.len() == native_doc.pages.len()
        && worker_doc
            .pages
            .iter()
            .zip(&native_doc.pages)
            .all(|(a, b)| same_size(a, b));
    report.check(
        "page geometry crosses the boundary unchanged",
        same_geometry,
        format!(
            "{} pages / {} sizes against {} and {}",
            worker_doc.page_count,
            worker_doc.pages.len(),
            native_doc.page_count,
            native_doc.pages.len()
        ),
    );

    // --------------------------------------------------------- the text layer
    // Not page 0 where there is a choice. Every one of these carries a page
    // number through the protocol, and a worker that ignored it and always read
    // the first page would be invisible to a check that only ever asks for the
    // first page.
    let page = u32::from(worker_doc.page_count > 1);
    let worker_text = wait(|reply| workers.text(worker_doc.id, page, None, reply));
    let native_text = wait(|reply| in_process.text(native_doc.id, page, None, reply));
    match (&worker_text, &native_text) {
        (Ok(theirs), Ok(ours)) => {
            let same = theirs.codes == ours.codes
                && theirs.boxes == ours.boxes
                && theirs.quarter_turns == ours.quarter_turns;
            report.check(
                "one page's characters and boxes survive the boundary",
                same,
                format!(
                    "{} codes, {} box values",
                    ours.codes.len(),
                    ours.boxes.len()
                ),
            );
        }
        _ => report.check(
            "one page's characters and boxes survive the boundary",
            false,
            format!("{worker_text:?} / {native_text:?}").replace('\n', " "),
        ),
    }

    // The control for the page number above: on a document whose pages carry
    // the same text, asking for a different one proves nothing, and this check
    // has to say so rather than looking like coverage.
    if page > 0 {
        let first = wait(|reply| in_process.text(native_doc.id, 0, None, reply));
        let distinguishable = match (&first, &native_text) {
            (Ok(a), Ok(b)) => a.codes != b.codes,
            _ => false,
        };
        if distinguishable {
            report.check(
                "the page asked for is one a wrong page number would betray",
                true,
                format!("page {page} reads differently from page 0"),
            );
        } else {
            report.skip(
                "the page asked for is one a wrong page number would betray",
                "page 0 and this page carry the same characters, so the checks \
                 around it cannot see a page number that was ignored",
            );
        }
    } else {
        // The name has to appear on a one-page document too. Without this the
        // check does not fail and does not skip --- it is simply absent, and the
        // only trace is the total moving from 32 to 31 between corpora. That is
        // the second time today the same shape has been found by diffing check
        // names across inputs rather than by anything going red.
        report.skip(
            "the page asked for is one a wrong page number would betray",
            "the document has one page, so there is no other page to confuse it with",
        );
    }

    // Searched for something a text page has and a vector one does not, so the
    // count below is evidence rather than a matching pair of zeroes.
    let query = "e".to_string();
    let plain = tpdf_lib::search::Options::default();
    let words = tpdf_lib::search::Options {
        whole_word: true,
        ..plain
    };
    let worker_hits =
        wait(|reply| workers.search(worker_doc.id, page, query.clone(), plain, None, reply));
    let native_hits =
        wait(|reply| in_process.search(native_doc.id, page, query.clone(), plain, None, reply));
    report.check(
        "a search returns the same ranges on both",
        same_matches(&worker_hits, &native_hits),
        describe_matches(&native_hits),
    );

    // The options travel down the same pipe as the query, and a worker that
    // dropped them would answer the unrestricted search --- which is the plain
    // result above, so the two checks together are what catches it. A count that
    // did not move means the option had nothing to bite on here, and that is a
    // skip rather than a pass: the check would agree with a worker ignoring it.
    let worker_words =
        wait(|reply| workers.search(worker_doc.id, page, query.clone(), words, None, reply));
    let native_words =
        wait(|reply| in_process.search(native_doc.id, page, query.clone(), words, None, reply));
    let hit_count =
        |result: &Result<PageMatches, String>| result.as_ref().ok().map(|m| m.matches.len());
    if hit_count(&native_words) == hit_count(&native_hits) {
        report.skip(
            "a search option crosses the worker boundary",
            format!(
                "a whole-word search for {query:?} finds what an unrestricted one does on this \
                 page ({}), so agreement would not show the option arriving",
                describe_matches(&native_hits)
            ),
        );
    } else {
        report.check(
            "a search option crosses the worker boundary",
            same_matches(&worker_words, &native_words),
            format!(
                "whole-word: {}; unrestricted: {}",
                describe_matches(&native_words),
                describe_matches(&native_hits)
            ),
        );
    }

    let worker_outline = wait(|reply| workers.outline(worker_doc.id, reply));
    let native_outline = wait(|reply| in_process.outline(native_doc.id, reply));
    report.check(
        "an outline returns the same tree on both",
        same_outline(&worker_outline, &native_outline),
        describe_outline(&native_outline),
    );

    // Comments cross the boundary too, and they are the one answer that does
    // not come from PDFium at all --- `annots.rs` reads the object graph, and a
    // worker reads it through the mapping it was handed while the in-process
    // backend reads it back off the path. Two different routes to the same
    // bytes, which is exactly the kind of difference this probe exists to find.
    let worker_comments = wait(|reply| workers.comments(worker_doc.id, reply));
    let native_comments = wait(|reply| in_process.comments(native_doc.id, reply));
    report.check(
        "comments return the same list on both",
        same_comments(&worker_comments, &native_comments),
        describe_comments(&native_comments),
    );

    // ------------------------------------------------------------ withdrawing
    // Two halves, and they are withdrawn at different moments on purpose ---
    // see `RenderService::cancel`. The first never reaches a worker at all; the
    // second has to arrive while Pdfium is already inside the render.
    let (ahead, queued) = {
        // Enough tiles to fill the pool, and *then* the one that is withdrawn ---
        // so it is still waiting for a free worker when the withdrawal lands.
        // Two would do on a single-worker service and does not here: with a pool
        // the second tile starts immediately in another process, and the check
        // would quietly become "withdrawn after it started", which passes for a
        // different reason entirely (the parent's own token).
        let filling: Vec<u64> = (0..workers.pool_size() as u64).map(|n| 9 + n).collect();
        let withdrawn = 9 + workers.pool_size() as u64;

        let (tx, rx) = channel();
        for rid in filling.iter().chain(std::iter::once(&withdrawn)) {
            let tx = tx.clone();
            let rid = *rid;
            workers.tile(
                request(worker_doc.id, rid, at),
                Box::new(move |result| {
                    let _ = tx.send((rid, result));
                }),
            );
        }
        drop(tx);
        workers.cancel(withdrawn);

        // Collected by rid, not by arrival. The pool renders concurrently, so
        // replies come back in completion order --- and the withdrawn one is the
        // fastest to answer, which is precisely why reading them in order
        // reported the two outcomes swapped.
        let mut replies: Vec<(u64, Result<TileOutcome, String>)> = rx.iter().collect();
        let take = |replies: &mut Vec<(u64, Result<TileOutcome, String>)>, want: u64| {
            replies
                .iter()
                .position(|(rid, _)| *rid == want)
                .map_or_else(|| Err("no reply".into()), |at| replies.remove(at).1)
        };
        let queued = take(&mut replies, withdrawn);
        let ahead = take(&mut replies, filling[0]);
        (ahead, queued)
    };
    report.check(
        "the tile ahead of a withdrawal still renders",
        matches!(&ahead, Ok(TileOutcome::Rendered(_))),
        outcome_of(&ahead),
    );
    report.check(
        "a tile withdrawn before it starts comes back abandoned",
        matches!(queued, Ok(TileOutcome::Abandoned)),
        outcome_of(&queued),
    );

    let render_ms = worker_tile
        .as_ref()
        .map_or(0.0, |t| t.render_us as f64 / 1e3);
    if render_ms >= WITHDRAWABLE_MS {
        let running = withdraw_in_flight(&workers, worker_doc.id, 11, at);
        // Both halves, and the second is the one that matters. `Abandoned`
        // alone is what this side's own token produces whatever the worker
        // did --- so a withdrawal that never crossed the pipe would still
        // report it, after waiting out the entire render. What says the
        // worker actually stopped is that the reply came back long before
        // the render could have finished.
        report.check(
            "a withdrawal reaches a render already inside Pdfium",
            withdrawn_promptly(&running, render_ms),
            describe_withdrawal(&running, render_ms),
        );
    } else {
        report.skip(
            "a withdrawal reaches a render already inside Pdfium",
            withdrawal_needs_a_slow_page(render_ms),
        );
    }

    // And the control: the service still works afterwards, so "abandoned" is an
    // answer to the withdrawal rather than a worker that has stopped answering.
    let after = tile_of(&workers, &worker_doc, 12, at);
    report.check(
        "the worker-backed service still renders after a withdrawal",
        after.as_ref().is_ok_and(|t| !t.bytes.is_empty()),
        match &after {
            Ok(t) => format!("{} bytes", t.bytes.len()),
            Err(e) => e.clone(),
        },
    );

    // ------------------------------------------------------- surviving a crash
    // The isolation is only worth having if the reader keeps reading. Everything
    // below is about a worker that dies mid-session, and every observation of a
    // *process* is taken from the OS table rather than from bookkeeping of ours
    // --- same reason as the libpdfium check at the top.
    let before = pool_pids(&workers);
    // The pool, as the concurrent tiles above will have grown it. Both bounds
    // matter and they fail differently: below two says nothing ever ran in
    // parallel, above capacity says the ceiling is not a ceiling. `opened_with`
    // is what says it did not simply start this large.
    report.check(
        "concurrent tiles grew the pool, and no further than its capacity",
        before.len() > 1 && before.len() <= workers.pool_size() && opened_with == 1,
        format!(
            "{} workers, capacity {}, opened with {opened_with} \
             (at open: pool {opened_pool:?}, children {opened_children:?}, \
             spares {opened_spares:?}; now pool {before:?})",
            before.len(),
            workers.pool_size(),
        ),
    );

    // The other direction, and it is the one a restart mechanism gets wrong by
    // being too eager. A live worker that answers with an error has *answered*:
    // replacing it would spend a process reopen on every malformed request, and
    // would hide a protocol bug behind a fresh process that gets the next
    // question right. One page past the end is the cheapest way to ask for one.
    let refused = wait(|reply| {
        workers.tile(
            TileRequest {
                crop: None,
                page: worker_doc.page_count as u32,
                ..request(worker_doc.id, 21, at)
            },
            reply,
        )
    });
    let unchanged = pool_pids(&workers);
    report.check(
        "a worker that answers with an error is not replaced",
        refused.is_err() && unchanged == before,
        format!(
            "{} / {before:?} then {unchanged:?}",
            refused.as_ref().err().map_or("rendered", String::as_str)
        ),
    );

    // Deliberately *not* `if let Some(victim) = ...`, which is how this was
    // first written and how it was wrong. A defect that stops workers being
    // replaced also leaves no worker to kill, so every check nested inside that
    // lookup disappeared --- not as a `[SKIP]` but silently, and the only trace
    // was the total dropping from 23 to 22. Found by mutation M1, which was
    // predicted to turn four checks red and turned three.
    let victim = before.first().copied();

    // Establishing that something broke, before asserting it recovered. A check
    // shaped "do X, then wait for the good state" passes on a worker nothing
    // ever touched --- `AGENTS.md` records that one twice.
    let death = kill_a_worker(victim);
    report.check(
        "the worker can be made to die",
        death.is_ok(),
        match &death {
            Ok(()) => format!("pid {victim:?} is gone"),
            Err(e) => e.clone(),
        },
    );

    // Pixels, not merely a reply. A replacement handed a *different* document
    // would answer perfectly well and render something plausible, and the reason
    // the document mapping is shared rather than re-read from its path is
    // precisely that it cannot.
    let recovered = tile_of(&workers, &worker_doc, 20, at);
    report.check(
        "a killed worker is replaced and the same tile returns",
        matches!((&recovered, &worker_tile), (Ok(new), Ok(old)) if new.bytes == old.bytes),
        // Says what was actually compared. Written first as "identical to the
        // tile before it died" on every `Ok`, which under mutation M2 printed
        // that sentence next to `[FAIL]` --- a detail line contradicting its own
        // verdict is worse than none.
        match (&recovered, &worker_tile) {
            (Ok(new), Ok(old)) => format!(
                "{} bytes against the earlier {}, {} differing",
                new.bytes.len(),
                old.bytes.len(),
                differing(&new.bytes, &old.bytes)
            ),
            (Err(e), _) | (_, Err(e)) => e.clone(),
        },
    );

    // Three claims, failing differently. The killed pid must be gone --- a
    // zombie still counts here, so this also says it was reaped. The *rest* of
    // the pool must be untouched, which is the half a single-worker service
    // could not express: a death must cost one process, not the document. And
    // something must still be there to serve the next request.
    let after = pool_pids(&workers);
    let survivors: Vec<u32> = before
        .iter()
        .copied()
        .filter(|p| Some(*p) != victim)
        .collect();
    report.check(
        "a dead worker is retired and the rest of the pool is not",
        victim.is_some_and(|pid| !after.contains(&pid))
            && survivors.iter().all(|pid| after.contains(pid))
            && !after.is_empty(),
        format!("{before:?} then {after:?}, killed {victim:?}"),
    );

    // The pool comes back after a death, which is a different claim from "the
    // request succeeded": a discard that never gave its slot back leaves a pool
    // convinced of a worker that does not exist, and the next burst of work
    // waits for it forever. Issued as a burst, because that is the only thing
    // that grows a pool.
    // Two more than the pool can hold, and every one of them wanted. The
    // withdrawal burst earlier cannot do this job: its extra request is the one
    // that gets withdrawn, and a withdrawn request is refused at the claim
    // *before* it reaches a worker --- by design, so that a tile nobody wants
    // does not occupy a process. So a pool with its ceiling removed grew to
    // exactly capacity there and the check passed. Here the surplus is real, and
    // the two spare service threads exist to carry it.
    let wanted = workers.pool_size() + 2;
    let regrown = {
        let rendered = burst(
            &workers,
            worker_doc.id,
            50,
            wanted,
            at,
            burst_bound(render_ms),
        );
        (pool_pids(&workers).len(), rendered)
    };
    // The worker count is reported and *not* asserted: growth is driven by
    // contention, so on a document whose tiles take half a millisecond a worker
    // is free again before the next request needs a new one, and the pool
    // legitimately stays below capacity. What has to hold on every corpus is
    // that the whole burst came back, within a bound --- which it cannot if a
    // retired worker's slot was never given up.
    report.check(
        "an oversized burst is served, and the pool stays at its ceiling",
        regrown.1 == wanted && regrown.0 <= workers.pool_size(),
        format!(
            "{}/{} tiles, {} workers, capacity {}",
            regrown.1,
            wanted,
            regrown.0,
            workers.pool_size()
        ),
    );

    // A restart has to re-point the withdrawal path at the new process, and
    // nothing above can see whether it did: a withdrawal sent down a dead pipe
    // still comes back `Abandoned` on this side's own token, after waiting out
    // the whole render. So this is the latency assertion again, for the same
    // reason as the first time --- an outcome two mechanisms can produce tests
    // neither of them.
    if render_ms >= WITHDRAWABLE_MS {
        let running = withdraw_in_flight(&workers, worker_doc.id, 22, at);
        report.check(
            "a withdrawal reaches the replacement too",
            withdrawn_promptly(&running, render_ms),
            describe_withdrawal(&running, render_ms),
        );
    } else {
        report.skip(
            "a withdrawal reaches the replacement too",
            withdrawal_needs_a_slow_page(render_ms),
        );
    }

    // The other call site. `with_worker` is shared between them, but a tile is
    // read out of the shared mapping and a JSON reply off the pipe, and only the
    // first of those was exercised above.
    let second = pool_pids(&workers).first().copied();
    let death = kill_a_worker(second);
    let again = wait(|reply| workers.text(worker_doc.id, page, None, reply));
    let same = matches!(
        (&again, &worker_text),
        (Ok(new), Ok(old)) if new.codes == old.codes && new.boxes == old.boxes
    );
    report.check(
        "a killed worker is replaced on the text path too",
        death.is_ok() && second.is_some() && same,
        match (&death, &again) {
            (Err(e), _) => e.clone(),
            (Ok(()), Err(e)) => e.clone(),
            // Said out loud when the page carries no text: the content half of
            // this comparison is then two empty vectors agreeing, and only the
            // recovery is still being asserted.
            (Ok(()), Ok(t)) if t.codes.is_empty() => {
                format!("recovered after killing {second:?}; this page has no text")
            }
            (Ok(()), Ok(t)) => format!("{} codes back after killing {second:?}", t.codes.len()),
        },
    );

    // ------------------------------------------------------ releasing a document
    // Two documents from the same file, so that closing one has something to be
    // measured against: a close that took down more than it was asked to would
    // otherwise look identical to one that worked.
    let before_second = pool_pids(&workers);
    let second_doc = wait(|reply| workers.open(document.clone(), false, None, reply));
    let closing = match &second_doc {
        Ok(info) => {
            // Which pids belong to the document about to be closed: everything
            // that was there before the second document opened. Recorded rather
            // than inferred, because after the close there is no way to ask.
            let closed_pool = before_second.clone();
            let held = pool_pids(&workers);
            let closed = wait(|reply| workers.close(worker_doc.id, reply));
            let left = pool_pids(&workers);
            Some((info.id, held, closed, left, closed_pool))
        }
        Err(_) => None,
    };

    match &closing {
        Some((_, held, closed, left, closed_pool)) => {
            // Every process of that document's pool, and only those. The
            // second document's workers are the ones that must survive, and
            // with a pool this is no longer a count --- it is which pids.
            let survivors: Vec<u32> = held
                .iter()
                .copied()
                .filter(|p| !closed_pool.contains(p))
                .collect();
            report.check(
                "closing a document kills every process holding it",
                closed.is_ok()
                    && !closed_pool.is_empty()
                    && closed_pool.iter().all(|pid| !left.contains(pid))
                    && survivors.iter().all(|pid| left.contains(pid))
                    && !left.is_empty(),
                format!("{held:?} then {left:?}, its pool was {closed_pool:?}"),
            );
            // The point of the second document. Without it "the worker is gone"
            // is equally satisfied by a close that killed every worker there
            // was, which is the failure that matters to a reader with two files
            // open and would look exactly the same here.
            let survivor = tile_of(&workers, second_doc.as_ref().expect("open"), 30, at);
            report.check(
                "the document that was not closed still renders",
                matches!((&survivor, &worker_tile), (Ok(new), Ok(old)) if new.bytes == old.bytes),
                match &survivor {
                    Ok(t) => format!("{} bytes from document {:?}", t.bytes.len(), left),
                    Err(e) => e.clone(),
                },
            );
            // A closed id must be refused rather than answered from whatever is
            // now at that index. Removing the entry instead of holing it shifts
            // every later document down one, and the reader gets tiles from the
            // wrong file with nothing anywhere reporting a problem.
            let stale = wait(|reply| workers.tile(request(worker_doc.id, 31, at), reply));
            report.check(
                "a closed document is refused rather than reused",
                stale.as_ref().err().is_some_and(|e| e.contains("closed")),
                match &stale {
                    Ok(outcome) => format!("answered anyway: {}", outcome_of(&Ok(outcome.clone()))),
                    Err(e) => e.clone(),
                },
            );
            // Everything opening a document took, given back. A worker costs
            // four descriptors here --- the document mapping, the tile mapping,
            // and the two ends of its pipe --- and the withdrawal broadcast
            // holds a *clone* of the pipe's write half, so dropping the worker
            // does not close it. Without clearing that entry the count settles
            // above where it started, which is a descriptor per worker that ever
            // existed and nothing else in this file can see.
            //
            // Its own open/close pair, and not a measurement across the ones
            // above: a document that has been *read* has grown its pool by an
            // amount that depends on timing, so only a freshly opened one --- one
            // worker, by the laziness this file asserts separately --- gives a
            // deterministic delta.
            // Every sample through `settled_descriptors`, not the raw count.
            // An `open` **consumes** the spare and prewarms a replacement on
            // another thread, so a raw sample can be taken with one spare's
            // worth of handles present or absent depending on how far that
            // thread has got --- and the miss resurfaces here as a leak of
            // exactly one spare, which is what this check is looking for. It was
            // raw until pre-spawning reached Windows, where the replacement is
            // slower to appear than on macOS; the race was always here and macOS
            // was winning it.
            let quiet = settled_descriptors(&workers);
            let throwaway = wait(|reply| workers.open(document.clone(), true, None, reply));
            let opened_fds = settled_descriptors(&workers);
            let released = match &throwaway {
                Ok(info) => wait(|reply| workers.close(info.id, reply)),
                Err(e) => Err(e.reason.clone()),
            };
            let settled = settled_descriptors(&workers);
            report.check(
                "closing gives back every descriptor opening took",
                released.is_ok() && opened_fds > quiet && settled == quiet,
                format!("{quiet} quiet, {opened_fds} with it open, {settled} after closing it"),
            );

            // And the id itself is spent. A backend that filled the hole would
            // hand this id to a document the caller has never seen, while every
            // check above still passed.
            let reopened = wait(|reply| workers.open(document.clone(), true, None, reply));
            report.check(
                "a closed id is not handed out to the next document",
                reopened.as_ref().is_ok_and(|info| {
                    info.id != worker_doc.id && info.id != second_doc_id(&second_doc)
                }),
                match &reopened {
                    Ok(info) => format!("id {} after closing {}", info.id, worker_doc.id),
                    Err(e) => e.reason.clone(),
                },
            );
        }
        None => {
            for name in [
                "closing a document kills every process holding it",
                "the document that was not closed still renders",
                "a closed document is refused rather than reused",
                "closing gives back every descriptor opening took",
                "a closed id is not handed out to the next document",
            ] {
                report.check(name, false, "a second document would not open");
            }
        }
    }

    // ------------------------------------------------------- closing under load
    // The drain, which nothing above can reach: every close so far has happened
    // with the pool idle, and a close that did not wait would look identical.
    // Here a render is deliberately still running when the close is issued --- so
    // a close that took the worker out from under it would lose the tile, and a
    // close that waits returns *after* the render it was queued behind.
    if render_ms >= WITHDRAWABLE_MS {
        let busy = wait(|reply| workers.open(document.clone(), true, None, reply));
        match &busy {
            Ok(info) => {
                let (tx, rx) = channel();
                workers.tile(
                    request(info.id, 40, at),
                    Box::new(move |result| {
                        let _ = tx.send(result);
                    }),
                );
                // Long enough that the worker is inside Pdfium, short enough
                // that it cannot have finished.
                std::thread::sleep(Duration::from_millis(60));
                let issued = Instant::now();
                let closed = wait(|reply| workers.close(info.id, reply));
                let waited = issued.elapsed().as_secs_f64() * 1e3;
                let tile = rx.recv().unwrap_or_else(|_| Err("no reply".into()));

                // Three things, and the timing is the one that discriminates. A
                // close that never waited would also see the tile arrive --- the
                // render is in another process and finishes regardless --- so
                // what says it drained is that the close itself did not return
                // until the render was done.
                report.check(
                    "a close waits for the render it interrupted",
                    closed.is_ok()
                        && matches!(&tile, Ok(TileOutcome::Rendered(_)))
                        && waited > render_ms / 3.0,
                    format!(
                        "{}, close returned after {waited:.0} ms of a {render_ms:.0} ms render",
                        outcome_of(&tile)
                    ),
                );
            }
            Err(e) => report.check(
                "a close waits for the render it interrupted",
                false,
                e.reason.clone(),
            ),
        }
    } else {
        report.skip(
            "a close waits for the render it interrupted",
            withdrawal_needs_a_slow_page(render_ms),
        );
    }

    // The other backend closes too, and it is not the same code: no process to
    // kill, no sender to clear, just the Pdfium handle dropped. It shares only
    // the lookup, which is exactly the part a check here can confirm agrees.
    let native_closed = wait(|reply| in_process.close(native_doc.id, reply));
    let native_stale = wait(|reply| in_process.tile(request(native_doc.id, 32, at), reply));
    report.check(
        "the in-process backend releases a document too",
        native_closed.is_ok()
            && native_stale
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("closed")),
        match (&native_closed, &native_stale) {
            (Err(e), _) => e.clone(),
            (Ok(()), Err(e)) => e.clone(),
            (Ok(()), Ok(outcome)) => {
                format!("answered anyway: {}", outcome_of(&Ok(outcome.clone())))
            }
        },
    );

    // ------------------------------------------------------ retiring idle workers
    // Everything already running, sampled before the retirement phase makes a
    // service of its own. `pool_pids` asks the OS for every child of this
    // process, which is exactly right with one service and wrong with two --- the
    // main service's workers would be counted as the new pool's and would never
    // retire, which is the failure this phase exists to detect.
    settle_for(Duration::from_secs(5), || workers.spares_settled());
    let others = worker_pids();
    retiring_idle_workers(&mut report, &document, render_ms, &others);

    // Skipped rather than absent where there are no spares to leak, and it says
    // why: `AGENTS.md` records that a control which silently disappears on some
    // inputs cannot be told apart from one that ran. Both platforms that have a
    // worker now pre-spawn, so in practice this runs everywhere the rest of the
    // file does --- the branch is kept because the *reason* it could skip is a
    // property of the platform rather than of the corpus, and a `PRESPAWNS` that
    // is always true is not a claim this file should be making on its own.
    if PRESPAWNS {
        spare_outlives_nothing(&mut report, &document);
    } else {
        report.skip(
            SPARE_LIFETIME,
            "not applicable --- this platform has no pre-spawned workers to leak",
        );
    }

    report.finish();
}

/// A pool that grew for a burst is given back, and one still in use is not.
///
/// Its **own service**, at its own idle timeout, for two independent reasons. A
/// four-second timeout on the service above would retire workers underneath every
/// pid-identity check in this file; and `TPDF_IDLE_MS` would do it to every
/// service in the process at once, which is the shape `AGENTS.md` records as a
/// control contaminated by the phase beside it.
///
/// The order is the argument. Growing, then a sample *before* the timeout, then
/// one after: without the middle sample "the pool shrank to one" is equally
/// satisfied by a reaper that kills everything it finds on every sweep, which is
/// not retirement --- it is a pool of one with extra steps.
fn retiring_idle_workers(report: &mut Report, document: &Path, render_ms: f64, others: &[u32]) {
    let service = RenderService::start_tuned(
        library_dir(),
        Backend::Worker,
        tpdf_lib::render::pool_size(),
        RETIRE_IDLE,
    );
    let doc = match wait(|reply| service.open(document.to_path_buf(), false, None, reply)) {
        Ok(info) => info,
        Err(e) => {
            // Every name, on every path. A defect that stops the document
            // opening must not make these checks *vanish* --- `AGENTS.md` records
            // a check lost that way whose only trace was the total moving.
            for name in RETIRE_CHECKS {
                report.check(name, false, format!("the document would not open: {e}"));
            }
            return dropping_a_service_kills_its_workers(report, document);
        }
    };
    let at = Placement::inside(doc.pages.first().unwrap_or(&PageSize {
        width_pt: 612.0,
        height_pt: 792.0,
    }));
    let bound = burst_bound(render_ms);

    // The tile the surviving worker has to reproduce, taken while the pool is
    // still the one worker `open` made.
    let before = tile_of(&service, &doc, 600, at);
    let lean_fds = settled_descriptors(&service);
    let lean = pool_pids_besides(&service, others);

    let wanted = service.pool_size();
    let (grown, attempts) = grow_pool(&service, &doc, at, bound, others);
    let grown_fds = settled_descriptors(&service);
    // The precondition, named rather than assumed. Everything below is about
    // giving workers back, and a pool that never grew has nothing to give.
    report.check(
        RETIRE_GROWS,
        grown.len() > lean.len() && lean.len() == KEPT_WARM,
        format!(
            "{} workers from {} after {attempts} burst(s) of {wanted} ({lean:?} then {grown:?})",
            grown.len(),
            lean.len()
        ),
    );

    // The control. A sweep has run by now and the timeout has not expired, so a
    // pool that has shrunk here shrank for a reason that is not idleness.
    std::thread::sleep(RETIRE_CONTROL_AT);
    let held = pool_pids_besides(&service, others);
    report.check(
        RETIRE_CONTROL,
        held.len() == grown.len(),
        format!(
            "{} of {} workers after {:.1} s of a {:.1} s timeout, {:.0} sweeps in",
            held.len(),
            grown.len(),
            RETIRE_CONTROL_AT.as_secs_f64(),
            RETIRE_IDLE.as_secs_f64(),
            RETIRE_CONTROL_AT.as_secs_f64() / (RETIRE_IDLE.as_secs_f64() / 4.0),
        ),
    );

    // Bounded, because the failure is a pool that *stays*, and a check whose
    // failure mode is a wait cannot fail. Polled coarsely: each poll shells out
    // to `pgrep`, and at five milliseconds this would spend the whole wait
    // spawning processes into the table it is counting.
    let shrank = settle_every(RETIRE_IDLE * 4, Duration::from_millis(200), || {
        pool_pids_besides(&service, others).len() <= KEPT_WARM
    });
    let left = pool_pids_besides(&service, others);
    // Exactly one, not "at most". Zero is a different defect with a different
    // cost --- the next page turn pays a spawn and a fresh parse --- and `<=`
    // would call it a pass.
    report.check(
        RETIRE_DOWN,
        shrank && left.len() == KEPT_WARM,
        format!("{} workers left of {}: {left:?}", left.len(), grown.len()),
    );

    // What a retired worker holds that dropping it does not release: the
    // withdrawal broadcast keeps a *clone* of the child's stdin, so the pipe
    // survives the process. It has no functional symptom at all --- writing to a
    // dead pipe fails harmlessly --- so only a count can see it.
    let settled_fds = settled_descriptors(&service);
    report.check(
        RETIRE_FDS,
        grown_fds > lean_fds && settled_fds == lean_fds,
        format!("{lean_fds} with one worker, {grown_fds} grown, {settled_fds} retired"),
    );

    // Pixels, not a reply. A pool left holding the wrong worker --- or a document
    // mapping released with them --- would answer perfectly well.
    let after = tile_of(&service, &doc, 620, at);
    report.check(
        RETIRE_PIXELS,
        matches!((&after, &before), (Ok(new), Ok(old)) if new.bytes == old.bytes),
        match (&after, &before) {
            (Ok(new), Ok(old)) => format!(
                "{} bytes against the earlier {}, {} differing",
                new.bytes.len(),
                old.bytes.len(),
                differing(&new.bytes, &old.bytes)
            ),
            (Err(e), _) | (_, Err(e)) => e.clone(),
        },
    );

    // The bookkeeping half, and it fails by *waiting* rather than by answering
    // wrongly: a retirement that took the process without lowering `spawned`
    // leaves a pool at a ceiling nothing is under, so the checkout blocks for a
    // worker that can never come home. Hence the bound, and hence bursts rather
    // than one tile --- one tile is served by the survivor and proves nothing.
    let (regrown, retries) = grow_pool(&service, &doc, at, bound, others);
    report.check(
        RETIRE_REGROWS,
        regrown.len() > KEPT_WARM,
        format!(
            "{} workers after {retries} burst(s): {regrown:?}",
            regrown.len()
        ),
    );

    // The other reader of `spawned`. `close` drains by comparing it against the
    // idle count, so a retirement that moved one and not the other hangs here
    // too --- differently enough to be worth its own check, since the close path
    // is the one a reader reaches by shutting a file rather than by scrolling.
    let closed = wait(|reply| service.close(doc.id, reply));
    report.check(
        RETIRE_CLOSES,
        closed.is_ok(),
        match &closed {
            Ok(()) => format!(
                "after retiring {} of {} workers",
                grown.len() - 1,
                grown.len()
            ),
            Err(e) => e.clone(),
        },
    );

    drop(service);
    dropping_a_service_kills_its_workers(report, document);
}

/// A service that goes away takes its processes with it.
///
/// This is the check the reaper's `Weak` handle is for. A reaper holding a strong
/// `Arc<Workers>` would keep the pool --- and every document mapping in it ---
/// alive for the life of the process after the last handle to the service was
/// dropped, and nothing else here could see it: every other check in this file
/// runs against a service that is still alive, which is exactly when the leak is
/// invisible. Same shape as the spare that outlived its parent.
///
/// A separate service, opened and dropped, rather than a claim about one of the
/// services above: the property is what happens *after* the last handle goes, and
/// only a service nothing else holds can be taken to that point.
fn dropping_a_service_kills_its_workers(report: &mut Report, document: &Path) {
    let before = worker_pids();
    let doomed = RenderService::start_tuned(library_dir(), Backend::Worker, 2, RETIRE_IDLE);
    let opened = wait(|reply| doomed.open(document.to_path_buf(), true, None, reply));

    // Waited for, so the spare is *in the slot* rather than owned by the thread
    // still warming it. Both die, but only the first dies promptly, and a check
    // measuring the second would be measuring a race.
    settle_for(Duration::from_secs(5), || {
        !doomed.spare_pids().is_empty() && doomed.spares_settled()
    });
    let mine: Vec<u32> = worker_pids()
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    drop(doomed);

    let gone = settle_every(Duration::from_secs(10), Duration::from_millis(100), || {
        mine.iter().all(|pid| !pid_is_running(*pid))
    });
    let survivors: Vec<u32> = mine
        .iter()
        .copied()
        .filter(|pid| pid_is_running(*pid))
        .collect();
    report.check(
        RETIRE_DROP,
        opened.is_ok() && !mine.is_empty() && gone,
        match &opened {
            Err(e) => e.reason.clone(),
            Ok(_) if mine.is_empty() => "the service spawned no process to lose".into(),
            Ok(_) if gone => format!("its {} process(es) {mine:?} went with it", mine.len()),
            Ok(_) => format!("{survivors:?} of {mine:?} outlived the service by 10 s"),
        },
    );
}

const RETIRE_GROWS: &str = "a burst grows the pool it will later give back";
const RETIRE_CONTROL: &str = "a worker idle for less than its timeout survives a sweep";
const RETIRE_DOWN: &str = "an idle pool is retired down to one worker";
const RETIRE_FDS: &str = "retiring gives back every descriptor growing took";
const RETIRE_PIXELS: &str = "the worker left after a retirement renders the same tile";
const RETIRE_REGROWS: &str = "a burst after a retirement grows the pool again";
const RETIRE_CLOSES: &str = "a close completes after a retirement";
const RETIRE_DROP: &str = "dropping a service kills the workers it owned";

/// Every check the retirement phase records, so that a document which will not
/// open reports them rather than losing them.
///
/// The names are shared with the call sites rather than repeated, because two
/// copies of a list like this drift and the drift is silent: a renamed check
/// would simply appear twice, once per path, and the invariant that says the set
/// of names is fixed would still hold.
const RETIRE_CHECKS: [&str; 7] = [
    RETIRE_GROWS,
    RETIRE_CONTROL,
    RETIRE_DOWN,
    RETIRE_FDS,
    RETIRE_PIXELS,
    RETIRE_REGROWS,
    RETIRE_CLOSES,
];

/// Argument that runs this binary as the short-lived service the check below
/// watches, rather than as the probe.
const LIFETIME_ARGV: &str = "--spare-lifetime";

/// Named beside the other check names so the skip and the check cannot drift ---
/// the same reason `RETIRE_CHECKS` exists.
const SPARE_LIFETIME: &str = "a spare does not outlive the service that started it";

/// A spare must not outlive the process that started it.
///
/// This is the only check here that needs a **second process**, and it needs one
/// for a reason no in-process assertion can work around: the leak it exists to
/// catch is invisible while the parent is alive. A spare waits in `recvmsg` on a
/// socket whose other end the parent holds, so during a run there is nothing
/// wrong to see --- the failure is that the socket does not reach EOF when the
/// parent goes away, because a sibling spawned later inherited a copy of that
/// end. `AGENTS.md` has the mechanism: descriptors from `socketpair` are not
/// close-on-exec, and `Drop` cannot help because `std::process::exit` runs no
/// destructors.
///
/// So the shape is: run a service in a child, let it exit, and ask whether the
/// grandchild is gone. Measured against the real defect --- with `FD_CLOEXEC`
/// removed, `backend-probe` left one orphaned `--prespawn` process per corpus,
/// reparented to init and still alive twenty minutes later.
///
/// **The parent must not read the child's output to EOF.** That pipe is exactly
/// what a leaked spare holds open, so waiting for it turns a red check into a
/// hang --- which is how this defect presented in the first place: a probe run
/// that printed a complete report and then never exited. One line, then `wait`.
fn spare_outlives_nothing(report: &mut Report, document: &Path) {
    let name = SPARE_LIFETIME;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return report.check(name, false, format!("cannot find this binary: {e}")),
    };
    let spawned = std::process::Command::new(exe)
        .arg(LIFETIME_ARGV)
        .arg(document)
        .stdout(std::process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return report.check(name, false, format!("cannot run the child service: {e}")),
    };

    // One line, never to EOF. See the note above.
    let announced = child.stdout.take().map(|out| {
        let mut line = String::new();
        let _ = std::io::BufReader::new(out).read_line(&mut line);
        line
    });
    let pids: Vec<u32> = announced
        .as_deref()
        .unwrap_or_default()
        .strip_prefix("SPARE ")
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .unwrap_or_default();

    let exited = child.wait();
    if pids.is_empty() {
        // Distinguished from a leak, because "no spare was ever made" and "the
        // spare survived" are different defects and only one of them is this
        // check's subject.
        return report.check(
            name,
            false,
            format!(
                "the child service announced no spare (it said {announced:?}, exit {exited:?})"
            ),
        );
    }

    // Bounded, not blocking: the property breaks by a process *staying*, and a
    // check whose failure mode is a wait cannot fail.
    let gone = settle_for(Duration::from_secs(5), || {
        pids.iter().all(|pid| !pid_is_running(*pid))
    });
    let survivors: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|pid| pid_is_running(*pid))
        .collect();
    report.check(
        name,
        gone,
        if gone {
            format!("its {} spare process(es) {pids:?} went with it", pids.len())
        } else {
            format!("{survivors:?} of {pids:?} still running 5 s after the service exited")
        },
    );
}

/// Runs a render service for as long as it takes to warm a spare, then exits.
///
/// A document is opened and tiles are requested first, so the pool grows and
/// spawns children *after* the surviving spare's socket pair exists --- which is
/// the whole condition for the leak. A service that only ever warmed a spare and
/// exited would clean up correctly even with the descriptor flags removed, and
/// the check watching it would pass while the defect was present.
fn run_spare_lifetime(document: PathBuf) -> ! {
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let opened = wait(|reply| service.open(document, false, None, reply));
    if let Ok(info) = &opened {
        let at = Placement::inside(info.pages.first().unwrap_or(&PageSize {
            width_pt: 612.0,
            height_pt: 792.0,
        }));
        // Concurrent, so the pool grows: every worker it spawns inherits any
        // descriptor the parent left inheritable.
        let mut pending = Vec::new();
        for rid in 0..4 {
            let (tx, rx) = channel();
            service.tile(
                request(info.id, 900 + rid, at),
                Box::new(move |r| {
                    let _ = tx.send(r);
                }),
            );
            pending.push(rx);
        }
        for rx in pending {
            let _ = rx.recv_timeout(Duration::from_secs(60));
        }
    }
    // Whatever the slot holds now, warm or still warming: a spare that leaks
    // while warming leaks just as thoroughly as one that finished.
    settle_for(Duration::from_secs(10), || !service.spare_pids().is_empty());
    let pids = service.spare_pids();
    println!(
        "SPARE {}",
        pids.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    );
    let _ = std::io::stdout().flush();
    // Exactly how the app and every probe leave: no destructors run, which is
    // the condition under which the leak happens at all.
    std::process::exit(0);
}

/// The id of a document that may not have opened.
///
/// `u32::MAX` for one that did not, which no real id reaches --- so a comparison
/// against it is false rather than accidentally true.
fn second_doc_id(info: &Result<DocumentInfo, tpdf_lib::progressive::Refusal>) -> u32 {
    info.as_ref().map_or(u32::MAX, |info| info.id)
}

/// Issues a tile and withdraws it once the worker is inside Pdfium, reporting
/// the outcome and how long the reply took to arrive.
///
/// The delay is what makes this a test of the *wire* withdrawal rather than of
/// the parent's queue: withdrawn before it is claimed, a request never reaches a
/// worker at all.
fn withdraw_in_flight(
    service: &RenderService,
    doc: u32,
    rid: u64,
    at: Placement,
) -> (Result<TileOutcome, String>, f64) {
    let (tx, rx) = channel();
    service.tile(
        request(doc, rid, at),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    // Long enough that the worker is inside Pdfium, short enough that it cannot
    // have finished: the render takes `render_ms`, which the caller checked.
    std::thread::sleep(Duration::from_millis(60));
    let sent = Instant::now();
    service.cancel(rid);
    let outcome = rx.recv().unwrap_or_else(|_| Err("no reply".into()));
    (outcome, sent.elapsed().as_secs_f64() * 1e3)
}

/// Whether a withdrawal both took effect and did so before the render could
/// have finished on its own.
fn withdrawn_promptly(result: &(Result<TileOutcome, String>, f64), render_ms: f64) -> bool {
    matches!(result.0, Ok(TileOutcome::Abandoned)) && result.1 < render_ms / 3.0
}

fn describe_withdrawal(result: &(Result<TileOutcome, String>, f64), render_ms: f64) -> String {
    format!(
        "{} after {:.1} ms, against a {render_ms:.0} ms render",
        outcome_of(&result.0),
        result.1
    )
}

fn withdrawal_needs_a_slow_page(render_ms: f64) -> String {
    format!(
        "a tile of this document renders in {render_ms:.1} ms, under the \
         {WITHDRAWABLE_MS:.0} ms a withdrawal needs to arrive --- run this on \
         testdata/vector-heavy.pdf"
    )
}

/// How many kernel handles this process currently holds open.
///
/// The Windows counterpart of counting `/dev/fd`, and the same question: does
/// closing a document give back what opening it took. A worker costs handles
/// here exactly as it costs descriptors there --- two pipe ends, a process, a
/// thread, a job, two sections --- so a leak shows up the same way.
///
/// `GetProcessHandleCount` is the kernel's own answer, which is the property
/// that matters; enumerating them would be a larger and no more truthful way to
/// arrive at a number that is only ever compared against another sample of
/// itself.
#[cfg(windows)]
fn open_descriptors() -> usize {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

    let mut count: u32 = 0;
    // SAFETY: a pseudo-handle to self, and `count` outlives the call.
    let ok = unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) };
    if ok == 0 {
        return 0;
    }
    count as usize
}

/// How many descriptors this process currently holds open.
///
/// `/dev/fd` is the kernel's own answer, listing exactly what this process has.
/// A count rather than a set because the question is whether closing a document
/// gives back what opening it took, and the numbers themselves are reused.
#[cfg(target_os = "macos")]
fn open_descriptors() -> usize {
    // The read_dir handle is itself a descriptor and is counted, which is fine:
    // it is counted identically in both samples, so it cancels.
    std::fs::read_dir("/dev/fd").map_or(0, |entries| entries.count())
}

/// The worker processes this probe has spawned, from the OS process table.
///
/// `pgrep`, rather than anything of ours: the claim is that one *process* was
/// replaced by another, and the kernel's table is the observable for that. A
/// count derived from our own `Vec<Held>` would report whatever the code under
/// test believes.
///
/// **Matched on argv, not merely on parentage, and that is a fix rather than a
/// refinement.** "Every child of this process is a worker" is false in the one
/// way nobody looks for: `caffeinate -du <utility>` does not run the utility as
/// its child --- it forks a helper to hold the power assertion and then `exec`s
/// the utility in the *parent*, so the helper ends up a child of the very process
/// it was wrapping. `AGENTS.md` tells you to wrap long batches in exactly that,
/// so following the standing advice made this probe report `7 workers, capacity
/// 6` and `opened with 2` --- a capacity overrun and a broken laziness claim, both
/// entirely fictitious, and both perfectly reproducible. Run bare, the same
/// binary passed.
///
/// The probe's own `--spare-lifetime` child is excluded by the same filter, which
/// it needed anyway.
///
/// The Windows twin below matches on the child's **image name** rather than on
/// argv, because Toolhelp reports a parent pid and an image but no command line
/// --- reaching that needs `NtQueryInformationProcess` and a read of the child's
/// PEB, which is a great deal of machinery for a filter. It is genuinely weaker,
/// and it is sufficient *here* for a reason worth stating rather than assuming:
/// the artifact it would miss is a same-image child of ours that is not a worker,
/// and nothing on this platform forks one --- the `caffeinate` shape that forced
/// the argv match is a macOS wrapper with no Windows counterpart, and the
/// `--spare-lifetime` child is never started because pre-spawning is not
/// implemented here.
#[cfg(windows)]
fn worker_pids() -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    // Our own image name, so a child that re-exec'd `current_exe` matches and
    // anything else does not. Compared case-insensitively: Windows paths are.
    let ours = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let Some(ours) = ours else {
        return Vec::new();
    };
    let us = std::process::id();

    // SAFETY: a documented flag and a zero pid meaning "all processes"; the
    // snapshot is closed on every path out.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut found = Vec::new();
    // SAFETY: zeroed is the documented initial state; `dwSize` is set as required.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: a live snapshot handle and an initialised entry.
    if unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0 {
        loop {
            if entry.th32ParentProcessID == us {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase() == ours {
                    found.push(entry.th32ProcessID);
                }
            }
            // SAFETY: as above.
            if unsafe { Process32NextW(snapshot, &raw mut entry) } == 0 {
                break;
            }
        }
    }
    // SAFETY: the snapshot handle, closed once.
    unsafe { CloseHandle(snapshot) };
    found
}

/// The worker processes this probe has spawned, from the OS process table.
#[cfg(target_os = "macos")]
fn worker_pids() -> Vec<u32> {
    let out = std::process::Command::new("pgrep")
        .arg("-P")
        .arg(std::process::id().to_string())
        // Matches against the full argument list, which is where the marker is.
        // `--` because the pattern begins with a dash.
        .arg("-f")
        .arg("--")
        .arg(worker::WORKER_ARGV)
        .output();
    out.map(|out| {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    })
    .unwrap_or_default()
}

/// Children of this process that are serving a document.
///
/// A warmed spare is a child too, and counting it as a pool worker is what first
/// turned the capacity check red at `7 workers, capacity 6` --- correctly, since
/// seven processes existed. What was wrong was the question, not the answer: the
/// bound is on workers *for a document*, and a spare has none. Excluded by
/// identity rather than by widening the bound to `capacity + 1`, which would also
/// have accepted a genuine overrun by one.
fn pool_pids(service: &RenderService) -> Vec<u32> {
    // Waited for, because a spare between `fork` and registration is a child this
    // process cannot name -- and an unnamed child is counted as a pool worker,
    // which reads as the pool's laziness being broken.
    settle_for(Duration::from_secs(5), || service.spares_settled());
    let spares = service.spare_pids();
    worker_pids()
        .into_iter()
        .filter(|pid| !spares.contains(pid))
        .collect()
}

/// The workers of one service, when more than one service is running.
///
/// [`pool_pids`] asks the OS for every child of this process, which is the right
/// question with one service and the wrong one with two: it cannot tell whose
/// child a pid is. Subtracting a snapshot taken before this service existed is
/// the only separation available, and it holds only because the *other* services
/// here are pinned against retirement and are issued nothing during the phase ---
/// a pool of theirs that moved would be attributed to this one.
fn pool_pids_besides(service: &RenderService, others: &[u32]) -> Vec<u32> {
    pool_pids(service)
        .into_iter()
        .filter(|pid| !others.contains(pid))
        .collect()
}

/// This process's descriptor count, once the spare slot holds a settled spare.
///
/// The wait is load-bearing, and one condition is not enough. A spare costs four
/// descriptors --- its tile mapping, our end of the handover socket, and the two
/// ends of its pipe --- and they appear the instant `Command::spawn` returns. But
/// `spares_settled` is *also* true before the prewarm thread has claimed the
/// slot, so a sample taken in that first window misses all four, and the miss
/// resurfaces later as a leak of exactly that size. Waiting for a pid as well as
/// for the claim brackets both windows.
///
/// **The pid clause is asked for only where a spare can exist**, and the reason
/// is worth more than the guard. Windows never pre-spawns, so `spare_pids` is
/// empty for the life of the process and this wait spent its whole five-second
/// bound on every call --- silently, because `settle_for`'s verdict was
/// discarded. Five seconds is longer than [`RETIRE_IDLE`], so the sample taken
/// to measure the grown pool *retired* it, and the phase then reported one
/// worker of six 1.2 s into the timeout and an unchanged handle count. Both
/// readings were true, and neither was about the pool: they described a pool
/// left alone for five seconds by its own instrument. It was recorded as a
/// pooling defect for a day.
///
/// So the verdict is no longer discarded. A wait that expires where it was meant
/// to succeed is a broken sample rather than a slow one, and the next platform
/// without spares must not be able to reintroduce this quietly.
fn settled_descriptors(service: &RenderService) -> usize {
    let bound = Duration::from_secs(5);
    let settled = settle_for(bound, || {
        (!PRESPAWNS || !service.spare_pids().is_empty()) && service.spares_settled()
    });
    if !settled {
        eprintln!(
            "[WARN] the descriptor sample waited out its {:.0} s bound; every count below \
             was taken {:.0} s after the state it is meant to describe",
            bound.as_secs_f64(),
            bound.as_secs_f64()
        );
    }
    open_descriptors()
}

/// How long a burst of tiles is given before it is called a wedge.
///
/// Scaled off the measured render rather than fixed, so that a corpus taking a
/// second a tile is not called hung, and a corpus taking a millisecond does not
/// have to be waited on for ten seconds to find out that it is.
fn burst_bound(render_ms: f64) -> Duration {
    Duration::from_secs_f64((render_ms * 20.0 / 1e3).max(10.0))
}

/// Issues `count` tiles at once and waits for them, bounded, returning how many
/// came back rendered.
///
/// All issued before any is awaited, which is what makes it a burst: one at a
/// time would never put two requests in front of the pool and would never grow
/// it. **Bounded** rather than blocking, because the defects it is used against
/// --- a pool that believes in a worker it retired, a ceiling nothing is under ---
/// fail by waiting, and a check that hangs is one the harness has to interpret
/// rather than read.
fn burst(
    service: &RenderService,
    doc: u32,
    first_rid: u64,
    count: usize,
    at: Placement,
    bound: Duration,
) -> usize {
    let (tx, rx) = channel();
    for n in 0..count as u64 {
        let tx = tx.clone();
        service.tile(
            request(doc, first_rid + n, at),
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
    }
    drop(tx);

    let started = Instant::now();
    let mut rendered = 0;
    while rendered < count {
        let left = bound.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Ok(Ok(TileOutcome::Rendered(_))) => rendered += 1,
            _ => break,
        }
    }
    rendered
}

/// Issues bursts until the document has more than one worker, or gives up.
///
/// **A burst does not reliably grow a pool, and that is correct behaviour rather
/// than a flaw to assert around.** Growth happens when a request arrives while
/// every worker is busy, so it is a race between a render and a checkout --- and
/// on `text-heavy`, where a tile is 0.6 ms against a 12 ms spawn, the first
/// worker is free again before the second request needs another. The check
/// immediately above this one in the main phase says as much about its own pool
/// and declines to assert a size for exactly this reason.
///
/// Retrying is sound because growth is a **precondition** here, not the property:
/// nothing about retirement is being measured yet, and a precondition driven to
/// hold is worth more than one asserted on a coin toss. What must not happen is
/// the retry hiding a failure, so the attempt count is returned and reported, and
/// exhausting the attempts leaves the pool at one and the check red.
///
/// The alternative --- skipping the whole phase on a fast corpus, as the
/// withdrawal checks do --- was rejected: it would leave retirement checked on one
/// fixture out of six, and the thing that makes withdrawal genuinely unskippable
/// there (a render long enough to interrupt) has no analogue here.
fn grow_pool(
    service: &RenderService,
    doc: &DocumentInfo,
    at: Placement,
    bound: Duration,
    others: &[u32],
) -> (Vec<u32>, usize) {
    /// Enough that a corpus which grows on one burst in three still gets there,
    /// and few enough that a pool which cannot grow at all fails in seconds.
    const ATTEMPTS: usize = 8;
    /// Rising across calls, so no two requests in a run share a `rid` --- a reused
    /// one is a request the queue has already seen and may refuse.
    static NEXT_RID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(600);

    let wanted = service.pool_size();
    let mut pids = pool_pids_besides(service, others);
    for attempt in 1..=ATTEMPTS {
        let first = NEXT_RID.fetch_add(wanted as u64, std::sync::atomic::Ordering::Relaxed);
        burst(service, doc.id, first, wanted, at, bound);
        pids = pool_pids_besides(service, others);
        if pids.len() > 1 {
            return (pids, attempt);
        }
    }
    (pids, ATTEMPTS)
}

/// Kills a worker, or reports that there was no worker to kill.
///
/// The absence is an outcome rather than a reason to stop: a defect that stops
/// workers being replaced arrives here with nothing to kill, and a check that
/// quietly does not run in that case is invisible.
fn kill_a_worker(pid: Option<u32>) -> Result<(), String> {
    match pid {
        Some(pid) => kill_and_wait(pid),
        None => Err("no worker process is holding the document by this point".into()),
    }
}

/// Kills a worker and waits for it to actually be gone.
///
/// **SIGKILL rather than SIGSEGV, and that is not a stylistic choice.** A Rust
/// process absorbs the first SIGSEGV *sent* to it: std installs a handler so a
/// stack overflow can be reported, and on a fault address outside the guard page
/// that handler restores the default disposition and returns --- which for a
/// signal that arrived by `kill(2)` simply resumes the process. Measured while
/// writing this: the worker survived SIGSEGV and `/bin/sleep` did not, and the
/// checks below then passed against a worker that had never died. A genuine
/// Pdfium fault still terminates, because the faulting instruction re-executes
/// against the restored default; only a sent one is swallowed.
///
/// The wait is not a sleep, and is not optional either: a signal is delivered
/// asynchronously, so asking the replacement question too early is answered by
/// the worker that is still alive.
/// Off unix there is no worker to kill, because none can be spawned.
///
/// Refuses rather than terminating by some other route: this check exists to
/// prove a worker is *replaced* after dying, and there is nothing to replace on
/// a platform where the pool never starts one.
///
/// # Errors
///
/// Always.
/// Kills a worker and waits for the kernel to agree it is gone.
///
/// `TerminateProcess` rather than a signal, because Windows has none --- the
/// exit code is the only channel, which is why `sandbox_win::KILLED_EXIT` exists.
/// This is a *hostile* kill from outside the pool, standing in for a worker that
/// crashed, so it deliberately does not go through `Contained::kill`: the pool
/// must notice a death it did not cause.
///
/// The wait matters as much as the kill. `TerminateProcess` is asynchronous ---
/// it returns once termination is *requested* --- so a probe that killed and
/// immediately counted processes would race the kernel and see the worker still
/// there, which reads exactly like a pool that failed to reap.
#[cfg(windows)]
fn kill_and_wait(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_FAILED};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };

    // SAFETY: a pid we spawned; the handle is closed on every path out.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "could not open pid {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a live handle opened with PROCESS_TERMINATE.
    let killed = unsafe { TerminateProcess(handle, 1) };
    // SAFETY: opened with PROCESS_SYNCHRONIZE, so it can be waited on.
    let waited = unsafe { WaitForSingleObject(handle, INFINITE) };
    // SAFETY: opened above and closed once.
    unsafe { CloseHandle(handle) };

    if killed == 0 {
        return Err(format!(
            "could not terminate pid {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if waited == WAIT_FAILED {
        return Err(format!(
            "pid {pid} was terminated but could not be waited on: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn kill_and_wait(_pid: u32) -> Result<(), String> {
    Err(worker::NO_WORKERS.into())
}

#[cfg(unix)]
fn kill_and_wait(pid: u32) -> Result<(), String> {
    // SAFETY: `kill` takes two integers and touches nothing this process owns.
    if unsafe { libc::kill(pid as i32, libc::SIGKILL) } != 0 {
        return Err(format!(
            "could not signal pid {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !pid_is_running(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(format!("pid {pid} was still running 5 s after SIGKILL"))
}

/// Whether a pid names a process that has not exited.
///
/// The state column rather than the pid's existence: a signalled child stays in
/// the table as a zombie until its parent reaps it, and `kill(pid, 0)` succeeds
/// on a zombie. Reading only "does the pid exist" would report a dead worker as
/// alive right up until the restart that reaps it.
/// Polls until a condition holds, or the bound expires.
///
/// Bounded rather than blocking, because the properties it is used for break by
/// *not happening*: a spare that never warms would otherwise stop the run with no
/// verdict, and `AGENTS.md` records that a check whose failure mode is a wait
/// cannot fail.
fn settle_for(bound: Duration, ready: impl FnMut() -> bool) -> bool {
    settle_every(bound, Duration::from_millis(5), ready)
}

/// [`settle_for`] with the poll interval named.
///
/// Some of these conditions are answered by shelling out to `pgrep` and `ps`, and
/// at five milliseconds a long wait spends itself spawning processes into the
/// very table it is counting. A condition that costs a syscall keeps the tight
/// interval; one that costs a fork gets a coarse one.
fn settle_every(bound: Duration, every: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + bound;
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(every);
    }
    ready()
}

fn pid_is_running(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "state=", "-p", &pid.to_string()])
        .output();
    out.map(|out| {
        let state = String::from_utf8_lossy(&out.stdout);
        let state = state.trim();
        !state.is_empty() && !state.starts_with('Z')
    })
    .unwrap_or(false)
}

/// Where on the page the compared tile is taken from.
///
/// Computed from the document's own geometry rather than fixed, because a
/// rectangle that lands in a margin renders a uniform buffer --- and two
/// backends agreeing about an empty tile is not evidence of anything. The
/// `rotated-90` fixture is where a fixed rectangle was caught doing exactly
/// that.
#[derive(Clone, Copy)]
struct Placement {
    scale: f32,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
}

impl Placement {
    /// A rectangle inside the page, deliberately asymmetric.
    ///
    /// Not square, not at the origin and not at 1x: every field here has to
    /// survive translation into the worker's protocol and back, and a request
    /// whose width equals its height and whose `x` equals its `y` cannot tell a
    /// field that was dropped from one that arrived --- the pixels come out the
    /// same either way.
    fn inside(page: &PageSize) -> Self {
        let scale = 1.25_f32;
        let scaled_width = page.width_pt * scale;
        let scaled_height = page.height_pt * scale;
        // Different fractions in each axis, so a transposed pair is visible.
        let width = clamp_side(scaled_width * 0.55);
        let height = clamp_side(scaled_height * 0.4);
        Self {
            scale,
            x: ((scaled_width - f32::from(width)) / 3.0).max(0.0) as i32,
            y: ((scaled_height - f32::from(height)) / 5.0).max(0.0) as i32,
            width,
            height,
        }
    }
}

/// A tile side, kept inside the range `AGENTS.md` measured as useful and off
/// zero, which Pdfium has nothing to render into.
fn clamp_side(pixels: f32) -> u16 {
    pixels.clamp(64.0, f32::from(TILE)) as u16
}

/// One tile request at the chosen placement.
fn request(doc: u32, rid: u64, at: Placement) -> TileRequest {
    TileRequest {
        crop: None,
        rid,
        doc,
        page: 0,
        scale: at.scale,
        turns: 0,
        invert: false,
        x: at.x,
        y: at.y,
        width: at.width,
        height: at.height,
        format: TileFormat::Raw,
    }
}

/// The same tile as the reader would see it turned and inverted.
///
/// `turns` and `invert` are the two request fields the plain comparison leaves
/// at their defaults, so they need a request that does not.
fn view_state_request(doc: u32, rid: u64, at: Placement) -> TileRequest {
    TileRequest {
        turns: 1,
        invert: true,
        ..request(doc, rid, at)
    }
}

/// Renders one tile and waits for it, failing an abandoned reply.
fn tile_of(
    service: &RenderService,
    doc: &DocumentInfo,
    rid: u64,
    at: Placement,
) -> Result<Tile, String> {
    match wait(|reply| service.tile(request(doc.id, rid, at), reply))? {
        TileOutcome::Rendered(tile) => Ok(tile),
        TileOutcome::Abandoned => Err("the tile was abandoned, and nothing withdrew it".into()),
    }
}

/// The longest this probe will wait for any single answer.
///
/// Far above every legitimate wait here --- the slowest is a 1.2 s render, and
/// the close that drains one --- and far below "forever". It exists because
/// several properties in this file fail by **waiting** rather than by answering
/// wrongly: a pool that believes in a worker it retired never finishes a close,
/// and a probe that blocked there would produce no verdict at all. A check that
/// hangs is one the harness has to interpret; a check that goes red says which.
const ANSWER_BOUND: Duration = Duration::from_secs(60);

/// Drives one of the service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx): (_, Receiver<Result<T, E>>) = channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(E::from(format!(
            "no answer within {} s --- the service is wedged, not slow",
            ANSWER_BOUND.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(E::from("the render thread stopped".to_string()))
        }
    }
}

/// Whether a startup milestone has been recorded in this process.
fn marked(name: &str) -> bool {
    startup::timeline().iter().any(|(mark, _)| mark == name)
}

/// Every dynamic library this process has mapped, by path.
///
/// Toolhelp's module list, which is what the loader itself holds --- the Windows
/// counterpart of dyld's image table below, and used for the same reason: a
/// milestone says what our code believes it did, and the question here is what
/// the process actually is.
///
/// Read of *this* process, unlike `scripts/win_modules.py`, which reads the app
/// from outside it. Both exist and neither replaces the other: the script is the
/// stronger oracle and can only watch a real application, while this one is
/// available to a probe that is its own subject.
#[cfg(windows)]
fn mapped_images() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };

    // SAFETY: our own pid; the snapshot is closed on every path out.
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, std::process::id())
    };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut found = Vec::new();
    // SAFETY: zeroed is the documented initial state; `dwSize` is set as the API
    // requires and the entry outlives every call it is passed to.
    let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<MODULEENTRY32W>()).unwrap_or(0);
    // SAFETY: a live snapshot handle and an initialised entry.
    if unsafe { Module32FirstW(snapshot, &raw mut entry) } != 0 {
        loop {
            let len = entry
                .szExePath
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExePath.len());
            found.push(String::from_utf16_lossy(&entry.szExePath[..len]));
            // SAFETY: as above.
            if unsafe { Module32NextW(snapshot, &raw mut entry) } == 0 {
                break;
            }
        }
    }
    // SAFETY: the snapshot handle, closed once.
    unsafe { CloseHandle(snapshot) };
    found
}

/// Every dynamic library this process has mapped, by path.
///
/// The dynamic linker's own table, rather than a mark of our own: a milestone
/// says what our code believes it did, and the question here is what the process
/// actually is. Same reason `print.rs` reads its output back with a parser that
/// did not write it.
#[cfg(target_os = "macos")]
fn mapped_images() -> Vec<String> {
    // Declared here rather than taken from `libc`, which deprecates both in
    // favour of the `mach2` crate --- a dependency this repository would be
    // adding for two symbols, against a licensing rule that makes every new
    // crate a decision. The signatures are dyld's own and have not changed.
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(index: u32) -> *const std::os::raw::c_char;
    }

    // SAFETY: the count bounds the index, and every name dyld returns is a live
    // NUL-terminated string for as long as the image stays loaded --- nothing
    // here unloads one.
    unsafe {
        (0.._dyld_image_count())
            .filter_map(|i| {
                let name = _dyld_get_image_name(i);
                (!name.is_null()).then(|| {
                    std::ffi::CStr::from_ptr(name)
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .collect()
    }
}

/// Whether the Pdfium library is mapped into this process at all.
fn pdfium_is_mapped() -> bool {
    mapped_images()
        .iter()
        .any(|image| image.to_lowercase().contains("pdfium"))
}

/// How many images are mapped, as the evidence that the scan read something.
fn loaded_images() -> usize {
    mapped_images().len()
}

/// The milestones recorded so far, as evidence for whichever check asks.
fn marks() -> String {
    startup::timeline()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Page sizes compared exactly: these are the same arithmetic on both sides, so
/// a tolerance would only hide a mapping that had genuinely changed.
fn same_size(a: &PageSize, b: &PageSize) -> bool {
    a.width_pt == b.width_pt && a.height_pt == b.height_pt
}

fn same_matches(a: &Result<PageMatches, String>, b: &Result<PageMatches, String>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => a.page == b.page && a.chars == b.chars && a.matches == b.matches,
        _ => false,
    }
}

fn describe_matches(result: &Result<PageMatches, String>) -> String {
    match result {
        Ok(m) => format!("{} hits over {} characters", m.matches.len(), m.chars),
        Err(e) => e.clone(),
    }
}

/// Outlines compared through their serialisation.
///
/// The tree is deep and its equality is structural, so comparing the JSON is
/// both shorter and stricter than a hand-written walk --- and it compares the
/// exact bytes the frontend would receive, which is what the claim is about.
fn same_outline(a: &Result<Outline, String>, b: &Result<Outline, String>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => {
            a.total == b.total
                && a.limits == b.limits
                && serde_json::to_string(&a.items).ok() == serde_json::to_string(&b.items).ok()
        }
        _ => false,
    }
}

/// Whether both backends produced the same comments.
///
/// Compared through serde like the outline above, and for the same reason: it
/// is the exact bytes the frontend receives. `scan_ms` is deliberately left out
/// of the comparison --- it is a duration, and two runs of the same scan are
/// never equal.
fn same_comments(a: &Result<Comments, String>, b: &Result<Comments, String>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => {
            a.limits == b.limits
                && serde_json::to_string(&a.items).ok() == serde_json::to_string(&b.items).ok()
        }
        _ => false,
    }
}

fn describe_comments(result: &Result<Comments, String>) -> String {
    match result {
        Ok(c) => format!("{} comments, limits {:?}", c.items.len(), c.limits),
        Err(e) => e.clone(),
    }
}

fn describe_outline(result: &Result<Outline, String>) -> String {
    match result {
        Ok(o) => format!("{} entries, limits {:?}", o.total, o.limits),
        Err(e) => e.clone(),
    }
}

/// The pixels of a rendered tile, or why there are none.
fn bytes_of(result: &Result<TileOutcome, String>) -> Result<Vec<u8>, String> {
    match result {
        Ok(TileOutcome::Rendered(tile)) => Ok(tile.bytes.clone()),
        Ok(TileOutcome::Abandoned) => Err("abandoned, and nothing withdrew it".into()),
        Err(e) => Err(e.clone()),
    }
}

fn outcome_of(result: &Result<TileOutcome, String>) -> String {
    match result {
        Ok(TileOutcome::Abandoned) => "abandoned".into(),
        Ok(TileOutcome::Rendered(tile)) => format!("rendered {} bytes", tile.bytes.len()),
        Err(e) => e.clone(),
    }
}

/// How many bytes differ, for a failure that has to say how badly.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// How many distinct values a tile holds, as evidence of content.
///
/// Over the whole tile, not a prefix: the first kilobyte of a text page is the
/// white margin, so a prefix reads `1` on a perfectly good render and makes the
/// control look like it is failing.
fn distinct_values(bytes: &[u8]) -> usize {
    let mut seen = [false; 256];
    for b in bytes {
        seen[*b as usize] = true;
    }
    seen.iter().filter(|s| **s).count()
}

/// Where Pdfium lives, matching the app's own resolution in development.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Prints each result as it is recorded and exits non-zero on any failure.
///
/// Printed immediately rather than buffered: `AGENTS.md` records an afternoon
/// spent on a harness that printed only at the end, where a run that stopped
/// midway was indistinguishable from one that never started.
#[derive(Default)]
struct Report {
    checks: usize,
    failures: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        self.checks += 1;
        if !ok {
            self.failures += 1;
        }
        // The label is padded to a fixed width, not merely bracketed. `[OK]` is
        // four characters and `[FAIL]`/`[SKIP]` six, so interpolating the word
        // put the detail column two to the left on exactly the rows that pass ---
        // and every documented `cut -c8-47` recipe for reading a name set then
        // sliced those rows two characters off, which reads as a *different
        // name* rather than as a misalignment. That produced a false "the name
        // sets diverge" here on 2026-07-31, which is the one conclusion this
        // whole arrangement exists to make trustworthy. Seven matches every
        // other harness in the repository.
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:56} {}", detail.as_ref());
    }

    /// Records a check that could not apply, with the reason.
    ///
    /// Counted and named rather than omitted: a control that silently
    /// disappears on some inputs is indistinguishable from one that ran.
    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.checks += 1;
        self.skipped += 1;
        println!("{:7}{name:56} {}", "[SKIP]", why.as_ref());
    }

    fn finish(&self) -> ! {
        println!(
            "\n{}/{} checks passed, {} skipped",
            self.checks - self.failures - self.skipped,
            self.checks,
            self.skipped
        );
        std::process::exit(i32::from(self.failures > 0));
    }
}
