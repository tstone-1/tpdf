/**
 * Search: the results list, text with no mapping, the search options, and stepping through matches.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import type { DocumentInfo } from "../ipc";
import { MAX_RESULT_ROWS } from "../results";
import { PLAIN_SEARCH } from "../search";
import { Sidebar } from "../sidebar";
import { Viewer, type ViewerStatus } from "../viewer";
import {
  check,
  eventually,
  key,
  settle,
  skip,
} from "./common";
import type { ReadingManifest } from "./reading";
import { pickNeedle, preview } from "./invert";

/**
 * Find in document.
 *
 * The load-bearing assertion, and the counterpart to the ordering check above,
 * is that the characters a match *covers* are the characters that were searched
 * for. Everything else about search --- that it finds something, that it counts
 * hits, that Cmd-G moves --- passes just as well when the returned indices are
 * off by one, or in a different index space from the boxes, and an off-by-one
 * highlight is exactly the bug this whole layer was designed to make impossible.
 *
 * The needle is taken from the document rather than hardcoded, so this runs
 * against any fixture; the negative control is that needle with letters glued to
 * the front, which cannot be in a document that the real one was drawn from.
 */
export async function searchChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  // Widened for `searchOptionChecks`, which needs the delivery counter and not
  // only the mirrored value. Every other consumer of `seen` takes the narrower
  // shape and still accepts this object.
  seen: { status: ViewerStatus | null; updates: number },
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle);

  const first = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const needle = first ? pickNeedle(first.codes) : null;

  // A document with no extractable text still has something to check, and it is
  // the one thing that matters for it: that the viewer says so instead of
  // reporting no matches for a query it never tested against anything.
  const probe = needle ?? "the";
  viewer.search(probe);
  await settle(() => (seen.status?.search.scanned ?? 0) >= doc.page_count);
  check(
    "tells no text apart from no matches",
    seen.status?.search.textless === (first?.codes.length === 0),
    `document reports textless=${seen.status?.search.textless}, page 1 has ` +
      `${first?.codes.length ?? "unknown"} characters`,
  );

  if (!needle) {
    // The reason has to distinguish the two ways this happens, because they call
    // for opposite responses. An empty page is a fixture that cannot exercise
    // search; a page full of text that `pickNeedle` could not read a word out of
    // is a **harness** that cannot exercise search, and printing the first when
    // the second is true is how seventeen checks stayed silently unexercised on a
    // Japanese document while claiming the page had no text.
    const why =
      (first?.codes.length ?? 0) === 0
        ? "page 1 has no extractable text"
        : `no word could be read out of page 1's ${first?.codes.length} characters`;
    skip("finds a word taken from the document", why);
    skip("a match covers the characters searched for", why);
    skip("case is ignored", why);
    skip("a word that is not there is not found", why);
    skip("searches forward from the page being read", why);
    skipSearchOptions(why);
    skip("finds something from the end of the document", why);
    skip("counts more than the matches on one page", why);
    skip("Cmd-G moves to a match on another page", why);
    return;
  }

  viewer.search(needle);
  const found = await eventually(
    "finds a word taken from the document",
    () => viewer.searchMatches.length > 0,
    () => `"${needle}" -> ${viewer.searchMatches.length} matches so far`,
  );

  const firstHit = viewer.searchMatches[0];
  if (found && firstHit) {
    // The assertion that ties an index range to specific content. The text is
    // re-extracted here rather than read out of the viewer's cache, so a match
    // cannot be confirmed by the viewer agreeing with itself.
    const page = await invoke<{ codes: number[] }>("page_text", {
      doc: doc.id,
      page: firstHit.page,
    }).catch(() => null);
    const covered = page
      ? String.fromCodePoint(...page.codes.slice(firstHit.start, firstHit.end))
      : "";
    check(
      "a match covers the characters searched for",
      covered.toLowerCase() === needle.toLowerCase(),
      `page ${firstHit.page} [${firstHit.start}, ${firstHit.end}) is "${preview(covered)}", ` +
        `searched for "${needle}"`,
    );
  } else {
    skip(
      "a match covers the characters searched for",
      "nothing was found to check",
    );
  }

  // Same first match, not merely some match: both scans start from the same
  // page and walk the same way, so anything other than the identical hit means
  // the two queries did not match the same text.
  viewer.search(needle.toUpperCase());
  await eventually(
    "case is ignored",
    () => {
      const upper = viewer.searchMatches[0];
      return (
        !!upper &&
        !!firstHit &&
        upper.page === firstHit.page &&
        upper.start === firstHit.start &&
        upper.end === firstHit.end
      );
    },
    () => {
      const upper = viewer.searchMatches[0];
      return (
        `"${needle.toUpperCase()}" first hit ${upper ? `${upper.page}:${upper.start}` : "none"}, ` +
        `"${needle}" first hit ${firstHit ? `${firstHit.page}:${firstHit.start}` : "none"}`
      );
    },
  );

  // The negative control. Without it, every check above is satisfied by a search
  // that returns matches for anything at all.
  const absent = `qxzj${needle}`;
  viewer.search(absent);
  // Waiting for `scanned` to reach the page count rather than for the scan to
  // stop running. They are the same thing only while cancellation works: a
  // mutation that removed the generation guard let an *older* scan finish and
  // clear the running flag, and this check then read its result at a moment when
  // neither scan had put anything in the list. It passed, for a reason that had
  // nothing to do with the query being absent.
  await settle(() => (seen.status?.search.scanned ?? 0) >= doc.page_count);
  check(
    "a word that is not there is not found",
    viewer.searchMatches.length === 0,
    `"${absent}" -> ${viewer.searchMatches.length} matches over ` +
      `${seen.status?.search.scanned ?? 0} pages in ` +
      `${viewer.searchElapsedMs.toFixed(0)} ms`,
  );

  await searchOptionChecks(viewer, seen, needle, firstHit, doc.page_count);
  await searchesFromHere(root, viewer, seen, needle, doc.page_count);
  await stepToAnotherPage(root, viewer, seen, needle, doc.page_count);
}

