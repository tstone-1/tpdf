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

import { DESTINATION_MARGIN_PT } from "./outline";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { TEXT_CACHE_CHARS, type PageText } from "./text";
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
    pages: [{ width_pt: 600, height_pt: 800 }],
    ...callbacks,
  });
}

/**
 * Presses the pointer inside a page's first glyph, left of its midpoint.
 *
 * Inside the glyph and well clear of the midpoint: the midpoint is where the
 * caret flips to the following character, and a screen point converted back
 * through the zoom lands a floating-point step either side of it.
 */
function press(dom: FakeDom, viewer: Viewer, page: number): void {
  const at = viewer.screenPoint(page, 12, 15);
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    target: dom.root,
    clientX: at.x,
    clientY: at.y,
  });
}

/** Moves a drag to a point already measured, which a closed document cannot. */
function movePointerTo(dom: FakeDom, at: { x: number; y: number }): void {
  dom.root.dispatch("pointermove", {
    pointerId: 1,
    target: dom.root,
    clientX: at.x,
    clientY: at.y,
  });
}

/** Moves a drag into a page's second glyph. */
function movePointer(dom: FakeDom, viewer: Viewer, page: number): void {
  movePointerTo(dom, viewer.screenPoint(page, 22, 15));
}

/**
 * Drags a selection from one page to another, leaving those between them never
 * asked for.
 *
 * Through the viewer's own pointer handlers rather than by reaching into its
 * selection, because the interesting case is the one the pointer produces: a
 * drag can only place a caret on a page whose text is already loaded, so the
 * pages *between* the ends are exactly the ones nothing has fetched.
 */
async function dragAcrossPages(
  dom: FakeDom,
  viewer: Viewer,
  from: number,
  to: number,
): Promise<void> {
  // The first press on each end only asks for the text; the caret cannot be
  // placed until it arrives.
  press(dom, viewer, from);
  await settle();
  press(dom, viewer, to);
  await settle();

  press(dom, viewer, from);
  movePointer(dom, viewer, to);
  dom.root.dispatch("pointerup", { pointerId: 1, target: dom.root });
  await settle();
}

/**
 * Installs a clipboard that records what it was handed.
 *
 * Returns whatever descriptor was there, for the test to put back: `navigator`
 * is a global, and a suite that left this one behind would decide what the next
 * one is testing.
 */
