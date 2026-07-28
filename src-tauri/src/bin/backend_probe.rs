//! Proves that moving every document behind a process boundary changed nothing
//! the reader can see --- and that it really moved.
//!
//! `bin/worker_probe.rs` compares a worker against an in-process render at the
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
//! cargo run --release --bin backend-probe -- testdata/text-heavy.pdf
//! ```

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use tpdf_lib::outline::Outline;
use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, Tile, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::search::PageMatches;
use tpdf_lib::startup;
use tpdf_lib::{worker, worker_child};

/// Tiles are compared at this size: inside the useful range `AGENTS.md`
/// measured, and small enough that a fixture renders quickly.
const TILE: u16 = 512;

/// A render at least this slow can have a withdrawal delivered into it.
///
/// Derived from the *first* tile's measured time, which no defect in the
/// withdrawal path can influence --- a skip condition read off the thing under
/// test is how a broken mechanism reports `[SKIP]` instead of `[FAIL]`.
const WITHDRAWABLE_MS: f64 = 120.0;

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let Some(document) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: backend-probe <file.pdf>");
        std::process::exit(2);
    };
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
    let workers = RenderService::start_with(library_dir.clone(), Backend::Worker);
    let worker_doc = match wait(|reply| workers.open(document.clone(), false, reply)) {
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
    let opened_with = worker_pids().len();

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
    let native_doc = match wait(|reply| in_process.open(document.clone(), false, reply)) {
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
    let worker_text = wait(|reply| workers.text(worker_doc.id, page, reply));
    let native_text = wait(|reply| in_process.text(native_doc.id, page, reply));
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
        let first = wait(|reply| in_process.text(native_doc.id, 0, reply));
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
    }

    // Searched for something a text page has and a vector one does not, so the
    // count below is evidence rather than a matching pair of zeroes.
    let query = "e".to_string();
    let worker_hits = wait(|reply| workers.search(worker_doc.id, page, query.clone(), reply));
    let native_hits = wait(|reply| in_process.search(native_doc.id, page, query.clone(), reply));
    report.check(
        "a search returns the same ranges on both",
        same_matches(&worker_hits, &native_hits),
        describe_matches(&native_hits),
    );

    let worker_outline = wait(|reply| workers.outline(worker_doc.id, reply));
    let native_outline = wait(|reply| in_process.outline(native_doc.id, reply));
    report.check(
        "an outline returns the same tree on both",
        same_outline(&worker_outline, &native_outline),
        describe_outline(&native_outline),
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
    let before = worker_pids();
    // The pool, as the concurrent tiles above will have grown it. Both bounds
    // matter and they fail differently: below two says nothing ever ran in
    // parallel, above capacity says the ceiling is not a ceiling. `opened_with`
    // is what says it did not simply start this large.
    report.check(
        "concurrent tiles grew the pool, and no further than its capacity",
        before.len() > 1 && before.len() <= workers.pool_size() && opened_with == 1,
        format!(
            "{} workers, capacity {}, opened with {opened_with}",
            before.len(),
            workers.pool_size()
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
                page: worker_doc.page_count as u32,
                ..request(worker_doc.id, 21, at)
            },
            reply,
        )
    });
    let unchanged = worker_pids();
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
    let after = worker_pids();
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
        let (tx, rx) = channel();
        for n in 0..wanted as u64 {
            let tx = tx.clone();
            workers.tile(
                request(worker_doc.id, 50 + n, at),
                Box::new(move |result| {
                    let _ = tx.send(result);
                }),
            );
        }
        drop(tx);
        // Bounded, and that is the whole design of this check. The failure it
        // exists for --- a pool that believes in a worker it retired --- is a
        // *wait*, not a wrong answer, so an unbounded collect could only stop,
        // never go red. A check that hangs instead of failing is one the harness
        // has to interpret rather than read.
        let bound = Duration::from_secs_f64((render_ms * 20.0 / 1e3).max(10.0));
        let started = Instant::now();
        let mut rendered = 0;
        while rendered < wanted {
            let left = bound.saturating_sub(started.elapsed());
            match rx.recv_timeout(left) {
                Ok(Ok(TileOutcome::Rendered(_))) => rendered += 1,
                _ => break,
            }
        }
        (worker_pids().len(), rendered)
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
    let second = worker_pids().first().copied();
    let death = kill_a_worker(second);
    let again = wait(|reply| workers.text(worker_doc.id, page, reply));
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
    let before_second = worker_pids();
    let second_doc = wait(|reply| workers.open(document.clone(), false, reply));
    let closing = match &second_doc {
        Ok(info) => {
            // Which pids belong to the document about to be closed: everything
            // that was there before the second document opened. Recorded rather
            // than inferred, because after the close there is no way to ask.
            let closed_pool = before_second.clone();
            let held = worker_pids();
            let closed = wait(|reply| workers.close(worker_doc.id, reply));
            let left = worker_pids();
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
            let quiet = open_descriptors();
            let throwaway = wait(|reply| workers.open(document.clone(), true, reply));
            let opened_fds = open_descriptors();
            let released = match &throwaway {
                Ok(info) => wait(|reply| workers.close(info.id, reply)),
                Err(e) => Err(e.clone()),
            };
            let settled = open_descriptors();
            report.check(
                "closing gives back every descriptor opening took",
                released.is_ok() && opened_fds > quiet && settled == quiet,
                format!("{quiet} quiet, {opened_fds} with it open, {settled} after closing it"),
            );

            // And the id itself is spent. A backend that filled the hole would
            // hand this id to a document the caller has never seen, while every
            // check above still passed.
            let reopened = wait(|reply| workers.open(document.clone(), true, reply));
            report.check(
                "a closed id is not handed out to the next document",
                reopened.as_ref().is_ok_and(|info| {
                    info.id != worker_doc.id && info.id != second_doc_id(&second_doc)
                }),
                match &reopened {
                    Ok(info) => format!("id {} after closing {}", info.id, worker_doc.id),
                    Err(e) => e.clone(),
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
        let busy = wait(|reply| workers.open(document.clone(), true, reply));
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
            Err(e) => report.check("a close waits for the render it interrupted", false, e),
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

    report.finish();
}

/// The id of a document that may not have opened.
///
/// `u32::MAX` for one that did not, which no real id reaches --- so a comparison
/// against it is false rather than accidentally true.
fn second_doc_id(info: &Result<DocumentInfo, String>) -> u32 {
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

/// How many descriptors this process currently holds open.
///
/// `/dev/fd` is the kernel's own answer, listing exactly what this process has.
/// A count rather than a set because the question is whether closing a document
/// gives back what opening it took, and the numbers themselves are reused.
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
fn worker_pids() -> Vec<u32> {
    let out = std::process::Command::new("pgrep")
        .arg("-P")
        .arg(std::process::id().to_string())
        .output();
    out.map(|out| {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    })
    .unwrap_or_default()
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
fn wait<T: Send + 'static>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, String>) + Send>),
) -> Result<T, String> {
    let (tx, rx): (_, Receiver<Result<T, String>>) = channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "no answer within {} s --- the service is wedged, not slow",
            ANSWER_BOUND.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("the render thread stopped".into())
        }
    }
}

/// Whether a startup milestone has been recorded in this process.
fn marked(name: &str) -> bool {
    startup::timeline().iter().any(|(mark, _)| mark == name)
}

/// Every dynamic library this process has mapped, by path.
///
/// The dynamic linker's own table, rather than a mark of our own: a milestone
/// says what our code believes it did, and the question here is what the process
/// actually is. Same reason `print.rs` reads its output back with a parser that
/// did not write it.
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
        .map(|root| root.join("vendor/pdfium/lib"))
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
        println!(
            "[{}] {name:56} {}",
            if ok { "OK" } else { "FAIL" },
            detail.as_ref()
        );
    }

    /// Records a check that could not apply, with the reason.
    ///
    /// Counted and named rather than omitted: a control that silently
    /// disappears on some inputs is indistinguishable from one that ran.
    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.checks += 1;
        self.skipped += 1;
        println!("[SKIP] {name:56} {}", why.as_ref());
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