/** What {@link resultsChecks} records, for its own skip path. */
const RESULTS_CHECKS = [
  "the results tab lists a row per hit",
  "a row shows the words around its hit, with the hit emboldened",
  "picking a row moves the document to that hit",
  "the results list is replaced when the query changes",
] as const;

/**
 * The search-results sidebar tab, against a real DOM.
 *
 * The unit tests cover the state machine --- appending rather than rebuilding,
 * the row cap, the status line --- against `testdom.ts`, which has no text
 * layout and no real elements. What only a running webview can answer is whether
 * a row actually *says* what the match found and whether pressing one moves the
 * document, so those are here and nothing else is.
 *
 * The snippet check is the load-bearing one, and it is the same shape as the
 * search check above it: a row is tied to *specific content* by comparing what
 * it displays against the page's own text, re-extracted independently. A check
 * that a row is non-empty passes for a row describing the wrong hit.
 */
export async function resultsChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const first = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const needle = first ? pickNeedle(first.codes) : null;
  if (!needle) {
    // Same distinction as in `searchesFromHere`: "no text" and "no word this
    // harness could read" are different facts and only one of them is about the
    // fixture.
    const why =
      (first?.codes.length ?? 0) === 0
        ? "page 1 has no extractable text"
        : `no word could be read out of page 1's ${first?.codes.length} characters`;
    for (const name of RESULTS_CHECKS) skip(name, why);
    return;
  }

  sidebar.selectTab("results");
  const results = sidebar.results;
  const feed = (): void =>
    results.update(
      viewer.searchMatches,
      viewer.matchIndex,
      seen.status?.search.query ?? "",
      viewer.searching,
    );

  viewer.search(needle);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );
  feed();

  const total = viewer.searchMatches.length;
  check(
    RESULTS_CHECKS[0],
    results.rowCount === Math.min(total, MAX_RESULT_ROWS) &&
      results.rowCount > 0,
    `${results.rowCount} rows for ${total} matches --- "${results.status}"`,
  );

  const hit = viewer.searchMatches[0];
  const page = hit
    ? await invoke<{ codes: number[] }>("page_text", {
        doc: doc.id,
        page: hit.page,
      }).catch(() => null)
    : null;
  if (hit && page) {
    // What the row displays, read back out of the DOM, against what the page
    // says at the indices the match reported. Both halves matter: the bold run
    // has to be the hit, and the row has to be about the right place.
    const row = results.rowText(0);
    const onPage = String.fromCodePoint(
      ...page.codes.slice(hit.start, hit.end),
    );
    check(
      RESULTS_CHECKS[1],
      row.bold === onPage &&
        row.page === String(hit.page + 1) &&
        row.whole.includes(onPage),
      `row 1 reads "${preview(row.whole)}" with "${row.bold}" bold on page ${row.page}; ` +
        `the document has "${preview(onPage)}" at ${hit.page}:${hit.start}`,
    );
  } else {
    skip(RESULTS_CHECKS[1], "nothing was found to compare against the page");
  }

  // A row other than the one the scan already jumped to, and on a different
  // page, so the assertion is that the document *moved* rather than that it was
  // already there. `viewer.search` auto-shows the first hit, so without the
  // first condition this picks the row the viewer is on and asserts nothing.
  const shown = viewer.matchIndex;
  const shownPage = viewer.searchMatches[shown]?.page ?? -1;
  const away = viewer.searchMatches.findIndex(
    (m, i) => i !== shown && i < MAX_RESULT_ROWS && m.page !== shownPage,
  );
  if (away < 0) {
    skip(
      RESULTS_CHECKS[2],
      `all ${total} hits are on page ${shownPage + 1}, which is the one already shown, ` +
        "so a jump would not be observable",
    );
  } else {
    const target = viewer.searchMatches[away];
    const from = viewer.offset;
    // Through the row's own listener rather than by calling `onPick`, so what is
    // tested is the wiring a reader's finger goes through.
    results
      .rowAt(away)
      ?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await settle(() => viewer.matchIndex === away && viewer.offset !== from);
    // **Not** `position.page`, which is the page at the *top edge*: `goToMatch`
    // deliberately leaves a third of a screen above the hit, so a match near the
    // top of page 2 is shown with page 1 still at the top edge and the top-edge
    // page never changes. It is also 0 on a rotated document. The scroll offset
    // is the quantity that actually says the document moved.
    check(
      RESULTS_CHECKS[2],
      viewer.matchIndex === away && viewer.offset !== from,
      `row ${away + 1} is on page ${(target?.page ?? -1) + 1}; current match went ` +
        `${shown} -> ${viewer.matchIndex}, offset ${from.toFixed(0)} -> ` +
        `${viewer.offset.toFixed(0)}`,
    );
  }

  const absent = `qxzj${needle}`;
  viewer.search(absent);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );
  feed();
  check(
    RESULTS_CHECKS[3],
    results.rowCount === 0,
    `"${absent}" -> ${results.rowCount} rows, "${results.status}"`,
  );

  viewer.clearSearch();
  feed();
  sidebar.selectTab("outline");
}