function installClipboard(written: string[]): PropertyDescriptor | undefined {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
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
  return previous;
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

  it("stops a drag the document was closed under", async () => {
    // `pointermove` and `pointerup` are added on the root when a drag starts
    // and removed when it ends --- and a document closed mid-drag never reaches
    // the end. `life.ended` stops the frame loop but says nothing about a
    // listener still bound: the move goes on extending the selection and goes
    // on asking for the text of every page it crosses, one IPC call each, for a
    // document the backend has closed.
    const viewer = build(dom);
    // Two presses: the first only asks for the text, since a caret cannot be
    // placed on a page that has not arrived.
    press(dom, viewer, 0);
    await settle();
    press(dom, viewer, 0);
    expect(asked).toEqual([0]);
    // Measured while the document is still open. A point is what a pointer
    // event carries, and asking a torn-down scroller where a page is would be
    // testing something else.
    const across = viewer.screenPoint(2, 22, 15);

    viewer.destroy();
    movePointerTo(dom, across);
    await settle();

    expect(asked).toEqual([0]);
  });

  it("still extends a drag while the document is open", async () => {
    // The control. The listener is the feature --- a drag across a page
    // boundary has to fetch what it crosses, or the highlight stops at the
    // boundary and the copy quietly omits it --- so a teardown that removed it
    // one press too early would pass the test above and break selecting.
    const viewer = build(dom);
    press(dom, viewer, 0);
    await settle();
    press(dom, viewer, 0);

    movePointerTo(dom, viewer.screenPoint(2, 22, 15));
    await settle();

    expect(asked).toContain(2);
    viewer.destroy();
  });

  it("stops a scrollbar drag the document was closed under", () => {
    // The same shape on the other surface that binds listeners for the length
    // of a gesture. Nothing here reaches the backend, so all the leak can move
    // is the closed viewer's own scroll offset --- which is both the extent of
    // the damage and the only thing that can say the listener is still bound.
    const viewer = build(dom);
    const track = dom.root.children[dom.root.children.length - 1];
    expect(track).toBeDefined();
    track?.dispatch("pointerdown", { pointerId: 2, target: track, clientY: 0 });
    const start = viewer.offset;

    viewer.destroy();
    track?.dispatch("pointermove", { pointerId: 2, target: track, clientY: 400 });

    expect(viewer.offset).toBe(start);
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

/**
 * What the status callback is allowed to stay quiet about.
 *
 * `report` fires only when something a reader could notice has changed, and it
 * decides that by joining the fields it considers noticeable into a summary
 * string. Anything the UI renders and that summary omits is a control that
 * sticks: the toolbar button keeps its old `class:on` and its old
 * `aria-pressed` while the viewer's actual setting has flipped. With an empty
 * query the search toggles move nothing else at all, so the omission is total.
 */
describe("Viewer status", () => {
  let dom: FakeDom;
  let statuses: ViewerStatus[];
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    dom = installFakeDom();
    failingPages = new Set();
    asked = [];
    statuses = [];
    core.invoke.mockReset();
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      return Promise.resolve(pageText());
    });
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    // Failing rather than pending, for the reason the lifetime suite gives: it
    // is what lets the loop reach idle, and every test here starts from idle.
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  it("reports a status for a search option flipped with no query", async () => {
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);

    // One at a time, and each asserted on its own: the summary names the
    // fields it watches individually, so a version that carried `matchCase` and
    // not `regex` would pass any test that flipped only the first.
    for (const option of ["matchCase", "wholeWord", "regex"] as const) {
      const before = statuses.length;
      viewer.setSearchOptions({ ...viewer.searchOptionsNow, [option]: true });
      dom.runFrames();

      expect(statuses.length).toBeGreaterThan(before);
      expect(statuses[statuses.length - 1]?.search.options[option]).toBe(true);
    }
    viewer.destroy();
  });

  it("reports a status when the search is confined to the selection", async () => {
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    viewer.selectPage();
    await settle();
    await quiesce(viewer, dom);
    // The selection is already reported by the time this runs, so the only
    // thing left to move is the scope itself --- without which this would be a
    // test of `selected`.
    expect(statuses[statuses.length - 1]?.selected).toBe(2);
    const before = statuses.length;

    expect(viewer.scopeSearchToSelection()).toBe(true);
    dom.runFrames();

    expect(statuses.length).toBeGreaterThan(before);
    expect(statuses[statuses.length - 1]?.search.scoped).toBe(true);
    viewer.destroy();
  });

  it("reports a status when a tool is armed, and again when it is dropped", async () => {
    // **The three mode fields were absent from the summary, so this could not
    // fire.** That string is what decides whether `onStatus` is called at all,
    // and arming a tool moves nothing else in it: no tile becomes pending, the
    // selection stays empty, the page and the zoom do not move. So the status
    // line that exists to make a mode visible was told about it only when
    // something unrelated happened to change --- or never.
    //
    // Both directions, because a version that reported the arming and not the
    // cancel would leave the line on screen naming a tool that is no longer
    // armed, which is worse than never showing it.
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);
    const before = statuses.length;

    viewer.armDraw("square");
    dom.runFrames();
    expect(statuses.length).toBeGreaterThan(before);
    expect(statuses[statuses.length - 1]?.armed).toBe("square");

    const armed = statuses.length;
    viewer.cancelDraw();
    dom.runFrames();
    expect(statuses.length).toBeGreaterThan(armed);
    expect(statuses[statuses.length - 1]?.armed).toBe(null);
    viewer.destroy();
  });

  it("names the armed crop, which is not a mark kind", async () => {
    // **The status is a copy, and the copy is what the reader sees.** The tests
    // around this file read the viewer's own accessors; the window reads
    // `ViewerStatus`, and a `report` that filled `armed` from `drawKind` alone
    // would leave a reader who armed the crop with a crosshair and no words ---
    // which is the exact complaint the field was added for, arriving through the
    // one tool that is not a `MarkKind`.
    //
    // Both directions, for the reason the test above gives: a version that
    // reported the arming and not the cancel leaves the line on screen naming a
    // tool that is no longer armed.
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);
    const before = statuses.length;

    viewer.armCrop();
    dom.runFrames();
    expect(statuses.length).toBeGreaterThan(before);
    expect(statuses[statuses.length - 1]?.armed).toBe("crop");

    const armed = statuses.length;
    viewer.cancelDraw();
    dom.runFrames();
    expect(statuses.length).toBeGreaterThan(armed);
    expect(statuses[statuses.length - 1]?.armed).toBe(null);
    viewer.destroy();
  });

  it("names a drawing in one field, not two", async () => {
    // The pen arms like every other tool, and the window already has a line for
    // it that counts strokes. Reporting it here as well would put two lines on
    // the status bar saying the same thing in different words --- so `armed`
    // stays `null` for ink while `drawing` carries it, and this is the
    // assertion that says so rather than a comment claiming it.
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);

    viewer.armDraw("ink");
    dom.runFrames();
    const last = statuses[statuses.length - 1];
    expect(last?.drawing).toBe(0);
    expect(last?.armed).toBe(null);
    viewer.destroy();
  });

  it("stays quiet when a frame changes nothing", async () => {
    // The control, and the two tests above are worth nothing without it: a
    // `report` that fired on every frame would satisfy them while telling a
    // status line nothing about what changed. It also pins the reason the
    // assertions above are attributable to the flip --- waking the loop and
    // running a frame is, by itself, not an event.
    const viewer = build(dom, { onStatus: (s: ViewerStatus) => statuses.push(s) });
    await quiesce(viewer, dom);
    const before = statuses.length;

    viewer.wake();
    dom.runFrames();

    expect(statuses.length).toBe(before);
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

    previousNavigator = installClipboard(written);
  });

  afterEach(() => {
    if (previousNavigator) {
      Object.defineProperty(globalThis, "navigator", previousNavigator);
    }
    dom.restore();
  });

  it("copies a selection whose pages all load", async () => {
    const viewer = build(dom, { onError: (message: string) => errors.push(message) });
    await dragAcrossPages(dom, viewer, 0, 2);

    const copied = await viewer.copySelection();

    expect(asked).toContain(1);
    expect(copied).toBe("ab\nab\na");
    expect(written).toEqual(["ab\nab\na"]);
    expect(errors).toEqual([]);
    viewer.destroy();
  });

  it("copies nothing when a page in the middle cannot be read", async () => {
    // The silent partial. A load resolves whether or not the extraction
    // succeeded --- `TextCache.load` resolves to `null` rather than rejecting
    // --- so a copy that took whatever text it happened to get would put a
    // selection with a page missing from the middle of it on the clipboard.
    const viewer = build(dom, { onError: (message: string) => errors.push(message) });
    await dragAcrossPages(dom, viewer, 0, 2);
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
    await dragAcrossPages(dom, viewer, 0, 2);

    const copied = await viewer.copySelection();

    expect(copied).toBeNull();
    expect(errors.length).toBe(1);
    expect(errors[0]).toContain("denied");
    viewer.destroy();
  });
});

