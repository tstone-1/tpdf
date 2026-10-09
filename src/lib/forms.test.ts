import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  FormLayer, answerError, fieldValue, sameAnswer, shownAlign, shownChoices, shownValue, standingChoice, tabStop,
  ringFrom,
  type FormWidget,
} from "./forms";
import { FakeElement, installFakeDom, type FakeDom } from "./testdom";

const text: FormWidget = {
  object: [12, 0], widget: [13, 0], page: 0,
  rect: [0, 0, 100, 20], display_rect: [0, 0, 100, 20],
  name: "ACME.answer", value: "original", control: { kind: "text" }, multiline: false,
  max_length: 20, reason: null,
};

describe("what a field's control shows", () => {
  const empty: FormWidget = { ...text, value: "" };
  // The same field shown a second time: another widget, one object.
  const twin: FormWidget = { ...empty, widget: [14, 0] };
  const other: FormWidget = { ...empty, object: [20, 0], widget: [21, 0] };
  const all = [empty, twin, other];
  const given = (object: [number, number], value: string, removed = false) =>
    ({ object, page: 5, removed, props: { default_value: value } });

  it("is the answer, pending or the file's, when the field holds one", () => {
    expect(shownValue(text, [], [given([13, 0], "n/a")], [text])).toBe("original");
    expect(shownValue(empty, [{ object: [12, 0], value: "typed" }], [given([13, 0], "n/a")], all)).toBe("typed");
    expect(shownValue(empty, [], [], all)).toBe("");
  });

  it("is the default a reader has given a field that holds nothing, as the save will answer it", () => {
    expect(shownValue(empty, [], [given([13, 0], "n/a")], all)).toBe("n/a");
    // Cleared by the reader: the save answers it with the default all the same.
    expect(shownValue(empty, [{ object: [12, 0], value: "" }], [given([13, 0], "n/a")], all)).toBe("n/a");
    // Held under the field's other widget, also when that one was removed.
    expect(shownValue(empty, [], [given([14, 0], "n/a")], all)).toBe("n/a");
    expect(shownValue(empty, [], [given([14, 0], "n/a", true)], all)).toBe("n/a");
    // Another field's default is not this one's, and a change with no default gives none.
    expect(shownValue(empty, [], [given([21, 0], "n/a")], all)).toBe("");
    expect(shownValue(empty, [], [{ object: [13, 0], page: 5, removed: false, props: { tooltip: "x" } }], all)).toBe("");
    // A default taken off again.
    expect(shownValue(empty, [], [given([13, 0], "")], all)).toBe("");
  });

  it("is never a default for a field that is not a text field", () => {
    const box: FormWidget = { ...empty, value: false, control: { kind: "checkbox" } };
    expect(shownValue(box, [], [given([13, 0], "n/a")], [box])).toBe(false);
    const list: FormWidget = { ...empty, value: "", control: { kind: "choice", combo: true, editable: true, multiple: false, options: [] } };
    expect(shownValue(list, [], [given([13, 0], "n/a")], [list])).toBe("");
    // A kind that is neither text nor a list, holding an empty string.
    const odd: FormWidget = { ...empty, control: { kind: "unsupported" } };
    expect(shownValue(odd, [], [given([13, 0], "n/a")], [odd])).toBe("");
  });
});

