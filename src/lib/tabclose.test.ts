/**
 * Which tabs closing several means, and the list of closed documents.
 */

import { describe, expect, it } from "vitest";

import { CLOSED_KEPT, ClosedTabs, tabsToClose } from "./tabclose";

describe("tabsToClose", () => {
  const all = [1, 2, 3, 4, 5];

  it("closes every other tab in the window", () => {
    expect(tabsToClose(all, all, 3, "others")).toEqual([1, 2, 4, 5]);
  });

  it("closes the tabs before or after one, in its strip", () => {
    expect(tabsToClose(all, all, 3, "left")).toEqual([1, 2]);
    expect(tabsToClose(all, all, 3, "right")).toEqual([4, 5]);
  });

  it("takes left and right in the tab's own side when there are two", () => {
    // Tabs 2 and 4 are on the right side.
    expect(tabsToClose(all, [2, 4], 4, "left")).toEqual([2]);
    expect(tabsToClose(all, [2, 4], 2, "right")).toEqual([4]);
    // Other tabs are still the whole window's.
    expect(tabsToClose(all, [2, 4], 2, "others")).toEqual([1, 3, 4, 5]);
  });

  it("closes nothing for a tab that is not in the strip", () => {
    expect(tabsToClose(all, [2, 4], 3, "left")).toEqual([]);
  });
});

describe("ClosedTabs", () => {
  it("offers the most recent closed document that is not open again", () => {
    const closed = new ClosedTabs();
    closed.closed("/a.pdf");
    closed.closed("/b.pdf");
    expect(closed.latest(() => false)).toBe("/b.pdf");
    expect(closed.latest((path) => path === "/b.pdf")).toBe("/a.pdf");
    expect(closed.latest(() => true)).toBeNull();
  });

  it("moves a document closed again to the end, and forgets one reopened", () => {
    const closed = new ClosedTabs();
    closed.closed("/a.pdf");
    closed.closed("/b.pdf");
    closed.closed("/a.pdf");
    expect(closed.latest(() => false)).toBe("/a.pdf");
    closed.reopened("/a.pdf");
    expect(closed.latest(() => false)).toBe("/b.pdf");
  });

  it("does not let one document closed many times push the others out", () => {
    const closed = new ClosedTabs();
    closed.closed("/b.pdf");
    for (let at = 0; at < CLOSED_KEPT + 5; at++) closed.closed("/a.pdf");
    closed.reopened("/a.pdf");
    expect(closed.latest(() => false)).toBe("/b.pdf");
  });

  it("keeps the most recent few", () => {
    const closed = new ClosedTabs();
    for (let at = 0; at <= CLOSED_KEPT; at++) closed.closed(`/${at}.pdf`);
    const kept: string[] = [];
    let next: string | null;
    while ((next = closed.latest((path) => kept.includes(path))) !== null) kept.push(next);
    expect(kept).toHaveLength(CLOSED_KEPT);
    expect(kept).not.toContain("/0.pdf");
  });
});
