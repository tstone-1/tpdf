import { describe, expect, it } from "vitest";
import {
  DocumentTabs, DocumentTasks, RESTORE_POINTS, freshState, keepScope, keepState, oneEach, partnersOf, restore,
  restoredState, restoredWith, scopeToRestore, twinsOf,
  type DocumentTab, type FreshState, type Restore, type TabState,
} from "./documenttabs";
import { pageId } from "./pages";
import app from "../App.svelte?raw";
import { functionIn, missingFrom } from "./sourcetext";
import { pickPartner } from "./panes";
import { PLAIN_SEARCH } from "./search";
import { handleWindowKey, registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { matches } from "./keys";

const tab = (id: number, path = `/fixture/${id}.pdf`) => ({ view: id, doc: { id }, path, edits: [id], page: id });

describe("document tabs", () => {
  it("keeps independent edit journals and positions across switches", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1));
    tabs.keep(tab(2));
    tabs.find(1)!.edits.push(10);
    tabs.find(1)!.page = 7;
    tabs.active = 1;
    expect(tabs.neighbour(1)?.doc.id).toBe(2);
    expect(tabs.neighbour(-1)?.doc.id).toBe(2);
    expect(tabs.find(1)).toMatchObject({ page: 7, edits: [1, 10] });
    expect(tabs.find(2)).toMatchObject({ page: 2, edits: [2] });
  });

  it("reuses Windows path spellings but distinguishes POSIX case", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1, "C:\\fixture\\Example.pdf"));
    tabs.keep(tab(2, "/fixture/Example.pdf"));
    expect(tabs.forPath("c:/fixture/example.pdf")?.doc.id).toBe(1);
    expect(tabs.forPath("/fixture/example.pdf")).toBeUndefined();
    expect(tabs.forPath("/fixture/Example.pdf")?.doc.id).toBe(2);
  });

  it("finds a document on a network share however the share is spelt", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1, "\\\\server\\share\\Example.pdf"));
    expect(tabs.forPath("\\\\SERVER\\Share\\example.pdf")?.doc.id).toBe(1);
    expect(tabs.forPath("\\\\server\\share/Example.pdf")?.doc.id).toBe(1);
    expect(tabs.forPath("\\\\server\\other\\Example.pdf")).toBeUndefined();
  });

  it("names a document shown twice by the view asked for, else by the first", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    const first = tab(1);
    const second = { ...tab(1), view: 900 };
    tabs.keep(tab(2)); tabs.keep(first); tabs.keep(tab(3));
    tabs.addAfter(second, 1);
    expect(tabs.all.map((entry) => entry.view)).toEqual([2, 1, 900, 3]);
    // Added behind: the tab in front is the one that was.
    expect(tabs.active).toBe(3);
    expect(tabs.forPath("/fixture/1.pdf")?.view).toBe(1);
    expect(tabs.forPath("/fixture/1.pdf", 900)?.view).toBe(900);
    // A view of another file is no reason to answer anything but the first.
    expect(tabs.forPath("/fixture/1.pdf", 3)?.view).toBe(1);
    expect(tabs.find(900)).toBe(second);
    expect(tabs.find(1)).toBe(first);
    // Twice is once, and a tab that is gone puts the new one last.
    tabs.addAfter(second, 2);
    tabs.addAfter({ ...tab(5), view: 901 }, 77);
    expect(tabs.all.map((entry) => entry.view)).toEqual([2, 1, 900, 3, 901]);
    // Removing one view leaves the other, and the row in order.
    tabs.remove(900);
    expect(tabs.all.map((entry) => entry.view)).toEqual([2, 1, 3, 901]);
    expect(tabs.forPath("/fixture/1.pdf", 900)?.view).toBe(1);
  });

  it("finds the other tabs of a document by its edit model, and counts it once", () => {
    const model = { pages: 3 };
    const other = { pages: 3 };
    const tabs = [
      { name: "a", edits: model }, { name: "b", edits: other }, { name: "a again", edits: model },
    ];
    const [a, b, again] = tabs as [typeof tabs[0], typeof tabs[0], typeof tabs[0]];
    expect(twinsOf(tabs, a)).toEqual([again]);
    expect(twinsOf(tabs, again)).toEqual([a]);
    // Equal is not the same: two documents with the same pages are two.
    expect(twinsOf(tabs, b)).toEqual([]);
    expect(oneEach(tabs)).toEqual([a, b]);
    expect(oneEach([])).toEqual([]);
  });

  it("moves a tab to either end and leaves the one in front in front", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    tabs.moveTo(3, "start");
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([3, 1, 2]);
    tabs.moveTo(3, "end");
    tabs.moveTo(1, "end");
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([2, 3, 1]);
    tabs.moveTo(9, "start");
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([2, 3, 1]);
    expect(tabs.active).toBe(3);
  });

  it("replaces a saved handle in its original position", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    tabs.keep(tab(4, "/fixture/2.pdf"), 2);
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([1, 4, 3]);
    expect(tabs.find(2)).toBeUndefined();
    expect(tabs.active).toBe(4);
  });

  it("closes a background tab without moving focus and chooses a neighbour for the active tab", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    tabs.active = 3;
    expect(tabs.remove(1)?.doc.id).toBe(1);
    expect(tabs.active).toBe(3);
    tabs.active = 2;
    tabs.remove(2);
    expect(tabs.active).toBe(3);
    tabs.remove(3);
    expect(tabs.active).toBe(-1);
    expect(tabs.neighbour(1)).toBeUndefined();
    expect(tabs.remove(999)).toBeUndefined();
  });

  it("adds a tab behind the one showing, and only once", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.add(tab(1));
    expect(tabs.active).toBe(-1);
    tabs.keep(tab(2));
    tabs.add(tab(3));
    tabs.add(tab(3));
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([1, 2, 3]);
    expect(tabs.active).toBe(2);
  });

  it("puts the tabs back in the order they were closed in", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    for (const id of [5, 3, 1, 4, 2]) tabs.keep(tab(id));
    tabs.active = 3;
    // 5 and 4 were not open last time: they follow, in the order they have.
    tabs.arrange(["/fixture/1.pdf", "/fixture/2.pdf", "/fixture/gone.pdf", "/fixture/3.pdf"]);
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([1, 2, 3, 5, 4]);
    expect(tabs.active).toBe(3);
    tabs.arrange([]);
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([1, 2, 3, 5, 4]);
  });

  it("wraps navigation and chooses the left tab when the last tab closes", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    expect(tabs.neighbour(1)?.doc.id).toBe(1);
    tabs.active = 1;
    expect(tabs.neighbour(-1)?.doc.id).toBe(3);
    tabs.active = 3;
    tabs.remove(3);
    expect(tabs.active).toBe(2);
  });
});

