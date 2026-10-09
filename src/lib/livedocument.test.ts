import { describe, expect, it } from "vitest";
import { Stage, blankLive, capture, install, type Slots } from "./livedocument";

interface Toy {
  id: number;
  name: string;
  words: string[];
}

/** Three variables and the slots over them, as `App.svelte` has them. */
function variables() {
  let id = -1;
  let name = "";
  let words: string[] = [];
  const slots: Slots<Toy> = {
    id: { get: () => id, set: (value) => { id = value; } },
    name: { get: () => name, set: (value) => { name = value; } },
    words: { get: () => words, set: (value) => { words = value; } },
  };
  const blank = (): Toy => ({ id: -1, name: "", words: [] });
  const stage = new Stage(slots, blank, () => id);
  return {
    slots,
    stage,
    now: (): Toy => ({ id, name, words }),
    open: (to: number, as: string) => { id = to; name = as; words = [as]; },
    rename: (as: string) => { name = as; },
    close: () => { id = -1; },
  };
}

describe("capture and install", () => {
  it("move every field, and the same objects rather than copies", () => {
    const v = variables();
    v.open(4, "four");
    const record = capture(v.slots);
    expect(record).toEqual({ id: 4, name: "four", words: ["four"] });
    expect(record.words).toBe(v.now().words);

    install(v.slots, { id: 9, name: "nine", words: [] });
    expect(v.now()).toEqual({ id: 9, name: "nine", words: [] });
    // The record taken earlier is not reached by the install.
    expect(record).toEqual({ id: 4, name: "four", words: ["four"] });
  });
});

describe("the stage", () => {
  it("holds nothing before a document is opened", () => {
    const v = variables();
    expect(v.stage.focused).toBe(-1);
    expect(v.stage.holds(-1)).toBe(false);
    expect(v.stage.parked).toEqual([]);
    // Parking nothing parks nothing.
    v.stage.park();
    expect(v.stage.parked).toEqual([]);
  });

  it("parks the focused document and leaves the variables blank", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    expect(v.now()).toEqual({ id: -1, name: "", words: [] });
    expect(v.stage.parked).toEqual([1]);
    expect(v.stage.holds(1)).toBe(true);
  });

  it("swaps two documents and keeps what each one held", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");

    expect(v.stage.focus(1)).toBe(true);
    expect(v.now()).toEqual({ id: 1, name: "one", words: ["one"] });
    expect(v.stage.parked).toEqual([2]);

    v.rename("one, edited");
    expect(v.stage.focus(2)).toBe(true);
    expect(v.now()).toEqual({ id: 2, name: "two", words: ["two"] });
    expect(v.stage.focus(1)).toBe(true);
    expect(v.now().name).toBe("one, edited");
  });

  it("refuses to focus a document that is not parked, and moves nothing", () => {
    const v = variables();
    v.open(1, "one");
    expect(v.stage.focus(7)).toBe(false);
    // The focused document is not parked by asking for itself either.
    expect(v.stage.focus(1)).toBe(false);
    expect(v.now().id).toBe(1);
    expect(v.stage.parked).toEqual([]);
  });

  it("clears the variables without touching a parked document", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");
    v.stage.clear();
    expect(v.now()).toEqual({ id: -1, name: "", words: [] });
    expect(v.stage.holds(2)).toBe(false);
    expect(v.stage.holds(1)).toBe(true);
  });

  it("hands a parked record over once", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    expect(v.stage.take(1)).toEqual({ id: 1, name: "one", words: ["one"] });
    expect(v.stage.take(1)).toBeUndefined();
    expect(v.stage.holds(1)).toBe(false);
  });
});

describe("work done within a document", () => {
  it("runs directly for the focused document", () => {
    const v = variables();
    v.open(1, "one");
    const result = v.stage.within(1, () => { v.rename("renamed"); return v.now().id; });
    expect(result).toEqual({ ran: true, value: 1 });
    expect(v.now().name).toBe("renamed");
  });

  it("lends the variables to a parked document and keeps what the work wrote", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");

    let seen = -1;
    const result = v.stage.within(1, () => { seen = v.now().id; v.rename("one, late reply"); });
    expect(result.ran).toBe(true);
    expect(seen).toBe(1);
    // The focused document is back and was not written to.
    expect(v.now()).toEqual({ id: 2, name: "two", words: ["two"] });
    v.stage.focus(1);
    expect(v.now().name).toBe("one, late reply");
  });

  it("does not run for a document that is not mounted", () => {
    const v = variables();
    v.open(2, "two");
    let ran = false;
    expect(v.stage.within(5, () => { ran = true; })).toEqual({ ran: false });
    expect(v.stage.within(-1, () => { ran = true; })).toEqual({ ran: false });
    expect(ran).toBe(false);
    expect(v.now().id).toBe(2);
  });

  it("puts the focused document back when the work throws", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");
    expect(() => v.stage.within(1, () => { v.rename("half done"); throw new Error("no"); })).toThrow("no");
    expect(v.now()).toEqual({ id: 2, name: "two", words: ["two"] });
    v.stage.focus(1);
    expect(v.now().name).toBe("half done");
  });

  it("drops the record of a document the work ended", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");
    v.stage.within(1, () => v.close());
    expect(v.stage.holds(1)).toBe(false);
    expect(v.now().id).toBe(2);
  });

  it("nests, each level returning the variables to the one outside it", () => {
    const v = variables();
    v.open(1, "one");
    v.stage.park();
    v.open(2, "two");
    v.stage.park();
    v.open(3, "three");
    const seen: number[] = [];
    v.stage.within(1, () => {
      seen.push(v.now().id);
      v.stage.within(2, () => seen.push(v.now().id));
      seen.push(v.now().id);
    });
    expect(seen).toEqual([1, 2, 1]);
    expect(v.now().id).toBe(3);
    expect(v.stage.parked.sort()).toEqual([1, 2]);
  });
});

describe("a blank live document", () => {
  it("names no document", () => {
    const blank = blankLive();
    expect(blank.openDoc).toBe(-1);
    expect(blank.viewer).toBeNull();
    expect(blank.edits).toBeNull();
    expect(blank.title).toBe("");
  });

  it("shares no collection with the one before it", () => {
    const first = blankLive();
    const second = blankLive();
    first.covered.set(1, "words");
    first.wordsAsked.add(1);
    first.offers.push({} as never);
    first.formNames.push("name");
    expect(second.covered.size).toBe(0);
    expect(second.wordsAsked.size).toBe(0);
    expect(second.offers).toEqual([]);
    expect(second.formNames).toEqual([]);
    expect(second.importedLinks).not.toBe(first.importedLinks);
    expect(second.degradedGate).not.toBe(first.degradedGate);
    expect(second.commentWords).not.toBe(first.commentWords);
    expect(second.redactionWords).not.toBe(first.redactionWords);
    expect(second.redactionPlans).not.toBe(first.redactionPlans);
  });
});