/**
 * Copying a selection larger than the text cache can hold.
 *
 * `TextCache` evicts least-recently-used down to {@link TEXT_CACHE_CHARS}, and a
 * copy loads its pages ascending and touches each of them exactly once --- so
 * the eviction order *is* the page order, and the front of a large selection is
 * dropped while its tail is still arriving. A copy that read the cache after
 * the wait could therefore never succeed past the bound, and reported it in the
 * one wording that is certainly wrong: that the document's text could not be
 * read, when every page of it had been read fine.
 *
 * This is the drag-to-the-end-of-775-pages case the copy path exists for, so
 * the fixture is deliberately built to cross the bound rather than to be small.
 */
describe("Viewer copy past the text cache", () => {
  /** Pages in the document, all of them selected. */
  const PAGES = 10;
  /** Characters on each. Ten pages of these is comfortably past the bound. */
  const CHARS = 45_000;
  /** Characters per line, so a page is a few dozen lines rather than one. */
  const PER_LINE = 500;

  let dom: FakeDom;
  let written: string[];
  let errors: string[];
  let previousNavigator: PropertyDescriptor | undefined;
  let big: PageText;

  /** A page of `CHARS` characters, laid out in rows of 10-point cells. */
  function bigPage(): PageText {
    const first = "a".codePointAt(0) ?? 0;
    const codes: number[] = [];
    const boxes: number[] = [];
    for (let index = 0; index < CHARS; index++) {
      codes.push(first + (index % 26));
      // The first two boxes are those of `pageText()` above, which is what the
      // drag helpers aim at.
      const left = 10 + (index % PER_LINE) * 10;
      const top = 10 + Math.floor(index / PER_LINE) * 14;
      boxes.push(left, top, left + 10, top + 12);
    }
    return { codes, boxes, width_pt: 600, height_pt: 800, quarter_turns: 0, extract_ms: 0 };
  }

  beforeEach(() => {
    dom = installFakeDom();
    failingPages = new Set();
    asked = [];
    written = [];
    errors = [];
    // Built once and handed out for every page: the cache counts characters,
    // not objects, so ten references to one page cost exactly what ten pages
    // would --- and building ten of these is the slowest thing here by far.
    big = bigPage();
    core.invoke.mockReset();
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      if (failingPages.has(args.page)) return Promise.reject(new Error("no text"));
      return Promise.resolve(big);
    });
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.resolve(null));

    previousNavigator = installClipboard(written);
  });

  afterEach(() => {
    if (previousNavigator) {
      Object.defineProperty(globalThis, "navigator", previousNavigator);
    }
    dom.restore();
  });

  function buildBig(onError: (message: string) => void): Viewer {
    return new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: PAGES,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onError,
    });
  }

  it("copies a selection whose pages cannot all be held at once", async () => {
    // The precondition, asserted against the constant rather than assumed:
    // raising the cache bound must turn this red rather than quietly leave a
    // fixture that fits in the cache and discriminates nothing.
    expect(PAGES * CHARS).toBeGreaterThan(TEXT_CACHE_CHARS);
    const viewer = buildBig((message) => errors.push(message));
    await dragAcrossPages(dom, viewer, 0, PAGES - 1);

    const copied = await viewer.copySelection();

    // Every page but the last in full, the last up to the caret after its first
    // character, and a newline between each pair.
    expect(copied?.length).toBe((PAGES - 1) * CHARS + 1 + (PAGES - 1));
    expect(errors).toEqual([]);
    expect(written.length).toBe(1);
    expect(written[0]).toBe(copied);
    viewer.destroy();
  });

  it("still copies nothing when a page of a large selection cannot be read", async () => {
    // The control, and the one that keeps the fix honest: a copy that stopped
    // consulting the cache could equally well have stopped checking that it had
    // every page, which is the silent partial the whole path exists to prevent.
    // A page in the middle of a selection this size is exactly where nobody
    // would notice it missing.
    const viewer = buildBig((message) => errors.push(message));
    await dragAcrossPages(dom, viewer, 0, PAGES - 1);
    failingPages.add(4);

    const copied = await viewer.copySelection();

    expect(asked).toContain(4);
    expect(copied).toBeNull();
    expect(written).toEqual([]);
    expect(errors.length).toBe(1);
    expect(errors[0]).toContain("nothing was copied");
    viewer.destroy();
  });
});

