import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  DIALOG_CLASS, FieldPropertiesDialog, MAX_LENGTH, MAX_TOOLTIP, changeProperties, pickedField, read, shown,
  type PropertiesDeps, type Typed,
} from "./fieldprops";
import type { Form, FormWidget } from "./forms";
import { pageId, type MarkView, type PageView, type PlacedField } from "./pages";
import { SAVED_BASE, type FieldEdited, type FieldProperties, type FieldProps, type FieldTarget } from "./savedfields";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

const TEXT: FieldProperties = {
  name: "Name", tooltip: "Your name", required: true, readOnly: false, maxLength: 30, align: "center", options: null,
};
const BOX: FieldProperties = {
  name: "Agree", tooltip: "", required: false, readOnly: true, maxLength: null, align: null, options: null,
};
const CHOICE: FieldProperties = {
  name: "Colour", tooltip: "", required: false, readOnly: false, maxLength: null, align: "left", options: ["Red", "Green"],
};

describe("what the panel shows and reads", () => {
  it("shows a field's properties as text and ticks", () => {
    expect(shown(TEXT)).toEqual({
      tooltip: "Your name", required: true, readOnly: false, maxLength: "30", align: "center", options: "",
    });
    expect(shown({ ...TEXT, maxLength: 0 }).maxLength).toBe("");
    expect(shown(BOX)).toEqual({ tooltip: "", required: false, readOnly: true, maxLength: "", align: "left", options: "" });
    expect(shown(CHOICE).options).toBe("Red; Green");
  });

  it("reads back what it showed as the same properties", () => {
    for (const now of [TEXT, BOX, CHOICE, { ...TEXT, maxLength: 0 }]) expect(read(now, shown(now))).toEqual(now);
  });

  it("reads what a reader typed, for the parts the field's kind has", () => {
    const typed: Typed = { tooltip: "  Tip  ", required: false, readOnly: true, maxLength: " 12 ", align: "right", options: "A; B;" };
    expect(read(TEXT, typed)).toEqual({
      name: "Name", tooltip: "Tip", required: false, readOnly: true, maxLength: 12, align: "right", options: null,
    });
    expect(read(BOX, typed)).toEqual({
      name: "Agree", tooltip: "Tip", required: false, readOnly: true, maxLength: null, align: null, options: null,
    });
    expect(read(CHOICE, typed)).toEqual({
      name: "Colour", tooltip: "Tip", required: false, readOnly: true, maxLength: null, align: "right", options: ["A", "B"],
    });
  });

  it("refuses what cannot be a field's, and says which", () => {
    const typed = (part: Partial<Typed>) => ({ ...shown(TEXT), ...part });
    const problem = (now: FieldProperties, part: Partial<Typed>) => {
      const got = read(now, { ...shown(now), ...part });
      return "problem" in got ? got.problem : null;
    };
    expect(problem(TEXT, { tooltip: "x".repeat(MAX_TOOLTIP + 1) })).toContain("at most 1024");
    expect(problem(TEXT, { tooltip: "x".repeat(MAX_TOOLTIP) })).toBeNull();
    expect(problem(TEXT, { tooltip: "a\tb" })).toContain("control character");
    for (const bad of ["0", "-1", "1.5", "abc", String(MAX_LENGTH + 1), "1234567"]) {
      expect(problem(TEXT, { maxLength: bad }), bad).toContain("from 1 to 16384");
    }
    expect(read(TEXT, typed({ maxLength: String(MAX_LENGTH) }))).toMatchObject({ maxLength: MAX_LENGTH });
    expect(read(TEXT, typed({ maxLength: "1" }))).toMatchObject({ maxLength: 1 });
    expect(problem(TEXT, { align: "justify" })).toContain("left, centre or right");
    expect(problem(CHOICE, { options: "" })).toContain("semicolon");
    expect(problem(CHOICE, { options: "A; A" })).toContain("twice");
    // A part the kind does not have is not judged.
    expect(problem(BOX, { maxLength: "abc", align: "justify", options: "A; A" })).toBeNull();
  });
});

function widget(n: number, name: string, extra: Partial<FormWidget> = {}): FormWidget {
  return {
    object: [n, 0], widget: [n, 0], page: 0, rect: [0, 0, 0, 0], display_rect: [20, 20, 120, 40], name,
    value: "", control: { kind: "text" }, multiline: false, max_length: null, reason: null,
    ...extra,
  };
}

