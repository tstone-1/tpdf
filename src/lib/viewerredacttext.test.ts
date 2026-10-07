/**
 * The redaction tool pressed on text.
 *
 * Armed, the tool has two gestures and the press decides which: on the words
 * it selects them and the selection is marked when the button comes up; on
 * blank paper it drags a rectangle. `viewercrop.test.ts` has the rectangle on
 * a page with no text at all, where the first gesture cannot happen.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { type PageText } from "./text";
import { Viewer } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

const PAGE = { width_pt: 600, height_pt: 800 };

/**
 * Two words across the top half of the page, in letters a hundred points
 * wide: `ab cd`. The bottom half has no text.
 */
const TEXT: PageText = {
  codes: [97, 98, 32, 99, 100],
  boxes: [
    50, 100, 150, 300,
    150, 100, 250, 300,
    0, 0, 0, 0,
    300, 100, 400, 300,
    400, 100, 500, 300,
  ],
  width_pt: 600,
  height_pt: 800,
  quarter_turns: 0,
  extract_ms: 0,
};

let dom: FakeDom;
/** Every rectangle the tool dragged out. */
let regions: number[] = [];
/** What was selected each time the tool asked for the selection to be marked. */
let marked: string[] = [];
/** The viewer under test, for {@link at} to ask where the page is. */
let current: Viewer | null = null;

beforeEach(() => {
  dom = installFakeDom();
  regions = [];
  marked = [];
  core.invoke.mockImplementation((command: string) =>
    command === "page_text"
      ? Promise.resolve(TEXT)
      : command === "page_geometry"
        ? Promise.resolve({ width_pt: 600, height_pt: 800, left: 0, top: 0 })
        : Promise.resolve(null),
  );
  tiles.fetchTile.mockRejectedValue(new Error("no tile"));
  let rid = 0;
  tiles.nextRequestId.mockImplementation(() => ++rid);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

async function build(): Promise<Viewer> {
  const viewer: Viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [PAGE],
    onRedacted: (page) => regions.push(page),
    onRedactSelection: () => {
      marked.push(viewer.selectionQuadsByPage().map((one) => one.text).join("|"));
      viewer.clearSelection();
    },
  });
  current = viewer;
  await settle();
  const pages: PageView[] = [{ id: pageId(1), source: { baseline: 0 }, turns: 0 }];
  viewer.setPages(pages);
  await settle();
  // A press is what asks for a page's text, and the first one finds none
  // yet. Asked for here, so every test starts with the text in hand.
  (viewer as unknown as { requestText(page: number): void }).requestText(0);
  await settle();
  return viewer;
}

/** A point of the page, in the root's client coordinates. */
function at(x: number, y: number): { x: number; y: number } {
  const inner = current as unknown as {
    scroller: { pageOrigin(slot: number): { left: number; top: number } };
    zoom: number;
    scrollTop: number;
  };
  const origin = inner.scroller.pageOrigin(0);
  return {
    x: origin.left + x * inner.zoom,
    y: origin.top + y * inner.zoom - inner.scrollTop,
  };
}

function hover(to: { x: number; y: number }): void {
  dom.root.dispatch("pointermove", { pointerId: 1, clientX: to.x, clientY: to.y });
}

function drag(from: { x: number; y: number }, to: { x: number; y: number }): void {
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: from.x,
    clientY: from.y,
    target: dom.root,
  });
  dom.root.dispatch("pointermove", { pointerId: 1, clientX: to.x, clientY: to.y });
  dom.root.dispatch("pointerup", { pointerId: 1, clientX: to.x, clientY: to.y });
}

describe("the redaction tool pressed on text", () => {
  it("marks the words dragged across and stays armed", async () => {
    const viewer = await build();
    viewer.armRedact();
    drag(at(60, 200), at(240, 200));

    expect(marked).toEqual(["ab"]);
    expect(regions).toEqual([]);
    expect(viewer.redactArmed).toBe(true);
    // The selection was handed over and cleared, so nothing stays selected
    // under the mark.
    expect(viewer.hasSelection).toBe(false);
    viewer.destroy();
  });

  it("drags a rectangle when pressed on blank paper", async () => {
    const viewer = await build();
    viewer.armRedact();
    drag(at(60, 500), at(240, 700));

    expect(marked).toEqual([]);
    expect(regions).toHaveLength(1);
    viewer.destroy();
  });

  it("marks nothing for a click that selects nothing", async () => {
    const viewer = await build();
    viewer.armRedact();
    drag(at(100, 200), at(100, 200));

    expect(marked).toEqual([]);
    expect(regions).toEqual([]);
    viewer.destroy();
  });

  it("selects without marking when the tool is not armed", async () => {
    // The control for the first test: the same drag, and the selection stays
    // the reader's.
    const viewer = await build();
    drag(at(60, 200), at(240, 200));

    expect(marked).toEqual([]);
    expect(viewer.hasSelection).toBe(true);
    viewer.destroy();
  });

  it("shows the text cursor over words and the crosshair over paper", async () => {
    const viewer = await build();
    viewer.armRedact();
    const cursor = (): string =>
      (viewer as unknown as { surfaceHost: { style: { cursor: string } } }).surfaceHost.style.cursor;

    hover(at(100, 200));
    expect(cursor()).toBe("text");
    hover(at(100, 600));
    expect(cursor()).toBe("crosshair");
    viewer.destroy();
  });
});
