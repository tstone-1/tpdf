/**
 * What an open decides, and that `openDocument` in `App.svelte` asks here.
 *
 * Every test below was checked by mutating `documentopen.ts` or `App.svelte`
 * and seeing it fail; the mutations are in `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import {
  dropView, openFailure, pageTable, placeOnSide, placeToResume, readerIsIn, repoint, sharedByTwin,
} from "./documentopen";
import { DocumentTabs } from "./documenttabs";
import { Panes } from "./panes";
import type { Place } from "./session";
import { functionIn, missingFrom } from "./sourcetext";

const size = (width_pt: number) => ({ width_pt, height_pt: 800 });

const place = (path: string, page: number, more: Partial<Place> = {}): Place => ({
  path, page, top_pt: 0, zoom: 1, fit: "none", turns: 0, sidebar: true, page_count: 40, ...more,
});

describe("the page sizes a viewer is built with", () => {
  it("are every size the open carried, in order", () => {
    expect(pageTable({ pages: [size(600), size(300), size(900)] })).toEqual([size(600), size(300), size(900)]);
    // A lazy open carries the first page alone, and that is the whole table.
    expect(pageTable({ pages: [size(600)] })).toEqual([size(600)]);
  });

  it("refuse a document that reports no pages", () => {
    expect(() => pageTable({ pages: [] })).toThrow("document reports no pages");
  });
});

describe("where an open puts the reader", () => {
  const snapshot = [place("/a.pdf", 3), place("/b.pdf", 7)];

  it("is the tab's own place before the caller's, and the caller's before the snapshot", () => {
    const kept = place("/b.pdf", 11);
    const override = place("/b.pdf", 22);
    expect(placeToResume(kept, override, snapshot, "/b.pdf", 40)?.page).toBe(11);
    expect(placeToResume(null, override, snapshot, "/b.pdf", 40)?.page).toBe(22);
    expect(placeToResume(undefined, null, snapshot, "/b.pdf", 40)?.page).toBe(7);
  });

  it("is the snapshot's place for this path and no other's", () => {
    expect(placeToResume(undefined, null, snapshot, "/a.pdf", 40)?.page).toBe(3);
    expect(placeToResume(undefined, null, snapshot, "/c.pdf", 40)).toBeNull();
  });

  it("is fitted to the pages the document has now", () => {
    const fitted = placeToResume(place("/b.pdf", 30), null, [], "/b.pdf", 12);
    expect(fitted).toMatchObject({ page: 11, page_count: 12 });
  });
});

describe("the other views of a document that was saved", () => {
  it("are pointed at the new handle and the new model, every one of them", () => {
    const twins = [{ doc: "old", edits: 1, path: "/a.pdf" }, { doc: "old", edits: 1, path: "/a.pdf" }];
    repoint(twins, "new", 2);
    expect(twins).toEqual([{ doc: "new", edits: 2, path: "/a.pdf" }, { doc: "new", edits: 2, path: "/a.pdf" }]);
  });
});

describe("what two views of one document share", () => {
  const model = {};
  const first = { view: 1, edits: model };
  const second = { view: 2, edits: model };
  const third = { view: 3, edits: model };
  const other = { view: 4, edits: {} };
  const row = [other, first, second, third];

  it("is what the first twin that answers holds, and never the view's own", () => {
    const asked: number[] = [];
    const held = sharedByTwin(row, third, (twin) => {
      asked.push(twin.view);
      return twin.view === 1 ? undefined : `list of ${twin.view}`;
    });
    expect(held).toBe("list of 2");
    // Another document's tab and the view itself are not asked.
    expect(asked).toEqual([1, 2]);
  });

  it("is nothing when no twin is mounted, when there is no twin and when there is no tab", () => {
    expect(sharedByTwin(row, first, () => undefined)).toBeUndefined();
    expect(sharedByTwin(row, other, () => "a list")).toBeUndefined();
    expect(sharedByTwin(row, undefined, () => "a list")).toBeUndefined();
  });
});

describe("the side a view goes on", () => {
  const order = [1, 2, 3];
  /** Tabs 1 and 2 on the left, 3 on the right, the reader on the right. */
  const split = () => {
    const panes = new Panes<number>();
    panes.move(3, "right", order);
    return panes;
  };

  it("is the side the reader is in for a document opened for the first time", () => {
    const panes = split();
    placeOnSide(panes, 9, undefined, false);
    expect(panes.sideOf(9)).toBe("right");
    expect(panes.front("right")).toBe(9);
    expect(panes.focused).toBe("right");
  });

  it("is the side it is already on for a tab returned to, in front and with the reader", () => {
    const panes = split();
    placeOnSide(panes, 2, undefined, true);
    expect(panes.sideOf(2)).toBe("left");
    expect(panes.front("left")).toBe(2);
    expect(panes.focused).toBe("left");
    expect(panes.sideOf(3)).toBe("right");
  });

  it("is the side of the handle a save replaced", () => {
    const panes = split();
    panes.fronted(1);
    placeOnSide(panes, 9, 3, false);
    // The reader was on the left; the new handle is where the old one was.
    expect(panes.sideOf(9)).toBe("right");
    expect(panes.front("right")).toBe(9);
    expect(panes.focused).toBe("right");
  });
});