const FORM: Form = { widgets: [widget(11, "Name", { tooltip: "Your name" }), widget(12, "Other")] };
const PAGES: PageView[] = [{ id: pageId(5), source: { baseline: 0 }, turns: 0 }];

/** A mark as the model reports one, a field when it is given what the field is. */
function mark(id: number, note: string, field?: PlacedField): MarkView {
  return {
    id, kind: field ? "field" : "square", stamp: null, page: pageId(5), quads: [20, 20, 120, 40], strokes: [],
    color: [0, 0, 0], width: 1, note, lines: [], ...(field ? { field } : {}),
  } as MarkView;
}

const MARKS: MarkView[] = [
  mark(3, "Placed", { kind: "text", border: true, tooltip: "Tip", max_length: 9 }),
  mark(4, "A box"),
  mark(5, "Pick", { kind: "dropdown", border: false, options: ["A", "B"], align: "center" }),
  mark(6, "Tick", { kind: "checkbox", border: false, required: true, read_only: true }),
];

function deps(update: {
  picked?: number[];
  form?: Form | null;
  marks?: MarkView[];
  fields?: FieldEdited[];
  answer?: (now: FieldProperties) => FieldProperties | null;
  /** What happens to the window while the panel is open. */
  meanwhile?: (world: { form: Form | null; marks: MarkView[] }) => void;
} = {}): {
  deps: PropertiesDeps; asked: FieldProperties[]; made: FieldTarget[]; fitted: [number, FieldProps][]; said: string[];
} {
  const asked: FieldProperties[] = [];
  const made: FieldTarget[] = [];
  const fitted: [number, FieldProps][] = [];
  const said: string[] = [];
  const world = { form: update.form === undefined ? FORM : update.form, marks: update.marks ?? MARKS };
  return {
    asked, made, fitted, said,
    deps: {
      picked: () => update.picked ?? [SAVED_BASE],
      form: () => world.form,
      state: () => ({ fields: update.fields ?? [], pages: PAGES, marks: world.marks }),
      refit: (id, props) => fitted.push([id, props]),
      ask: async (now) => {
        asked.push(now);
        update.meanwhile?.(world);
        return update.answer ? update.answer(now) : null;
      },
      refield: (target) => made.push(target),
      say: (message) => said.push(message),
    },
  };
}

describe("the picked field", () => {
  it("is the one saved field that is picked", () => {
    expect(pickedField(deps().deps)).toBe(SAVED_BASE);
    expect(pickedField(deps({ picked: [SAVED_BASE + 1] }).deps)).toBe(SAVED_BASE + 1);
  });

  it("is none with nothing picked, several picked, a mark that is no field, or a field that is gone", () => {
    expect(pickedField(deps({ picked: [] }).deps)).toBeNull();
    expect(pickedField(deps({ picked: [SAVED_BASE, SAVED_BASE + 1] }).deps)).toBeNull();
    expect(pickedField(deps({ picked: [3, 5] }).deps)).toBeNull();
    expect(pickedField(deps({ picked: [4] }).deps)).toBeNull();
    expect(pickedField(deps({ picked: [7] }).deps)).toBeNull();
    expect(pickedField(deps({ picked: [SAVED_BASE + 9] }).deps)).toBeNull();
    expect(pickedField(deps({ form: null }).deps)).toBeNull();
  });

  it("is a field placed in this session, whether or not the file's fields are being changed", () => {
    expect(pickedField(deps({ picked: [3] }).deps)).toBe(3);
    expect(pickedField(deps({ picked: [3], form: null }).deps)).toBe(3);
  });
});