/**
 * What the viewer does with a document whose pages are not all the same size.
 *
 * The backend sends page 1's geometry alone unless `TPDF_EAGER_GEOMETRY` is set,
 * so every other page starts out estimated and is corrected as the reader
 * arrives at it. That correction rides the text extraction the frame loop was
 * performing anyway, which is why these tests drive `page_text` rather than any
 * new command --- there is no second request to intercept.
 *
 * Both were checked by mutating `viewer.ts` and confirming they went red; the
 * mutations are named beside the assertions they are aimed at.
 */
describe("Viewer geometry on a mixed-size document", () => {
  let dom: FakeDom;
  let warn: ReturnType<typeof vi.spyOn>;
  /** Page sizes `page_text` reports, by page. */
  let sizes: Map<number, { width_pt: number; height_pt: number }>;
  /** Pages whose extraction is held open, so a test can time its arrival. */
  let held: Map<number, () => void>;

  /** A two-character page of a stated size, which is enough to carry geometry. */
  function sized(width_pt: number, height_pt: number): PageText {
    return { ...pageText(), width_pt, height_pt };
  }

  /**
   * Runs frames whether or not the loop is awake.
   *
   * `quiesce` above stops as soon as the viewer idles, which here would stop
   * *before* the extraction lands: the reply's `.then` calls `wake`, and a wake
   * schedules a callback that something still has to run. A fixed number of
   * rounds is what lets a reply that arrives after the loop went quiet still be
   * acted on --- and every assertion below names the state it is waiting for, so
   * a round count that was too small fails rather than passing quietly.
   */
  async function pump(rounds = 24): Promise<void> {
    for (let round = 0; round < rounds; round++) {
      dom.runFrames();
      await settle();
    }
  }

  function build3(): Viewer {
    return new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 3,
      // A lazy open: page 1 and nothing else, which is what the default path
      // hands over and the state every document starts in.
      pages: [{ width_pt: 600, height_pt: 800 }],
    });
  }

  beforeEach(() => {
    dom = installFakeDom();
    sizes = new Map();
    held = new Map();
    asked = [];
    core.invoke.mockReset();
    core.invoke.mockImplementation((command: string, args: { page: number }) => {
      if (command !== "page_text") return Promise.resolve(null);
      asked.push(args.page);
      const size = sizes.get(args.page) ?? { width_pt: 600, height_pt: 800 };
      const text = sized(size.width_pt, size.height_pt);
      if (!held.has(args.page)) return Promise.resolve(text);
      return new Promise<PageText>((resolve) => {
        held.set(args.page, () => resolve(text));
      });
    });
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  it("fits the page being read rather than page 1", async () => {
    // Mutation: `displayedPage()` reading `this.opts.pages[0]` instead of the
    // scroller's size for the current page. Every fit then follows page 1 and
    // the wide page overflows the window with no way to reach its edge.
    sizes.set(1, { width_pt: 1200, height_pt: 800 });
    const viewer = build3();
    await pump();

    viewer.setFit("width");
    const onNarrow = viewer.currentZoom;
    expect(onNarrow).toBeGreaterThan(0);

    viewer.goToPage(1);
    await pump();
    // The precondition, asserted rather than assumed: without the size having
    // been learned there is nothing for the fit to be different about, and the
    // comparison below would be measuring one page twice.
    expect(viewer.knowsPageSize(1)).toBe(true);
    expect(viewer.pageSize(1).width_pt).toBe(1200);

    viewer.setFit("width");
    const onWide = viewer.currentZoom;
    // Half, give or take the fixed margin a fit leaves around the page.
    expect(onWide).toBeLessThan(onNarrow * 0.55);

    // The control. A viewer that had simply halved its zoom and stayed there
    // would pass everything above; fitting page 1 again has to return the
    // original number.
    viewer.goToPage(0);
    await pump();
    viewer.setFit("width");
    expect(viewer.currentZoom).toBeCloseTo(onNarrow, 6);
    viewer.destroy();
  });

  it("corrects the scrollbar extent without moving the reader off their line", async () => {
    // Mutation: dropping the re-anchor at the end of `learnGeometry`. The
    // extent still corrects --- so an assertion on `maxOffset` alone cannot see
    // it --- while the reader slides to a quarter of the way down a page they
    // were halfway through.
    sizes.set(1, { width_pt: 600, height_pt: 1600 });
    held.set(1, () => {});
    const viewer = build3();
    await pump();

    viewer.goToPage(1);
    await pump();
    // Half a page down, through the wheel handler a trackpad reaches.
    dom.root.dispatch("wheel", {
      deltaY: viewer.pageBoxCss.height / 2,
      deltaMode: 0,
      target: dom.root,
    });
    await pump();

    expect(viewer.knowsPageSize(1)).toBe(false);
    const before = viewer.position;
    const fractionBefore = before.top / viewer.pageSize(1).height_pt;
    const extentBefore = viewer.maxOffset;
    // The precondition. At the top of a page every fraction is zero and is
    // preserved by doing nothing at all, which is the shape of assertion this
    // repository has already been caught writing.
    expect(before.page).toBe(1);
    expect(fractionBefore).toBeGreaterThan(0.3);

    held.get(1)?.();
    await pump();

    expect(viewer.knowsPageSize(1)).toBe(true);
    expect(viewer.pageSize(1).height_pt).toBe(1600);
    // The extent corrected: the document really is taller than it was laid out.
    expect(viewer.maxOffset).toBeGreaterThan(extentBefore + 700);
    // And the reader is where they were, to within the fixed gap between pages
    // --- which does not scale with the page and so cannot be preserved exactly.
    const after = viewer.position;
    expect(after.page).toBe(1);
    expect(Math.abs(after.top / viewer.pageSize(1).height_pt - fractionBefore))
      .toBeLessThan(0.02);
    viewer.destroy();
  });

  it("records the document's geometry, not the rotated view's", async () => {
    // Mutation: recording `PageText`'s width and height as they arrive, without
    // taking the view's own rotation back out. An identity at every even number
    // of quarter-turns, which is why nothing else here can see it: the two are
    // different only on a rotated document whose pages are not square.
    sizes.set(1, { width_pt: 600, height_pt: 1600 });
    const viewer = build3();
    await pump();

    viewer.rotateBy(1);
    viewer.goToPage(1);
    await pump();

    expect(viewer.knowsPageSize(1)).toBe(true);
    expect(viewer.pageSize(1)).toEqual({ width_pt: 600, height_pt: 1600 });
    // The control, and the reason the assertion above is not a tautology: the
    // text layer really does report this page the other way round, so a viewer
    // that stored what it was handed would have stored 1600 x 600.
    expect(viewer.textOn(1)?.width_pt).toBe(1600);
    viewer.destroy();
  });
});

