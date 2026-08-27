import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { RedactList, noticeFor, rowLineFor, takesFor, warningFor } from "./redactlist";
import {
  PageMap,
  pageId,
  pairPlans,
  redactionRows,
  unedited,
  type RedactionView,
  type RegionPlan,
} from "./pages";
import { installFakeDom, type FakeDom } from "./testdom";

/** One pending region, with the fields a test is not about filled in. */
function region(over: Partial<RedactionView> & { id: number }): RedactionView {
  return {
    page: pageId(1),
    area: [100, 100, 300, 140],
    ...over,
  };
}

describe("redactionRows", () => {
  it("lists regions down the document, not in the order they were dragged", () => {
    // Page id 3 has been moved to the front, so its region is reviewed first.
    // A reader checks a redaction by scrolling the document, and the list has to
    // be the order they will meet them in.
    const moved = new PageMap([
      { id: pageId(3), source: 2, turns: 0 },
      { id: pageId(1), source: 0, turns: 0 },
    ]);
    const rows = redactionRows(
      [region({ id: 10, page: pageId(1) }), region({ id: 11, page: pageId(3) })],
      moved,
    );
    expect(rows.map((row) => row.redaction.id)).toEqual([11, 10]);
    expect(rows.map((row) => row.page)).toEqual([0, 1]);
  });

  it("orders two regions on one page by their top edges", () => {
    const rows = redactionRows(
      [
        region({ id: 10, area: [10, 500, 90, 540] }),
        region({ id: 11, area: [10, 100, 90, 140] }),
      ],
      unedited(2),
    );
    expect(rows.map((row) => row.redaction.id)).toEqual([11, 10]);
  });

  it("breaks a shared top edge by id rather than by the sort's stability", () => {
    // Two regions dragged along one line share a top edge exactly, which is not
    // exotic --- it is what redacting two columns of the same row looks like.
    // Without the tiebreak, which comes first would depend on how the engine
    // happens to sort, and the panel's order would differ between runs.
    const same: [number, number, number, number] = [10, 200, 90, 240];
    const rows = redactionRows(
      [
        region({ id: 12, area: same }),
        region({ id: 9, area: same }),
        region({ id: 11, area: same }),
      ],
      unedited(2),
    );
    expect(rows.map((row) => row.redaction.id)).toEqual([9, 11, 12]);
  });

  it("keeps a region nothing could place, with no page against it", () => {
    // Built by hand, because the model refuses to make one: `all_redactions`
    // walks the live page order, so a region on a deleted page is not in the
    // list at all. What this guards against is a review panel that silently
    // drops a row --- which tells a reader a pending redaction is gone.
    const rows = redactionRows(
      [region({ id: 10 }), region({ id: 11, page: pageId(99) })],
      unedited(2),
    );
    expect(rows.map((row) => row.redaction.id)).toEqual([10, 11]);
    expect(rows.map((row) => row.page)).toEqual([0, null]);
  });
});

describe("the four things a row can say about its words", () => {
  it("says the page is still being read when nothing has answered yet", () => {
    expect(rowLineFor(undefined)).toEqual({
      text: "Reading the page…",
      own: false,
    });
  });

  it("says the page could not be read, which is not the same as empty", () => {
    // The distinction the whole panel turns on. Collapsing these two would have
    // a region on an unreadable page report that it covers no text, which is a
    // claim about the document made from a failure to look at it.
    expect(rowLineFor(null)).toEqual({
      text: "Could not read the page",
      own: false,
    });
    expect(rowLineFor("")).toEqual({
      text: "No text in this region",
      own: false,
    });
  });

  it("reports the words, and marks them as the document's own", () => {
    expect(rowLineFor("clause 4")).toEqual({ text: "clause 4", own: true });
  });

  it("flattens words that ran over several lines of the page", () => {
    // A region is dragged over a block, so its words routinely arrive with the
    // page's own line breaks in them. A row is one line high.
    expect(rowLineFor("first\nsecond\n\n third").text).toBe("first second third");
  });

  it("treats whitespace under the region as no words at all", () => {
    expect(rowLineFor("  \n ").text).toBe("No text in this region");
  });
});

