/**
 * Which released press asks for the graphic under it (`isGraphicClick` in
 * `viewer.ts`). The graphic found is `tests/cli/graphic.rs`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installFakeDom, type FakeDom } from "./testdom";
import { isGraphicClick, Viewer, type SelectedGraphic } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({ fetchTile: vi.fn(), cancelTile: vi.fn(), nextRequestId: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
  core.invoke.mockResolvedValue(null);
  tiles.fetchTile.mockReturnValue(new Promise(() => {}));
  tiles.nextRequestId.mockReturnValue(1);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

function build(): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 3,
    pages: [{ width_pt: 600, height_pt: 800 }],
  });
  viewer.setZoomFixed(1);
  return viewer;
}

const BOX = { left: 10, top: 10, right: 90, bottom: 60 };
const HELD = {
  doc: 1,
  request: { page: 0, object: 4 },
  pageNumber: 1,
} as unknown as SelectedGraphic;

const PRESS = { x: 100, y: 100, text: false };
const HERE = { clientX: 101, clientY: 102 };

describe("isGraphicClick", () => {
  it("takes a still, single press on paper with no tool and nothing selected", () => {
    expect(isGraphicClick(PRESS, HERE, "none", "char", false)).toBe(true);
  });

  it("leaves a drag, text, a tool, a double click and a selection alone", () => {
    // Mutation: any one condition dropped -> one of these goes red.
    expect(isGraphicClick(PRESS, { clientX: 110, clientY: 100 }, "none", "char", false)).toBe(false);
    expect(isGraphicClick({ ...PRESS, text: true }, HERE, "none", "char", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "region", "char", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "none", "word", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "none", "char", true)).toBe(false);
  });
});

describe("the graphic selection", () => {
  it("turning the view ends the graphic selection", () => {
    // The outline is a box in the page as it was turned when found, and Copy
    // and Save act on what is held. Mutation: drop `this.endGraphic()` from
    // `applyTurns` -> red.
    for (const delta of [1, -1]) {
      const viewer = build();
      expect(viewer.showGraphic(0, BOX, HELD, viewer.graphicTicket())).toBe(true);
      expect(viewer.selectedGraphic()).toBe(HELD);
      viewer.rotateBy(delta);
      expect(viewer.selectedGraphic()).toBeNull();
    }
  });

  it("keeps the selection through a turn that changes nothing", () => {
    // Control: a whole turn is no turn, and `rotateBy` returns before
    // `applyTurns`, so the selection is the same one.
    const viewer = build();
    viewer.showGraphic(0, BOX, HELD, viewer.graphicTicket());
    viewer.rotateBy(4);
    expect(viewer.selectedGraphic()).toBe(HELD);
  });

  it("does not show an answer asked for before the selection ended", () => {
    // A click asks, an edit clears, the answer lands: its object index is from
    // before the edit. Mutation: drop the ticket comparison in `showGraphic`,
    // or count only the clears that had something shown -> red.
    for (const end of [
      (viewer: Viewer) => viewer.clearGraphic(),
      (viewer: Viewer) => viewer.rotateBy(1),
      (viewer: Viewer) => viewer.setPages([...viewer.pageOrder]),
    ]) {
      const viewer = build();
      const ticket = viewer.graphicTicket();
      end(viewer);
      expect(viewer.showGraphic(0, BOX, HELD, ticket)).toBe(false);
      expect(viewer.selectedGraphic()).toBeNull();
      // Control: a ticket taken after the end is shown.
      expect(viewer.showGraphic(0, BOX, HELD, viewer.graphicTicket())).toBe(true);
      expect(viewer.selectedGraphic()).toBe(HELD);
    }
  });
});
