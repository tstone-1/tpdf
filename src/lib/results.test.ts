/**
 * The search-results panel.
 *
 * Two things here are worth testing and the rest is DOM. The **status line** is
 * the only pure decision in the module and has four cases a reader reads as four
 * different situations. And the **append rather than rebuild** rule is the one
 * piece of state: it is what makes a 775-page scan affordable, and it is the
 * thing that would silently interleave two queries if it got the "is this the
 * same search" test wrong.
 *
 * Every test below was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { beforeEach, describe, expect, it } from "vitest";

import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";
import { MAX_RESULT_ROWS, Results, statusFor } from "./results";
import type { Match } from "./search";

/** A hit on `page`, spelling its own snippet so a row can be identified. */
function match(page: number, hit: string, before = "the ", after = " and"): Match {
  return { page, start: 0, end: hit.length, before, hit, after };
}

describe("statusFor", () => {
  it("tells an empty query apart from a search that has found nothing", () => {
    // They are the same match list and opposite situations: one has not been
    // asked anything, the other has been asked and answered no.
    expect(statusFor(0, "", false)).toBe("Type in the find field to search.");
    expect(statusFor(0, "cat", false)).toBe("No matches.");
  });

  it("says a scan is still running, so a partial count is not read as final", () => {
    expect(statusFor(0, "cat", true)).toBe("Searching…");
    expect(statusFor(3, "cat", true)).toBe("3 matches, still searching…");
    expect(statusFor(3, "cat", false)).toBe("3 matches");
  });

  it("agrees with itself about one match", () => {
    expect(statusFor(1, "cat", false)).toBe("1 match");
  });

  it("does not claim a page was searched when its text could not be read", () => {
    // The defect this exists for. A CID font with no /ToUnicode makes PDFium
    // read glyph ids as character codes, so the page has text of the right
    // length that means nothing -- and "No matches." asserts the query was
    // tested and absent, which is false about a page nobody could search.
    const said = statusFor(0, "cat", false, 1);
    expect(said).not.toBe("No matches.");
    expect(said).toContain("1 page");
    expect(said).toContain("could not be searched");
  });

  it("says it even when there were hits, because a partial answer reads as a total one", () => {
    // The quieter half of the same defect: three matches from a document with
    // an unreadable page is not "3 matches", it is "3 matches out of what could
    // be looked at".
    const said = statusFor(3, "cat", false, 2);
    expect(said).toContain("3 matches");
    expect(said).toContain("2 pages");
  });

  it("agrees with itself about one unreadable page", () => {
    expect(statusFor(0, "cat", false, 1)).toContain("1 page ");
    expect(statusFor(0, "cat", false, 2)).toContain("2 pages ");
  });

  it("stays quiet until the scan has finished", () => {
    // While running, "Searching…" is true and complete, and interrupting a
    // reader with a caveat about a scan still in progress is a nag. The count
    // is also not final yet -- the pages carrying it may not have been reached.
    expect(statusFor(0, "cat", true, 3)).toBe("Searching…");
  });

  it("says nothing when no page is unreadable, which is almost every document", () => {
    // The control. A rule that fired on a normal document would be worse than
    // the defect it fixes: 36 fixtures and ~1,700 pages produce exactly one
    // page that should trip this, so the common path must stay untouched.
    expect(statusFor(0, "cat", false, 0)).toBe("No matches.");
    expect(statusFor(3, "cat", false, 0)).toBe("3 matches");
  });

  it("states the row cap rather than applying it silently", () => {
    // A list that stopped at the cap without saying so is a document that
    // appears to contain exactly that many hits.
    const over = statusFor(MAX_RESULT_ROWS + 1, "cat", false);
    expect(over).toContain(`showing the first ${MAX_RESULT_ROWS}`);
    expect(statusFor(MAX_RESULT_ROWS, "cat", false)).not.toContain("showing the first");
  });
});