/** The page name a generator gives a page whose fonts state no mapping. */
export const UNMAPPED_PAGE = "no-mapping";
/** The manifest query that is on such a page and cannot be found. */
const UNMAPPED_QUERY = "unmapped-text-is-not-found";

/** The two checks {@link mappingChecks} records. Both run on every fixture. */
const MAPPING_CHECKS = [
  "the pages whose text is a guess reach the frontend",
  "a reader is told when a page could not be searched",
] as const;

/**
 * The third state: a page that has text, in the right places, meaning nothing.
 *
 * `encoding.rs` decides it, and until this ran the whole path from that decision
 * to the line a reader reads had never executed in a window on any platform ---
 * six hops, each typechecked and unit-tested, which is exactly the arrangement
 * that passes while the wiring between two of them is missing.
 *
 * **The expectation comes from the fixture's generator, not from the subject.**
 * `encodings.pdf` names its first page `no-mapping`, and that name is written by
 * a program that has never heard of `encoding.rs`. Waiting for
 * `unsearchablePages` to go positive instead --- the obvious shape --- would make
 * a backend that always answered zero pass on all ten corpora, including the one
 * built so that it must not.
 *
 * **Both checks run everywhere**, and the nine fixtures with nothing to report
 * are the control: the same query, the same panel, and the line must be absent.
 * A one-sided check here would be satisfied by a frontend that said it about
 * every document.
 *
 * The wait is on `mappingKnown` rather than on the count, because on those nine
 * the count starts at the value being asserted --- see the getter's own note.
 */