describe("the choices and the alignment a control shows before the save", () => {
  const list: FormWidget = {
    ...text, value: [1], align: "right",
    control: { kind: "choice", combo: true, editable: false, multiple: false,
      options: [{ export: "y", label: "Yes" }, { export: "n", label: "No" }, { export: "n2", label: "No" }, { export: "l", label: "Later" }] },
  };
  const twin: FormWidget = { ...list, widget: [14, 0] };
  const changed = (object: [number, number], props: object, removed = false) => ({ object, page: 5, removed, props });

  it("are the file's while a reader has changed neither", () => {
    expect(shownChoices(list, [], [list])).toBeNull();
    expect(shownChoices(list, [changed([13, 0], { tooltip: "x" })], [list])).toBeNull();
    expect(shownChoices(text, [changed([13, 0], { options: ["a"] })], [text])).toBeNull();
    expect(shownAlign(list, [], [list])).toBe("right");
    expect(shownAlign(text, [], [text])).toBe("left");
    // With the file's choices an answer is the file's own, two equal labels apart.
    expect(shownValue({ ...list, value: [2] }, [], [], [list])).toEqual([2]);
  });

  it("are the ones a reader has just given, each at the first place the file has its label", () => {
    const to = [changed([13, 0], { options: ["Later", "No", "Maybe"], align: "center" })];
    expect(shownChoices(list, to, [list])).toEqual([
      { label: "Later", index: 3 }, { label: "No", index: 1 }, { label: "Maybe", index: null },
    ]);
    expect(shownAlign(list, to, [list])).toBe("center");
    // Held under the field's other widget, also when that one was removed.
    expect(shownChoices(list, [changed([14, 0], { options: ["Maybe"] }, true)], [list, twin])).toEqual([{ label: "Maybe", index: null }]);
    expect(shownAlign(list, [changed([14, 0], { align: "left" })], [list, twin])).toBe("left");
    // Another field's are not this one's.
    expect(shownChoices(list, [changed([99, 0], { options: ["Maybe"] })], [list, twin])).toBeNull();
    expect(shownAlign(list, [changed([99, 0], { align: "center" })], [list, twin])).toBe("right");
  });

  it("keep chosen what the new choices still have by label, as the save does", () => {
    const to = [changed([13, 0], { options: ["Later", "No", "Maybe"] })];
    expect(shownValue(list, [], to, [list])).toEqual([1]);
    // The second "No" of the file is shown where the control offers "No".
    expect(shownValue({ ...list, value: [2] }, [], to, [list])).toEqual([1]);
    // "Yes" is gone, so nothing is chosen; a pending answer is treated as the file's is.
    expect(shownValue({ ...list, value: [0] }, [], to, [list])).toEqual([]);
    expect(shownValue(list, [{ object: [12, 0], value: [0, 3] }], to, [list])).toEqual([3]);
    expect(standingChoice([{ label: "a" }, { label: "b" }, { label: "a" }], [{ label: "b", index: 1 }, { label: "a", index: 0 }], [2, 1, 0])).toEqual([0, 1]);
    expect(standingChoice([{ label: "a" }], [{ label: "a", index: null }], [0, 7])).toEqual([]);
    // Text typed into a list that takes it is not a choice and stays.
    expect(shownValue({ ...list, value: "own words" }, [], to, [list])).toBe("own words");
  });
});

describe("form answers", () => {
  const choice: FormWidget = { ...text, value: [0], control: { kind: "choice", combo: true, editable: false, multiple: false,
    options: [{ export: "SAME", label: "First" }, { export: "SAME", label: "Second" }, { export: "OTHER", label: "Third" }] } };
  const radio: FormWidget = { ...text, value: [0], control: { kind: "radio", index: 0, states: [[65], [66]], unison: false, no_toggle_off: true } };
  it("keeps duplicate exports distinct and compares selections across IPC replies", () => {
    expect(fieldValue(choice, [{ object: choice.object, value: [1] }])).toEqual([1]);
    expect(sameAnswer([0, 2], [0, 2])).toBe(true);
    expect(sameAnswer([0], [1])).toBe(false);
    expect(sameAnswer([], "")).toBe(false);
    expect(sameAnswer([0, 2], [0])).toBe(false);
  });
  it("validates option indices and selection cardinality", () => {
    expect(answerError(choice, [1])).toBeNull();
    expect(answerError(choice, [])).toBeNull();
    for (const answer of [[3], [-1], [0.5], [NaN], [0, 1], [1, 1], "SAME", true]) expect(answerError(choice, answer)).not.toBeNull();
    const list: FormWidget = { ...choice, control: { ...choice.control as Extract<FormWidget["control"], { kind: "choice" }>, combo: false, multiple: true } };
    expect(answerError(list, [0, 2])).toBeNull();
    expect(answerError(list, [2, 0])).not.toBeNull();
    expect(answerError(list, [1, 1])).not.toBeNull();
    expect(answerError(radio, [1])).toBeNull();
    expect(answerError(radio, [])).not.toBeNull();
    expect(answerError(radio, [0, 1])).not.toBeNull();
    expect(answerError(radio, true)).not.toBeNull();
  });
  it("accepts custom text only in editable dropdowns", () => {
    const editable: FormWidget = { ...choice, control: { ...choice.control as Extract<FormWidget["control"], { kind: "choice" }>, editable: true } };
    expect(answerError(editable, "Custom answer")).toBeNull();
    expect(answerError(editable, "漢")).not.toBeNull();
    expect(answerError(editable, [2])).toBeNull();
    expect(answerError(editable, false)).not.toBeNull();
  });
  it("clears a field, shares an answer, and restores the original on undo", () => {
    const other = { ...text, widget: [14, 0] as [number, number], page: 1 };
    expect(fieldValue(text, [{ object: [12, 0], value: "" }])).toBe("");
    expect(fieldValue(other, [{ object: [12, 0], value: "new" }])).toBe("new");
    expect(fieldValue(text, [])).toBe("original");
    expect(fieldValue(text, [{ object: [12, 1], value: "wrong generation" }])).toBe("original");
  });
  it("does not confuse an unchecked checkbox with an absent answer", () => {
    expect(fieldValue({ ...text, value: true }, [{ object: [12, 0], value: false }])).toBe(false);
  });
  it("accepts supported text and refuses loss, wrong types and field restrictions", () => {
    expect(answerError(text, "Grüße")).toBeNull();
    expect(answerError(text, "")).toBeNull();
    expect(answerError(text, "a\nb")).not.toBeNull();
    expect(answerError({ ...text, multiline: true }, "a\nb")).toBeNull();
    expect(answerError(text, "漢")).not.toBeNull();
    expect(answerError(text, "x".repeat(21))).not.toBeNull();
    expect(answerError(text, false)).not.toBeNull();
    expect(answerError({ ...text, reason: "Read-only" }, "new")).toBe("Read-only");
    expect(answerError({ ...text, value: false, control: { kind: "checkbox" } }, true)).toBeNull();
  });
});