describe("the documents that can be shown beside this one", () => {
  const view = (id: number, path: string, edits: unknown[]) => ({ view: id, path, edits });

  it("lists a document on both sides once, by the view already across", () => {
    const a = view(1, "/fixture/A.pdf", [1]);
    const model = [2];
    const b = view(2, "/fixture/B.pdf", model);
    const bAgain = view(9, "/fixture/B.pdf", model);
    const tabs = [a, b, bAgain];
    const partners = partnersOf(tabs, 1, (tab) => tab === bAgain);
    expect(partners).toEqual([{ tab: bAgain, name: "B.pdf" }]);
    // Which is what lets the question be answered: with both views listed it
    // could not be (`panes.test.ts`, two documents with one name).
    const names = partners.map((partner) => partner.name);
    expect(pickPartner("b.pdf", names)).toEqual({ index: 0 });
    // With neither view across, the first tab in the row stands for it.
    expect(partnersOf(tabs, 1, () => false).map((partner) => partner.tab)).toEqual([b]);
  });

  it("leaves out the view the reader is in and keeps its second view", () => {
    const model = [1];
    const a = view(1, "/fixture/A.pdf", model);
    const aAgain = view(9, "/fixture/A.pdf", model);
    const b = view(2, "/fixture/B.pdf", [2]);
    expect(partnersOf([a, aAgain, b], 1, () => true).map((partner) => partner.tab)).toEqual([aAgain, b]);
    expect(partnersOf([a], 1, () => true)).toEqual([]);
  });

  it("keeps the tab row's order, and tells two files with one name apart", () => {
    const first = view(1, "/fixture/2024/report.pdf", [1]);
    const second = view(2, "/fixture/2025/report.pdf", [2]);
    const third = view(3, "/fixture/notes.pdf", [3]);
    const here = view(4, "/fixture/here.pdf", [4]);
    const partners = partnersOf([first, here, second, third], 4, (tab) => tab === third);
    expect(partners.map((partner) => partner.tab)).toEqual([first, second, third]);
    const names = partners.map((partner) => partner.name);
    expect(names).toEqual(["2024/report.pdf", "2025/report.pdf", "notes.pdf"]);
    expect(pickPartner("2025", names)).toEqual({ index: 1 });
  });
});

