/**
 * What an open decides, and that `openDocument` in `App.svelte` asks here.
 *
 * Every test below was checked by mutating `documentopen.ts` or `App.svelte`
 * and seeing it fail; the mutations are in `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import {
  abandonedBy, documentsBusy, dropAbandoned, dropView, openFailure, pageTable, keepSavedPlaces, placeOnSide, placeToResume, readerIsIn, repoint, retainedPlace, sharedByTwin,
  tabToKeep, toldAfterFallback,
  type OpenFound,
} from "./documentopen";
import {
  DocumentTabs, freshState, keepScope, keepState, restoredState, scopeToRestore, type DocumentTab,
} from "./documenttabs";
import type { Edits } from "./edits";
import type { DocumentInfo } from "./ipc";
import { pageId } from "./pages";
import { Panes } from "./panes";
import type { Place } from "./session";
import { functionIn, missingFrom, withoutComments } from "./sourcetext";

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

it("uses the settled session on a fresh open and the same open path for closed tabs", () => {
  const open = functionIn(app, "async function openDocument(");
  expect(open).toMatch(/places\.flush\(\);[\s\S]*await places\.settled\(\);\s*remembered = \(await loadSession\(\)\)\.places;/);
  expect(open).toMatch(/await loadSession\(\)[\s\S]*settlingOpen = !discard;[\s\S]*await settleDocument\([\s\S]*keepActiveTab\(\);[\s\S]*unmountDocument\(\);/);
  expect(app).toContain("inert={settlingOpen || updateExitHeld}");
  expect(functionIn(app, "async function reopenClosedTab(")).toContain("await closedTabs.reopen(isOpenTab, openPath)");
});

it("applies an external location to the view found by the shared path lookup", () => {
  expect(functionIn(app, "function placeOpened(")).toContain("(await matchingTab(given.path, view))?.view !== view");
});

describe("the other views of a document that was saved", () => {
  it("are pointed at the new handle and the new model, every one of them", () => {
    const twin = () => ({ doc: "old", edits: 1, path: "/a.pdf", ...freshState(), place: null });
    const twins = [twin(), twin()];
    repoint(twins, "new", 2);
    expect(twins).toEqual([{ ...twin(), doc: "new", edits: 2 }, { ...twin(), doc: "new", edits: 2 }]);
  });

  it("move a comparison about the document to its new view, out of date", () => {
    // The rule is `ComparisonState.reopened` (`comparison.test.ts`); this is
    // the call in the open, read as text. Mutation: drop it -> red.
    expect(missingFrom(functionIn(app, "async function openDocument("), [
      "if (replacing && comparisons.reopened(reopenedViews(replacing.view, view, twins))) refreshComparison();",
    ])).toEqual([]);
  });
});

describe("the tabs from last time a launch could not open", () => {
  it("are kept and said before the recorder is released", () => {
    // The rules are `unopenedAtLaunch` and `TabRecorder.keepUnopened`
    // (`tabrestore.test.ts`); these are their calls in the launch.
    expect(missingFrom(app, [
      "if (await openPath(path, plan.resuming) === null) refusedFront.push(path);",
      "const unopened = unopenedAtLaunch(session, [...refusedFront, ...refused], isOpenTab);",
      "tabRecorder.keepUnopened(unopened, plan.order);",
      ".then(async (refused) => { await restoreSides(plan.sides); unopenedTabs(refused); })",
      ".finally(() => { tabRecorder.release(); refreshTabs(); refreshMenu(); });",
    ])).toEqual([]);
  });
});

describe("a search confined to a selection, when the file is opened again", () => {
  const ranges = [{ page: 1, from: 4, to: 9 }];
  const pages = (...ids: number[]) => ids.map((id) => ({ id: pageId(id) }));
  // Two models of one file with as many pages: the ids start again with the
  // model, so both number their pages alike.
  const before = {} as Edits;
  const after = {} as Edits;
  const info = (id: number) => ({ id, page_count: 3 }) as DocumentInfo;
  /** A tab on the old model, left with a search of "clause" inside a selection. */
  const scoped = (view: number): DocumentTab => ({
    view: view as DocumentTab["view"], doc: info(1), path: "/a.pdf", edits: before, place: null,
    ...freshState(), query: "clause", searchScope: keepScope(ranges, pages(1, 2, 3)),
  });
  const found = (more: Partial<OpenFound>): OpenFound => ({
    retained: undefined, replacing: undefined, replaceId: undefined, twins: [], resume: null,
    kept: freshState(), ...more,
  });

  it("remounts a hidden view on the same page after a deletion in its twin", () => {
    const tab = scoped(3);
    const state = { pages: pages(1, 2, 3, 4) };
    keepState(tab, { ...tab, edits: { state } as unknown as Edits, place: place(tab.path, 2, { top_pt: 75 }) });
    state.pages.shift();
    expect(retainedPlace(tab)).toMatchObject({ page: 1, top_pt: 75 });
  });

  it("restores both parked twins to their saved pages and forgets words from the replaced model", () => {
    const tabs = [scoped(3), scoped(4)];
    const state = { pages: pages(1, 2, 3) };
    const old = { state } as unknown as Edits;
    for (const [index, tab] of tabs.entries()) {
      keepState(tab, { ...tab, edits: old, place: place(tab.path, index * 2, { top_pt: 75 }) });
      tab.covered.note(1, "Old highlight");
      expect(restoredState(tab).covered.of(1)).toBe("Old highlight");
    }
    state.pages.reverse();
    keepSavedPlaces(tabs, old);
    repoint(tabs, info(2), after);
    expect(tabs.map((tab) => retainedPlace(tab)?.page)).toEqual([2, 0]);
    for (const tab of tabs) {
      expect(tab.place?.top_pt).toBe(75);
      expect(tab.place?.pageId).toBeUndefined();
      // The next model can issue mark 1 again.
      expect(restoredState(tab).covered.of(1)).toBe("");
    }
  });

  it("rebases the saved order only after a successful in-place write", () => {
    for (const method of ["saveDocument", "redactAnyway"]) {
      const body = functionIn(app, `async function ${method}(`);
      expect(body).toMatch(/return;\s*}\s*await rebaseAfterSave\(path, model\);/);
      const afterRebase = body.slice(body.indexOf("rebaseAfterSave(path, model);"));
      if (method === "saveDocument") expect(afterRebase).toContain("await reopenAfterWrite(");
      else {
        expect(afterRebase).toContain("await afterRedactionReopen(result.message, result.success, reopen)");
        expect(afterRebase).toContain("say(report.message, [], resultTone(report.success))");
      }
    }
    expect(functionIn(app, "function rebaseAfterSave(")).toContain("keepSavedPlaces(tabs.all, model)");
  });

  it("is gone from the tab a save or a reload opens the file again in", () => {
    const old = scoped(1);
    // The control, and the defect: by the page ids alone the scope goes back.
    expect(scopeToRestore(old.searchScope, pages(1, 2, 3))).toBe(ranges);
    const tab = tabToKeep(2 as DocumentTab["view"], info(2), "/a.pdf", after, found({ replacing: old, replaceId: 1 }));
    expect(tab).not.toBe(old);
    expect(tab.edits).toBe(after);
    expect(tab.searchScope).toBeNull();
    expect(scopeToRestore(restoredState(tab).searchScope, pages(1, 2, 3))).toBeNull();
  });

  it("is gone from the document's other views too, and their words stay", () => {
    const twin = scoped(3);
    expect(scopeToRestore(twin.searchScope, pages(1, 2, 3))).toBe(ranges);
    const line = twin.measures.add(pageId(1), { x: 0, y: 0 }, { x: 10, y: 0 });
    twin.measures.setScale(line, { value: 3, unit: "mm", decimals: 1 });
    expect(restoredState(twin).measures.scaled(pageId(1))).toBe(true);
    repoint([twin], info(2), after);
    expect(twin.measures.scaled(pageId(1))).toBe(false);
    expect(twin.measures.count).toBe(0);
    expect(twin.edits).toBe(after);
    expect(twin.searchScope).toBeNull();
    // What mounting the twin again reads back.
    expect(scopeToRestore(restoredState(twin).searchScope, pages(1, 2, 3))).toBeNull();
    expect(restoredState(twin).query).toBe("clause");
  });

  it("stays with a tab the reader switched away from and came back to, on the model it had", () => {
    const tab = scoped(1);
    const kept = tabToKeep(tab.view, tab.doc, tab.path, tab.edits, found({ retained: tab, kept: restoredState(tab) }));
    expect(kept).toBe(tab);
    expect(kept.edits).toBe(before);
    expect(scopeToRestore(restoredState(kept).searchScope, pages(1, 2, 3))).toBe(ranges);
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

describe("the views a failed open leaves without a document", () => {
  /**
   * A document shown twice beside another one: `other` and the first view on
   * the left, the second view on the right with the reader in it.
   */
  const shownTwice = () => {
    const tabs = new DocumentTabs<{ view: number; path: string }>();
    for (const view of [1, 2, 3]) tabs.keep({ view, path: view === 1 ? "/other.pdf" : "/twice.pdf" });
    const panes = new Panes<number>();
    panes.fronted(2);
    panes.move(3, "right", [1, 2, 3]);
    return { tabs, panes, twin: tabs.find(2)! };
  };
  /** What the row would draw, each time it is drawn. */
  const rowOf = (tabs: DocumentTabs<{ view: number; path: string }>) => {
    const drawn: number[][] = [];
    return { drawn, refresh: () => { drawn.push(tabs.all.map((tab) => tab.view)); } };
  };

  it("are gone from the row when it is drawn, which is once and after the last of them", () => {
    const { tabs, panes, twin } = shownTwice();
    // A reload got as far as its new handle taking the tab over, and failed.
    tabs.keep({ view: 9, path: "/twice.pdf" }, 3);
    panes.replaced(3, 9);
    const row = rowOf(tabs);
    const dropped: number[] = [];
    dropAbandoned(panes, tabs, { own: 9, replaced: true, twins: [twin] }, row.refresh, (id) => dropped.push(id));
    expect(dropped).toEqual([9, twin.view]);
    expect(row.drawn).toEqual([[1]]);
    expect(panes.split).toBe(false);
    expect(panes.front("left")).toBe(1);
  });

  it("are the other views alone when the handle was a tab's own, and the row is drawn then too", () => {
    const { tabs, panes, twin } = shownTwice();
    const row = rowOf(tabs);
    dropAbandoned(panes, tabs, { own: undefined, replaced: true, twins: [twin] }, row.refresh);
    expect(row.drawn).toEqual([[1, 3]]);
    expect(panes.sideOf(3)).toBe("right");
  });

  it("do not include the other views of a document that was never torn down", () => {
    const { tabs, panes, twin } = shownTwice();
    const row = rowOf(tabs);
    // The handle is one the row never held: the open failed before its tab.
    dropAbandoned(panes, tabs, { own: 9, replaced: false, twins: [twin] }, row.refresh);
    expect(row.drawn).toEqual([[1, 2, 3]]);
    expect(panes.split).toBe(true);
  });
});

describe("a reopen after a write that fails", () => {
  /** `/twice.pdf` shown as tabs 2 and 3, beside `/other.pdf` as tab 1; one model each. */
  const written = () => {
    const tabs = new DocumentTabs<{ view: number; path: string; edits: string }>();
    tabs.keep({ view: 1, path: "/other.pdf", edits: "other" });
    tabs.keep({ view: 2, path: "/twice.pdf", edits: "twice" });
    tabs.keep({ view: 3, path: "/twice.pdf", edits: "twice" });
    const panes = new Panes<number>();
    panes.fronted(2);
    panes.move(3, "right", [1, 2, 3]);
    return { tabs, panes, tab: tabs.find(2)!, twin: tabs.find(3)! };
  };

  it("a reopen that fails after a write drops the view", () => {
    // The save closed the handle and the window forgot it; the open then
    // failed at its first step, before tearing anything down itself.
    const { tabs, panes, tab, twin } = written();
    const left = abandonedBy(tabs.all, tab, false, []);
    expect(left).toEqual({ replaced: true, twins: [tab, twin] });
    dropAbandoned(panes, tabs, { own: undefined, ...left }, () => {});
    expect(tabs.all.map((each) => each.view)).toEqual([1]);
    expect(panes.split).toBe(false);
    expect(panes.front("left")).toBe(1);
  });

  it("takes out the written tab too when it failed after its own teardown", () => {
    // A failed reload mounts its tab again on the handle it still has; a
    // write's handle is closed, so its tab goes with the other views.
    const { tabs, tab, twin } = written();
    expect(abandonedBy(tabs.all, tab, true, [twin])).toEqual({ replaced: true, twins: [tab, twin] });
  });

  it("does not take out a tab the open already took out of the row", () => {
    const { tabs, tab, twin } = written();
    tabs.remove(2);
    expect(abandonedBy(tabs.all, tab, true, [twin])).toEqual({ replaced: true, twins: [twin] });
  });

  it("is any other open left as it recorded itself", () => {
    const { tabs, twin } = written();
    expect(abandonedBy(tabs.all, undefined, false, [])).toEqual({ replaced: false, twins: [] });
    expect(abandonedBy(tabs.all, undefined, true, [twin])).toEqual({ replaced: true, twins: [twin] });
  });

  it("is named and taken down by the open, and only a write's reopen skips the wait for tasks", () => {
    expect(missingFrom(functionIn(app, "async function reopenAfterWrite("), [
      "await openPath(path, false, place, true);",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function openPath("), [
      "const ready = afterWrite ? Promise.resolve() : documentTasks.idle();",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function abandonOpen("), [
      "for (const twin of twins) { asDocument(twin.view, () => unmountDocument()); panes.unmounted(twin.view); }",
    ])).toEqual([]);
  });
});

/**
 * The tasks that settle nothing first, by the function that runs them, and
 * why. Every other `documentTasks.run(` commits what the reader is still
 * typing before its first wait: a form answer commits when its control loses
 * the keyboard, a task's first wait is usually a panel that takes it, and the
 * commit then lands inside the running task, where `applyEdit` refuses it in
 * silence. Print, Save a copy, a redacted copy, Extract, Split and Merge each
 * left it out, so the answer on screen was missing from the paper or the file.
 */
const SETTLES_NOTHING: Record<string, string> = {
  compareWith: "`moveTab` settled both documents just before the task, and a comparison writes nothing",
  fromPictures: "needs no open document and reads none: it builds a new file from pictures",
};

/** Each `documentTasks.run(` in `code`, with the function it is in and its argument. */
function tasksIn(code: string): { name: string; body: string }[] {
  const found: { name: string; body: string }[] = [];
  const opener = "documentTasks.run(";
  for (let at = code.indexOf(opener); at >= 0; at = code.indexOf(opener, at + 1)) {
    const named = [...code.slice(0, at).matchAll(/function (\w+)\(/g)].at(-1);
    let depth = 0;
    let end = at + opener.length - 1;
    for (; end < code.length; end++) {
      if (code[end] === "(") depth++;
      else if (code[end] === ")" && --depth === 0) break;
    }
    found.push({ name: named?.[1] ?? "", body: code.slice(at + opener.length, end) });
  }
  return found;
}

/** Whether `body` settles the drafts before, or as part of, its first wait. */
function settlesFirst(body: string): boolean {
  const settle = body.indexOf("settleDrafts");
  if (settle < 0) return false;
  const wait = body.search(/\bawait\b/);
  if (wait < 0 || settle < wait) return true;
  // `await settleDrafts()` itself, or `await signing.signDocument({ settle:
  // settleDrafts, ... })`, which hands it to the call it waits on.
  const between = body.slice(wait, settle);
  return between.trim() === "await" || (!between.includes(";") && between.includes("settle:"));
}

describe("the document tasks in App.svelte", () => {
  const tasks = tasksIn(withoutComments(app));

  it("are found, so the rule below is about something", () => {
    // Fifteen and more on the day this was written; a parser that found none
    // would pass every task.
    expect(tasks.length).toBeGreaterThanOrEqual(15);
    expect(tasks.every((task) => task.name !== "")).toBe(true);
  });

  it("settle the drafts before their first wait", () => {
    const unsettled = tasks
      .filter((task) => !(task.name in SETTLES_NOTHING) && !settlesFirst(task.body))
      .map((task) => task.name);
    expect(unsettled).toEqual([]);
  });

  it("name no exemption that is not a task", () => {
    const names = new Set(tasks.map((task) => task.name));
    expect(Object.keys(SETTLES_NOTHING).filter((name) => !names.has(name))).toEqual([]);
  });

  it("reads a task that waits before settling as one that does not settle", () => {
    expect(settlesFirst("async () => { const chosen = await saveDialog({}); await settleDrafts(); }")).toBe(false);
    expect(settlesFirst("async () => { await print(); }")).toBe(false);
    expect(settlesFirst("async () => { await settleDrafts(); await saveDialog({}); }")).toBe(true);
    expect(settlesFirst("async () => { const said = await flow({ settle: settleDrafts, dirty }); }")).toBe(true);
    expect(settlesFirst("async () => { await other(); const s = await flow({ settle: settleDrafts }); }")).toBe(false);
  });
});

describe("the comment-word walk in App.svelte", () => {
  it("hands the walk this document's comments, its set and its viewer", () => {
    // The walk and the set it keeps are `commentwalk.ts`, tested there.
    expect(missingFrom(functionIn(app, "async function fillCommentWords("), [
      "const id = openView, reader = viewer, walk = commentWords;",
      "await walk.fill({",
      "comments: () => asDocument(id, () => rawComments === source ? commentsIn(source.items, edits?.map ?? NO_PAGES, edits?.state.notes) : null) ?? null,",
      "if (rawComments !== source) return false;",
      "if (words.size > 0) sidebar?.setCommentWords(words);",
    ])).toEqual([]);
  });
});

describe("whether anything may be started on a document", () => {
  const idle = { opening: false, documentBusy: false, copyTaskBusy: false };

  it("is refused while an open, a task or a hold, or a copy task is running", () => {
    expect(documentsBusy({ ...idle, opening: true })).toBe(true);
    expect(documentsBusy({ ...idle, documentBusy: true })).toBe(true);
    expect(documentsBusy({ ...idle, copyTaskBusy: true })).toBe(true);
  });

  it("is allowed when none is", () => {
    expect(documentsBusy(idle)).toBe(false);
  });

  it("greys the menu, the message bar's buttons and refuses a reload by that one answer", () => {
    expect(missingFrom(app, [
      "const busyNow = $derived(documentsBusy({ opening, documentBusy, copyTaskBusy }));",
      "busyDocument: () => busyNow,",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function reloadAnyway("), ["if (!path || busyNow) return;"])).toEqual([]);
    expect(missingFrom(functionIn(app, "function focusSide("), ["if (busyNow || !stage.parked.includes(id)) return false;"]))
      .toEqual([]);
    for (const offer of ["open-redacted-copy", "offer-saveCopy", "offer-reload", "offer-rasterCopy", "offer-redact"]) {
      expect(missingFrom(app, [`<button data-testid="${offer}" disabled={busyNow}`])).toEqual([]);
    }
  });

  it("names the absence of a view by NO_VIEW, never by null", () => {
    // A view is a number: a comparison with null is always true, and "Close
    // other tabs" with no view in front counted every tab as another.
    expect(withoutComments(app)).not.toMatch(/\b(openView|from)\s*[!=]==?\s*null\b/);
  });

  it("is read off the one `opening` flag, which no local or parameter hides", () => {
    // `const opening = adoptModel(...)` inside `openDocument`'s try made the
    // flag unreadable in the rest of that block.
    const code = withoutComments(app);
    expect(code.match(/\b(?:const|let|var)\s+opening\b/g)).toEqual(["let opening"]);
    expect(code).not.toMatch(/function\s+\w+\([^)]*\bopening\s*:/);
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

describe("what the reader is told once the window has gone back to another tab", () => {
  it("is why the open failed, which the tab's own message would otherwise replace", () => {
    expect(toldAfterFallback("Error: no frame for this open", null)).toBe("Error: no frame for this open");
  });

  it("is why a tab would not mount again, when that is all there is", () => {
    expect(toldAfterFallback(null, "The file is not a PDF.")).toBe("The file is not a PDF.");
  });

  it("is nothing when nothing failed, so the tab's own message stays", () => {
    expect(toldAfterFallback(null, null)).toBeNull();
  });

  it("is both when there are both, the open's first and neither twice", () => {
    expect(toldAfterFallback("The reload failed.", "The file is not a PDF."))
      .toBe("The reload failed.\nThe file is not a PDF.");
    expect(toldAfterFallback("No such file.", "No such file.")).toBe("No such file.");
  });

  it("is said by the function that mounts the tabs again, with what the open answered", () => {
    expect(missingFrom(functionIn(app, "async function openDocument("), [
      "let failed: string | null = null;",
      "failed = said;",
      "return failed;",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function openPath("), [
      "const failed = await openDocument(path, resuming, resume, undefined, afterWrite, discard);",
      "try { await showPanes(failed); }",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "async function showPanes("), [
      "const told = toldAfterFallback(failed, refused);",
      "if (told !== null) say(told);",
    ])).toEqual([]);
  });
});

describe("edits requested in a mounted view", () => {
  it("binds the page before queuing each window action, including a thumbnail drop", () => {
    for (const method of ["deletePage", "rotatePage", "insertBlankPage", "insertSizedPage"]) {
      expect(functionIn(app, `async function ${method}(`)).toContain("await applyToPage(at,");
    }
    expect(functionIn(app, "function applyToPage(")).toContain("applyEdit(queuedPage(edits.map, slot, edit))");
    expect(functionIn(app, "function applyEdit(")).toContain("owner = asDocument(view, () => edits)");
    expect(functionIn(app, "function applyEdit(")).toContain("runEdit(view, owner, run)");
    expect(functionIn(app, "async function runEdit(")).toMatch(/if \(!model \|\| model !== owner\) return null;[\s\S]*await run\(model\)/);
    expect(functionIn(app, "function buildSidebar(")).toMatch(/onReorder:[^}]*movePageTo\(from, to\)/);
    expect(functionIn(app, "async function movePage(")).toContain("await movePageTo(at, at + delta)");
    expect(functionIn(app, "function movePageTo(")).toMatch(/const order = edits\?\.state\.pages;[\s\S]*applyEdit\(\(model\) => model.move\(from, to, order\)/);
  });

  it("wires queued Duplicate and Arrange to the document that owns the selection", () => {
    const duplicate = functionIn(app, "function duplicatePicked(");
    expect(missingFrom(duplicate, [
      "const source = viewer, model = edits, form = scannedForm, view = openView;",
      "copies: () => source.copiesOfPicked()",
      "state: () => model.state",
      "form: () => form",
      "(run) => applyEdit(run, view, model)",
      "edits === model && viewer === source) source.pick(ids)",
    ])).toEqual([]);
    const viewer = functionIn(app, "function buildViewer(");
    expect(viewer).toMatch(/onMarksArranged: [^{]+\{\s*const form = scannedForm;\s*void applyEdit\(\(e\) => arrangeBoth\(form, e.state, e, moves, sweep\)\)/);
    expect(viewer).toContain("nextGesture: () => model.nextGesture()");
  });

  it("shows the attachment chooser for the document that requested it after dismissing the previous question", () => {
    const saving = functionIn(app, "async function saveAttachment(");
    expect(saving).toMatch(/await call\("document_properties", \{ doc \}\);\s*if \(!stillIn\(view, model\)\) return;/);
    expect(saving).toContain('palette?.askFor("file.saveAttachment.choice", () => { attachmentQuestion = asked; });');
  });

  it("sends both window routes through the gesture's commit, with the palette requesting a rearm", () => {
    expect(functionIn(app, "function buildViewer(")).toContain("onRedactSelection: () => void redactSelection()");
    expect(withoutComments(app)).toContain("redactSelection: () => void redactSelection(true)");
    const body = functionIn(app, "async function redactSelection(");
    expect(body).toContain("viewer?.redactSelected(");
    expect(body).toContain("applyEdit(e => e.redactText(selected), view, model)");
    expect(body).toContain("}, keepArmed)");
  });
});

describe("the open's wiring in App.svelte", () => {
  // Read as text with the comments taken out, because `App.svelte` is the join
  // and nothing imports it. That sees that a line is there and not whether it
  // runs (`sourcetext.ts`), so these hold the hand-over and no decision.
  it("checks the settled original journal before replacing a reload and closes only the unused handle", () => {
    const open = functionIn(app, "async function openDocument(");
    expect(open).toMatch(/await openOrAsk\(path, !resuming\)[\s\S]*await settleDocument\([\s\S]*if \(replacing && !afterWrite && !discard\)[\s\S]*beforeReload\(replacing.edits.state.dirty \|\| hasDraft\)[\s\S]*if \(prompt\)[\s\S]*await call\("close_document", \{ doc: doc.id \}\)[\s\S]*return prompt.message;[\s\S]*unmountDocument\(\)/);
  });

  it("resolves background restore by identity before opening a new handle", () => {
    expect(functionIn(app, "function tabHost(")).toMatch(/opens.run\([\s\S]*if \(await matchingTab\(path, tabs.active\)\) return;[\s\S]*await openOrAsk\(path, asking\)/);
  });

  it("invalidates a comparison when temporary recognised text changes or leaves its viewer", () => {
    expect(functionIn(app, "async function recogniseCurrentPage(")).toMatch(
      /await shown\.recognisePage\([\s\S]*?comparisons\.invalidate\(view\)\) refreshComparison\(\)/);
    expect(functionIn(app, "function unmountDocument(")).toMatch(
      /viewer\?\.hasRecognisedText && comparisons\.invalidate\(openView\)\) refreshComparison\(\);[\s\S]*?viewer\?\.destroy\(\)/);
  });

  it("asks this module for each decision, in the function that needs it", () => {
    expect(missingFrom(functionIn(app, "async function openDocument("), [
      "const pages = pageTable(doc);",
      "const resume = placeToResume(retainedPlace(retained), override, remembered, path, retained?.edits.state.pages.length ?? doc.page_count);",
      "const written = afterWrite ? tabs.forPath(path, tabs.active) : undefined;",
      "if (written) endComparison(written.view);",
      "const left = abandonedBy(tabs.all, written, replaced, twins);",
      "abandonOpen(acquired, left.replaced, left.twins);",
      "const said = openFailure(e, resuming);",
      "if (said !== null) say(said);",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function adoptModel("), [
      "tabs.keep(tabToKeep(view, doc, path, model, from), replacing?.view);",
      "repoint(twins, doc, model);",
      "const shared = sharedByTwin(tabs.all, tabs.find(view), (twin) => asDocument(twin.view, () => covered));",
      "if (shared) covered = shared;",
      "placeOnSide(panes, view, replacing?.view, retained !== undefined);",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function abandonOpen("), [
      "dropAbandoned(panes, tabs, { own: acquired >= 0 ? viewOf(acquired) : undefined, replaced, twins }, refreshTabs, endComparison);",
    ])).toEqual([]);
  });

  it("places a form control, runs what it says and a press on a place for a signature, as the form's own document", () => {
    expect(missingFrom(functionIn(app, "function buildFormLayer("), [
      "const rect = asDocument(view, () => controlRect(widget, formEditing, edits?.state ?? null)) ?? null;",
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