export async function mappingChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const manifest = await manifestOf();
  const unreadable = (manifest?.pages ?? []).filter(
    (page) => page.name === UNMAPPED_PAGE,
  ).length;
  // A query the generator says is on the page and cannot be found, so what a
  // reader sees is the whole defect: a word plainly visible, "No matches.", and
  // now the sentence saying why. Elsewhere any absent query does, since what is
  // being asserted there is that the sentence stays away.
  const query =
    manifest?.queries?.find((entry) => entry.name === UNMAPPED_QUERY)?.query ??
    "qxzjabsentfromeverything";

  sidebar.selectTab("results");
  const results = sidebar.results;

  viewer.search(query);
  await settle(
    () =>
      !viewer.searching &&
      (seen.status?.search.scanned ?? 0) >= doc.page_count &&
      (seen.status?.search.mappingKnown ?? false),
  );

  const known = seen.status?.search.mappingKnown ?? false;
  const reported = seen.status?.search.unsearchablePages ?? 0;
  check(
    MAPPING_CHECKS[0],
    known && reported === unreadable,
    known
      ? `"${query}" -> ${viewer.searchMatches.length} matches; the backend reports ` +
          `${reported} unsearchable page(s) and the generator wrote ${unreadable}`
      : "the backend never answered which pages store unreadable text",
  );

  // Fed from the status the viewer emitted, which is the object `App.svelte`
  // reads at its own call site -- not from a number computed here, which would
  // be a second implementation agreeing with the first.
  results.update(
    viewer.searchMatches,
    viewer.matchIndex,
    seen.status?.search.query ?? "",
    viewer.searching,
    reported,
  );

  const said = results.status;
  const names = said.includes("could not be searched");
  const counts =
    unreadable === 0 ||
    said.includes(unreadable === 1 ? "1 page" : `${unreadable} pages`);
  check(
    MAPPING_CHECKS[1],
    names === unreadable > 0 && counts,
    `${unreadable} page(s) unreadable, the panel says "${said}"`,
  );

  viewer.clearSearch();
  results.update(viewer.searchMatches, viewer.matchIndex, "", false, 0);
  sidebar.selectTab("outline");
}

/**
 * The fixture's manifest, or null when it has none.
 *
 * {@link readingChecks} parses it separately and on purpose: a manifest that
 * does not parse is a broken fixture and it fails there, loudly and once,
 * rather than in every phase that reads one.
 */
export async function manifestOf(): Promise<ReadingManifest | null> {
  const raw = await invoke<string | null>("reading_manifest").catch(() => null);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as ReadingManifest;
  } catch {
    return null;
  }
}

/**
 * What this fixture's generator says is in it, or `null` if it says nothing.
 *
 * The entry for the open document, already selected by `corpus_manifest` --- one
 * generator writes several fixtures, and the process that knows which is open is
 * the backend. `null` covers three different absences (no sidecar, no entry for
 * this fixture, nothing that parses) and every one of them has to read as
 * "nothing to compare against" rather than as a pass.
 */
