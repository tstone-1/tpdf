import { describe, expect, it } from "vitest";

import { copyOf, duplicate, type Copy, type DuplicateDeps } from "./duplicate";
import type { Form, FormWidget } from "./forms";
import { pageId, type MarkKind, type MarkView, type PageView, type PlacedField } from "./pages";
import { SAVED_BASE, type FieldEdited } from "./savedfields";

type Rect = [number, number, number, number];

function mark(id: number, kind: MarkKind, note: string, field?: PlacedField): MarkView {
  return {
    id, kind, stamp: null, page: pageId(5), quads: [20, 20, 120, 40], strokes: [],
    color: [0.2, 0.4, 0.6], width: 3, note, lines: [], ...(field ? { field } : {}),
  } as MarkView;
}

function widget(n: number, name: string, extra: Partial<FormWidget> = {}): FormWidget {
  return {
    object: [n, 0], widget: [n, 0], page: 0, rect: [0, 0, 0, 0], display_rect: [20, 20, 120, 40], name,
    value: "", control: { kind: "text" }, multiline: false, max_length: null, reason: null,
    ...extra,
  };
}

const bytes = (text: string) => [...new TextEncoder().encode(text)];
const PAGES: PageView[] = [{ id: pageId(5), source: { baseline: 0 }, turns: 0 }];
const RECT: Rect = [32, 32, 132, 52];

const MARKS: MarkView[] = [
  mark(1, "field", "Text 1", { kind: "text", border: true, tooltip: "Tip", required: true, max_length: 9, align: "right" }),
  mark(2, "field", "Pay", { kind: "radio", border: false, options: ["Choice 1"], read_only: true }),
  mark(3, "field", "Dropdown 1", { kind: "dropdown", border: false, options: ["A", "B"] }),
  mark(4, "square", "a box"),
  mark(5, "ellipse", ""),
  mark(6, "textbox", "words"),
  mark(7, "field", "Checkbox 1", { kind: "checkbox", border: false }),
];

const FORM: Form = {
  widgets: [
    widget(11, "Name", { tooltip: "Your name", max_length: 30, align: "center", required: true }),
    widget(12, "Notes", { multiline: true }),
    widget(13, "Agree", { control: { kind: "checkbox" }, read_only: true }),
    widget(14, "Colour", {
      control: { kind: "choice", combo: true, editable: false, multiple: false,
        options: [{ export: "r", label: "Red" }, { export: "g", label: "Green" }] },
      value: [],
    }),
    widget(15, "Send", { control: { kind: "radio", index: 0, states: [bytes("Choice 1")], unison: false, no_toggle_off: true }, value: [] }),
    widget(16, "List", {
      control: { kind: "choice", combo: false, editable: false, multiple: false, options: [{ export: "a", label: "a" }] }, value: [],
    }),
    widget(17, "Typed", {
      control: { kind: "choice", combo: true, editable: true, multiple: false, options: [{ export: "a", label: "a" }] }, value: "",
    }),
    widget(18, "Group.Inner", { control: { kind: "radio", index: 0, states: [bytes("x")], unison: false, no_toggle_off: true }, value: [] }),
    widget(19, "Sig", { control: { kind: "unsupported" } }),
  ],
};
const NAMES = FORM.widgets.map((one) => one.name);

const state = (marks: MarkView[] = MARKS, fields: FieldEdited[] = []) => ({ marks, fields, pages: PAGES });
const copy = (id: number, marks: MarkView[] = MARKS, border = false) => copyOf(id, RECT, state(marks), FORM, NAMES, border);

describe("the copy of a placed mark", () => {
  it("is a field of the same kind and properties under the next free name, where it was told to go", () => {
    expect(copy(1)).toEqual({
      kind: "field", page: 5, quads: [32, 32, 132, 52], note: "Text 2", color: [0.2, 0.4, 0.6], width: 3,
      field: { kind: "text", border: true, tooltip: "Tip", required: true, max_length: 9, align: "right" },
    });
    expect(copy(7)).toMatchObject({ note: "Checkbox 2", field: { kind: "checkbox", border: false } });
  });

  it("keeps a dropdown's choices", () => {
    expect(copy(3)).toMatchObject({ note: "Dropdown 2", field: { kind: "dropdown", border: false, options: ["A", "B"] } });
  });

  it("keeps a radio button in its group, with the next value the group does not have", () => {
    expect(copy(2)).toMatchObject({
      note: "Pay", field: { kind: "radio", border: false, options: ["Choice 2"], read_only: true },
    });
  });

  it("is a box or an ellipse with its note, colour and line", () => {
    expect(copy(4)).toEqual({
      kind: "square", page: 5, quads: [32, 32, 132, 52], note: "a box", color: [0.2, 0.4, 0.6], width: 3,
    });
    expect(copy(5)).toMatchObject({ kind: "ellipse", note: "" });
  });

  it("is refused for a kind that is not copied, and for a mark that is gone", () => {
    expect(copy(6)).toBe("Only form fields, boxes and ellipses are duplicated");
    expect(copy(99)).toBe("That mark is no longer on the page");
  });
});

