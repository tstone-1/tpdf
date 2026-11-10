import { afterEach, describe, expect, it, vi } from "vitest";

import type { Place, Session, Sides } from "./session";
import {
  TabRecorder, afterLaunch, afterReopen, launchPlan, openBehind, sidesToRestore, tabsToReopen, unopenedAtLaunch,
  withUnopened, type TabHost,
} from "./tabrestore";

const place = (path: string): Place => ({
  path, page: 0, top_pt: 0, zoom: 1, fit: "width", turns: 0, sidebar: false, page_count: 1,
});

const session = (extra: Partial<Session> = {}): Session => ({
  // Most recent first, and deliberately not the tab order.
  places: [place("/c.pdf"), place("/a.pdf"), place("/b.pdf"), place("/closed.pdf")],
  tabs: ["/a.pdf", "/b.pdf", "/c.pdf"],
  active_tab: "/b.pdf",
  restore_tabs: true,
  ...extra,
});

describe("what a launch opens", () => {
  it("reopens only the last document unless every tab was asked for", () => {
    const plan = { show: ["/c.pdf"], resuming: true, behind: [], order: [], sides: null };
    expect(launchPlan(session({ restore_tabs: false }), [])).toEqual(plan);
    // A session file written before the preference existed.
    const { restore_tabs: _, ...older } = session();
    expect(launchPlan(older, [])).toEqual(plan);
    expect(launchPlan({ places: [] }, [])).toEqual({
      show: [], resuming: true, behind: [], order: [], sides: null,
    });
  });

  it("shows the tab that was showing and opens the others behind it, in tab order", () => {
    expect(launchPlan(session(), [])).toEqual({
      show: ["/b.pdf"],
      resuming: true,
      behind: ["/a.pdf", "/c.pdf"],
      order: ["/a.pdf", "/b.pdf", "/c.pdf"],
      sides: null,
    });
  });

  it("carries the two sides when the tabs come back, and only then", () => {
    const sides: Sides = { right: ["/c.pdf"], beside: "/c.pdf", share: 0.3 };
    expect(launchPlan(session({ sides }), []).sides).toEqual(sides);
    expect(launchPlan(session({ sides, restore_tabs: false }), []).sides).toBeNull();
    // What the launcher handed over is not in the record, and gets the window.
    expect(launchPlan(session({ sides }), ["/new.pdf"]).sides).toBeNull();
  });

  it("falls back to the tab read most recently, then to the first", () => {
    for (const active_tab of [null, "/closed.pdf"]) {
      expect(launchPlan(session({ active_tab }), []).show, String(active_tab)).toEqual(["/c.pdf"]);
    }
    const { active_tab: _, ...older } = session();
    expect(launchPlan(older, []).show).toEqual(["/c.pdf"]);
    const unread = session({ active_tab: null, places: [place("/closed.pdf")] });
    expect(launchPlan(unread, [])).toMatchObject({ show: ["/a.pdf"], behind: ["/b.pdf", "/c.pdf"] });
  });

  it("falls back to the last document when no tab was recorded", () => {
    const plan = { show: ["/c.pdf"], resuming: true, behind: [], order: [], sides: null };
    expect(launchPlan(session({ tabs: [] }), [])).toEqual(plan);
    const { tabs: _, ...older } = session();
    expect(launchPlan(older, [])).toEqual(plan);
  });

  it("shows what the launcher handed over, with the tabs behind it", () => {
    expect(launchPlan(session(), ["/new.pdf", "/b.pdf"])).toEqual({
      show: ["/new.pdf", "/b.pdf"],
      resuming: false,
      behind: ["/a.pdf", "/c.pdf"],
      order: ["/a.pdf", "/b.pdf", "/c.pdf", "/new.pdf"],
      sides: null,
    });
    // With the preference off a handed file is all that opens, as before.
    expect(launchPlan(session({ restore_tabs: false }), ["/new.pdf"])).toEqual({
      show: ["/new.pdf"], resuming: false, behind: [], order: [], sides: null,
    });
  });
});