describe("what a row says a removal will take", () => {
  /** A plan taking some text and `many` pictures. */
  function taking(many: number): RegionPlan {
    return {
      shows: [0],
      taking: "clause 4",
      unhandled: [],
      images: Array.from({ length: many }, (_, at) => at),
    };
  }

  it("says nothing at all when the plan has not arrived", () => {
    expect(takesFor(undefined)).toBe("");
  });

  it("says nothing when the region takes no picture", () => {
    // The control. A sentence that appeared on every region would be one a
    // reader stops seeing, and this is the only state in which it must not.
    expect(takesFor(taking(0))).toBe("");
  });

  it("says a picture goes whole, because that cannot be undone afterwards", () => {
    expect(takesFor(taking(1))).toBe("Also removes a picture it covers, whole");
  });

  it("counts them when there is more than one", () => {
    expect(takesFor(taking(3))).toBe("Also removes 3 pictures it covers, whole");
  });

  it("survives a plan written before pictures were removable", () => {
    // The field is optional because a reply from an older build carries none,
    // and reading `.length` off `undefined` would break the panel rather than
    // the sentence.
    expect(takesFor({ shows: [0], taking: "clause 4", unhandled: [] })).toBe("");
  });

  it("is a different sentence from the warning, not the same one", () => {
    // They are opposite facts about one region -- what goes and what survives --
    // and a reader seeing them merged would have to work out which half was
    // which. Only one of the two is a reason to distrust the result.
    const both: RegionPlan = {
      shows: [0],
      taking: "clause 4",
      unhandled: [{ at: 0, kind: "path" }],
      images: [0],
    };
    expect(takesFor(both)).toBe("Also removes a picture it covers, whole");
    expect(warningFor(both)).toBe("Also covers a path, which a removal cannot take");
  });
});

describe("what a row says a removal cannot take", () => {
  /** A plan that takes some text and cannot take the objects named. */
  function plan(kinds: string[]): RegionPlan {
    return {
      shows: [0],
      taking: "clause 4",
      unhandled: kinds.map((kind, at) => ({ at, kind })),
    };
  }

  it("says nothing at all when the plan has not arrived", () => {
    // The silence that matters: a warning that has not been computed and a
    // region with nothing to warn about must not look alike, and the way they
    // are told apart is that only one of them ever draws a line.
    expect(warningFor(undefined)).toBe("");
  });

  it("says nothing when a removal would take everything in the region", () => {
    expect(warningFor(plan([]))).toBe("");
  });

  it("names the one object a removal cannot take", () => {
    expect(warningFor(plan(["image"]))).toBe(
      "Also covers an image, which a removal cannot take",
    );
  });

  it("counts objects of a kind rather than repeating the sentence", () => {
    // A page with three pictures on it reports three findings, and a reader
    // reading three identical sentences cannot tell that from one printed
    // thrice.
    expect(warningFor(plan(["image", "image", "image"]))).toBe(
      "Also covers 3 images, which a removal cannot take",
    );
  });

  it("names every kind, in an order that does not depend on the file", () => {
    // Sorted rather than left in the object order PDFium enumerated, so two
    // regions covering the same two kinds read the same way.
    expect(warningFor(plan(["path", "image", "path"]))).toBe(
      "Also covers an image and 2 paths, which a removal cannot take",
    );
  });
});

describe("pairing plans with the regions they were asked about", () => {
  const region = (id: number): RedactionView => ({
    id,
    page: pageId(1),
    area: [0, 0, 10, 10],
  });
  const plan = (taking: string): RegionPlan => ({
    shows: [0],
    taking,
    unhandled: [],
  });

  it("attaches each plan to the region it was asked about", () => {
    const paired = pairPlans([region(4), region(9)], [plan("first"), plan("second")]);
    expect(paired.get(4)?.taking).toBe("first");
    expect(paired.get(9)?.taking).toBe("second");
  });

  it("attaches nothing at all when the counts disagree", () => {
    // The reply is a list in the order the request put the regions, so one plan
    // too few attaches every later plan to the wrong region --- and a plan is a
    // claim about what a removal takes, so a reader would be shown the wrong
    // words beside the wrong rectangle. Empty is what the rows said before the
    // reply arrived, which is the honest thing for them to go on saying.
    expect(pairPlans([region(4), region(9)], [plan("first")]).size).toBe(0);
    expect(
      pairPlans([region(4)], [plan("first"), plan("second")]).size,
    ).toBe(0);
  });

  it("attaches nothing for no regions, which is not a mismatch", () => {
    expect(pairPlans([], []).size).toBe(0);
  });
});