describe("changing the picked field's properties", () => {
  it("asks with what the field has and makes the change that differs", async () => {
    const run = deps({ answer: (now) => ({ ...now, tooltip: "Full name", required: true }) });
    await expect(changeProperties(run.deps)).resolves.toBe(true);
    expect(run.asked).toEqual([
      { name: "Name", tooltip: "Your name", required: false, readOnly: false, maxLength: 0, align: "left", options: null },
    ]);
    expect(run.made).toEqual([
      { object: [11, 0], page: 5, patch: { props: { tooltip: "Full name", required: true } } },
    ]);
    expect(run.said).toEqual([]);
  });

  it("makes nothing and says nothing when the panel is dismissed", async () => {
    const run = deps();
    await expect(changeProperties(run.deps)).resolves.toBe(false);
    expect(run.asked.length).toBe(1);
    expect(run.made).toEqual([]);
    expect(run.said).toEqual([]);
  });

  it("says so when the panel is applied with nothing changed", async () => {
    const run = deps({ answer: (now) => now });
    await expect(changeProperties(run.deps)).resolves.toBe(false);
    expect(run.made).toEqual([]);
    expect(run.said).toEqual(["Nothing about the field was changed."]);
  });

  it("does not ask with no one field picked", async () => {
    const run = deps({ picked: [], answer: (now) => ({ ...now, tooltip: "x" }) });
    await expect(changeProperties(run.deps)).resolves.toBe(false);
    expect(run.asked).toEqual([]);
  });

  it("makes nothing when the document went while the panel was open", async () => {
    const run = deps({
      answer: (now) => ({ ...now, tooltip: "x" }),
      meanwhile: (world) => { world.form = null; },
    });
    await expect(changeProperties(run.deps)).resolves.toBe(false);
    expect(run.made).toEqual([]);
  });
});

describe("changing a placed field's properties", () => {
  it("asks with what the field was placed with, by its kind", async () => {
    const asked = async (id: number) => {
      const run = deps({ picked: [id] });
      await changeProperties(run.deps);
      return run.asked[0];
    };
    await expect(asked(3)).resolves.toEqual({
      name: "Placed", tooltip: "Tip", required: false, readOnly: false, maxLength: 9, align: "left", options: null,
    });
    await expect(asked(5)).resolves.toEqual({
      name: "Pick", tooltip: "", required: false, readOnly: false, maxLength: null, align: "center", options: ["A", "B"],
    });
    await expect(asked(6)).resolves.toEqual({
      name: "Tick", tooltip: "", required: true, readOnly: true, maxLength: null, align: null, options: null,
    });
    const lines = deps({ picked: [8], marks: [mark(8, "Lines", { kind: "multiline", border: false })] });
    await changeProperties(lines.deps);
    expect(lines.asked[0]).toMatchObject({ maxLength: 0, align: "left", options: null });
  });

  it("changes the mark, with the parts that differ, and not a field of the file", async () => {
    const run = deps({ picked: [3], answer: (now) => ({ ...now, tooltip: "", maxLength: 0, align: "right" }) });
    await expect(changeProperties(run.deps)).resolves.toBe(true);
    expect(run.fitted).toEqual([[3, { tooltip: "", max_length: 0, align: "right" }]]);
    expect(run.made).toEqual([]);
    const choices = deps({ picked: [5], answer: (now) => ({ ...now, options: ["A", "B", "C"] }) });
    await changeProperties(choices.deps);
    expect(choices.fitted).toEqual([[5, { options: ["A", "B", "C"] }]]);
  });

  it("says so when nothing was changed, and makes nothing for a mark removed meanwhile", async () => {
    const same = deps({ picked: [3], answer: (now) => now });
    await expect(changeProperties(same.deps)).resolves.toBe(false);
    expect(same.fitted).toEqual([]);
    expect(same.said).toEqual(["Nothing about the field was changed."]);
    const gone = deps({
      picked: [3],
      answer: (now) => ({ ...now, required: true }),
      meanwhile: (world) => { world.marks = []; },
    });
    await expect(changeProperties(gone.deps)).resolves.toBe(false);
    expect(gone.fitted).toEqual([]);
    expect(gone.said).toEqual([]);
  });

  it("changes the field that was picked when the panel opened, whatever is picked when it closes", async () => {
    let picked = [3];
    const run = deps({ answer: (now) => ({ ...now, required: true }) });
    const ask = run.deps.ask;
    run.deps.picked = () => picked;
    run.deps.ask = (now) => { picked = [5]; return ask(now); };
    await expect(changeProperties(run.deps)).resolves.toBe(true);
    expect(run.fitted).toEqual([[3, { required: true }]]);
  });
});