describe("the tabs from last time", () => {
  it("lists the ones not open now, whatever the preference says", () => {
    const open = new Set(["/b.pdf"]);
    const isOpen = (path: string) => open.has(path);
    expect(tabsToReopen(session(), isOpen)).toEqual(["/a.pdf", "/c.pdf"]);
    expect(tabsToReopen(session({ restore_tabs: false }), isOpen)).toEqual(["/a.pdf", "/c.pdf"]);
    expect(tabsToReopen(session(), () => true)).toEqual([]);
    expect(tabsToReopen({ places: [place("/a.pdf")] }, () => false)).toEqual([]);
  });
});

describe("opening tabs behind the one showing", () => {
  const host = (missing: string[] = [], showing = true) => {
    const log: string[] = [];
    const tabs: TabHost = {
      openBehind: async (path) => {
        if (missing.includes(path)) throw new Error("no such file");
        log.push(`open ${path}`);
      },
      arrange: (order) => { log.push(`arrange ${order.join(" ")}`); },
      showing: () => showing,
      showFirst: async () => { log.push("show first"); },
    };
    return { log, tabs };
  };

  it("opens each in turn, then orders the strip", async () => {
    const { log, tabs } = host();
    expect(await openBehind(["/a.pdf", "/c.pdf"], ["/a.pdf", "/b.pdf", "/c.pdf"], tabs)).toEqual([]);
    expect(log).toEqual(["open /a.pdf", "open /c.pdf", "arrange /a.pdf /b.pdf /c.pdf"]);
  });

  it("carries on past a tab that will not open, and names it", async () => {
    const { log, tabs } = host(["/a.pdf"]);
    expect(await openBehind(["/a.pdf", "/c.pdf"], [], tabs)).toEqual(["/a.pdf"]);
    // No order was given, so the strip is left as it is.
    expect(log).toEqual(["open /c.pdf"]);
  });

  it("shows a tab when the one that should be showing did not open", async () => {
    const { log, tabs } = host([], false);
    await openBehind(["/a.pdf"], ["/b.pdf", "/a.pdf"], tabs);
    expect(log).toEqual(["open /a.pdf", "arrange /b.pdf /a.pdf", "show first"]);
  });

  it("says which tabs could not be reopened", () => {
    const name = (path: string) => path.slice(1);
    expect(afterReopen([], name)).toBeNull();
    expect(afterReopen(["/a.pdf"], name)).toBe(
      "Could not reopen a.pdf. It may have been moved, renamed or deleted.",
    );
    expect(afterReopen(["/a.pdf", "/b.pdf", "/c.pdf"], name)).toBe(
      "Could not reopen a.pdf and 2 more. They may have been moved, renamed or deleted.",
    );
  });
});

describe("the sides to put back", () => {
  const open = ["/a.pdf", "/b.pdf", "/c.pdf"];
  const sides: Sides = { right: ["/c.pdf", "/b.pdf"], beside: "/b.pdf", share: 0.3 };

  it("are the recorded ones when every tab opened, the right in tab order", () => {
    expect(sidesToRestore(sides, open, "/a.pdf")).toEqual({
      right: ["/b.pdf", "/c.pdf"], beside: "/b.pdf", share: 0.3,
    });
  });

  it("are none without a record, or without a document on screen", () => {
    expect(sidesToRestore(null, open, "/a.pdf")).toBeNull();
    expect(sidesToRestore(sides, open, null)).toBeNull();
    expect(sidesToRestore(sides, open, "/gone.pdf")).toBeNull();
  });

  it("are none when a whole side would not open", () => {
    expect(sidesToRestore(sides, ["/a.pdf"], "/a.pdf")).toBeNull();
    expect(sidesToRestore(sides, ["/b.pdf", "/c.pdf"], "/b.pdf")).toBeNull();
    // One tab of the right side is enough for a split.
    expect(sidesToRestore(sides, ["/a.pdf", "/c.pdf"], "/a.pdf")).toEqual({
      right: ["/c.pdf"], beside: null, share: 0.3,
    });
  });

  it("name a tab beside only on the side the reader is not in", () => {
    // The tab that was showing would not open, and its neighbour is shown.
    expect(sidesToRestore(sides, open, "/c.pdf")?.beside).toBeNull();
    expect(sidesToRestore({ ...sides, beside: "/a.pdf" }, open, "/c.pdf")?.beside).toBe("/a.pdf");
    expect(sidesToRestore({ ...sides, beside: null }, open, "/a.pdf")?.beside).toBeNull();
    // A tab that did not open is on neither side, the left included.
    expect(sidesToRestore({ ...sides, beside: "/gone.pdf" }, open, "/c.pdf")?.beside).toBeNull();
  });
});

