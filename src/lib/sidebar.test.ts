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
      comments: { onPick: () => {} },
      marks: { onPick: () => {}, onRemove: () => {}, coveredFor: () => "" },
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

  /** A parent holding `kids` children, drawn expanded. */
  function nested(kids: number): Outline {
    const children = Array.from({ length: kids }, (_, at) => item(`Child ${at + 1}`, at + 1));
    return {
      items: [
        { title: "Parent", open: true, target: { kind: "page", page: 0, top_pt: null }, children },
        item("After", kids + 1),
      ],
      total: kids + 2,
      limits: { cycles: 0, too_deep: 0, over_budget: false, titles_clipped: 0 },
      walk_ms: 0,
    };
  }

  function nestedTree(): Sidebar {
    const bar = new Sidebar(dom.root as unknown as HTMLElement, {
      onNavigate: () => {},
      results: { onPick: () => {} },
      comments: { onPick: () => {} },
      marks: { onPick: () => {}, onRemove: () => {}, coveredFor: () => "" },
      pages: {
        doc: 1,
        pageCount: 40,
        page: { width_pt: 600, height_pt: 800 },
        tier1: { placeholderFor: () => null },
        onNavigate: () => {},
      },
    });
    bar.setOutline(nested(2));
    return bar;
  }

  /**
   * Sends `key` to the tree as though it arrived on row `id`.
   *
   * The element is re-read every time because toggling repaints the tree, so a
   * row held from before is detached and its parent is null.
   */
  function sendTo(bar: Sidebar, id: string, key: string): void {
    const row = bar.elementFor(id) as unknown as {
      parent: { dispatch: (t: string, e: unknown) => void };
    } | null;
    expect(row).not.toBeNull();
    row!.parent.dispatch("keydown", { key, target: row });
  }

  it("collapses the row the key reached, not the one it last tracked", () => {
    // The same stale-mirror defect as the Enter case above, fixed there and
    // left in place for the arrows. The `focusin` listener keeps the mirror
    // current only when focus is *delivered*, and a document without system
    // focus moves `activeElement` without delivering it -- which is why this
    // presented as `viewer_check`'s "collapsing a row hides its children"
    // failing one run in three rather than as a bug.
    //
    // The mirror is moved off the parent first, deliberately: with it left
    // where `setOutline` puts it the target and the mirror name the same row,
    // and the assertion passes whichever one the code reads.
    const bar = nestedTree();
    expect(bar.visible.length).toBe(4);
    sendTo(bar, "0", "ArrowDown");
    expect(bar.focusedRow).toBe("0.0");

    sendTo(bar, "0", "ArrowLeft");

    expect(bar.visible.length).toBe(2);
    bar.destroy();
  });

  it("expands the row the key reached, so the collapse is not one-way", () => {
    // "Fewer rows afterwards" is also what a tree that lost its children would
    // report, so the same key has to bring them back. The mirror is moved off
    // the parent again for the same reason as above.
    const bar = nestedTree();
    sendTo(bar, "0", "ArrowDown");
    sendTo(bar, "0", "ArrowLeft");
    expect(bar.visible.length).toBe(2);
    sendTo(bar, "0", "ArrowDown");
    expect(bar.focusedRow).toBe("1");

    sendTo(bar, "0", "ArrowRight");

    expect(bar.visible.length).toBe(4);
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