export async function corpusEntry(): Promise<Record<string, unknown> | null> {
  const raw = await invoke<string | null>("corpus_manifest").catch(() => null);
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as unknown;
    return typeof value === "object" && value !== null
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

/** The three checks {@link searchOptionChecks} records, for the skip path. */
const SEARCH_OPTION_CHECKS = [
  "matching case rejects the hit that ignoring it accepted",
  "whole words rejects a hit inside a longer word",
  "turning the options off finds the hit again",
  "a pattern finds what the same text as a literal does not",
  "a pattern that does not compile says so instead of finding nothing",
] as const;

function skipSearchOptions(why: string): void {
  for (const name of SEARCH_OPTION_CHECKS) skip(name, why);
}

/**
 * That the two matching options reach the matcher and change what it accepts.
 *
 * Each is asserted against **one known occurrence** --- the first hit of the
 * plain search, whose position and spelling are both already established ---
 * rather than against a count. A count is the tempting assertion and it cannot
 * fail honestly: whole-word narrows a search on most documents and widens it on
 * none, so "fewer than before" passes for a fixture that simply has fewer of
 * something, and it passes for an option that was dropped on the way to the
 * worker whenever the document happens to agree.
 *
 * The waits are on `searching` **and** `scanned`, where the checks above wait on
 * `scanned` alone. `Search.run` sets `running` synchronously before its first
 * await, so a wait that includes it cannot be satisfied by the *previous* scan's
 * finished state --- which is a race the `scanned`-only form has on entry. The
 * `scanned` half is kept because it is what notices a missing generation guard,
 * for the reason the negative control above spells out.
 */
async function searchOptionChecks(
  viewer: Viewer,
  seen: { status: ViewerStatus | null; updates: number },
  needle: string,
  firstHit: { page: number; start: number } | undefined,
  pageCount: number,
): Promise<void> {
  const done = (): boolean =>
    !viewer.searching && (seen.status?.search.scanned ?? 0) >= pageCount;
  const hitAtFirst = (): boolean =>
    !!firstHit &&
    viewer.searchMatches.some(
      (m) => m.page === firstHit.page && m.start === firstHit.start,
    );

  if (!firstHit) {
    skipSearchOptions("the plain search found nothing to reason about");
    return;
  }

  const shouted = needle.toUpperCase();
  if (shouted === needle) {
    skip(
      SEARCH_OPTION_CHECKS[0],
      `"${needle}" is already upper case, so there is no spelling of it that ` +
        "matching case would reject",
    );
  } else {
    // `case is ignored` has just established that this exact query finds this
    // exact occurrence. Turning the option on must stop it, because the text
    // there is spelled the other way.
    viewer.setSearchOptions({ ...PLAIN_SEARCH, matchCase: true });
    viewer.search(shouted);
    await settle(done);
    check(
      SEARCH_OPTION_CHECKS[0],
      !hitAtFirst(),
      `"${shouted}" with match-case -> ${viewer.searchMatches.length} matches, none at ` +
        `${firstHit.page}:${firstHit.start} where "${needle}" is`,
    );
  }

  // A proper prefix of the needle occurs at the needle's own position and is
  // followed there by a letter, so it is never a whole word there. `pickNeedle`
  // returns five letters or more, so the prefix is at least four.
  const stem = needle.slice(0, -1);
  viewer.setSearchOptions({ ...PLAIN_SEARCH, wholeWord: true });
  viewer.search(stem);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[1],
    !hitAtFirst(),
    `"${stem}" with whole-word -> ${viewer.searchMatches.length} matches, none at ` +
      `${firstHit.page}:${firstHit.start} where it is followed by ` +
      `"${needle.slice(-1)}"`,
  );

  // The control, and the one that says the two above rejected something rather
  // than the search having stopped working. Same query, options off, hit back.
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.search(stem);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[2],
    hitAtFirst(),
    `"${stem}" unrestricted -> ${viewer.searchMatches.length} matches, including ` +
      `${firstHit.page}:${firstHit.start}`,
  );

  // A pattern built from the needle by replacing its second character with a
  // dot. As a pattern it must find the needle where the needle is; as a literal
  // the same string contains a dot the page does not have there, so it finds
  // nothing --- which is the control that says the option is what did it rather
  // than the query happening to match twice.
  const pattern = `${needle[0] ?? ""}.${needle.slice(2)}`;
  viewer.setSearchOptions({ ...PLAIN_SEARCH, regex: true });
  viewer.search(pattern);
  await settle(done);
  const asPattern = viewer.searchMatches.length;
  const foundHere = hitAtFirst();
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.search(pattern);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[3],
    foundHere && asPattern > 0 && viewer.searchMatches.length === 0,
    `"${pattern}" as a pattern -> ${asPattern} matches including ` +
      `${firstHit.page}:${firstHit.start}; as a literal -> ` +
      `${viewer.searchMatches.length}`,
  );

  // An unclosed group. The reader has to be told the pattern is broken, because
  // "no matches" for it is a statement about the document instead.
  //
  // Both reads below wait for a status DELIVERED AFTER THE SCAN STOPPED, and
  // the emphasis is the whole of a fix made on 2026-08-12. `seen` is a mirror
  // the viewer fills through `onStatus`; `viewer.searching` is a live read of
  // the searcher. Those are different clocks.
  //
  // The wait used to be `!viewer.searching && seen.updates > beforeBroken`, and
  // it flaked --- twice in about seven runs on the day it was fixed, against a
  // comment here that recorded once in three.
  //
  // `Search.run` emits a status at the *start* of a scan, on purpose, because a
  // search over 775 pages has to be visibly working. But `onSearchProgress`
  // only calls `wake()`, so delivery waits for the next frame --- and a broken
  // pattern is rejected so fast that the start and the completion normally both
  // land before that frame. The mirror then sees one status carrying the final
  // state, and the check passes. That is the usual run, and it is why this
  // looked fine for weeks.
  //
  // Occasionally a frame lands *between* them. Now the first status delivered
  // after `beforeBroken` is the start one: `running: true`, `problem: ""`. The
  // counter half of the old wait was satisfied by exactly that status --- the
  // event it existed to exclude --- and its other half, `!viewer.searching`, is
  // a live read of the searcher that went false the moment the invoke resolved.
  // Both true, mirror holding the start state, `problem` read as empty.
  //
  // Reading `running` OUT OF THE MIRROR fixes it by construction rather than by
  // timing: whichever of the two statuses is delivered, only one taken after
  // the scan stopped satisfies this. The counter stays and is still doing work
  // --- it excludes the idle status from *before* the search, which also reads
  // `running: false`.
  //
  // Waiting on `problem` itself would be the obvious fix and the wrong one: it
  // is the value being asserted, so the check could then only pass or hang.
  //
  // A control asserting "the start status is always delivered first" was
  // written before this and is deliberately **not** here: it went red on the
  // first run, which is what corrected the account above. The start status is
  // usually coalesced away, so that control asserted the race rather than the
  // behaviour --- a check whose truth depends on the timing it is meant to
  // remove. There is no deterministic control for this from inside the harness;
  // what stands behind the fix is the construction argument plus repetition.
  const settledScan = (since: number) =>
    seen.updates > since && seen.status?.search.running === false;

  const broken = `${needle}(`;
  viewer.setSearchOptions({ ...PLAIN_SEARCH, regex: true });
  const beforeBroken = seen.updates;
  viewer.search(broken);
  await settle(() => settledScan(beforeBroken));
  const problem = seen.status?.search.problem ?? "";
  // The control: the same characters as a literal are a perfectly ordinary
  // query, so a `problem` there would mean the reporting is about the text
  // rather than about the pattern. It needs the same wait for the opposite
  // reason --- a stale mirror here still holds the problem set just above, so
  // reading too early fails the control rather than passing it.
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  const beforeLiteral = seen.updates;
  viewer.search(broken);
  // `done()` reads the live `viewer.searching` and the mirror's `scanned`, so
  // it carries the same defect as the wait above did; `settledScan` is what
  // makes the delivered status the post-scan one. Both, because this one also
  // needs the whole document scanned rather than merely stopped.
  await settle(() => settledScan(beforeLiteral) && done());
  check(
    SEARCH_OPTION_CHECKS[4],
    problem !== "" && (seen.status?.search.problem ?? "") === "",
    `"${broken}" as a pattern -> "${preview(problem)}"; as a literal -> ` +
      `"${preview(seen.status?.search.problem ?? "")}", ` +
      `${viewer.searchMatches.length} matches`,
  );
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.clearSearch();
}

