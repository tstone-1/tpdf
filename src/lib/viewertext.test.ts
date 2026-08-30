/**
 * Which page's text the viewer reads, and when it asks for it again.
 *
 * Two defects, found together and sharing a subject: `TextCache` is keyed by a
 * number the viewer does not work in, and the viewer kept a record of what it
 * had asked for in a place that could not see the cache discard an answer.
 *
 * `TextCache` is keyed by page of the **file**, deliberately --- a page's text
 * is a property of the document, so deleting a page above it must not make it
 * be fetched again. Everything in `viewer.ts` works in **slots**, because a slot
 * is where the pointer went and where the scroller laid a page out. The two are
 * equal on every document nobody has edited, which is why fifteen call sites
 * passed a slot straight into the cache for months and every gate stayed green.
 *
 * What that cost is measurable and is the first test below: the same drag
 * selects `"ab"` on an untouched document and nothing at all once the first page
 * is deleted, because the loader stored the text under the page's number and the
 * paint path looked under the slot's. Text selection, select-all-on-page, the
 * copy path and the search highlights were all dead on any document with an
 * edit in it.
 *
 * Every test here therefore comes in a pair: the untouched document, which
 * passes with or without the fix and is the control, and the edited one, which
 * is the assertion. A test on an edited document alone could not tell a viewer
 * that translates from one that has no text at all.
 *
 * The edit is always **deleting the first page**, because it is the smallest one
 * that makes every slot differ from its page, and `sources: [1, 2]` is then
 * readable as what it is.
 *
 * The second defect --- a cropped page never fetched again --- has its last two
 * tests here and needs no edit at all. Its evidence is a measurement rather than
 * a mutation, and the reason is worth stating: the record lived in `viewer.ts`,
 * where nothing could clear it, and it now lives beside the extraction it is
 * about, where `forget` clears it. No one-line edit reproduces "kept somewhere
 * that cannot see the answer being discarded", so what was measured is the code
 * before the move --- the same drag, on the same page, after a crop: `asked`
 * stayed `[0]` and the selection was `""`. The rules the move rests on are
 * mutated in `textcache.test.ts`'s terms instead.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { type PageText } from "./text";
import { Viewer, type ViewerStatus } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

/**
 * Two characters side by side, spelling **which page of the file this is**.
 *
 * Page 0 is `"ab"`, page 1 `"cd"`, page 2 `"ef"`. Two characters is enough to
 * place a caret between and to drag across; that they differ per page is what
 * makes the assertions below about the answer rather than about the question.
 * A fixture whose pages all read the same would let a viewer asking for the
 * wrong page pass every content assertion, and select-all on an edited document
 * really did hand back the deleted page's characters --- not nothing.
 *
 * The boxes are identical on every page, so the pointer arithmetic is the same
 * wherever the drag lands.
 */
function pageText(page: number): PageText {
  const first = 97 + page * 2;
  return {
    codes: [first, first + 1],
    boxes: [10, 10, 20, 22, 20, 10, 30, 22],
    width_pt: 600,
    height_pt: 800,
    quarter_turns: 0,
    extract_ms: 0,
  };
}

/** What {@link pageText} spells for a page of the file. */
function said(page: number): string {
  return String.fromCodePoint(97 + page * 2, 98 + page * 2);
}

/** Which pages `page_text` was asked for, in order. */
let asked: number[] = [];
/** The last status the viewer reported. */
let status: ViewerStatus | null = null;
/** Everything the viewer reported to the reader as an error. */
let errors: string[] = [];
/** Everything written to the clipboard. */
let written: string[] = [];

