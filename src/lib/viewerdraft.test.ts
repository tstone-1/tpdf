/**
 * What the page draws while a reader types in the box beside a mark.
 *
 * `markdraft.test.ts` asks the drafts directly. This is the join: a keystroke
 * in the viewer's own box, through the viewer, to the words the overlay paints.
 *
 * ## The overlay is read here, which `testdom.ts` does not do
 *
 * The fake canvas hands out no context, so no other test sees a painted word.
 * Here the overlay's context is a recorder: every call is kept, and
 * {@link painted} answers with the strings the last frame drew with
 * `fillText`. That is the call the text box and the field's name go on with,
 * and it says nothing about a pixel. The tile surface asks for its context
 * with options and still gets none.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { INK_WIDTH } from "./markband";
import { pageId, type MarkKind, type MarkView } from "./pages";
import { FakeElement, installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

/** One wrap the viewer asked for, and the two ways to answer it. */
interface Question {
  note: string;
  left: number;
  right: number;
  answer: (lines: string[]) => void;
  refuse: () => void;
}

let dom: FakeDom;
/** Every call the overlay's context was given, in order. */
let calls: { name: string; args: unknown[] }[];
let asked: Question[];
let notes: { mark: number; note: string }[];
/** Settles the edit the last committed note was sent with. */
let settleEdit: () => void;

beforeEach(() => {
  dom = installFakeDom();
  calls = [];
  asked = [];
  notes = [];
  settleEdit = () => {};
  core.invoke.mockResolvedValue(null);
  // No tile ever arrives: these tests are about the overlay, which is drawn
  // every frame whether or not there is a page under it.
  tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
  const recorder = new Proxy({} as Record<string, unknown>, {
    get: (held, name: string) =>
      name in held
        ? held[name]
        : (...args: unknown[]) => {
            calls.push({ name, args });
            return { width: 0 };
          },
    set: (held, name: string, value) => {
      held[name] = value;
      return true;
    },
  });
  vi.spyOn(FakeElement.prototype, "getContext").mockImplementation(function (
    ...args: unknown[]
  ) {
    return (args.length === 1 ? recorder : null) as null;
  });
});

afterEach(() => {
  dom.restore();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

function one(id: number, kind: MarkKind, over: Partial<MarkView> = {}): MarkView {
  return {
    id, kind, stamp: null, page: pageId(1), quads: [100, 100, 300, 160], strokes: [],
    color: [0.85, 0.15, 0.15], width: INK_WIDTH, note: "", lines: [], ...over,
  };
}

const BOX = one(1, "textbox", { note: "old words", lines: ["old", "words"] });
const FIELD = one(2, "field", { quads: [100, 300, 300, 330], note: "Name" });
const HIGHLIGHT = one(3, "highlight", { quads: [100, 500, 300, 512], note: "a remark" });
const MARKS = [BOX, FIELD, HIGHLIGHT];

function build(wrap = true): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
    onMarkNote: (mark, note) => {
      notes.push({ mark, note });
      return new Promise<void>((done) => {
        settleEdit = done;
      });
    },
    ...(wrap
      ? {
          onMarkDraft: (note: string, left: number, right: number) =>
            new Promise<string[]>((answer, refuse) => {
              asked.push({ note, left, right, answer, refuse: () => refuse(new Error("no")) });
            }),
        }
      : {}),
  });
  viewer.setMarks(MARKS);
  return viewer;
}

/** Runs the frames that are due and answers with the words the last one drew. */
async function painted(): Promise<string[]> {
  await settle();
  for (let turn = 0; turn < 4; turn++) dom.runFrames();
  const from = calls.map((call) => call.name).lastIndexOf("clearRect");
  if (from < 0) throw new Error("the overlay was never painted");
  return calls.slice(from).filter((call) => call.name === "fillText").map((call) => String(call.args[0]));
}

/** Types `text` into the open box, as a keystroke does: the value, then the event. */
function type(viewer: Viewer, text: string): void {
  const field = viewer.markNoteField as unknown as FakeElement & { value: string };
  field.value = text;
  field.dispatch("input", {});
}

