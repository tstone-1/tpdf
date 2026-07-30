import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Outline, OutlineItem } from "./outline";
import { Sidebar } from "./sidebar";
import { installFakeDom, type FakeDom } from "./testdom";

const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("./tiles", () => tiles);

/** One outline entry pointing at a page, with no children. */
function item(title: string, page: number): OutlineItem {
  return { title, open: false, target: { kind: "page", page, top_pt: null }, children: [] };
}

/** An outline of `n` top-level entries, entry `i` pointing at page `i`. */
function outline(n: number): Outline {
  return {
    items: Array.from({ length: n }, (_, at) => item(`Section ${at + 1}`, at)),
    total: n,
    limits: { cycles: 0, too_deep: 0, over_budget: false, titles_clipped: 0 },
    walk_ms: 0,
  };
}

describe("Sidebar keyboard activation", () => {
  let dom: FakeDom;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    tiles.nextRequestId.mockImplementation(() => 1);
    tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
  });

  afterEach(() => {
    dom.restore();
  });

  function tree(navigated: number[]): Sidebar {
    const bar = new Sidebar(dom.root as unknown as HTMLElement, {
      onNavigate: (page: number) => navigated.push(page),
      results: { onPick: () => {} },
      pages: {
        doc: 1,
        pageCount: 40,
        page: { width_pt: 600, height_pt: 800 },
        tier1: { placeholderFor: () => null },
        // The strip is built beside the tree and is not what these tests drive;
        // it is here because a sidebar without one is a different object.
        onNavigate: () => {},
      },
    });
    bar.setOutline(outline(5));
    return bar;
  }

  it("activates the row the key reached, not the one it last tracked", () => {
    // The same defect the page strip had, in the class beside it: `focused` is
    // a mirror of the DOM's focus kept by the `focusin` listener, and a
    // document without system focus moves `activeElement` without delivering
    // that event. The mirror then names a row the key never touched, and the
    // reader is sent somewhere they did not ask for.
    const navigated: number[] = [];
    const bar = tree(navigated);
    const row = bar.elementFor("2") as unknown as { parent: { dispatch: (t: string, e: unknown) => void } } | null;
    expect(row).not.toBeNull();

    row!.parent.dispatch("keydown", { key: "Enter", target: row });

    expect(navigated).toEqual([2]);
    bar.destroy();
  });

  it("falls back to the tracked row when the key did not come from one", () => {
    // The control: without it, "use the event's row" is satisfied by a tree
    // that activates nothing at all. A key on the tree carries no id, and
    // nothing has been focused, so nothing should be activated either.
    const navigated: number[] = [];
    const bar = tree(navigated);
    const row = bar.elementFor("0") as unknown as { parent: { dispatch: (t: string, e: unknown) => void } };
    row.parent.dispatch("keydown", { key: "Enter", target: { dataset: {} } });

    expect(navigated).toEqual([0]);
    bar.destroy();
  });
});