describe("where Tab takes the keyboard in a form", () => {
  const controls = ["a", "b", "c", "d"];
  /** Asks about `controls` with these put away and these out of reach, and notes what was reached for. */
  const from = (at: number, step: 1 | -1, away: string[] = [], unreachable: string[] = []) => {
    const reached: string[] = [];
    const stop = tabStop(controls, at, step, (one) => !away.includes(one), (one) => {
      reached.push(one);
      return !unreachable.includes(one);
    });
    return { ...stop, reached };
  };

  it("is the next control, and the one before with Shift", () => {
    expect(from(1, 1)).toEqual({ to: "c", further: true, moved: false, reached: ["c"] });
    expect(from(1, -1)).toEqual({ to: "a", further: true, moved: false, reached: ["a"] });
  });

  it("passes over a control that is put away, without scrolling to it", () => {
    // Its field is being changed and it has no place on the page: reaching
    // for it would move the page for a control that cannot be shown.
    expect(from(0, 1, ["b", "c"])).toEqual({ to: "d", further: true, moved: false, reached: ["d"] });
    expect(from(3, -1, ["c"])).toEqual({ to: "b", further: true, moved: false, reached: ["b"] });
  });

  it("passes over a control that could not be brought to the keyboard, and says the page moved", () => {
    expect(from(0, 1, [], ["b"])).toEqual({ to: "c", further: true, moved: true, reached: ["b", "c"] });
  });

  it("is nowhere when no control further on can take it, and says there were some", () => {
    expect(from(1, 1, ["c", "d"])).toEqual({ to: null, further: true, moved: false, reached: [] });
    expect(from(1, 1, ["c"], ["d"])).toEqual({ to: null, further: true, moved: true, reached: ["d"] });
  });

  it("is the web view's from the last control and from the first with Shift", () => {
    expect(from(3, 1)).toEqual({ to: null, further: false, moved: false, reached: [] });
    expect(from(0, -1)).toEqual({ to: null, further: false, moved: false, reached: [] });
    // A control that is not one of these has no next.
    expect(from(-1, 1)).toEqual({ to: null, further: false, moved: false, reached: [] });
  });
});

describe("a radio group as an arrow key goes round it", () => {
  it("is the rest of the group in the order an arrow key reaches it", () => {
    expect(ringFrom(["a", "b", "c", "d"], 1, 1)).toEqual(["b", "c", "d", "a"]);
    expect(ringFrom(["a", "b", "c", "d"], 1, -1)).toEqual(["b", "a", "d", "c"]);
    expect(ringFrom(["a"], 0, 1)).toEqual(["a"]);
    expect(ringFrom(["a", "b"], -1, 1)).toEqual([]);
  });
});