describe("recording the open tabs", () => {
  const recorder = () => {
    const sent: [string[], string | null][] = [];
    const answers: (() => void)[] = [];
    const tabs = new TabRecorder((paths, active) => {
      sent.push([paths, active]);
      return new Promise<void>((resolve) => answers.push(resolve));
    });
    return { sent, answers, tabs };
  };

  it("writes a change once, and one write at a time", async () => {
    const { sent, answers, tabs } = recorder();
    tabs.note(["/a.pdf"], "/a.pdf");
    tabs.note(["/a.pdf"], "/a.pdf");
    tabs.note(["/a.pdf", "/b.pdf"], "/b.pdf");
    await Promise.resolve();
    // The second list waits for the first to be answered.
    expect(sent).toEqual([[["/a.pdf"], "/a.pdf"]]);
    answers[0]?.();
    await new Promise((resolve) => setTimeout(resolve));
    expect(sent).toEqual([[["/a.pdf"], "/a.pdf"], [["/a.pdf", "/b.pdf"], "/b.pdf"]]);
  });

  it("writes when only the tab showing changed, or only the order", async () => {
    const sent: [string[], string | null][] = [];
    const tabs = new TabRecorder(async (paths, active) => { sent.push([paths, active]); });
    tabs.note(["/a.pdf", "/b.pdf"], "/a.pdf");
    tabs.note(["/a.pdf", "/b.pdf"], "/b.pdf");
    tabs.note(["/b.pdf", "/a.pdf"], "/b.pdf");
    tabs.note([], null);
    await tabs.settled();
    expect(sent).toEqual([
      [["/a.pdf", "/b.pdf"], "/a.pdf"],
      [["/a.pdf", "/b.pdf"], "/b.pdf"],
      [["/b.pdf", "/a.pdf"], "/b.pdf"],
      [[], null],
    ]);
  });

  it("writes the two sides with the tabs, and when only they changed", async () => {
    const sent: unknown[] = [];
    const tabs = new TabRecorder(async (paths, active, sides) => { sent.push([paths, active, sides]); });
    const open = ["/a.pdf", "/b.pdf"];
    const right = ["/b.pdf"];
    tabs.note(open, "/a.pdf");
    tabs.note(open, "/a.pdf", { right, beside: "/b.pdf", share: 0.5 });
    tabs.note(open, "/a.pdf", { right, beside: "/b.pdf", share: 0.5 });
    tabs.note(open, "/a.pdf", { right, beside: "/b.pdf", share: 0.4 });
    right.push("/later.pdf");
    tabs.hold();
    tabs.note(open, "/a.pdf", { right: ["/a.pdf"], beside: null, share: 0.4 });
    tabs.release();
    await tabs.settled();
    expect(sent).toEqual([
      [open, "/a.pdf", null],
      [open, "/a.pdf", { right: ["/b.pdf"], beside: "/b.pdf", share: 0.5 }],
      [open, "/a.pdf", { right: ["/b.pdf"], beside: "/b.pdf", share: 0.4 }],
      [open, "/a.pdf", { right: ["/a.pdf"], beside: null, share: 0.4 }],
    ]);
  });

  it("copies the list, so a later change to it is not what gets written", async () => {
    const sent: string[][] = [];
    let release = () => {};
    const tabs = new TabRecorder((paths) => {
      sent.push(paths);
      return new Promise<void>((resolve) => { release = resolve; });
    });
    const live = ["/a.pdf"];
    tabs.note(["/first.pdf"], null);
    tabs.note(live, null);
    live.push("/b.pdf");
    await Promise.resolve();
    release();
    await new Promise((resolve) => setTimeout(resolve));
    expect(sent).toEqual([["/first.pdf"], ["/a.pdf"]]);
  });

  afterEach(() => vi.restoreAllMocks());

  it("keeps writing after a write fails", async () => {
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    const sent: string[][] = [];
    const tabs = new TabRecorder((paths) => {
      sent.push(paths);
      return sent.length === 1 ? Promise.reject(new Error("disk full")) : Promise.resolve();
    });
    tabs.note(["/a.pdf"], null);
    tabs.note(["/b.pdf"], null);
    await expect(tabs.settled()).resolves.toBeUndefined();
    expect(sent).toEqual([["/a.pdf"], ["/b.pdf"]]);
    expect(warned).toHaveBeenCalledWith("could not record the open tabs: Error: disk full");
  });

  it("sends a record again when the write of it failed", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const sent: string[][] = [];
    const tabs = new TabRecorder((paths) => {
      sent.push(paths);
      return sent.length === 1 ? Promise.reject(new Error("disk full")) : Promise.resolve();
    });
    tabs.note(["/a.pdf"], null);
    // Still on its way: the same record noted meanwhile is one write.
    tabs.note(["/a.pdf"], null);
    await tabs.settled();
    expect(sent).toEqual([["/a.pdf"]]);
    // It did not arrive, so the session does not hold it and it is not "the same".
    tabs.note(["/a.pdf"], null);
    await tabs.settled();
    expect(sent).toEqual([["/a.pdf"], ["/a.pdf"]]);
    // That one did arrive, and is not sent a third time.
    tabs.note(["/a.pdf"], null);
    await tabs.settled();
    expect(sent.length).toBe(2);
  });

  it("does not forget a later record because an earlier write failed", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const sent: string[][] = [];
    const tabs = new TabRecorder((paths) => {
      sent.push(paths);
      return sent.length === 1 ? Promise.reject(new Error("disk full")) : Promise.resolve();
    });
    tabs.note(["/a.pdf"], null);
    tabs.note(["/b.pdf"], null);
    await tabs.settled();
    tabs.note(["/b.pdf"], null);
    await tabs.settled();
    expect(sent).toEqual([["/a.pdf"], ["/b.pdf"]]);
  });

  it("writes nothing while a launch is still opening tabs, then the final list", async () => {
    const sent: [string[], string | null][] = [];
    const tabs = new TabRecorder(async (paths, active) => { sent.push([paths, active]); });
    tabs.hold();
    tabs.note(["/b.pdf"], "/b.pdf");
    const growing = ["/b.pdf", "/a.pdf"];
    tabs.note(growing, "/b.pdf");
    growing.push("/later.pdf");
    await tabs.settled();
    expect(sent).toEqual([]);
    tabs.release();
    await tabs.settled();
    expect(sent).toEqual([[["/b.pdf", "/a.pdf"], "/b.pdf"]]);
    // Released, it writes as it goes again; released with nothing noted, nothing.
    tabs.note(["/b.pdf"], "/b.pdf");
    tabs.hold();
    tabs.release();
    await tabs.settled();
    expect(sent).toEqual([[["/b.pdf", "/a.pdf"], "/b.pdf"], [["/b.pdf"], "/b.pdf"]]);
  });
});

