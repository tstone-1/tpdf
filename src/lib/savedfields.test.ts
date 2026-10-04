import { describe, expect, it } from "vitest";

import type { Form, FormWidget } from "./forms";
import { pageId, type MarkView, type PageView } from "./pages";
import {
  SAVED_BASE, arrangeBoth, asMarks, isSaved, leafName, moved, placed, removed, renamed, shownAt, split,
  standing,
  type FieldEdited,
} from "./savedfields";

function widget(n: number, name: string, page: number, rect: [number, number, number, number], extra: Partial<FormWidget> = {}): FormWidget {
  return {
    object: [n, 0], widget: [n, 0], page, rect: [0, 0, 0, 0], display_rect: rect, name,
    value: "", control: { kind: "text" }, multiline: false, max_length: null, reason: null,
    ...extra,
  };
}

const FORM: Form = {
  widgets: [
    widget(11, "Name", 0, [20, 20, 120, 40]),
    widget(12, "Agree", 0, [20, 60, 32, 72], { control: { kind: "checkbox" } }),
    widget(13, "Notes", 1, [20, 20, 220, 90], { multiline: true }),
    widget(14, "Group.Inner", 0, [20, 100, 120, 120]),
  ],
};

// The file's two pages, in the other order, with ids that are not positions.
const PAGES: PageView[] = [
  { id: pageId(8), source: { baseline: 1 }, turns: 0 },
  { id: pageId(5), source: { baseline: 0 }, turns: 0 },
];

const state = (fields: FieldEdited[] = [], marks: MarkView[] = [], pages = PAGES) => ({ fields, marks, pages });

describe("the saved fields as marks", () => {
  it("shows each field on the page it is on, under an id of its own, with its kind and name", () => {
    const marks = asMarks(FORM, state());
    expect(marks.map((mark) => [mark.id - SAVED_BASE, mark.page, mark.note, mark.field?.kind, mark.quads])).toEqual([
      [0, 5, "Name", "text", [20, 20, 120, 40]],
      [1, 5, "Agree", "checkbox", [20, 60, 32, 72]],
      [2, 8, "Notes", "multiline", [20, 20, 220, 90]],
      [3, 5, "Inner", "text", [20, 100, 120, 120]],
    ]);
    expect(marks.every((mark) => mark.kind === "field" && isSaved(mark.id))).toBe(true);
    expect(isSaved(41)).toBe(false);
    expect(leafName("a.b.c")).toBe("c");
    expect(leafName("plain")).toBe("plain");
  });

  it("shows a field where it was moved to and under its new name, and not one that was removed", () => {
    const edits: FieldEdited[] = [
      { object: [11, 0], page: 5, rect: [50, 50, 150, 70], name: "Full name", removed: false },
      { object: [12, 0], page: 5, removed: true },
    ];
    const marks = asMarks(FORM, state(edits));
    expect(marks.map((mark) => [mark.id - SAVED_BASE, mark.note, mark.quads])).toEqual([
      [0, "Full name", [50, 50, 150, 70]],
      [2, "Notes", [20, 20, 220, 90]],
      [3, "Inner", [20, 100, 120, 120]],
    ]);
    // The filling layer gets the same form: moved, and without the removed one.
    expect(standing(FORM, state(edits)).map((w) => [w.name, w.display_rect])).toEqual([
      ["Name", [50, 50, 150, 70]],
      ["Notes", [20, 20, 220, 90]],
      ["Group.Inner", [20, 100, 120, 120]],
    ]);
    // The control: with nothing changed it is the form.
    expect(standing(FORM, state())).toEqual(FORM.widgets);
  });

  it("shows nothing for a field whose page is gone", () => {
    const onePage = state([], [], [PAGES[1]!]);
    expect(asMarks(FORM, onePage).map((mark) => mark.note)).toEqual(["Name", "Agree", "Inner"]);
  });
});