describe("what the page draws while a note is typed", () => {
  it("draws the model's words for a text box and a field before anything is typed", async () => {
    build();
    // The control for every assertion below: this is what "the old words" is,
    // and that the recorder sees the overlay at all.
    expect(await painted()).toEqual(["old", "words", "Name"]);
  });

  it("draws a text box's words as they are typed, in the lines it was answered with", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "hello there");
    expect(asked.map(({ note, left, right }) => ({ note, left, right }))).toEqual([
      { note: "hello there", left: 100, right: 300 },
    ]);
    asked[0]!.answer(["hello", "there"]);
    expect(await painted()).toEqual(["hello", "there", "Name"]);
    // The box is still open and the model has heard nothing.
    expect(viewer.markOpen).toBe(1);
    expect(notes).toEqual([]);
  });

  it("does not draw an answer to a question that is no longer the newest", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "h");
    type(viewer, "he");
    asked[1]!.answer(["he"]);
    expect(await painted()).toEqual(["he", "Name"]);
    asked[0]!.answer(["h"]);
    expect(await painted()).toEqual(["he", "Name"]);
  });

  it("does not draw an answer that arrives after the box closed without a change", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "hello");
    // Back to what the model has, so closing sends nothing.
    type(viewer, "old words");
    viewer.closeMark();
    expect(notes).toEqual([]);
    asked[0]!.answer(["hello"]);
    asked[1]!.answer(["old words, wrapped otherwise"]);
    expect(await painted()).toEqual(["old", "words", "Name"]);
  });

  it("keeps the last words it drew when the wrap fails, and types on", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "h");
    asked[0]!.answer(["h"]);
    expect(await painted()).toEqual(["h", "Name"]);
    type(viewer, "he");
    asked[1]!.refuse();
    expect(await painted()).toEqual(["h", "Name"]);
    type(viewer, "hel");
    asked[2]!.answer(["hel"]);
    expect(await painted()).toEqual(["hel", "Name"]);
  });

  it("draws a field's name as it is typed, and asks nobody", async () => {
    const viewer = build();
    viewer.showMark(2);
    type(viewer, "Surname");
    expect(await painted()).toEqual(["old", "words", "Surname"]);
    expect(asked).toEqual([]);
    expect(notes).toEqual([]);
  });

  it("draws a field's name as typed with no wrap wired, and a text box's words on close as before", async () => {
    const viewer = build(false);
    viewer.showMark(2);
    type(viewer, "Surname");
    expect(await painted()).toEqual(["old", "words", "Surname"]);
    viewer.showMark(1);
    type(viewer, "hello");
    expect((await painted()).slice(0, 2)).toEqual(["old", "words"]);
  });

  it("changes nothing on the page for a note on a highlight", async () => {
    const viewer = build();
    const before = await painted();
    viewer.showMark(3);
    type(viewer, "something else entirely");
    expect(await painted()).toEqual(before);
    expect(asked).toEqual([]);
  });

  it("sends the model nothing while typing: one note, when the box closes", async () => {
    const viewer = build();
    viewer.showMark(1);
    for (const text of ["h", "he", "hel", "hell", "hello"]) type(viewer, text);
    expect(notes).toEqual([]);
    viewer.closeMark();
    expect(notes).toEqual([{ mark: 1, note: "hello" }]);
  });

  it("drops the draft when the mark is removed under the box", async () => {
    const viewer = build();
    viewer.showMark(2);
    type(viewer, "Surname");
    expect(await painted()).toEqual(["old", "words", "Surname"]);
    // An undo took the box's mark away and brought it back: the box closed
    // without committing, and what was typed in it is not drawn.
    viewer.setMarks([BOX, HIGHLIGHT]);
    await painted();
    viewer.setMarks(MARKS);
    expect(await painted()).toEqual(["old", "words", "Name"]);
    expect(notes).toEqual([]);
  });

  it("on Escape sends the note and keeps drawing it until the model has answered", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "hello there");
    asked[0]!.answer(["hello", "there"]);
    await painted();
    (viewer.markPopup as unknown as FakeElement).dispatch("keydown", { key: "Escape" });
    expect(viewer.markOpen).toBe(-1);
    expect(notes).toEqual([{ mark: 1, note: "hello there" }]);
    // The model still has the old words, and they are not drawn.
    expect(await painted()).toEqual(["hello", "there", "Name"]);
    // The model answers: its marks arrive, and then the edit settles.
    viewer.setMarks([{ ...BOX, note: "hello there", lines: ["hello there"] }, FIELD, HIGHLIGHT]);
    expect(await painted()).toEqual(["hello", "there", "Name"]);
    settleEdit();
    expect(await painted()).toEqual(["hello there", "Name"]);
  });

  it("shows what the model has when the edit settles without taking the note", async () => {
    const viewer = build();
    viewer.showMark(2);
    type(viewer, "a.b");
    viewer.closeMark();
    expect(await painted()).toEqual(["old", "words", "a.b"]);
    // Refused: no new marks, and the edit is over.
    settleEdit();
    expect(await painted()).toEqual(["old", "words", "Name"]);
  });

  it("asks again for a text box resized under its open box", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "hello there");
    asked[0]!.answer(["hello there"]);
    await painted();
    viewer.setMarks([{ ...BOX, quads: [100, 100, 140, 160] }, FIELD, HIGHLIGHT]);
    expect(asked).toHaveLength(2);
    expect(asked[1]).toMatchObject({ note: "hello there", left: 100, right: 140 });
    asked[1]!.answer(["hello", "there"]);
    expect(await painted()).toEqual(["hello", "there", "Name"]);
  });

  it("starts no frame for an answer that arrives after the viewer is gone", async () => {
    const viewer = build();
    viewer.showMark(1);
    type(viewer, "hello");
    await painted();
    viewer.destroy();
    dom.runFrames();
    dom.reset();
    asked[0]!.answer(["hello"]);
    await settle();
    expect(dom.scheduledFrames()).toBe(0);
  });
});