describe("Reading a page's text, and asking for it again", () => {
  let dom: FakeDom;
  /** Whatever `navigator` descriptor was there, for `afterEach` to put back. */
  let clipboardWas: PropertyDescriptor | undefined;

  beforeEach(() => {
    dom = installFakeDom();
    asked = [];
    status = null;
    errors = [];
    written = [];
    clipboardWas = Object.getOwnPropertyDescriptor(globalThis, "navigator");
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
    core.invoke.mockReset();
    core.invoke.mockImplementation(
      (command: string, args: { page: number }) => {
        if (command === "page_text") {
          asked.push(args.page);
          return Promise.resolve(pageText(args.page));
        }
        if (command === "page_geometry") {
          return Promise.resolve({
            width_pt: 300,
            height_pt: 400,
            left_pt: 0,
            top_pt: 0,
          });
        }
        return Promise.resolve(null);
      },
    );
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    // Every tile fails, so the frame loop can reach idle: an abandoned request
    // is deliberately not backed off, so a mock resolving to `null` would have
    // the scroller re-issue it on every frame.
    tiles.fetchTile.mockRejectedValue(new Error("no tile"));
  });

  afterEach(() => {
    // Put `navigator` back. It is a global, and a suite that left this one
    // behind would decide what the next one is testing.
    if (clipboardWas) {
      Object.defineProperty(globalThis, "navigator", clipboardWas);
    }
    dom.restore();
    vi.clearAllMocks();
  });

  /** A three-page document. */
  function build(): Viewer {
    return new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 3,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onStatus: (next) => {
        status = next;
      },
      onError: (message) => errors.push(message),
    });
  }

  /**
   * Deletes the document's first page, leaving two slots over pages 1 and 2.
   *
   * By hand rather than through `Edits`, which would need the whole model
   * behind it: what these tests need is a `PageMap` whose slots differ from its
   * pages, and this is the shortest thing that produces one.
   */
  function deleteFirstPage(viewer: Viewer): void {
    const pages: PageView[] = [
      { id: pageId(2), source: { baseline: 1 }, turns: 0 },
      { id: pageId(3), source: { baseline: 2 }, turns: 0 },
    ];
    viewer.setPages(pages);
  }

  /** Presses inside a slot's first glyph, left of its midpoint. */
  function press(viewer: Viewer, slot: number): void {
    const at = viewer.screenPoint(slot, 12, 15);
    dom.root.dispatch("pointerdown", {
      button: 0,
      pointerId: 1,
      target: dom.root,
      clientX: at.x,
      clientY: at.y,
    });
  }

  /**
   * Drags across both characters of a slot's only line.
   *
   * The first press only asks for the text --- a caret cannot be placed until it
   * arrives --- so the drag is the second press onwards.
   */
  async function dragAcross(viewer: Viewer, slot: number): Promise<void> {
    press(viewer, slot);
    await settle();
    press(viewer, slot);
    const to = viewer.screenPoint(slot, 28, 15);
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      target: dom.root,
      clientX: to.x,
      clientY: to.y,
    });
    dom.root.dispatch("pointerup", { pointerId: 1, target: dom.root });
    await settle();
  }

  it("selects what the pointer dragged over, on an untouched document", async () => {
    const viewer = build();
    await dragAcross(viewer, 0);
    expect(asked).toEqual([0]);
    expect(viewer.selectedText).toBe(said(0));
  });

  it("selects what the pointer dragged over, after the page above went", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    await dragAcross(viewer, 0);
    // Slot 0 draws page 1, so that is what was fetched --- and the caret has to
    // find it under the same number. Both asserted: the request alone would be
    // satisfied by a viewer that asked correctly and then read the answer back
    // under the slot, and the text alone would be satisfied by one that asked
    // for the slot and got a self-consistent wrong page.
    expect(asked).toEqual([1]);
    expect(viewer.selectedText).toBe(said(1));
  });

  it("selects a whole page, after the page above went", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    // The first call finds nothing and asks; the retry runs when it lands.
    viewer.selectPage();
    await settle();
    // The text matters more here than anywhere else on this page: select-all did
    // not fail on an edited document, it succeeded and handed back the *deleted*
    // page's characters, which a reader then copied.
    expect(asked).toEqual([1]);
    expect(viewer.selectedText).toBe(said(1));
  });

  it("counts the selected characters, after the page above went", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    await dragAcross(viewer, 0);
    dom.runFrames();
    await settle();
    expect(status?.selected).toBe(2);
  });

  it("answers a page's unturned text, after the page above went", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    const text = await viewer.unturnedText(0);
    expect(asked).toEqual([1]);
    expect(text?.codes.length).toBe(2);
  });

  it("hands back a selection's quads by page, after the page above went", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    await dragAcross(viewer, 0);
    const byPage = viewer.selectionQuadsByPage();
    // One entry, naming the page's **id** rather than either of the two page
    // numbers, with the characters page 1 actually spells. A mark is built from
    // this, so a wrong page here writes a highlight into the file over words
    // nobody selected.
    expect(byPage.map((entry) => [entry.page, entry.text])).toEqual([
      [2, said(1)],
    ]);
  });

  it("drops the cropped page's extraction, not the extraction at its slot", async () => {
    const viewer = build();
    deleteFirstPage(viewer);
    await dragAcross(viewer, 0);
    expect(asked).toEqual([1]);
    expect(viewer.textOn(0)).not.toBeNull();

    // A crop on slot 0, which is page 1. Character boxes are measured from the
    // displayed page's corner and a crop moves that corner, so an extraction
    // taken under the old box is not stale --- it is in another space --- and
    // has to be dropped under the number it was stored under. A viewer that
    // dropped the *slot*'s number would leave page 1's boxes in the cache and
    // go on placing carets with them, which is what this asserts against: the
    // observable is the entry being gone, not a re-fetch.
    viewer.setPages([
      {
        id: pageId(2),
        source: { baseline: 1 },
        turns: 0,
        crop: [0, 0, 300, 400],
      },
      { id: pageId(3), source: { baseline: 2 }, turns: 0 },
    ]);
    await settle();
    expect(viewer.textOn(0)).toBeNull();
  });

  it("places a caret on a page after it has been cropped", async () => {
    // The other half of the crop, and a defect in its own right: the page's
    // extraction is dropped because it was measured in another space, so the
    // pointer needs a *new* one. The viewer used to keep its own record of what
    // had been asked for, which nothing cleared --- so it never asked again and
    // no caret could be placed on that page for the rest of the session.
    //
    // On an untouched document deliberately. This one is not about slots at
    // all: it was reachable by cropping any page of any document, and putting a
    // deletion in the fixture would suggest an edit was needed.
    const viewer = build();
    await dragAcross(viewer, 0);
    expect(asked).toEqual([0]);
    expect(viewer.selectedText).toBe(said(0));

    viewer.setPages([
      {
        id: pageId(1),
        source: { baseline: 0 },
        turns: 0,
        crop: [0, 0, 300, 400],
      },
      { id: pageId(2), source: { baseline: 1 }, turns: 0 },
      { id: pageId(3), source: { baseline: 2 }, turns: 0 },
    ]);
    await settle();
    viewer.clearSelection();

    await dragAcross(viewer, 0);
    // Asked a second time --- the extraction under the new box --- and the
    // caret placed from it. Both, because a viewer that re-asked and still
    // could not place a caret would pass the first assertion alone.
    expect(asked).toEqual([0, 0]);
    expect(viewer.selectedText).toBe(said(0));
  });

  it("copies a selection that runs across a page tpdf made", async () => {
    // A blank page has no text, which is not the same as a page whose text
    // could not be read --- and the copy path took the same `null` for both.
    // The status line and the copy therefore disagreed about one selection:
    // `"ab\ncd"` on screen, and "Some of the selected pages' text could not be
    // read, so nothing was copied" when the reader pressed Copy.
    const viewer = build();
    viewer.setPages([
      { id: pageId(1), source: { baseline: 0 }, turns: 0 },
      { id: pageId(9), source: { blank: { width: 600, height: 800 } }, turns: 0 },
      { id: pageId(2), source: { baseline: 1 }, turns: 0 },
    ]);

    // Both ends warmed, then dragged across the blank page between them.
    press(viewer, 0);
    await settle();
    press(viewer, 2);
    await settle();
    press(viewer, 0);
    const to = viewer.screenPoint(2, 28, 15);
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      target: dom.root,
      clientX: to.x,
      clientY: to.y,
    });
    dom.root.dispatch("pointerup", { pointerId: 1, target: dom.root });
    await settle();

    // What the reader sees, first --- a copy that matched a *broken* selection
    // would satisfy the rest of this test.
    expect(viewer.selectedText).toBe(`${said(0)}\n${said(1)}`);

    expect(await viewer.copySelection()).toBe(`${said(0)}\n${said(1)}`);
    expect(written).toEqual([`${said(0)}\n${said(1)}`]);
    // And no complaint about pages that were read without difficulty. Asserted
    // because the failure this replaced was a message rather than a silence.
    expect(errors).toEqual([]);
  });

  it("does not ask again for a page that could not be read", async () => {
    // What the record the viewer used to keep was protecting, and it has to
    // survive moving into the cache: a failure caches no text, so a frame loop
    // that took "not here" for "never asked" issues a fresh `page_text` every
    // frame for the life of the document.
    const viewer = build();
    core.invoke.mockImplementation((command: string) =>
      command === "page_text"
        ? Promise.reject(new Error("no text"))
        : Promise.resolve(null),
    );
    press(viewer, 0);
    await settle();
    for (let frame = 0; frame < 5; frame++) {
      dom.runFrames();
      await settle();
      press(viewer, 0);
      await settle();
    }
    expect(
      core.invoke.mock.calls.filter((call) => call[0] === "page_text").length,
    ).toBe(1);
  });
});