describe("what a change to a saved field asks for", () => {
  it("moves a field from where it now is, by widget and page id", () => {
    expect(moved(FORM, state(), SAVED_BASE, 10, -5)).toEqual({
      object: [11, 0], page: 5, patch: { rect: [30, 15, 130, 35] },
    });
    // From where an earlier move left it, not from where the file has it.
    const edits: FieldEdited[] = [{ object: [11, 0], page: 5, rect: [50, 50, 150, 70], removed: false }];
    expect(moved(FORM, state(edits), SAVED_BASE, 10, -5)?.patch.rect).toEqual([60, 45, 160, 65]);
    expect(moved(FORM, state(), SAVED_BASE + 2, 1, 1)?.page).toBe(8);
  });

  it("gives a field a rectangle, and removes one", () => {
    expect(placed(FORM, state(), SAVED_BASE + 1, [1, 2, 3, 4])).toEqual({
      object: [12, 0], page: 5, patch: { rect: [1, 2, 3, 4] },
    });
    expect(removed(FORM, state(), SAVED_BASE + 1)).toEqual({
      object: [12, 0], page: 5, patch: { removed: true },
    });
  });

  it("answers nothing for an id that names no saved field, or one whose page is gone", () => {
    expect(moved(FORM, state(), 7, 1, 1)).toBeNull();
    expect(placed(FORM, state(), SAVED_BASE + 99, [1, 2, 3, 4])).toBeNull();
    expect(removed(FORM, state([], [], [PAGES[1]!]), SAVED_BASE + 2)).toBeNull();
    expect(renamed(FORM, state(), 7, "x")).toBeNull();
  });

  it("renames a field, and refuses a name another field will have", () => {
    expect(renamed(FORM, state(), SAVED_BASE, "Full name")).toEqual({
      object: [11, 0], page: 5, patch: { name: "Full name" },
    });
    expect(renamed(FORM, state(), SAVED_BASE, "Agree")).toBe("`Agree`: another field has this name");
    // Its own name is not another field's.
    expect(renamed(FORM, state(), SAVED_BASE, "Name")).toEqual({
      object: [11, 0], page: 5, patch: { name: "Name" },
    });
    // A name a removed field had is free, and so is one a field gave up.
    const freed: FieldEdited[] = [
      { object: [12, 0], page: 5, removed: true },
      { object: [13, 0], page: 8, name: "Remarks", removed: false },
    ];
    expect(typeof renamed(FORM, state(freed), SAVED_BASE, "Agree")).toBe("object");
    expect(typeof renamed(FORM, state(freed), SAVED_BASE, "Notes")).toBe("object");
    // And the name it was given is taken.
    expect(typeof renamed(FORM, state(freed), SAVED_BASE, "Remarks")).toBe("string");
    // A field placed in this session and not saved yet holds its name too.
    const placedMark = { kind: "field", note: "Fresh" } as MarkView;
    expect(typeof renamed(FORM, state([], [placedMark]), SAVED_BASE, "Fresh")).toBe("string");
    // A field inside a group is not compared with the top of the form.
    expect(typeof renamed(FORM, state(), SAVED_BASE + 3, "Name")).toBe("object");
  });

  it("splits an arrangement into the placed marks and the saved fields", () => {
    const moves = [
      { mark: 4, rect: [1, 1, 2, 2] as [number, number, number, number] },
      { mark: SAVED_BASE + 1, rect: [5, 5, 9, 9] as [number, number, number, number] },
      { mark: SAVED_BASE + 77, rect: [5, 5, 9, 9] as [number, number, number, number] },
    ];
    expect(split(FORM, state(), moves)).toEqual({
      marks: [{ mark: 4, rect: [1, 1, 2, 2] }],
      fields: [{ object: [12, 0], page: 5, patch: { rect: [5, 5, 9, 9] } }],
    });
  });
});

describe("where a field's control is put", () => {
  it("is where the field was moved to, where the file has it, or nowhere once removed", () => {
    const name = FORM.widgets[0]!;
    expect(shownAt(name, state())).toEqual([20, 20, 120, 40]);
    expect(shownAt(name, state([{ object: [11, 0], page: 5, rect: [1, 2, 3, 4], removed: false }]))).toEqual([1, 2, 3, 4]);
    expect(shownAt(name, state([{ object: [11, 0], page: 5, rect: [1, 2, 3, 4], removed: true }]))).toBeNull();
    // Another widget's change is not this one's.
    expect(shownAt(name, state([{ object: [12, 0], page: 5, removed: true }]))).toEqual([20, 20, 120, 40]);
  });
});

describe("an arrangement of placed marks, saved fields or both", () => {
  const rect = [1, 1, 2, 2] as [number, number, number, number];
  function recorder() {
    const calls: string[] = [];
    const reply = { fields: [], pages: PAGES, marks: [] } as never;
    return {
      calls,
      edits: {
        arrange: async (moves: { mark: number }[], sweep: number) => {
          calls.push(`arrange ${moves.map((m) => m.mark).join()} under ${sweep}`);
          return reply;
        },
        refield: async (targets: { object: [number, number] }[], sweep = 0) => {
          calls.push(`refield ${targets.map((t) => t.object[0]).join()} under ${sweep}`);
          return reply;
        },
      },
    };
  }

  it("sends placed marks alone as one arrangement", async () => {
    const { calls, edits } = recorder();
    await arrangeBoth(FORM, state(), edits, [{ mark: 4, rect }, { mark: 6, rect }], 9);
    expect(calls).toEqual(["arrange 4,6 under 9"]);
  });

  it("sends saved fields alone as one change, and asks for no empty arrangement", async () => {
    const { calls, edits } = recorder();
    await arrangeBoth(FORM, state(), edits, [{ mark: SAVED_BASE, rect }, { mark: SAVED_BASE + 1, rect }], 9);
    expect(calls).toEqual(["refield 11,12 under 9"]);
  });

  it("sends both under the one gesture, the marks first", async () => {
    const { calls, edits } = recorder();
    await arrangeBoth(FORM, state(), edits, [{ mark: SAVED_BASE, rect }, { mark: 4, rect }], 9);
    expect(calls).toEqual(["arrange 4 under 9", "refield 11 under 9"]);
  });

  it("treats every move as a mark's when no form was scanned", async () => {
    const { calls, edits } = recorder();
    await arrangeBoth(null, state(), edits, [{ mark: 4, rect }], 9);
    expect(calls).toEqual(["arrange 4 under 9"]);
  });
});