/**
 * The scan starts at the page being read, not at page 1.
 *
 * A reader on page 700 who searches for a common word wants the next one, not a
 * jump to the beginning of the document. Nothing else here would notice if that
 * were wrong: every other search check runs from `Home`, where "starts at the
 * current page" and "starts at page 1" are the same thing.
 */
async function searchesFromHere(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
  needle: string,
  pageCount: number,
): Promise<void> {
  const name = "searches forward from the page being read";
  // Both names, on every path out. This function records *two* checks, and its
  // early returns skipped only the first --- so on a one-page document the
  // second did not fail, did not skip, and did not appear at all. The name set
  // is the invariant here (86 across every corpus but one, which is how it was
  // found); a check that evaporates is invisible in a way a red one is not.
  const scanned = "finds something from the end of the document";
  if (pageCount < 2) {
    skip(name, "the document has one page");
    skip(scanned, "the document has one page");
    return;
  }

  key(root, "End");
  await settle(() => viewer.idle);
  const from = (seen.status?.page ?? 1) - 1;
  if (from === 0) {
    skip(name, "the whole document fits on one screen");
    skip(scanned, "the whole document fits on one screen");
    return;
  }

  viewer.search(needle);
  const found = await eventually(
    scanned,
    () => viewer.searchMatches.length > 0,
    () => `${viewer.searchMatches.length} matches so far`,
  );
  const first = viewer.searchMatches[0];
  // A needle with nothing at or after the page being read cannot distinguish the
  // two answers: the scan is *supposed* to wrap when there is nothing ahead, so
  // a first hit before `from` is correct there and a defect anywhere else. The
  // check reported that as a failure on the first two-page fixture it met ---
  // the needle comes from page 1 and this document does not repeat it --- which
  // is a check that cannot tell its own subject from its precondition.
  if (found && !viewer.searchMatches.some((m) => m.page >= from)) {
    skip(
      name,
      `no match at or after page ${from + 1}, so wrapping to the start is correct`,
    );
    return;
  }
  check(
    name,
    found && !!first && first.page >= from,
    `reading page ${from + 1}, first hit on page ${first ? first.page + 1 : "none"}` +
      (first && first.page < from
        ? " -- the scan restarted at the beginning"
        : ""),
  );
}

