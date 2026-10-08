/**
 * The sidebar tab listing text the pages do not show.
 *
 * The case it exists for is the empty one: a check that found nothing must
 * still show what was not compared and what is never looked at, and a panel
 * nobody has run a check for must not look like one that found nothing.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { HiddenList } from "./hiddenlist";
import { MAX_ROWS, NOT_CHECKED, UNSAVED, type HiddenText, type Passage } from "./hiddentext";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

const LIMITS = "Not looked at: comments.";

function passage(page: number, text: string, offPage = false): Passage {
  return { page, text, rect: [10, 20, 30, 40], characters: text.length, offPage };
}

function checked(found: Passage[], summary: string, unsaved = false): HiddenText {
  return { found, summary, notLookedAt: LIMITS, unsaved };
}

let dom: FakeDom;
let picked: Passage[];

beforeEach(() => {
  dom = installFakeDom();
  picked = [];
});

afterEach(() => {
  dom.restore();
});

function panel(): { list: HiddenList; notice: FakeElement; rows: FakeElement } {
  const list = new HiddenList(dom.root as unknown as HTMLElement, {
    onPick: (found) => picked.push(found),
  });
  const [notice, rows] = dom.root.children;
  return { list, notice: notice!, rows: rows! };
}

/** The text of every element directly inside `host`. */
function lines(host: FakeElement): string[] {
  return host.children.map((child) => child.textContent);
}

describe("the tab for text the pages do not show", () => {
  it("says no check has been run, and that is not a result", () => {
    const { list, notice, rows } = panel();
    expect(lines(rows)).toEqual([NOT_CHECKED]);
    expect(notice.style.display).toBe("none");
    expect([list.rowCount, list.status]).toEqual([0, []]);
  });

  it("shows what was not looked at when nothing was found", () => {
    const { list, notice, rows } = panel();
    list.setChecked(checked([], "No hidden text found: 12 characters on 1 page compared.", true));
    // Every sentence is drawn, a line each, and the placeholder is gone: an
    // empty list under these lines is the answer, not an absence of one.
    expect(lines(notice)).toEqual([
      "No hidden text found: 12 characters on 1 page compared.",
      UNSAVED,
      LIMITS,
    ]);
    expect(list.status).toEqual(lines(notice));
    expect(notice.style.display).toBe("block");
    expect(rows.children.length).toBe(0);
  });

  it("lists each passage with its page and its words, in the order found", () => {
    const { list, rows } = panel();
    list.setChecked(
      checked([passage(1, "Jane Example"), passage(4, "left in the margin", true)], "2 passages."),
    );
    expect(list.rowCount).toBe(2);
    expect(rows.children.length).toBe(2);
    expect(list.rowText(0)).toEqual({ label: "Page 1", words: "Jane Example" });
    expect(list.rowText(1)).toEqual({
      label: "Page 4, outside the page",
      words: "left in the margin",
    });
    // A button, so the platform gives it Tab, Enter and Space.
    expect(list.rowAt(0)?.tagName.toLowerCase()).toBe("button");
  });

  it("hands the passage of the row that was pressed, and marks that row", () => {
    const { list } = panel();
    const found = [passage(1, "one"), passage(2, "two"), passage(3, "three")];
    list.setChecked(checked(found, "3 passages."));
    const row = (index: number) => list.rowAt(index) as unknown as FakeElement;

    row(1).dispatch("click", {});
    expect(picked).toEqual([found[1]]);
    expect(list.picked).toBe(1);
    expect(row(1).getAttribute("aria-current")).toBe("true");

    row(2).dispatch("click", {});
    expect(picked).toEqual([found[1], found[2]]);
    // One row at a time is the current one.
    expect(row(1).getAttribute("aria-current")).toBeNull();
    expect(row(2).getAttribute("aria-current")).toBe("true");
  });

  it("draws no more rows than its cap", () => {
    const { list } = panel();
    const found = Array.from({ length: MAX_ROWS + 3 }, (_, at) => passage(1, `words ${at}`));
    list.setChecked(checked(found, "many"));
    expect(list.rowCount).toBe(MAX_ROWS);
    expect(list.rowText(MAX_ROWS - 1).words).toBe(`words ${MAX_ROWS - 1}`);
    expect(list.status).toContain(`Showing the first ${MAX_ROWS} of ${MAX_ROWS + 3} passages.`);
  });

  it("forgets the last result when it is given another, or none", () => {
    const { list, notice, rows } = panel();
    list.setChecked(checked([passage(1, "one"), passage(2, "two")], "2 passages."));
    (list.rowAt(1) as unknown as FakeElement).dispatch("click", {});

    list.setChecked(checked([passage(5, "other")], "1 passage."));
    expect([list.rowCount, list.picked]).toEqual([1, -1]);
    expect(list.rowText(0).words).toBe("other");
    expect(lines(notice)).toEqual(["1 passage.", LIMITS]);

    list.setChecked(null);
    expect(list.rowCount).toBe(0);
    expect(lines(rows)).toEqual([NOT_CHECKED]);
    expect(lines(notice)).toEqual([]);
    expect(notice.style.display).toBe("none");
  });
});
