/**
 * Tests for what the viewer does after a continuation it cannot cancel comes
 * back.
 *
 * Almost nothing here is cancellable. A text extraction is an IPC round trip
 * with no withdrawal, and `TextCache.load` never rejects --- a failure resolves
 * to `null` --- so every one of those `.then` callbacks *will* run, whatever has
 * happened to the document meanwhile. The three cases below are the ones where
 * running it was wrong: a wake after the viewer was destroyed, a retry that a
 * failure cannot end, and a copy that reaches the clipboard with pages missing.
 *
 * They are asserted through the frame scheduler rather than through the screen.
 * "Did this restart the loop" is a question about a `requestAnimationFrame`
 * callback being handed over, and `testdom.ts` counts them instead of running
 * them --- a harness that ran them would answer a different question, since a
 * loop that reschedules itself never returns.
 *
 * Every test below was checked by mutating `viewer.ts` and confirming it went
 * red. Nothing here says anything about pixels; that is `viewercheck.ts`'s job,
 * against a real webview.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installFakeDom, settle, type FakeDom } from "./testdom";
import type { PageText } from "./text";
import { Viewer, type ViewerOptions, type ViewerStatus } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

/** Two characters side by side, which is enough to place a caret. */
function pageText(): PageText {
  return {
    codes: ["a", "b"].map((c) => c.codePointAt(0) ?? 0),
    boxes: [10, 10, 20, 22, 20, 10, 30, 22],
    width_pt: 600,
    height_pt: 800,
    quarter_turns: 0,
    extract_ms: 0,
  };
}

/** Pages whose extraction should fail, by page index. */
let failingPages = new Set<number>();
/** Pages `page_text` was asked for, in order. */
let asked: number[] = [];

/** A three-page document, with whichever callbacks a test wants to watch. */
function build(
  dom: FakeDom,
  callbacks: Pick<ViewerOptions, "onStatus" | "onPosition" | "onError"> = {},
): Viewer {
  return new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 3,
    page: { width_pt: 600, height_pt: 800 },
    ...callbacks,
  });
}

/** Runs frames until the loop stops of its own accord, or gives up. */
async function quiesce(viewer: Viewer, dom: FakeDom): Promise<void> {
  for (let round = 0; round < 40 && !viewer.idle; round++) {
    dom.runFrames();
    await settle();
  }
  // A loop that never idled would make every assertion below vacuous: `wake` on
  // a loop that is already running schedules nothing, so "no frame was
  // scheduled" would be true of a perfectly healthy viewer.
  if (!viewer.idle) throw new Error("the frame loop never settled");
}

describe("Viewer lifetime", () => {
  let dom: FakeDom;
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    dom = installFakeDom();
    failingPages = new Set();
    asked = [];
    core.invoke.mockReset();
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      if (failingPages.has(args.page)) return Promise.reject(new Error("no text"));
      return Promise.resolve(pageText());
    });
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    // Every tile fails, which is what lets the loop reach idle here: an
    // *abandoned* request is deliberately not backed off, so a mock that
    // returned `null` would have the scroller re-issue it on every frame and
    // the loop would never settle --- the tests below would then be waiting for
    // a state that cannot arrive.
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  it("does not restart the frame loop when a text load lands after destroy", async () => {
    // The zombie. A load outstanding at destroy is guaranteed --- it never
    // rejects --- and its `.then` calls `wake`, which used to restart the loop
    // unconditionally. The tick then drives a destroyed scroller, whose failing
    // requests arm a retry, which wakes the zombie again, every eight seconds,
    // for the life of the process.
    let resolveText: (text: PageText) => void = () => {};
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      return new Promise<PageText>((resolve) => {
        resolveText = resolve;
      });
    });

    const statuses: ViewerStatus[] = [];
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);
    expect(asked.length).toBeGreaterThan(0);

    viewer.destroy();
    dom.reset();
    const before = statuses.length;

    resolveText(pageText());
    await settle();

    expect(dom.scheduledFrames()).toBe(0);
    expect(statuses.length).toBe(before);
  });

  it("does restart it when the same load lands and the viewer is alive", async () => {
    // The control, and the test above is worthless without it: a harness that
    // never delivered the text, or a wake that could not reach the loop from
    // here, would report the guard as working while proving nothing.
    let resolveText: (text: PageText) => void = () => {};
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      return new Promise<PageText>((resolve) => {
        resolveText = resolve;
      });
    });

    const viewer = build(dom);
    await quiesce(viewer, dom);
    dom.reset();

    resolveText(pageText());
    await settle();

    expect(dom.scheduledFrames()).toBeGreaterThan(0);
    viewer.destroy();
  });

  it("arms the retry against the frame's own clock, not a later one", async () => {
    // The dropped wake. Two decisions are made about a backed-off request ---
    // "may this be issued yet", inside the frame, and "when should the loop be
    // woken", as it goes idle --- and a second clock reading for the second one
    // lets a request come due *between* them: the frame did not issue it
    // because it was not due yet, and no wake is armed because by then it is
    // already past. The tile then waits for some unrelated input, which on a
    // reader who has stopped scrolling is forever.
    //
    // Reproduced by scripting the clock rather than by racing it: the frame is
    // handed 1249 with the wait ending at 1250, and any *later* reading returns
    // 1251. The two readings straddle the entry, which is the whole condition.
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const readings: number[] = [];
    let clock = 1000;
    const now = vi
      .spyOn(performance, "now")
      .mockImplementation(() => readings.shift() ?? clock);

    const viewer = build(dom);
    await quiesce(viewer, dom);

    // Both requests failed at 1000, so both may be issued again at 1250.
    readings.push(1249, 1249);
    clock = 1251;
    viewer.wake();
    dom.runFrames();
    dom.runFrames();
    expect(viewer.idle).toBe(true);

    vi.advanceTimersByTime(2);
    expect(viewer.idle).toBe(false);

    viewer.destroy();
    now.mockRestore();
    vi.useRealTimers();
  });

  it("stops select-all on a page whose text cannot be read", async () => {
    // `TextCache.load` resolves to `null` on failure and caches nothing, so a
    // retry that does not look at the result issues a *fresh* `page_text` every
    // turn --- an unbounded loop of IPC calls that a document being closed did
    // not end either.
    //
    // The extraction relents after a few attempts so that the *defect*
    // terminates. Rejecting forever, the unfixed retry is a promise chain that
    // never yields: it starves the microtask queue, the test hangs, and a hang
    // is a much worse signal than a red assertion --- it looks exactly like a
    // broken harness. Made to succeed eventually, the count says plainly how
    // many attempts there were.
    let attempts = 0;
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      attempts++;
      if (attempts <= 5) return Promise.reject(new Error("no text"));
      return Promise.resolve(pageText());
    });
    const viewer = build(dom);

    viewer.selectPage();
    await settle(60);

    expect(asked.filter((page) => page === 0).length).toBe(1);
    expect(viewer.selectedText).toBe("");
    viewer.destroy();
  });

  it("retries select-all exactly once when the text does arrive", async () => {
    // The control. The retry is the feature --- a reader who presses ⌘A before
    // the extraction lands must still get the page selected --- so a guard that
    // stopped both cases would pass the test above and break select-all.
    const viewer = build(dom);

    viewer.selectPage();
    await settle(40);

    expect(asked.filter((page) => page === 0).length).toBe(1);
    expect(viewer.selectedText).toBe("ab");
    viewer.destroy();
  });
});