/**
 * Steps through matches with Cmd-G until one is on a different page.
 *
 * The check is that the viewport followed, and it needs the control beside it:
 * "the viewport is on the match's page" is trivially true if the match was on
 * the page already, which is how a jump check on a 775-page document once passed
 * without the jump having rendered anything. So the target is the first match
 * on a page other than the one being read, the precondition that we are *not*
 * there is asserted first, and a document that cannot offer one is skipped with
 * its reason printed rather than silently dropped.
 */
async function stepToAnotherPage(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
  needle: string,
  pageCount: number,
): Promise<void> {
  const name = "Cmd-G moves to a match on another page";
  const spread = "counts more than the matches on one page";
  if (pageCount < 2) {
    skip(spread, "the document has one page");
    skip(name, "the document has one page");
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle);
  viewer.search(needle);
  // Waited out to the *end of the search*, not until matches appear on two
  // pages. Written the second way first, and it is a wait for a condition that
  // may never hold: on a document whose needle sits on one page it spent its
  // whole bound and then reported a failure, when what it had established is
  // that this fixture cannot exercise the check below. That is a precondition
  // rather than a claim about searching, so it says so.
  await settle(() => !viewer.searching);
  const pages = new Set(viewer.searchMatches.map((m) => m.page));
  const detail = `${viewer.searchMatches.length} matches across ${pages.size} pages`;
  if (pages.size < 2) {
    skip(spread, detail);
    skip(name, detail);
    return;
  }
  check(spread, true, detail);

  const start = viewer.searchMatches[0]?.page ?? 0;
  const target = viewer.searchMatches.findIndex((m) => m.page !== start);
  if (target < 0) {
    skip(name, `every match is on page ${start + 1}`);
    return;
  }

  const goal = viewer.searchMatches[target];
  const before = (seen.status?.page ?? 0) - 1;
  if (!goal || before === goal.page) {
    skip(name, `already on page ${goal ? goal.page + 1 : "?"} before stepping`);
    return;
  }

  // One press per step, each awaited: `goToMatch` has to load the target page's
  // text before it knows where to scroll, so pressing faster than that races the
  // reply it is waiting for.
  for (let step = 0; step < target; step++) {
    const at = seen.status?.search.index ?? 0;
    key(root, "g", true);
    await settle(() => (seen.status?.search.index ?? 0) !== at);
  }
  await settle(() => seen.status?.page === goal.page + 1);

  check(
    name,
    seen.status?.page === goal.page + 1,
    `${target} presses from page ${before + 1} to page ${seen.status?.page}, ` +
      `match ${target + 1} is on page ${goal.page + 1}`,
  );
}

/** The check that a guessed page is not read aloud, named once for its skips. */
export const GUESSED_TEXT_CHECK =
  "a page whose characters mean nothing is not read out";