describe("what the panel says above the rows", () => {
  it("says nothing at all when there is nothing marked", () => {
    expect(noticeFor([])).toBe("");
  });

  it("counts the regions and says nothing has been removed", () => {
    // The standing fact about every row here, written down rather than left to
    // be inferred from the tab being called Redactions. §6's thesis is that a
    // redaction which looks done and is not is worse than none.
    const rows = redactionRows([region({ id: 1 })], unedited(2));
    expect(noticeFor(rows)).toBe("1 region marked. Nothing has been removed yet.");
  });

  it("counts more than one", () => {
    const rows = redactionRows(
      [region({ id: 1 }), region({ id: 2, area: [10, 400, 90, 440] })],
      unedited(2),
    );
    expect(noticeFor(rows)).toBe("2 regions marked. Nothing has been removed yet.");
  });

  it("names regions that are on no page, without dropping the standing line", () => {
    const rows = redactionRows(
      [region({ id: 1 }), region({ id: 2, page: pageId(99) })],
      unedited(2),
    );
    expect(noticeFor(rows)).toBe(
      "2 regions marked. Nothing has been removed yet. 1 is not on any page.",
    );
  });
});

describe("RedactList", () => {
  let dom: FakeDom;
  let picked: number[];
  let removed: number[];
  /** What the panel is told each region covers, by id. */
  const words = new Map<number, string | null>();
  /** What the panel is told a removal would take, by id. */
  const plans = new Map<number, RegionPlan>();

  beforeEach(() => {
    dom = installFakeDom();
    picked = [];
    removed = [];
    words.clear();
    plans.clear();
  });

  afterEach(() => {
    dom.restore();
  });

  function panel(): RedactList {
    return new RedactList(dom.root as unknown as HTMLElement, {
      onPick: (id) => picked.push(id),
      onRemove: (id) => removed.push(id),
      // `has` before `get`, so a region with no entry answers `undefined` and a
      // region recorded as unreadable answers `null`. `get` alone collapses the
      // two, which is the defect this panel exists not to have.
      wordsFor: (id) => (words.has(id) ? words.get(id) : undefined),
      planFor: (id) => plans.get(id),
    });
  }

  function show(
    list: RedactList,
    items: RedactionView[],
    pages = unedited(3),
  ): void {
    list.setRedactions(redactionRows(items, pages));
  }

  it("says nothing is marked for removal, before anything is pushed at it", () => {
    const list = panel();
    expect(list.rowCount).toBe(0);
    const text = (dom.root.children[1]?.children[0]?.textContent ?? "") as string;
    expect(text).toContain("not marked anything for removal");
  });

  it("shows the page and the words under the region", () => {
    const list = panel();
    words.set(7, "the sum of £4,200");
    show(list, [region({ id: 7, page: pageId(3) })]);
    expect(list.rowText(7)).toEqual({
      words: "the sum of £4,200",
      warning: null,
      page: "3",
      own: true,
    });
  });

  it("draws a region whose page has not been read yet as still reading", () => {
    const list = panel();
    show(list, [region({ id: 7 })]);
    expect(list.rowText(7)).toEqual({
      words: "Reading the page…",
      warning: null,
      page: "1",
      own: false,
    });
  });

  it("shows a word that arrives after the row was drawn", () => {
    // The two setters exist because the list and the words change for different
    // reasons: the row is drawn the moment the region is dragged, and the page
    // it is on is extracted afterwards.
    const list = panel();
    show(list, [region({ id: 7 })]);
    words.set(7, "clause 4");
    list.setWords();
    expect(list.rowText(7).words).toBe("clause 4");
    expect(list.rowCount).toBe(1);
  });

  it("draws the warning under the words, and nothing when there is none", () => {
    const list = panel();
    words.set(7, "clause 4");
    plans.set(7, { shows: [0], taking: "clause 4", unhandled: [{ at: 2, kind: "image" }] });
    show(list, [region({ id: 7 }), region({ id: 8, area: [10, 400, 90, 440] })]);
    expect(list.rowText(7).warning).toBe(
      "Also covers an image, which a removal cannot take",
    );
    // The control, on a row of the same panel: a region with no plan draws no
    // second line **at all**, so the warning is a fact about the region rather
    // than something every row carries. `null` rather than `""` on purpose --- a
    // mutation that drew an empty warning element on every row survived a check
    // that read the text, because an empty line and no line are the same string.
    expect(list.rowText(8).warning).toBeNull();
  });

  it("says the count and the standing line above the rows", () => {
    const list = panel();
    show(list, [region({ id: 7 })]);
    expect(list.status).toBe("1 region marked. Nothing has been removed yet.");
  });

  it("is one tab stop for the whole list of regions", () => {
    const list = panel();
    show(list, [
      region({ id: 0, area: [10, 100, 90, 140] }),
      region({ id: 1, area: [10, 200, 90, 240] }),
      region({ id: 2, area: [10, 300, 90, 340] }),
    ]);
    const tabbable = [0, 1, 2].filter((id) => list.elementFor(id)?.tabIndex === 0);
    expect(tabbable).toEqual([0]);
  });

  it("moves the roving tabindex through the regions with the arrow keys", () => {
    const list = panel();
    show(list, [
      region({ id: 0, area: [10, 100, 90, 140] }),
      region({ id: 1, area: [10, 200, 90, 240] }),
    ]);
    dom.root.children[1]?.dispatch("keydown", { key: "ArrowDown" });
    expect(list.focusedId).toBe(1);
    expect(list.elementFor(1)?.tabIndex).toBe(0);
    expect(list.elementFor(0)?.tabIndex).toBe(-1);
  });

  it("scrolls to the region the key landed on, not the one it remembered", () => {
    // The stale-focus-mirror trap: a window without system focus moves
    // `activeElement` without delivering `focusin`.
    const list = panel();
    show(list, [
      region({ id: 0, area: [10, 100, 90, 140] }),
      region({ id: 1, area: [10, 200, 90, 240] }),
    ]);
    dom.root.children[1]?.dispatch("keydown", {
      key: "Enter",
      target: list.elementFor(1),
    });
    expect(picked).toEqual([1]);
  });

  it("reports a press on a region's row", () => {
    const list = panel();
    show(list, [region({ id: 3 })]);
    (
      list.elementFor(3) as unknown as { dispatch: (t: string, e: object) => void }
    )?.dispatch("pointerdown", {});
    expect(picked).toEqual([3]);
  });

  /** A row's remove control, found by its part rather than by position. */
  function removeControl(
    list: RedactList,
    id: number,
  ): {
    dispatch: (t: string, e: object) => void;
    getAttribute: (n: string) => unknown;
  } {
    const row = list.elementFor(id) as unknown as {
      children: { dataset?: { part?: string } }[];
    };
    const found = row.children.find((child) => child.dataset?.part === "remove");
    expect(found).toBeDefined();
    return found as never;
  }

  it("takes a region off from its row, without also scrolling to it", () => {
    const list = panel();
    show(list, [
      region({ id: 3, area: [10, 100, 90, 140] }),
      region({ id: 4, area: [10, 200, 90, 240] }),
    ]);
    removeControl(list, 4).dispatch("click", {});
    expect(removed).toEqual([4]);
    // What this does NOT prove: the fake DOM does not bubble, so the
    // `stopPropagation` that stops a real browser firing the row's own
    // `pointerdown` first is not under test here.
    expect(picked).toEqual([]);
  });

  it("names the control for what it takes off", () => {
    const list = panel();
    show(list, [region({ id: 1 })]);
    expect(removeControl(list, 1).getAttribute("aria-label")).toBe("Remove region");
  });

  it("offers the control on a region that is on no page", () => {
    // Undo is chronological and this row cannot be scrolled to, so the control
    // is the only way a reader ever gets it off the list.
    const list = panel();
    show(list, [region({ id: 7, page: pageId(99) })]);
    expect(list.rowText(7).page).toBe("—");
    removeControl(list, 7).dispatch("click", {});
    expect(removed).toEqual([7]);
  });

  it("refuses to scroll to a region that is on no page", () => {
    const list = panel();
    show(list, [region({ id: 7, page: pageId(99) })]);
    (
      list.elementFor(7) as unknown as { dispatch: (t: string, e: object) => void }
    )?.dispatch("pointerdown", {});
    dom.root.children[1]?.dispatch("keydown", {
      key: "Enter",
      target: list.elementFor(7),
    });
    expect(picked).toEqual([]);
  });

  it("removes with Delete and with Backspace, including a region on no page", () => {
    const list = panel();
    show(list, [
      region({ id: 0, area: [10, 100, 90, 140] }),
      region({ id: 1, page: pageId(99) }),
    ]);
    dom.root.children[1]?.dispatch("keydown", {
      key: "Delete",
      target: list.elementFor(0),
    });
    dom.root.children[1]?.dispatch("keydown", {
      key: "Backspace",
      target: list.elementFor(1),
    });
    expect(removed).toEqual([0, 1]);
    expect(picked).toEqual([]);
  });

  it("leaves a key pressed on the region's remove control to the control", () => {
    // Enter on the button reaches the list's own handler too, and `idOf` finds
    // no id on a button --- so without the guard the fallback hands it the
    // focused row and the panel scrolls instead of the region coming off.
    const list = panel();
    show(list, [region({ id: 5 })]);
    dom.root.children[1]?.dispatch("keydown", {
      key: "Enter",
      target: removeControl(list, 5),
    });
    expect(picked).toEqual([]);
  });
});
