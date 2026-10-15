/**
 * The tooltip over a link: where it goes, shown while the pointer is on it.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Link } from "./links";
import { linkTitle } from "./links";
import { installFakeDom, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";

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

const LINKS: Link[] = [
  { id: 1, page: 0, rect: [50, 50, 150, 70], target: { kind: "page", page: 4, top_pt: null } },
  { id: 2, page: 0, rect: [50, 200, 150, 220], target: { kind: "web", token: 0, host: "xn--80ak6aa92e.com", rest: "/a" } },
];

function build(): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 6,
    pages: [{ width_pt: 600, height_pt: 800 }],
  });
  viewer.setZoomFixed(1);
  viewer.setLinks(LINKS);
  return viewer;
}

/** The surface's tooltip. The fake element keeps it as a plain property. */
const tooltip = (): string => (dom.root as unknown as { title?: string }).title ?? "";

/** Moves the pointer to a point of page 1, in its points. */
function hover(viewer: Viewer, x: number, y: number): void {
  const at = viewer.screenPoint(0, x, y);
  dom.root.dispatch("pointermove", { pointerId: 1, clientX: at.x, clientY: at.y, target: dom.root });
}

describe("linkTitle", () => {
  it("names the page for a link inside the document and the address for a web link", () => {
    expect(linkTitle(LINKS[0]!.target)).toBe("Go to page 5");
    expect(linkTitle(LINKS[1]!.target)).toBe("Open xn--80ak6aa92e.com/a");
    expect(linkTitle({ kind: "none" })).toBe("");
  });
});

describe("The tooltip over a link", () => {
  it("says where the link under the pointer goes, and goes away off it", () => {
    const viewer = build();
    hover(viewer, 100, 60);
    expect(tooltip()).toBe("Go to page 5");
    hover(viewer, 100, 210);
    expect(tooltip()).toBe("Open xn--80ak6aa92e.com/a");
    hover(viewer, 400, 600);
    expect(tooltip()).toBe("");
  });
});