/**
 * Tests for Back and Forward restoring a place rather than re-deriving one.
 *
 * The distinction they turn on is that a *destination* and a *place* are
 * different things scrolled to differently. `goToDestination` leaves
 * `DESTINATION_MARGIN_PT` of air above the point it was given, because a
 * heading flush against the top edge reads as cut off. A recorded place is
 * where the reader already was, and moving them 6 pt off it is not a
 * courtesy --- it is a wrong answer that compounds, since the margin is
 * subtracted again on every replay.
 *
 * These exist because the window harness found it and the window harness is
 * the wrong instrument for a scroll arithmetic bug: it wants an unlocked
 * screen, a built bundle and about ninety seconds per document, and it
 * reported the symptom (`"Back": 773 -> 0` on a 775-page file) rather than
 * the cause. Each assertion below was checked by restoring the defect ---
 * routing `jumpTo` back through `goToDestination` --- and confirming it goes
 * red.
 */
describe("Viewer history", () => {
  let dom: FakeDom;
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    dom = installFakeDom();
    core.invoke.mockReset();
    core.invoke.mockImplementation(() => Promise.resolve(null));
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  /** A document long enough that page 2 is reachable well clear of the end. */
  function build8(): Viewer {
    return new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 8,
      pages: [{ width_pt: 600, height_pt: 800 }],
    });
  }

  /**
   * A viewer that reports every history change, and the count of them.
   *
   * The window's menu is a *pushed* map --- `refreshMenu` sends every guard's
   * answer once --- so a Back item guarded on the history is only honest while
   * something announces that the history moved. That announcement is
   * `onNavigate`, and until 2026-08-23 exactly one of its four causes fired it.
   */
  function build8Watching(): { viewer: Viewer; said: () => number } {
    let count = 0;
    const viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 8,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onNavigate: () => {
        count += 1;
      },
    });
    return { viewer, said: () => count };
  }

  it("says nothing until the history moves", () => {
    // The control for the four below. Without it, a viewer that announced on
    // every frame would satisfy all of them.
    const { viewer, said } = build8Watching();
    expect(viewer.canGoBack).toBe(false);
    expect(viewer.canGoForward).toBe(false);
    expect(said()).toBe(0);
    viewer.destroy();
  });

  it("announces a jump, which is what makes Back live", () => {
    // Through `goToDestination` rather than `followLink`, deliberately: this is
    // the outline's and the search result's route, and it was the one that did
    // *not* announce. The link's did, which is why the defect was invisible to
    // anyone testing with a link.
    const { viewer, said } = build8Watching();
    viewer.goToDestination(2, 200);
    expect(viewer.canGoBack).toBe(true);
    expect(said()).toBe(1);
    viewer.destroy();
  });

  it("announces the step back, and the step forward after it", () => {
    // Each step moves both answers, so each has to be announced: after Back
    // there is somewhere to go forward to, and the menu learns that from here.
    const { viewer, said } = build8Watching();
    viewer.goToDestination(2, 200);
    const after = said();
    expect(viewer.goBack()).toBe(true);
    expect(viewer.canGoForward).toBe(true);
    expect(said()).toBeGreaterThan(after);
    const back = said();
    expect(viewer.goForward()).toBe(true);
    expect(viewer.canGoForward).toBe(false);
    expect(said()).toBeGreaterThan(back);
    viewer.destroy();
  });

  it("announces a new document emptying the history", () => {
    // The mirror, and the one a reader meets every time they open a second
    // file: without it Back stays live in the menu on a document with nowhere
    // to go back to. `setLinks` is where a document's history is cleared.
    const { viewer, said } = build8Watching();
    viewer.goToDestination(2, 200);
    const jumped = said();
    expect(viewer.canGoBack).toBe(true);
    viewer.setLinks([]);
    expect(viewer.canGoBack).toBe(false);
    expect(said()).toBeGreaterThan(jumped);
    viewer.destroy();
  });

  it("announces every jump, including one the stack collapses", () => {
    // **Written the other way round first, and the test corrected it.** The
    // premise was that a jump landing where the reader already is records
    // nothing, so it should announce nothing. `History.push` skips only when
    // the *top of the stack* is that place --- two presses on the same
    // cross-reference --- so a first jump to where you already are does record,
    // and the assertion went red.
    //
    // What is true, and what this pins: the announcement is unconditional for
    // any jump that is not a replay, and the stack collapses the repeat. That
    // is deliberate rather than sloppy --- `refreshMenu` compares the map it
    // built against the one it last pushed and returns early when they match,
    // so a redundant announcement costs twenty closures and no message. Making
    // the viewer answer "did the stack actually move" would mean `push`
    // reporting it, which is a second statement of a rule that already lives in
    // one place.
    const { viewer, said } = build8Watching();
    viewer.goToDestination(4, 100);
    const first = said();
    expect(viewer.canGoBack).toBe(true);
    // Twice more to the same destination. What `push` records is the place
    // being *left*, so the second jump records page 4 (a new place) and only
    // the third finds it already on top and folds --- measured, after the first
    // draft of this test asserted the fold one jump too early.
    viewer.goToDestination(4, 100);
    viewer.goToDestination(4, 100);
    expect(said()).toBe(first + 2);
    expect(viewer.historyDepths.back).toBe(2);
    // And the reading the product actually uses agrees with it, twice down and
    // then empty.
    expect(viewer.goBack()).toBe(true);
    expect(viewer.canGoBack).toBe(true);
    expect(viewer.goBack()).toBe(true);
    expect(viewer.canGoBack).toBe(false);
    viewer.destroy();
  });

  it("leaves air above a destination, and none above a recorded place", () => {
    // The control first: a destination really is placed `DESTINATION_MARGIN_PT`
    // above the point it names. Without this the assertion below would hold
    // just as well for a viewer that had no margin anywhere, and would then be
    // testing nothing.
    const viewer = build8();
    viewer.goToDestination(2, 200);
    const arrived = viewer.position;
    expect(arrived.page).toBe(2);
    expect(arrived.top).toBeCloseTo(200 - DESTINATION_MARGIN_PT, 6);

    // Leave, and come back. The place recorded was `arrived`, margin and all,
    // so restoring it must reproduce `arrived` exactly --- not `arrived` with
    // the margin taken off a second time.
    viewer.goToDestination(0, 0);
    expect(viewer.goBack()).toBe(true);
    expect(viewer.position).toEqual(arrived);
    viewer.destroy();
  });

  it("does not drift a little further off on every round trip", () => {
    // The shape that made this read as "Back is unreliable" rather than as an
    // off-by-one: each replay subtracted the margin again, so the error grew
    // with the number of jumps rather than staying put. Three round trips is
    // enough for a 6 pt margin to move a reader off a page top.
    const viewer = build8();
    viewer.goToDestination(5, 300);
    const away = viewer.position;
    viewer.goToDestination(1, 100);
    const near = viewer.position;

    for (let round = 0; round < 3; round++) {
      expect(viewer.goBack()).toBe(true);
      expect(viewer.position).toEqual(away);
      expect(viewer.goForward()).toBe(true);
      expect(viewer.position).toEqual(near);
    }
    viewer.destroy();
  });

  it("restores a place on a rotated view, which records no offset", () => {
    // A quarter turn scrolls along the page's horizontal axis, so `position`
    // reports the page and no offset, and a recorded place carries none
    // either. That is the branch of `jumpTo` the two tests above cannot
    // reach: they both replay a non-zero `top`.
    const viewer = build8();
    viewer.rotateBy(1);
    viewer.goToDestination(4, 300);
    const away = viewer.position;
    expect(away).toEqual({ page: 4, top: 0 });

    viewer.goToDestination(0, 0);
    expect(viewer.goBack()).toBe(true);
    expect(viewer.position).toEqual(away);
    viewer.destroy();
  });
});