describe("Tab in the form's controls", () => {
  let dom: FakeDom;
  let previous: [unknown, unknown];
  const globals = globalThis as unknown as Record<string, unknown>;

  beforeEach(() => {
    dom = installFakeDom();
    // The layer asks which element it made. Every one here is a text input.
    previous = [globals.HTMLInputElement, globals.HTMLSelectElement];
    globals.HTMLInputElement = FakeElement;
    globals.HTMLSelectElement = class {};
  });

  afterEach(() => {
    [globals.HTMLInputElement, globals.HTMLSelectElement] = previous;
    dom.restore();
  });

  /** Three text fields one under another, the ones named in `away` with no place on the page. */
  function form(away: Set<number>, below = new Set<number>()) {
    const widgets = [0, 1, 2].map((at): FormWidget => ({ ...text, object: [30 + at, 0], widget: [40 + at, 0], name: `Field ${at}` }));
    const revealed: string[] = [];
    // One in `below` has a place the page never scrolls to: the layer's
    // `reveal` here moves nothing.
    const top = (at: number) => (below.has(at) ? 5000 : 20) + 30 * at;
    const layer = new FormLayer(dom.root as unknown as HTMLElement, { widgets },
      (widget) => away.has(widgets.indexOf(widget)) ? null
        : { left: 10, right: 110, top: top(widgets.indexOf(widget)), bottom: top(widgets.indexOf(widget)) + 20 },
      async () => {}, (widget) => void revealed.push(widget.name), () => {});
    const node = dom.root.children[0]!;
    node.clientHeight = 700;
    layer.layout();
    const inputs = node.children;
    const tab = (at: number, shiftKey = false) => {
      let prevented = 0;
      inputs[at]!.dispatch("keydown", { key: "Tab", shiftKey, preventDefault: () => { prevented++; } });
      return prevented;
    };
    return { layer, inputs, revealed, tab };
  }

  it("moves an arrow key's answer to the next button that is shown", () => {
    // Mutation: the handler taking the neighbour whatever it shows. An arrow
    // key in a radio group also answers, so it answered with a button the
    // reader could not see.
    const away = new Set<number>([1]);
    const widgets = [0, 1, 2].map((at): FormWidget => ({
      ...text, widget: [50 + at, 0], value: [0],
      control: { kind: "radio", index: at, states: [[65], [66], [67]], unison: false, no_toggle_off: true },
    }));
    const layer = new FormLayer(dom.root as unknown as HTMLElement, { widgets },
      (widget) => away.has(widgets.indexOf(widget)) ? null
        : { left: 10, right: 30, top: 20 + 30 * widgets.indexOf(widget), bottom: 40 + 30 * widgets.indexOf(widget) },
      async () => {}, () => {}, () => {});
    const node = dom.root.children[0]!;
    node.clientHeight = 700;
    layer.layout();
    const inputs = node.children;
    const clicked: number[] = [];
    inputs.forEach((input, at) => { (input as unknown as { click: () => void }).click = () => { clicked.push(at); }; });
    expect(inputs[1]!.style.display).toBe("none");

    inputs[0]!.dispatch("keydown", { key: "ArrowDown", preventDefault: () => {} });

    expect(clicked).toEqual([2]);
    expect(inputs.map((input) => input.focused)).toEqual([false, false, true]);
    layer.destroy();
  });

  it("goes to the next control that is shown, past one that is put away", () => {
    // Mutation: the handler focusing `controls[at + 1]` whatever it shows. A
    // control that is put away takes no keyboard, so the key was taken and
    // the keyboard went nowhere.
    const away = new Set<number>();
    const { layer, inputs, revealed, tab } = form(away);
    expect(inputs.map((input) => input.style.display)).toEqual(["block", "block", "block"]);
    away.add(1);
    layer.layout();
    expect(inputs[1]!.style.display).toBe("none");

    expect(tab(0)).toBe(1);

    expect(inputs.map((input) => input.focused)).toEqual([false, false, true]);
    // Brought into view, and the one put away was not scrolled to.
    expect(revealed).toEqual(["Field 2"]);
    layer.destroy();
  });

  it("leaves the keyboard where it is when no control further on is shown", () => {
    const { layer, inputs, revealed, tab } = form(new Set([1, 2]));
    // Taken, so the web view does not move the keyboard out of the form past
    // controls that are only put away for now.
    expect(tab(0)).toBe(1);
    expect(inputs.map((input) => input.focused)).toEqual([false, false, false]);
    expect(revealed).toEqual([]);
    layer.destroy();
  });

  it("goes back to the control the keyboard is in after looking for one it could not reach", () => {
    const { layer, inputs, revealed, tab } = form(new Set([2]), new Set([1]));
    expect(inputs[1]!.style.display).toBe("none");
    expect(tab(0)).toBe(1);
    expect(inputs.map((input) => input.focused)).toEqual([false, false, false]);
    // Reaching for the second scrolled the page, so the first is shown again.
    expect(revealed).toEqual(["Field 1", "Field 0"]);
    layer.destroy();
  });

  it("passes over a control that is withheld while the file is written", () => {
    const { layer, inputs, tab } = form(new Set());
    (inputs[1] as unknown as { disabled: boolean }).disabled = true;
    expect(tab(0)).toBe(1);
    expect(inputs.map((input) => input.focused)).toEqual([false, false, true]);
    layer.destroy();
  });

  it("leaves the key to the web view from the last control", () => {
    const { layer, inputs, tab } = form(new Set());
    expect(tab(2)).toBe(0);
    expect(tab(0, true)).toBe(0);
    expect(inputs.map((input) => input.focused)).toEqual([false, false, false]);
    layer.destroy();
  });
});