describe("FieldPropertiesDialog", () => {
  let dom: FakeDom;
  beforeEach(() => { dom = installFakeDom(); });
  afterEach(() => { dom.restore(); });

  function open() {
    const dialog = new FieldPropertiesDialog(dom.root as unknown as HTMLElement);
    const backdrop = dom.root.children.find((c) => c.classList.contains(DIALOG_CLASS));
    const panel = backdrop?.children[0];
    if (!backdrop || !panel) throw new Error("the dialog did not mount");
    const problem = panel.children.find((c) => c.getAttribute("role") === "alert")!;
    const buttons = panel.children.find((c) => c.children.some((b) => b.tagName === "button"))!;
    const [cancel, apply] = buttons.children as [FakeElement, FakeElement];
    /** The rows, by their caption, and whether each is shown. */
    const rows = () => Object.fromEntries(
      panel.children.filter((c) => c.tagName === "label")
        .map((row) => [row.children[row.children.length - 1]!.getAttribute("aria-label") ?? row.textContent, row.style.display]),
    );
    return { dialog, backdrop, panel, problem, cancel, apply, rows };
  }

  it("shows the field's name and applies what the controls hold", async () => {
    const { dialog, panel, apply } = open();
    const answer = dialog.ask(TEXT);
    expect(panel.children[0]!.textContent).toBe("Properties of Name");
    expect(dialog.isOpen).toBe(true);
    dialog.put({ ...shown(TEXT), tooltip: "Full name", maxLength: "" });
    apply.dispatch("click", {});
    await expect(answer).resolves.toEqual({ ...TEXT, tooltip: "Full name", maxLength: 0 });
    expect(dialog.isOpen).toBe(false);
  });

  it("applies what it showed, unchanged, on Enter", async () => {
    const { dialog, backdrop } = open();
    const answer = dialog.ask(CHOICE);
    backdrop.dispatch("keydown", { key: "Enter" });
    await expect(answer).resolves.toEqual(CHOICE);
  });

  it("shows only the rows the field's kind has", () => {
    const { dialog, rows } = open();
    void dialog.ask(BOX);
    expect(rows()).toMatchObject({ "Most characters": "none", Alignment: "none", Choices: "none" });
    void dialog.ask(TEXT);
    expect(rows()).toMatchObject({ "Most characters": "block", Alignment: "block", Choices: "none" });
    void dialog.ask(CHOICE);
    expect(rows()).toMatchObject({ "Most characters": "none", Alignment: "block", Choices: "block" });
  });

  it("stays open and says why when a value cannot be the field's, then takes the correction", async () => {
    const { dialog, apply, problem } = open();
    let settled = false;
    const answer = dialog.ask(TEXT).then((to) => { settled = true; return to; });
    dialog.put({ ...shown(TEXT), maxLength: "many" });
    apply.dispatch("click", {});
    await Promise.resolve();
    expect(settled).toBe(false);
    expect(dialog.isOpen).toBe(true);
    expect(problem.textContent).toContain("from 1 to 16384");
    dialog.put({ ...shown(TEXT), maxLength: "8" });
    apply.dispatch("click", {});
    await expect(answer).resolves.toMatchObject({ maxLength: 8 });
    // And the next field does not open with the last one's complaint.
    void dialog.ask(BOX);
    expect(problem.textContent).toBe("");
  });

  it.each([
    ["Cancel", (c: ReturnType<typeof open>) => c.cancel.dispatch("click", {})],
    ["Escape", (c: ReturnType<typeof open>) => c.backdrop.dispatch("keydown", { key: "Escape" })],
    ["the backdrop", (c: ReturnType<typeof open>) => c.backdrop.dispatch("click", { target: c.backdrop })],
    ["close()", (c: ReturnType<typeof open>) => c.dialog.close()],
    ["a second ask", (c: ReturnType<typeof open>) => void c.dialog.ask(BOX)],
  ])("resolves with null when dismissed by %s", async (_name, dismiss) => {
    const controls = open();
    const answer = controls.dialog.ask(TEXT);
    dismiss(controls);
    await expect(answer).resolves.toBeNull();
  });

  it("does not dismiss on a click inside the panel, and keeps keys from the window", () => {
    const controls = open();
    void controls.dialog.ask(TEXT);
    controls.backdrop.dispatch("click", { target: controls.panel });
    expect(controls.dialog.isOpen).toBe(true);
    for (const key of ["a", "Enter", "Escape"]) {
      void controls.dialog.ask(TEXT);
      let stopped = 0;
      controls.backdrop.dispatch("keydown", { key, preventDefault: () => {}, stopPropagation: () => { stopped += 1; } });
      expect(stopped, key).toBe(1);
    }
  });
});