describe("a view an open could not finish", () => {
  it("leaves the tabs, and its side shows the neighbour it had in the row", () => {
    const tabs = new DocumentTabs<{ view: number; path: string }>();
    for (const view of [1, 2, 3]) tabs.keep({ view, path: `/${view}.pdf` });
    const panes = new Panes<number>();
    panes.fronted(3);
    dropView(panes, tabs, 3);
    expect(tabs.all.map((tab) => tab.view)).toEqual([1, 2]);
    // Told with the row as it was: asked after the tab had gone, the side
    // would have had no neighbour to name.
    expect(panes.front("left")).toBe(2);
  });
});

describe("what a failed open says", () => {
  it("is the refusal's own sentence, and anything else as it is", () => {
    expect(openFailure({ reason: "The file is not a PDF.", locked: false }, false)).toBe("The file is not a PDF.");
    expect(openFailure(new Error("no surface to mount into"), false)).toBe("Error: no surface to mount into");
  });

  it("is nothing for a document the launch tried to put back", () => {
    expect(openFailure({ reason: "No such file.", locked: false }, true)).toBeNull();
    expect(openFailure(new Error("gone"), true)).toBeNull();
  });
});

describe("the open's wiring in App.svelte", () => {
  // Read as text with the comments taken out, because `App.svelte` is the join
  // and nothing imports it. That sees that a line is there and not whether it
  // runs (`sourcetext.ts`), so these hold the hand-over and no decision.
  it("asks this module for each decision, in the function that needs it", () => {
    expect(missingFrom(functionIn(app, "async function openDocument("), [
      "const pages = pageTable(doc);",
      "const resume = placeToResume(retained?.place, override, session.places, path, retained?.edits.state.pages.length ?? doc.page_count);",
      "abandonOpen(acquired, replaced, twins);",
      "const said = openFailure(e, resuming);",
      "if (said !== null) error = said;",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function adoptModel("), [
      "repoint(twins, doc, model);",
      "const shared = sharedByTwin(tabs.all, tabs.find(view), (twin) => asDocument(twin.view, () => covered));",
      "if (shared) covered = shared;",
      "placeOnSide(panes, view, replacing?.view, retained !== undefined);",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function abandonOpen("), [
      "dropView(panes, tabs, viewOf(acquired));",
      "for (const twin of twins) dropView(panes, tabs, twin.view);",
    ])).toEqual([]);
  });

  it("runs what a form control says, and a press on a place for a signature, as the form's own document", () => {
    expect(missingFrom(functionIn(app, "function buildFormLayer("), [
      "(message) => { asDocument(view, () => say(message)); },",
      "if (!readerIsIn(view, openView, () => focusSide(panes.sideOf(view)))) {",
    ])).toEqual([]);
  });

  it("starts for the document the reader is in without taking them anywhere", () => {
    let taken = 0;
    expect(readerIsIn(3, 3, () => { taken++; return false; })).toBe(true);
    expect(taken).toBe(0);
  });

  it("starts for another document only when the reader can be taken there", () => {
    expect(readerIsIn(3, 4, () => true)).toBe(true);
    expect(readerIsIn(3, 4, () => false)).toBe(false);
  });

  it("builds the viewer's and the sidebar's callbacks to run as the document they were built for", () => {
    expect(missingFrom(functionIn(app, "function buildViewer("), ["}, runningAs(view)));"])).toEqual([]);
    expect(missingFrom(functionIn(app, "function buildSidebar("), ["}, runningAs(view)));"])).toEqual([]);
    expect(missingFrom(functionIn(app, "function runningAs("), [
      "return (work) => asDocument(view, work);",
    ])).toEqual([]);
  });
});