describe("Results", () => {
  let dom: FakeDom;
  let host: FakeElement;
  let picked: number[];
  let panel: Results;

  beforeEach(() => {
    dom = installFakeDom();
    host = dom.root;
    picked = [];
    panel = new Results(host as unknown as HTMLElement, {
      onPick: (index) => picked.push(index),
    });
    return () => dom.restore();
  });

  /** The rows the panel has built, which are the list panel's own children. */
  function rows(): FakeElement[] {
    const list = host.children.find((child) => child.getAttribute("role") === "listbox");
    return list?.children ?? [];
  }

  it("builds one row per match, in order", () => {
    panel.update([match(0, "cat"), match(4, "cattle")], -1, "cat", false);
    expect(panel.rowCount).toBe(2);
    expect(rows().length).toBe(2);
    expect(rows().map((row) => row.dataset.index)).toEqual(["0", "1"]);
  });

  it("appends only what has arrived since the last paint", () => {
    // The point of the whole module: 775 pages report 775 times, and a panel
    // that rebuilt each time would build the same rows hundreds of times over.
    const found: Match[] = [match(0, "cat")];
    panel.update(found, -1, "cat", true);
    const first = rows()[0];
    found.push(match(1, "cat"));
    panel.update(found, -1, "cat", true);
    expect(panel.rowCount).toBe(2);
    // The same element, not a rebuilt copy of it: identity is what says the row
    // survived rather than that an equal one was built again.
    expect(rows()[0]).toBe(first);
  });

  it("rebuilds when the match list is replaced", () => {
    // A new query gets a new array. Appending to the old rows would show two
    // searches at once, and the second query's counts would be wrong by exactly
    // the first query's hits.
    panel.update([match(0, "cat"), match(1, "cat")], -1, "cat", false);
    const stale = rows()[0];
    panel.update([match(7, "dog")], -1, "dog", false);
    expect(panel.rowCount).toBe(1);
    expect(rows().length).toBe(1);
    expect(rows()[0]).not.toBe(stale);
  });

  it("stops building rows at the cap while the count stays exact", () => {
    const many = Array.from({ length: MAX_RESULT_ROWS + 5 }, (_, i) => match(i, "cat"));
    panel.update(many, -1, "cat", false);
    expect(panel.rowCount).toBe(MAX_RESULT_ROWS);
    expect(rows().length).toBe(MAX_RESULT_ROWS);
    expect(panel.status).toContain(`${MAX_RESULT_ROWS + 5} matches`);
  });

  it("moves the highlight to the current match and off the previous one", () => {
    const found = [match(0, "cat"), match(1, "cat"), match(2, "cat")];
    panel.update(found, 0, "cat", false);
    expect(panel.highlighted).toBe(0);
    panel.update(found, 2, "cat", false);
    expect(panel.highlighted).toBe(2);
    // Both directions, because a highlight that is only ever added leaves every
    // row it has visited looking current --- and the index getter cannot see
    // that, since it would still read 2.
    expect(rows()[0]?.getAttribute("aria-selected")).toBe("false");
    expect(rows()[2]?.getAttribute("aria-selected")).toBe("true");
  });

  it("numbers pages as a reader does, from one", () => {
    // The match is zero-based and the row is not. Both spellings look right in
    // isolation, and the difference only shows next to the page counter in the
    // toolbar, which is the last place anybody checks.
    panel.update([match(0, "cat"), match(41, "cat")], -1, "cat", false);
    expect(panel.rowText(0).page).toBe("1");
    expect(panel.rowText(1).page).toBe("42");
  });

  it("shows the words around the hit, with only the hit emboldened", () => {
    // A row that emboldened the whole snippet is unreadable at a glance, which
    // is the only thing a list of five thousand rows is good for.
    panel.update([match(3, "cat", "a ", " b")], -1, "cat", false);
    const row = panel.rowText(0);
    expect(row.whole).toBe("a cat b");
    expect(row.bold).toBe("cat");
  });

  it("reports the index of the row that was pressed", () => {
    panel.update([match(0, "cat"), match(1, "cat"), match(2, "cat")], -1, "cat", false);
    rows()[1]?.dispatch("pointerdown", {});
    expect(picked).toEqual([1]);
  });

  it("writes the status line only when it changes", () => {
    // 775 pages report 775 times and the line says the same thing for most of
    // them. Writing it every time is 775 live-region announcements, which for a
    // screen reader is the panel talking over the document.
    panel.update([match(0, "cat")], -1, "cat", true);
    const notice = host.children.find((child) => child.getAttribute("role") === "status");
    expect(notice?.textContent).toBe("1 match, still searching…");
    if (notice) notice.textContent = "clobbered";
    panel.update([match(0, "cat")], -1, "cat", true);
    expect(notice?.textContent).toBe("clobbered");
  });
});