describe("document tasks", () => {
  it("holds a transition until a save and its reopen finish", async () => {
    let finish!: () => void;
    const events: string[] = [];
    const tasks = new DocumentTasks();
    const save = tasks.run(async () => {
      events.push("save");
      await new Promise<void>((resolve) => { finish = resolve; });
      events.push("reopen");
    });
    const switchTab = tasks.idle().then(() => events.push("switch"));
    await Promise.resolve();
    expect(events).toEqual(["save"]);
    expect(tasks.busy).toBe(true);
    finish();
    await Promise.all([save, switchTab]);
    expect(events).toEqual(["save", "reopen", "switch"]);
    expect(tasks.busy).toBe(false);
  });

  it("releases waiters after failure and refuses an overlapping write", async () => {
    const states: boolean[] = [];
    const tasks = new DocumentTasks((busy) => states.push(busy));
    const first = tasks.run(async () => { throw new Error("fixture refusal"); });
    let second = false;
    const overlap = tasks.run(async () => { second = true; });
    await expect(first).rejects.toThrow("fixture refusal");
    await expect(overlap).rejects.toThrow("fixture refusal");
    await tasks.idle();
    expect(second).toBe(false);
    expect(states).toEqual([true, false]);
    await tasks.run(async () => { second = true; });
    expect(second).toBe(true);
  });
});

describe("tab commands", () => {
  it("routes shortcuts and palette commands to the tab actions", () => {
    const calls: string[] = [];
    const actions = {
      viewer: () => ({}), busyOpening: () => false, documentCount: () => 2,
      closeDocument: () => calls.push("close"),
      nextDocument: (delta: number) => calls.push(String(delta)),
    } as unknown as AppActions;
    const registry = new CommandRegistry();
    registerAppCommands(registry, actions);
    registry.run("file.close"); registry.run("view.nextTab"); registry.run("view.previousTab");
    const key = (key: string, shiftKey = false) => ({
      key, ctrlKey: true, metaKey: false, altKey: false, shiftKey,
      preventDefault() {},
    }) as KeyboardEvent;
    const deps = { actions, palette: () => null, hasDocument: () => true, refreshRecents() {} };
    handleWindowKey(key("w"), deps);
    handleWindowKey(key("Tab"), deps);
    handleWindowKey(key("Tab", true), deps);
    expect(calls).toEqual(["close", "1", "-1", "close", "1", "-1"]);
    actions.busyOpening = () => true;
    handleWindowKey(key("w"), deps);
    expect(calls).toHaveLength(6);
    expect(matches("view.nextTab", { ...key("Tab"), ctrlKey: false, metaKey: true } as KeyboardEvent)).toBe(false);
  });
});

describe("a search confined to a selection, kept by a tab", () => {
  const pages = (...ids: number[]) => ids.map((id) => ({ id: pageId(id) }));
  const ranges = [{ page: 1, from: 4, to: 9 }, { page: 2, from: 0, to: 3 }];

  it("keeps nothing for a search of the whole document", () => {
    expect(keepScope(null, pages(1, 2, 3))).toBeNull();
    expect(scopeToRestore(null, pages(1, 2, 3))).toBeNull();
  });

  it("puts the scope back while the pages are the ones it was kept with", () => {
    const kept = keepScope(ranges, pages(1, 2, 3));
    expect(kept?.order).toEqual([1, 2, 3]);
    // Other objects with the same ids: a turn or a crop makes a new page
    // record and moves no page.
    expect(scopeToRestore(kept, pages(1, 2, 3))).toBe(ranges);
  });

  it("drops the scope when a page went, came or moved while the tab was away", () => {
    // The document's other view did it, and a tab without a viewer was not
    // told: slot 1 and slot 2 hold other pages in each of these.
    const kept = keepScope(ranges, pages(1, 2, 3));
    // A move, which leaves the count as it was.
    expect(scopeToRestore(kept, pages(3, 1, 2))).toBeNull();
    // A deletion after the scope's pages, where every id left is in its slot.
    expect(scopeToRestore(kept, pages(1, 2))).toBeNull();
    // An insertion at the end, the mirror of it.
    expect(scopeToRestore(kept, pages(1, 2, 3, 4))).toBeNull();
    // One page replaced by another in the same slot.
    expect(scopeToRestore(kept, pages(1, 5, 3))).toBeNull();
  });

  // Read as text, since nothing imports `App.svelte`: that the lines are
  // there, and not that they run (`sourcetext.ts`).
  it("is kept with the document's pages and put back through them, in the window", () => {
    expect(missingFrom(functionIn(app, "function keepActiveTab("), [
      "searchScope: keepScope(viewer.searchScopeRanges, edits.state.pages),",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "const restoring: Restore = {"), [
      "searchOptions: (value, kept) => viewer?.restoreSearch(kept.query, value, scopeToRestore(kept.searchScope, edits?.state.pages ?? [])),",
    ])).toEqual([]);
  });
});