describe("tabs from last time that a launch could not open", () => {
  it("are kept in the list written after the restore, at their places", async () => {
    // A tab behind a password, or on a drive not mounted yet, did not open.
    // Before, the release wrote the tabs that had opened as the whole list,
    // and the next launch did not try the other again. Mutation: write `paths`
    // in `note` instead of `withUnopened(...)` -> red.
    const sent: string[][] = [];
    const tabs = new TabRecorder(async (paths) => { sent.push(paths); });
    tabs.hold();
    tabs.note(["/b.pdf", "/c.pdf"], "/b.pdf");
    tabs.keepUnopened(["/a.pdf"], ["/a.pdf", "/b.pdf", "/c.pdf"]);
    tabs.release();
    await tabs.settled();
    expect(sent).toEqual([["/a.pdf", "/b.pdf", "/c.pdf"]]);
    // Still kept when the reader closes a tab later in the run.
    tabs.note(["/c.pdf"], "/c.pdf");
    await tabs.settled();
    expect(sent[1]).toEqual(["/a.pdf", "/c.pdf"]);
  });

  it("is an ordinary tab once it has opened, and goes when it is closed", async () => {
    // Mutation: drop the filter that forgets an unopened tab once it is open -> red.
    const sent: string[][] = [];
    const tabs = new TabRecorder(async (paths) => { sent.push(paths); });
    tabs.keepUnopened(["/a.pdf"], ["/a.pdf", "/b.pdf"]);
    tabs.note(["/b.pdf", "/a.pdf"], "/a.pdf");
    tabs.note(["/b.pdf"], "/b.pdf");
    await tabs.settled();
    expect(sent).toEqual([["/b.pdf", "/a.pdf"], ["/b.pdf"]]);
  });

  it("goes back after the tab before it, or first when none is open", () => {
    const order = ["/a.pdf", "/b.pdf", "/c.pdf", "/d.pdf"];
    expect(withUnopened(["/a.pdf", "/d.pdf"], ["/b.pdf", "/c.pdf"], order))
      .toEqual(["/a.pdf", "/b.pdf", "/c.pdf", "/d.pdf"]);
    expect(withUnopened(["/c.pdf"], ["/a.pdf"], order)).toEqual(["/a.pdf", "/c.pdf"]);
    expect(withUnopened([], ["/b.pdf", "/a.pdf"], order)).toEqual(["/a.pdf", "/b.pdf"]);
    // Control: nothing unopened is the list as it is.
    expect(withUnopened(["/c.pdf", "/a.pdf"], [], order)).toEqual(["/c.pdf", "/a.pdf"]);
  });

  it("are the refused tabs and a first tab that did not open, from the recorded list only", () => {
    const plan = launchPlan(session(), []);
    const open = new Set(["/a.pdf"]);
    // `/b.pdf` was to be shown first and is not open; `/c.pdf` was refused.
    // Mutation: leave `plan.show` out -> red.
    expect(unopenedAtLaunch(plan, ["/c.pdf"], (path) => open.has(path))).toEqual(["/b.pdf", "/c.pdf"]);
    // Control: everything opened.
    expect(unopenedAtLaunch(plan, [], () => true)).toEqual([]);
    // A file handed over reports its own failure, and the last document resumed
    // alone was never a list of tabs. Mutation: drop `plan.order.includes(path)` -> red.
    const handed = launchPlan(session({ restore_tabs: false }), ["/new.pdf"]);
    expect(unopenedAtLaunch(handed, [], () => false)).toEqual([]);
    const alone = launchPlan(session({ restore_tabs: false }), []);
    expect(unopenedAtLaunch(alone, [], () => false)).toEqual([]);
  });

  it("are said, by name for one and counted for several", () => {
    const name = (path: string) => path.slice(1);
    expect(afterLaunch([], name)).toBeNull();
    expect(afterLaunch(["/a.pdf"], name)).toContain("a.pdf");
    expect(afterLaunch(["/a.pdf"], name)).toContain("tried again");
    expect(afterLaunch(["/a.pdf", "/b.pdf", "/c.pdf"], name)).toContain("a.pdf and 2 more");
  });
});