describe("Viewer copy", () => {
  let dom: FakeDom;
  let written: string[];
  let errors: string[];
  let previousNavigator: PropertyDescriptor | undefined;

  beforeEach(() => {
    dom = installFakeDom();
    failingPages = new Set();
    asked = [];
    written = [];
    errors = [];
    core.invoke.mockReset();
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      if (failingPages.has(args.page)) return Promise.reject(new Error("no text"));
      return Promise.resolve(pageText());
    });
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.resolve(null));

    previousNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    Object.defineProperty(globalThis, "navigator", {
      value: {
        clipboard: {
          writeText: (text: string) => {
            written.push(text);
            return Promise.resolve();
          },
        },
      },
      configurable: true,
      writable: true,
    });
  });

  afterEach(() => {
    if (previousNavigator) {
      Object.defineProperty(globalThis, "navigator", previousNavigator);
    }
    dom.restore();
  });

  /**
   * Drags a selection from page 0 to page 2, leaving page 1 never asked for.
   *
   * Through the viewer's own pointer handlers rather than by reaching into its
   * selection, because the interesting case is the one the pointer produces: a
   * drag can only place a caret on a page whose text is already loaded, so the
   * pages *between* the ends are exactly the ones nothing has fetched.
   */
  async function dragAcrossPages(viewer: Viewer): Promise<void> {
    const down = (page: number): void => {
      // Inside the first glyph and well clear of its midpoint: the midpoint is
      // where the caret flips to the following character, and a screen point
      // converted back through the zoom lands a floating-point step either side
      // of it.
      const at = viewer.screenPoint(page, 12, 15);
      dom.root.dispatch("pointerdown", {
        button: 0,
        pointerId: 1,
        target: dom.root,
        clientX: at.x,
        clientY: at.y,
      });
    };

    // The first press on each end only asks for the text; the caret cannot be
    // placed until it arrives.
    down(0);
    await settle();
    down(2);
    await settle();

    down(0);
    const focus = viewer.screenPoint(2, 22, 15);
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      target: dom.root,
      clientX: focus.x,
      clientY: focus.y,
    });
    dom.root.dispatch("pointerup", { pointerId: 1, target: dom.root });
    await settle();
  }

  it("copies a selection whose pages all load", async () => {
    const viewer = build(dom, { onError: (message: string) => errors.push(message) });
    await dragAcrossPages(viewer);

    const copied = await viewer.copySelection();

    expect(asked).toContain(1);
    expect(copied).toBe("ab\nab\na");
    expect(written).toEqual(["ab\nab\na"]);
    expect(errors).toEqual([]);
    viewer.destroy();
  });

  it("copies nothing when a page in the middle cannot be read", async () => {
    // The silent partial. `loadPages` resolves whether or not the extractions
    // succeeded, and `Selection.text` skips a page it cannot read by design ---
    // so without a second completeness test the clipboard quietly ends up
    // holding a selection with a page missing from the middle of it.
    const viewer = build(dom, { onError: (message: string) => errors.push(message) });
    await dragAcrossPages(viewer);
    failingPages.add(1);

    const copied = await viewer.copySelection();

    expect(asked).toContain(1);
    expect(copied).toBeNull();
    expect(written).toEqual([]);
    expect(errors.length).toBe(1);
    expect(errors[0]).toContain("nothing was copied");
    viewer.destroy();
  });

  it("says so when the clipboard itself refuses", async () => {
    // Every caller voids this promise, so a rejection here reached nobody at
    // all --- the copy simply did not happen and nothing said why.
    Object.defineProperty(globalThis, "navigator", {
      value: {
        clipboard: { writeText: () => Promise.reject(new Error("denied")) },
      },
      configurable: true,
      writable: true,
    });
    const viewer = build(dom, { onError: (message: string) => errors.push(message) });
    await dragAcrossPages(viewer);

    const copied = await viewer.copySelection();

    expect(copied).toBeNull();
    expect(errors.length).toBe(1);
    expect(errors[0]).toContain("denied");
    viewer.destroy();
  });
});