describe("what a tab keeps across a switch", () => {
  /** A value for every kept field, none of them a default. */
  const touched = (): TabState => ({
    edits: { journal: "edited" } as unknown as TabState["edits"],
    place: { path: "/fixture/1.pdf", page: 4 } as unknown as TabState["place"],
    covered: new Map([[3, "kept words"]]),
    query: "needle",
    findShown: true,
    searchOptions: { ...PLAIN_SEARCH, matchCase: true },
    searchScope: [{ from: 1, to: 2 }] as unknown as TabState["searchScope"],
    sidebarTab: "comments",
    error: "could not render page 2",
    offers: [{ kind: "recover" }] as unknown as TabState["offers"],
    notice: "saved",
    redactedCopyPath: "/fixture/1-redacted.pdf",
  });
  const blank = () => ({ doc: { id: 1 }, path: "/fixture/1.pdf" }) as unknown as DocumentTab;

  it("restores every field that was kept, and nothing from another tab", () => {
    const tab = blank();
    const state = touched();
    keepState(tab, state);
    const { edits: _edits, place: _place, ...restorable } = state;
    expect(restoredState(tab)).toEqual(restorable);
    expect(tab.edits).toBe(state.edits);
    expect(tab.place).toBe(state.place);
  });

  it("restores the fresh state for a document that was never kept", () => {
    expect(restoredState(undefined)).toEqual(freshState());
    expect(freshState()).toMatchObject({
      query: "", findShown: false, searchOptions: PLAIN_SEARCH, searchScope: null,
      sidebarTab: "outline", error: null, offers: [], notice: null, redactedCopyPath: null,
    });
    expect(freshState().covered.size).toBe(0);
  });

  it("hands every fresh document its own containers", () => {
    // Shared ones would put one tab's covered words on the next tab's rows.
    const one = freshState();
    const two = freshState();
    expect(one.covered).not.toBe(two.covered);
    expect(one.offers).not.toBe(two.offers);
  });

  /** An applier for every field that records which were handed over, and with what. */
  function recording() {
    const applied: [string, unknown][] = [];
    const seen: FreshState[] = [];
    const note = (key: keyof FreshState) => (value: unknown, kept: FreshState) => {
      applied.push([key, value]);
      seen.push(kept);
    };
    const apply: Restore = {
      covered: note("covered"), query: note("query"), findShown: note("findShown"),
      searchOptions: note("searchOptions"), searchScope: note("searchScope"),
      sidebarTab: note("sidebarTab"), error: note("error"), offers: note("offers"),
      notice: note("notice"), redactedCopyPath: note("redactedCopyPath"),
    };
    return { applied, seen, apply };
  }

  it("applies every restored field exactly once across the three points of an open", () => {
    const tab = blank();
    keepState(tab, touched());
    const kept = restoredState(tab);
    const { applied, seen, apply } = recording();
    for (const point of RESTORE_POINTS) restore(kept, point, apply);
    expect(applied.map(([key]) => key).sort()).toEqual(Object.keys(kept).sort());
    // Each with its own value, and the whole record beside it for a field
    // that is shown together with another.
    for (const [key, value] of applied) expect(value).toBe(kept[key as keyof FreshState]);
    expect(seen.every((record) => record === kept)).toBe(true);
  });

  it("applies each field at its point, in the order the open depends on", () => {
    const kept = restoredState(undefined);
    const at = (point: (typeof RESTORE_POINTS)[number]) => {
      const { applied, apply } = recording();
      restore(kept, point, apply);
      return applied.map(([key]) => key);
    };
    // The message before the copy it clears the offer of: `say` resets it.
    expect(at("unmounted")).toEqual(["query", "findShown", "error", "offers", "notice", "redactedCopyPath"]);
    expect(at("model")).toEqual(["covered"]);
    expect(at("mounted")).toEqual(["searchOptions", "searchScope", "sidebarTab"]);
  });

  it("names the field another is applied with, and does nothing for it", () => {
    const { applied, apply } = recording();
    apply.offers = restoredWith("error");
    restore(restoredState(undefined), "unmounted", apply);
    expect(applied.map(([key]) => key)).not.toContain("offers");
    expect(applied.map(([key]) => key)).toContain("error");
  });
});