/**
 * Tests for where a destination puts the reader, and the page it must not
 * put them on.
 *
 * `DESTINATION_MARGIN_PT` reveals what sits above a heading, so the heading
 * does not read as cut off against the top edge. The whole of the rule below
 * is that there is nothing above a heading which *is* the top of the page,
 * and revealing 6 pt of the page before it is a different page rather than
 * air. `outline.ts`'s `currentId` then drops the entry --- it skips any row
 * whose page is past the reader, before `REACHED_TOLERANCE_PT` is consulted
 * --- so the reader clicks one entry and watches another light up.
 *
 * Found by `viewer_check.py` on `links.pdf`, which is the only corpus that
 * can see it: its outline is deliberately out of page order, so the entry
 * wrongly chosen is a visibly different one rather than the neighbour.
 */
describe("Viewer destinations", () => {
  let dom: FakeDom;
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    dom = installFakeDom();
    core.invoke.mockReset();
    core.invoke.mockImplementation(() => Promise.resolve(null));
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  function build8(): Viewer {
    return new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 8,
      pages: [{ width_pt: 600, height_pt: 800 }],
    });
  }

  it("lands on the page a top-of-page destination names, not the one before", () => {
    // `/Fit` and `/FitB` name no coordinate, which `goToDestination` takes as
    // the page's top --- so this is not an edge case, it is what a whole
    // destination family does.
    const viewer = build8();
    viewer.goToDestination(5, null);
    expect(viewer.position).toEqual({ page: 5, top: 0 });

    // And an explicit zero, which is what `/XYZ x 0 z` and a heading at the
    // very top of its page both produce.
    viewer.goToDestination(3, 0);
    expect(viewer.position).toEqual({ page: 3, top: 0 });
    viewer.destroy();
  });

  it("still leaves air above a heading that has room for it", () => {
    // The control. Without it every assertion above would hold just as well
    // for a viewer that had deleted the margin outright, which would bring
    // back the cut-off heading the margin exists to prevent --- and
    // `REACHED_TOLERANCE_PT`, which must strictly exceed it, would then be
    // guarding nothing.
    const viewer = build8();
    viewer.goToDestination(5, 200);
    expect(viewer.position.page).toBe(5);
    expect(viewer.position.top).toBeCloseTo(200 - DESTINATION_MARGIN_PT, 6);
    viewer.destroy();
  });

  it("clamps the margin rather than dropping it near the top of a page", () => {
    // The boundary the clamp is written on: an offset smaller than the margin
    // is the case where subtracting it crosses the page. Landing at the page
    // top is right; landing 2 pt above it is the defect at its smallest, and
    // is exactly as wrong as the 6 pt version --- `position` reports the page
    // before either way.
    const viewer = build8();
    viewer.goToDestination(6, 4);
    expect(viewer.position).toEqual({ page: 6, top: 0 });
    viewer.destroy();
  });
});