describe("the copy of a field of the file", () => {
  const saved = (at: number, border = false) => copy(SAVED_BASE + at, MARKS, border);

  it("is a placed field of its kind, with its properties and a name the form does not have", () => {
    expect(saved(0, true)).toEqual({
      kind: "field", page: 5, quads: [32, 32, 132, 52], note: "Text 2", color: [0.1, 0.35, 0.75], width: 1,
      field: { kind: "text", border: true, tooltip: "Your name", required: true, max_length: 30, align: "center" },
    });
    expect(saved(1)).toMatchObject({ field: { kind: "multiline", border: false } });
    expect(saved(2)).toMatchObject({ note: "Checkbox 2", field: { kind: "checkbox", read_only: true } });
    expect(saved(3)).toMatchObject({ note: "Dropdown 2", field: { kind: "dropdown", options: ["Red", "Green"] } });
  });

  it("carries what a reader has changed about it and not yet saved", () => {
    const edits: FieldEdited[] = [{ object: [11, 0], page: 5, removed: false, props: { tooltip: "Changed", required: false } }];
    const got = copyOf(SAVED_BASE, RECT, state(MARKS, edits), FORM, NAMES, false);
    expect(got).toMatchObject({ field: { tooltip: "Changed" } });
    expect((got as Copy).field?.required).toBeUndefined();
  });

  it("joins a radio button's group, past the values the file's buttons have", () => {
    expect(saved(4)).toMatchObject({ note: "Send", field: { kind: "radio", options: ["Choice 2"] } });
  });

  it("is refused for a kind tpdf does not place, a button inside a group of fields, and a field that is gone", () => {
    expect(saved(5)).toContain("cannot be duplicated yet");
    expect(saved(6)).toContain("cannot be duplicated yet");
    expect(saved(7)).toBe("A radio button inside a group of fields cannot be duplicated yet");
    expect(saved(8)).toBe("This kind of field cannot be duplicated");
    expect(saved(40)).toBe("That field is no longer in the document");
    expect(copyOf(SAVED_BASE, RECT, state(), null, NAMES, false)).toBe("That field is no longer in the document");
  });
});

function deps(picked: number[], update: { refuse?: boolean; none?: boolean } = {}) {
  const world = { marks: [...MARKS], next: 50 };
  const made: [Copy, number][] = [];
  const said: string[] = [];
  const pickedAfter: number[][] = [];
  const it: DuplicateDeps = {
    copies: () => (update.none ? null : { sweep: 9, rects: picked.map((id) => ({ mark: id, rect: RECT })) }),
    state: () => state(world.marks),
    form: () => FORM,
    formNames: () => NAMES,
    border: () => true,
    make: async (one, sweep) => {
      made.push([one, sweep]);
      if (update.refuse) return;
      world.marks = [...world.marks, { ...mark(world.next, one.kind, one.note, one.field), quads: one.quads }];
      world.next += 1;
    },
    pick: (ids) => pickedAfter.push(ids),
    say: (message) => said.push(message),
  };
  return { deps: it, made, said, pickedAfter };
}

describe("duplicating the picked marks", () => {
  it("makes a copy of each under the one gesture and picks the copies", async () => {
    const run = deps([1, 4]);
    await expect(duplicate(run.deps)).resolves.toBe(2);
    expect(run.made.map(([one, sweep]) => [one.kind, one.note, sweep])).toEqual([["field", "Text 2", 9], ["square", "a box", 9]]);
    expect(run.pickedAfter).toEqual([[50, 51]]);
    expect(run.said).toEqual([]);
  });

  it("names each copy after the copy made before it", async () => {
    const run = deps([1, 1, 2, 2]);
    await duplicate(run.deps);
    expect(run.made.map(([one]) => [one.note, one.field?.options?.[0]])).toEqual([
      ["Text 2", undefined], ["Text 3", undefined], ["Pay", "Choice 2"], ["Pay", "Choice 3"],
    ]);
  });

  it("says why one is not copied and copies the rest", async () => {
    const run = deps([6, 1]);
    await expect(duplicate(run.deps)).resolves.toBe(1);
    expect(run.said).toEqual(["Only form fields, boxes and ellipses are duplicated"]);
    expect(run.pickedAfter).toEqual([[50]]);
  });

  it("picks nothing and makes nothing with none picked", async () => {
    const run = deps([], { none: true });
    await expect(duplicate(run.deps)).resolves.toBe(0);
    expect(run.made).toEqual([]);
    expect(run.pickedAfter).toEqual([]);
  });

  it("leaves the pick alone when the model made none of them", async () => {
    const run = deps([1], { refuse: true });
    await expect(duplicate(run.deps)).resolves.toBe(0);
    expect(run.made).toHaveLength(1);
    expect(run.pickedAfter).toEqual([]);
  });
});
